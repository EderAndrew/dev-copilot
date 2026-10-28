//! Local whisper transcription on short, synthetic, non-confidential fixtures (T065) and the
//! latency measurement of spike S2 (T067).
//!
//! Ignored by default: they need a downloaded model. Run with
//!
//! ```sh
//! DEVCOPILOT_WHISPER_MODEL=/path/to/ggml-base.bin \
//!   cargo test -p devcopilot-intelligence --release --test whisper_local -- --ignored --nocapture
//! ```
//!
//! `pinned_model_downloads_from_origin` fetches a catalog model over the real network into
//! `DEVCOPILOT_MODEL_DOWNLOAD_DIR` (default model `base`, override with
//! `DEVCOPILOT_MODEL_ID`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

use devcopilot_application::ports::{
    AudioChunk, BoundedAudioSink, ProviderDescriptor, Transcriber, TranscriptUpdate,
};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, Locality};
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::model_download::{self, ModelDownloader};
use devcopilot_intelligence::whisper_local::WhisperTranscriber;
use tokio_util::sync::CancellationToken;

const CHUNK: usize = 1_600; // 100 ms

fn model_path() -> Option<PathBuf> {
    std::env::var_os("DEVCOPILOT_WHISPER_MODEL").map(PathBuf::from)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}

/// Reads a 16 kHz mono PCM16 WAV, skipping chunks other than `fmt ` and `data`.
fn read_wav(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut offset = 12;
    let mut samples = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let len = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let body = &bytes[offset + 8..offset + 8 + len];
        match id {
            b"fmt " => {
                let format = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([body[14], body[15]]);
                assert_eq!((format, channels, rate, bits), (1, 1, 16_000, 16));
            }
            b"data" => {
                samples = Some(
                    body.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
                        .collect(),
                );
            }
            _ => {}
        }
        offset += 8 + len + (len % 2);
    }
    samples.expect("no data chunk")
}

struct Observed {
    /// (wall-clock since start, update)
    updates: Vec<(StdDuration, TranscriptUpdate)>,
    audio: StdDuration,
}

/// Streams `samples` (plus 1.5 s of trailing silence) in real time through the transcriber.
async fn stream(model: PathBuf, language: SessionLanguage, mut samples: Vec<f32>) -> Observed {
    let audio = StdDuration::from_millis(samples.len() as u64 * 1_000 / 16_000);
    samples.extend(std::iter::repeat_n(0.0, 24_000));
    let transcriber = WhisperTranscriber::from_model_path(
        ProviderDescriptor {
            id: ProviderId::new(),
            locality: Locality::Local,
            data_categories: vec![DataCategory::Audio],
        },
        model,
        language,
    );
    let (sink, input) = BoundedAudioSink::channel(1_000);
    let (out, mut updates) = tokio::sync::mpsc::channel(64);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(async move { transcriber.run(input, out, cancel).await });
    let start = Instant::now();
    let collector = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Some(update) = updates.recv().await {
            seen.push((start.elapsed(), update));
        }
        seen
    });
    // The model loads during the first chunks, as in a real session.
    for (i, chunk) in samples.chunks(CHUNK).enumerate() {
        let due = StdDuration::from_millis(i as u64 * 100);
        if let Some(wait) = due.checked_sub(start.elapsed()) {
            tokio::time::sleep(wait).await;
        }
        let sent = sink.try_send(AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_millis(i as u64 * 100),
            samples: chunk.to_vec(),
        });
        if sent.is_err() {
            break; // The run ended; its result says why.
        }
    }
    drop(sink);
    task.await.unwrap().expect("transcriber run failed");
    Observed {
        updates: collector.await.unwrap(),
        audio,
    }
}

fn final_texts(observed: &Observed) -> String {
    observed
        .updates
        .iter()
        .filter_map(|(_, u)| match u {
            TranscriptUpdate::Final { text, .. } => Some(text.expose().as_str().to_owned()),
            TranscriptUpdate::Partial { .. } => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Spike S2 figures: partial arrivals (windows start at speech onset, ~0 here) and final
/// latency after the end of the window it covers.
fn report(label: &str, observed: &Observed) {
    for (at, update) in &observed.updates {
        match update {
            TranscriptUpdate::Partial { start, .. } => eprintln!(
                "[S2 {label}] partial window_start={:>5} ms arrived={:>5} ms",
                StdDuration::from(*start).as_millis(),
                at.as_millis()
            ),
            TranscriptUpdate::Final { end, text, .. } => {
                let end = StdDuration::from(*end);
                eprintln!(
                    "[S2 {label}] final   window_end={:>5} ms arrived={:>5} ms latency={:>4} ms text={:?}",
                    end.as_millis(),
                    at.as_millis(),
                    at.saturating_sub(end).as_millis(),
                    text.expose().as_str()
                );
            }
        }
    }
    eprintln!(
        "[S2 {label}] speech length {} ms",
        observed.audio.as_millis()
    );
}

fn assert_shape(observed: &Observed) {
    let finals: Vec<_> = observed
        .updates
        .iter()
        .filter_map(|(_, u)| match u {
            TranscriptUpdate::Final { start, end, .. } => Some((*start, *end)),
            TranscriptUpdate::Partial { .. } => None,
        })
        .collect();
    assert!(!finals.is_empty(), "no final segment");
    let limit = Duration::from_millis(observed.audio.as_millis() as u64 + 1_600);
    for (start, end) in finals {
        assert!(start <= end && end <= limit, "offsets inside the audio");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs DEVCOPILOT_WHISPER_MODEL"]
async fn transcribes_portuguese_fixture() {
    let Some(model) = model_path() else { return };
    let observed = stream(
        model,
        SessionLanguage::PtBr,
        read_wav(&fixture("pt-br-short.wav")),
    )
    .await;
    report("pt-BR", &observed);
    assert_shape(&observed);
    let text = final_texts(&observed).to_lowercase();
    assert!(
        text.contains("pagina"),
        "expected 'paginação' in transcript"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs DEVCOPILOT_WHISPER_MODEL"]
async fn transcribes_english_fixture() {
    let Some(model) = model_path() else { return };
    let observed = stream(
        model,
        SessionLanguage::En,
        read_wav(&fixture("en-short.wav")),
    )
    .await;
    report("en", &observed);
    assert_shape(&observed);
    let text = final_texts(&observed).to_lowercase();
    assert!(
        text.contains("pagination"),
        "expected 'pagination' in transcript"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a model file"]
async fn cancellation_returns_promptly() {
    let Some(model) = model_path() else { return };
    let transcriber = WhisperTranscriber::from_model_path(
        ProviderDescriptor {
            id: ProviderId::new(),
            locality: Locality::Local,
            data_categories: vec![DataCategory::Audio],
        },
        model,
        SessionLanguage::PtBr,
    );
    let (sink, input) = BoundedAudioSink::channel(1_000);
    let (out, _updates) = tokio::sync::mpsc::channel(64);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let task = tokio::spawn(async move { transcriber.run(input, out, token).await });
    for (i, chunk) in read_wav(&fixture("pt-br-short.wav"))
        .chunks(CHUNK)
        .enumerate()
    {
        sink.try_send(AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_millis(i as u64 * 100),
            samples: chunk.to_vec(),
        })
        .unwrap();
    }
    tokio::time::sleep(StdDuration::from_millis(500)).await;
    let cancelled = Instant::now();
    cancel.cancel();
    let result = tokio::time::timeout(StdDuration::from_secs(5), task).await;
    assert!(result.is_ok(), "run must honour cancel within 5 s");
    eprintln!(
        "[S2] cancel returned after {} ms",
        cancelled.elapsed().as_millis()
    );
}

#[tokio::test]
#[ignore = "downloads a model over the network"]
async fn pinned_model_downloads_from_origin() {
    let Some(dir) = std::env::var_os("DEVCOPILOT_MODEL_DOWNLOAD_DIR") else {
        return;
    };
    let id = std::env::var("DEVCOPILOT_MODEL_ID").unwrap_or_else(|_| "base".into());
    let spec = model_download::find(&id).expect("unknown model id");
    let downloader = ModelDownloader::new(PathBuf::from(dir));
    let started = Instant::now();
    let path = downloader
        .download(spec, |_, _| {}, CancellationToken::new())
        .await
        .unwrap();
    eprintln!(
        "downloaded {} ({} bytes) in {} s to {}",
        spec.id(),
        spec.size_bytes(),
        started.elapsed().as_secs(),
        path.display()
    );
    assert!(downloader.is_downloaded(spec));
}
