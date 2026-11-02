//! Local transcription with whisper.cpp through `whisper-rs` (research R4).
//!
//! Audio never leaves the machine. Per source, a [`Chunker`] cuts the stream into windows;
//! partial windows are re-transcribed about every 2 s while speech continues, but only when
//! the input is caught up, so a slow machine spends its time on finals. Inference runs on the
//! blocking thread pool; cancellation returns immediately and the window in flight (at most
//! ~25 s of audio) finishes in the background with its result discarded. whisper's abort
//! callback is not used: `whisper-rs` 0.16 `set_abort_callback_safe` passes the closure with
//! the wrong pointer type and makes every inference abort (error -6).
//!
//! The model is loaded lazily on the first run and shared by later sessions; only the most
//! recently used model stays in memory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, PoisonError};

use devcopilot_application::ports::{
    AudioChunkStream, BoxFuture, ProviderDescriptor, ProviderError, Transcriber,
    TranscriberFactory, TranscriptSink, TranscriptUpdate,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{Provider, ProviderAdapterKind};
use devcopilot_domain::text::SegmentText;
use tokio_util::sync::CancellationToken;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::model_download::{self, ModelDownloader};
use crate::vad::{Chunker, ChunkerConfig, Cut, CutKind};

/// Segments whisper flags as probably silent are dropped above this probability.
const NO_SPEECH_THRESHOLD: f32 = 0.6;
/// Inference threads; whisper.cpp scales poorly beyond this on laptops.
const MAX_THREADS: usize = 8;

fn language_code(language: SessionLanguage) -> &'static str {
    match language {
        SessionLanguage::PtBr => "pt",
        SessionLanguage::En => "en",
    }
}

/// Routes whisper.cpp and GGML logs away from stderr; they are not needed and must not be
/// mixed with app logs.
fn silence_native_logs() {
    static ONCE: Once = Once::new();
    ONCE.call_once(whisper_rs::install_logging_hooks);
}

/// Whether whisper produced a non-speech marker such as `[BLANK_AUDIO]` or `(música)`.
fn is_marker(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with('(') && text.ends_with(')'))
        || (text.starts_with('*') && text.ends_with('*'))
}

/// Joins whisper segments into one bounded text, or `None` when nothing was said.
fn join_segments<'a>(segments: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut text = String::new();
    for segment in segments {
        let segment = segment.trim();
        if is_marker(segment) {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(segment);
    }
    let text: String = text.chars().take(SegmentText::MAX_CHARS).collect();
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Loaded model, shared across sessions.
#[derive(Default)]
struct ModelCache {
    loaded: Mutex<Option<(PathBuf, Arc<WhisperContext>)>>,
}

impl ModelCache {
    fn get(&self, path: &Path) -> Option<Arc<WhisperContext>> {
        self.loaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|(loaded, _)| loaded == path)
            .map(|(_, ctx)| Arc::clone(ctx))
    }

    async fn load(&self, path: &Path) -> Result<Arc<WhisperContext>, ProviderError> {
        if let Some(ctx) = self.get(path) {
            return Ok(ctx);
        }
        silence_native_logs();
        let owned = path.to_owned();
        let ctx = tokio::task::spawn_blocking(move || {
            WhisperContext::new_with_params(&owned, WhisperContextParameters::default())
        })
        .await
        .map_err(|_| ProviderError::Other("model loading panicked".into()))?
        .map_err(|err| {
            tracing::warn!(error = %err, "whisper model failed to load");
            ProviderError::Unavailable
        })?;
        let ctx = Arc::new(ctx);
        *self.loaded.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((path.to_owned(), Arc::clone(&ctx)));
        Ok(ctx)
    }
}

/// Builds [`WhisperTranscriber`]s for `whisper_local` providers whose model is downloaded.
pub struct WhisperTranscriberFactory {
    downloader: ModelDownloader,
    cache: Arc<ModelCache>,
}

impl WhisperTranscriberFactory {
    /// Factory reading models from `models_dir`.
    #[must_use]
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            downloader: ModelDownloader::new(models_dir),
            cache: Arc::new(ModelCache::default()),
        }
    }
}

impl TranscriberFactory for WhisperTranscriberFactory {
    fn create(
        &self,
        provider: &Provider,
        language: SessionLanguage,
    ) -> Result<Arc<dyn Transcriber>, ProviderError> {
        if provider.adapter() != ProviderAdapterKind::WhisperLocal {
            return Err(ProviderError::Unavailable);
        }
        let spec = model_download::find(provider.model()).ok_or(ProviderError::Unavailable)?;
        if !self.downloader.is_downloaded(spec) {
            return Err(ProviderError::Unavailable);
        }
        Ok(Arc::new(WhisperTranscriber {
            descriptor: ProviderDescriptor {
                id: provider.id(),
                locality: provider.locality(),
                data_categories: provider.data_categories().iter().copied().collect(),
            },
            model_path: self.downloader.path_for(spec),
            language,
            chunker: ChunkerConfig::default(),
            cache: Arc::clone(&self.cache),
        }))
    }
}

/// Streaming transcriber over a local whisper.cpp model.
pub struct WhisperTranscriber {
    descriptor: ProviderDescriptor,
    model_path: PathBuf,
    language: SessionLanguage,
    chunker: ChunkerConfig,
    cache: Arc<ModelCache>,
}

impl WhisperTranscriber {
    /// Transcriber for the model at `model_path`, outside any provider configuration (tests
    /// and the latency spike).
    #[must_use]
    pub fn from_model_path(
        descriptor: ProviderDescriptor,
        model_path: PathBuf,
        language: SessionLanguage,
    ) -> Self {
        Self {
            descriptor,
            model_path,
            language,
            chunker: ChunkerConfig::default(),
            cache: Arc::new(ModelCache::default()),
        }
    }

    /// Transcribes one window; `None` when it holds no speech.
    async fn transcribe(
        &self,
        ctx: &Arc<WhisperContext>,
        samples: Vec<f32>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        let ctx = Arc::clone(ctx);
        let language = language_code(self.language);
        let job = tokio::task::spawn_blocking(move || infer(&ctx, language, &samples));
        tokio::select! {
            () = cancel.cancelled() => Err(ProviderError::Cancelled),
            joined = job => joined.map_err(|_| ProviderError::Other("inference panicked".into()))?,
        }
    }

    async fn emit(
        &self,
        ctx: &Arc<WhisperContext>,
        source: AudioSourceKind,
        cut: Cut,
        out: &TranscriptSink,
        cancel: &CancellationToken,
    ) -> Result<bool, ProviderError> {
        let Some(text) = self.transcribe(ctx, cut.samples, cancel).await? else {
            return Ok(true);
        };
        let update = match cut.kind {
            CutKind::Partial => TranscriptUpdate::Partial {
                source,
                start: cut.start,
                text: Redacted::new(text),
            },
            CutKind::Final => TranscriptUpdate::Final {
                source,
                start: cut.start,
                end: cut.end,
                text: Redacted::new(
                    SegmentText::parse(text).map_err(|_| ProviderError::InvalidOutput)?,
                ),
            },
        };
        Ok(out.send(update).await.is_ok())
    }

    async fn run_inner(
        &self,
        mut input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> Result<(), ProviderError> {
        let ctx = tokio::select! {
            () = cancel.cancelled() => return Err(ProviderError::Cancelled),
            ctx = self.cache.load(&self.model_path) => ctx?,
        };
        let mut chunkers: BTreeMap<AudioSourceKind, Chunker> = BTreeMap::new();
        loop {
            let first = tokio::select! {
                () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                chunk = input.recv() => chunk,
            };
            let Some(first) = first else {
                for (source, chunker) in &mut chunkers {
                    if let Some(cut) = chunker.flush()
                        && !self.emit(&ctx, *source, cut, &out, &cancel).await?
                    {
                        return Ok(());
                    }
                }
                return Ok(());
            };
            // Take everything already queued so finals are never starved by partials.
            let mut finals = Vec::new();
            let mut partials: BTreeMap<AudioSourceKind, Cut> = BTreeMap::new();
            let mut next = Some(first);
            while let Some(chunk) = next.take() {
                let chunker = chunkers
                    .entry(chunk.source)
                    .or_insert_with(|| Chunker::new(self.chunker));
                for cut in chunker.push(chunk.start, &chunk.samples) {
                    match cut.kind {
                        CutKind::Final => {
                            partials.remove(&chunk.source);
                            finals.push((chunk.source, cut));
                        }
                        CutKind::Partial => {
                            partials.insert(chunk.source, cut);
                        }
                    }
                }
                next = input.try_recv().ok();
            }
            for (source, cut) in finals {
                if !self.emit(&ctx, source, cut, &out, &cancel).await? {
                    return Ok(());
                }
            }
            if input.is_empty() {
                for (source, cut) in partials {
                    if !self.emit(&ctx, source, cut, &out, &cancel).await? {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Runs whisper on one window (blocking).
fn infer(
    ctx: &WhisperContext,
    language: &'static str,
    samples: &[f32],
) -> Result<Option<String>, ProviderError> {
    let mut state = ctx
        .create_state()
        .map_err(|_| ProviderError::Other("whisper state allocation failed".into()))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(MAX_THREADS));
    params.set_n_threads(i32::try_from(threads).unwrap_or(4));
    params.set_language(Some(language));
    params.set_no_context(true);
    params.set_no_timestamps(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Whisper needs at least one second of input; pad short windows with silence.
    let samples: std::borrow::Cow<'_, [f32]> = if samples.len() < 16_000 {
        let mut padded = samples.to_vec();
        padded.resize(16_100, 0.0);
        padded.into()
    } else {
        samples.into()
    };
    state
        .full(params, &samples)
        .map_err(|err| ProviderError::Other(format!("whisper inference failed: {err}")))?;
    let texts: Vec<String> = (0..state.full_n_segments())
        .filter_map(|i| state.get_segment(i))
        .filter(|segment| segment.no_speech_probability() < NO_SPEECH_THRESHOLD)
        .filter_map(|segment| segment.to_str_lossy().ok().map(|t| t.into_owned()))
        .collect();
    Ok(join_segments(texts.iter().map(String::as_str)))
}

impl Transcriber for WhisperTranscriber {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use devcopilot_domain::ids::ProviderId;
    use devcopilot_domain::provider::{DataCategory, ProviderRole, ProviderSpec};

    use super::*;

    #[test]
    fn markers_and_blank_output_are_dropped() {
        assert_eq!(join_segments(["[BLANK_AUDIO]"]), None);
        assert_eq!(join_segments([" (música) ", "", "*risos*"]), None);
        assert_eq!(
            join_segments([" Vamos usar", "[Música]", " PostgreSQL. "]).as_deref(),
            Some("Vamos usar PostgreSQL.")
        );
    }

    #[test]
    fn joined_text_is_bounded() {
        let long = "a".repeat(SegmentText::MAX_CHARS + 10);
        assert_eq!(
            join_segments([long.as_str()]).map(|t| t.chars().count()),
            Some(SegmentText::MAX_CHARS)
        );
    }

    #[test]
    fn languages_map_to_whisper_codes() {
        assert_eq!(language_code(SessionLanguage::PtBr), "pt");
        assert_eq!(language_code(SessionLanguage::En), "en");
    }

    fn provider(model: &str) -> Provider {
        Provider::new(ProviderSpec {
            id: ProviderId::new(),
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::WhisperLocal,
            endpoint: None,
            model: model.into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Audio]),
        })
        .unwrap()
    }

    #[test]
    fn factory_requires_a_downloaded_catalog_model() {
        let dir = tempfile::tempdir().unwrap();
        let factory = WhisperTranscriberFactory::new(dir.path());
        assert_eq!(
            factory
                .create(&provider("unknown"), SessionLanguage::PtBr)
                .err(),
            Some(ProviderError::Unavailable)
        );
        assert_eq!(
            factory
                .create(&provider("base"), SessionLanguage::PtBr)
                .err(),
            Some(ProviderError::Unavailable),
            "not downloaded"
        );
    }
}
