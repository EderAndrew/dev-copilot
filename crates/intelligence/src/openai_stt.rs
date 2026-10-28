//! [`Transcriber`] over an API compatible with OpenAI `/audio/transcriptions` (research R5), as
//! an authorized remote provider.
//!
//! Finals only: per source, a [`Chunker`] closes windows of speech (up to 25 s, after a pause),
//! and each window is uploaded once as a 16-bit PCM WAV file. Partials are not produced in
//! remote mode (the UI says so). Before each upload the [`RemoteGate`] is asked for `audio`;
//! a refusal ends the run with [`ProviderError::AuthorizationRequired`] without sending
//! anything, so a revoked authorization stops uploads at the next window (FR-047). Any request
//! failure ends the run: the transcription stage records the lost audio as a gap and may move
//! to a local provider (FR-045b).
//!
//! [`RemoteGate`]: devcopilot_application::ports::RemoteGate

use std::collections::BTreeMap;

use devcopilot_application::ports::{
    AudioChunkStream, BoxFuture, ProviderDescriptor, ProviderError, SAMPLE_RATE_HZ, Transcriber,
    TranscriptSink, TranscriptUpdate,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::DataCategory;
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::remote::{RemoteTarget, http_error, read_json, secret_header, status_error};
use crate::vad::{Chunker, ChunkerConfig, Cut, CutKind};

/// Window settings for uploads: longer pauses and windows than the local model, so audio goes
/// out in fewer, larger requests (research R5: ~10–25 s).
#[must_use]
pub fn upload_chunker() -> ChunkerConfig {
    ChunkerConfig {
        silence_to_finalize: Duration::from_millis(1_200),
        max_window: Duration::from_secs(25),
        // Partials are never uploaded; keep them out of the way.
        partial_interval: Duration::from_secs(3_600),
        ..ChunkerConfig::default()
    }
}

#[derive(Deserialize)]
struct TranscriptionReply {
    text: String,
}

/// Remote speech-to-text over an OpenAI-compatible transcription API.
#[derive(Debug, Clone)]
pub struct OpenAiSttTranscriber {
    target: RemoteTarget,
    model: String,
    language: SessionLanguage,
    chunker: ChunkerConfig,
}

impl OpenAiSttTranscriber {
    /// Transcriber for `target` with `model`, transcribing `language`.
    #[must_use]
    pub fn new(target: RemoteTarget, model: &str, language: SessionLanguage) -> Self {
        Self {
            target,
            model: model.to_owned(),
            language,
            chunker: upload_chunker(),
        }
    }

    /// As [`Self::new`] with custom window settings (tests).
    #[must_use]
    pub fn with_chunker(mut self, chunker: ChunkerConfig) -> Self {
        self.chunker = chunker;
        self
    }

    async fn upload(&self, samples: &[f32]) -> Result<Option<String>, ProviderError> {
        self.target.authorize(&[DataCategory::Audio])?;
        let boundary = format!("devcopilot-{:016x}", samples.len() as u64 ^ 0x5eed_cafe);
        let language = match self.language {
            SessionLanguage::PtBr => "pt",
            SessionLanguage::En => "en",
        };
        let body = multipart(
            &boundary,
            &[
                ("model", self.model.as_bytes()),
                ("language", language.as_bytes()),
                ("response_format", b"json"),
            ],
            &wav(samples),
        );
        let mut request = self
            .target
            .client()
            .post(self.target.url("/audio/transcriptions"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        if let Some(key) = self.target.api_key() {
            let (name, value) = secret_header("authorization", &format!("Bearer {key}"))?;
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(|err| http_error(&err))?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        let reply: TranscriptionReply = read_json(response).await?;
        let text: String = reply
            .text
            .trim()
            .chars()
            .take(SegmentText::MAX_CHARS)
            .collect();
        let text = text.trim().to_owned();
        Ok((!text.is_empty()).then_some(text))
    }

    /// Uploads one final window; `false` once the output is closed.
    async fn emit(
        &self,
        source: AudioSourceKind,
        cut: Cut,
        out: &TranscriptSink,
        cancel: &CancellationToken,
    ) -> Result<bool, ProviderError> {
        if cut.kind != CutKind::Final {
            return Ok(true);
        }
        let text = tokio::select! {
            () = cancel.cancelled() => return Err(ProviderError::Cancelled),
            text = self.upload(&cut.samples) => text?,
        };
        let Some(text) = text else {
            return Ok(true);
        };
        let text = SegmentText::parse(text).map_err(|_| ProviderError::InvalidOutput)?;
        let update = TranscriptUpdate::Final {
            source,
            start: cut.start,
            end: cut.end,
            text: Redacted::new(text),
        };
        Ok(out.send(update).await.is_ok())
    }

    async fn run_inner(
        &self,
        mut input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> Result<(), ProviderError> {
        let mut chunkers: BTreeMap<AudioSourceKind, Chunker> = BTreeMap::new();
        loop {
            let chunk = tokio::select! {
                () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                chunk = input.recv() => chunk,
            };
            let Some(chunk) = chunk else {
                for (source, chunker) in &mut chunkers {
                    if let Some(cut) = chunker.flush()
                        && !self.emit(*source, cut, &out, &cancel).await?
                    {
                        return Ok(());
                    }
                }
                return Ok(());
            };
            let cuts = chunkers
                .entry(chunk.source)
                .or_insert_with(|| Chunker::new(self.chunker))
                .push(chunk.start, &chunk.samples);
            for cut in cuts {
                if !self.emit(chunk.source, cut, &out, &cancel).await? {
                    return Ok(());
                }
            }
        }
    }
}

impl Transcriber for OpenAiSttTranscriber {
    fn descriptor(&self) -> ProviderDescriptor {
        self.target.descriptor().clone()
    }

    fn run(
        &self,
        input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(self.run_inner(input, out, cancel))
    }
}

/// 16-bit PCM mono WAV at [`SAMPLE_RATE_HZ`].
#[must_use]
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len().saturating_mul(2)).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1_u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE_HZ.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE_HZ * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        // Saturating float-to-int conversion after clamping to [-1, 1].
        #[allow(clippy::cast_possible_truncation)]
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// `multipart/form-data` body with text `fields` and the WAV as `file`.
fn multipart(boundary: &str, fields: &[(&str, &[u8])], file: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(file.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_describes_16_bit_mono() {
        let bytes = wav(&[0.0, 1.0, -1.0]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            16_000
        );
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), -i16::MAX);
    }

    #[test]
    fn multipart_ends_with_closing_boundary() {
        let body = multipart("b", &[("model", b"m")], b"WAV");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--b\r\nContent-Disposition: form-data; name=\"model\""));
        assert!(text.contains("filename=\"audio.wav\""));
        assert!(text.ends_with("\r\n--b--\r\n"));
    }
}
