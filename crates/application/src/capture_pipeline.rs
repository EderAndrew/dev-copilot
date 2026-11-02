//! Capture pipeline: opens the selected sources into one bounded audio queue under a
//! per-session [`CancellationToken`] and reports source faults (research R3, Art. V.3).
//!
//! The session service owns the pipeline and turns its signals into state changes and
//! indicator events. The audio queue feeds the [`Transcription`] stage
//! ([`crate::transcription`]); audio is never persisted (FR-020).
//!
//! A remote transcriber is wrapped in a [`FallbackTranscriber`] when a local one exists: the
//! first failure of the remote provider (including a revoked authorization) moves the session
//! to the local provider for good, with a notice (FR-045b). There is never a switch to another
//! remote provider (FR-048).
//!
//! [`Transcription`]: crate::transcription::Transcription

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

use devcopilot_domain::audio_source::AudioSourceKind;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::events::SessionEvent;
use crate::ports::{
    AudioChunkStream, AudioError, AudioSource, BoundedAudioSink, BoxFuture, CaptureHandle,
    ProviderDescriptor, ProviderError, SourceFault, SourceFaultNotifier, Transcriber,
    TranscriptSink,
};

/// Capacity of the audio queue in chunks (~30 s of 100 ms chunks, research R3).
pub const CHUNK_QUEUE_CAPACITY: usize = 300;

struct OpenSource {
    kind: AudioSourceKind,
    handle: Box<dyn CaptureHandle>,
    cancel: CancellationToken,
}

/// Open captures of one session. Dropping it closes every source.
///
/// Each source runs under a child of the session token, so cancelling the session closes
/// them all; the pipeline itself never cancels the session token.
pub struct CapturePipeline {
    cancel: CancellationToken,
    open: Vec<OpenSource>,
}

/// Receiving ends handed to the session's background tasks.
pub struct PipelineStreams {
    /// Audio from every open source, each chunk tagged with its source.
    pub chunks: AudioChunkStream,
    /// Faults reported by the open sources.
    pub faults: FaultStream,
    /// Sources that failed to open while others did; the session captures without them and
    /// reports them (US8.3).
    pub failed: Vec<OpenError>,
}

/// A source failed to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenError {
    /// Source that failed.
    pub source: AudioSourceKind,
    /// Why.
    pub error: AudioError,
}

impl CapturePipeline {
    /// Opens `sources` in order, each under a child of `cancel`. A source that fails does not
    /// stop the others (US8.3); it is listed in [`PipelineStreams::failed`].
    ///
    /// # Errors
    ///
    /// The first [`OpenError`] when no source opens.
    pub fn open(
        sources: &[Arc<dyn AudioSource>],
        cancel: CancellationToken,
    ) -> Result<(Self, PipelineStreams), OpenError> {
        let (sink, chunks) = BoundedAudioSink::channel(CHUNK_QUEUE_CAPACITY);
        let mut pipeline = Self {
            cancel,
            open: Vec::with_capacity(sources.len()),
        };
        let mut faults = Vec::with_capacity(sources.len());
        let mut failed = Vec::new();
        for source in sources {
            let kind = source.kind();
            let (notifier, fault_rx) = SourceFaultNotifier::channel();
            let source_cancel = pipeline.cancel.child_token();
            match source.start(sink.clone(), source_cancel.clone(), notifier) {
                Ok(handle) => {
                    pipeline.open.push(OpenSource {
                        kind,
                        handle,
                        cancel: source_cancel,
                    });
                    faults.push((kind, fault_rx));
                }
                Err(error) => failed.push(OpenError {
                    source: kind,
                    error,
                }),
            }
        }
        if pipeline.open.is_empty() {
            return Err(failed.into_iter().next().unwrap_or(OpenError {
                source: AudioSourceKind::Microphone,
                error: AudioError::Backend("no source selected".into()),
            }));
        }
        Ok((
            pipeline,
            PipelineStreams {
                chunks,
                faults: FaultStream { pending: faults },
                failed,
            },
        ))
    }

    /// Sources still capturing.
    #[must_use]
    pub fn active_sources(&self) -> Vec<AudioSourceKind> {
        self.open.iter().map(|open| open.kind).collect()
    }

    /// Closes one source immediately; the others keep capturing.
    pub fn close_source(&mut self, kind: AudioSourceKind) {
        if let Some(index) = self.open.iter().position(|open| open.kind == kind) {
            let open = self.open.swap_remove(index);
            open.cancel.cancel();
            open.handle.stop();
        }
        self.open.sort_by_key(|open| open.kind);
    }

    /// Closes every source immediately (FR-005).
    pub fn close_all(&mut self) {
        for open in self.open.drain(..) {
            open.cancel.cancel();
            open.handle.stop();
        }
    }
}

impl Drop for CapturePipeline {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Faults of the open sources, delivered as they happen.
pub struct FaultStream {
    pending: Vec<(AudioSourceKind, oneshot::Receiver<SourceFault>)>,
}

impl FaultStream {
    /// The next fault, or `None` once no open source can report one any more.
    pub async fn next(&mut self) -> Option<(AudioSourceKind, SourceFault)> {
        poll_fn(|cx| {
            let mut index = 0;
            while index < self.pending.len() {
                let (kind, rx) = &mut self.pending[index];
                match Pin::new(rx).poll(cx) {
                    Poll::Ready(Ok(fault)) => {
                        let kind = *kind;
                        self.pending.swap_remove(index);
                        return Poll::Ready(Some((kind, fault)));
                    }
                    // The source closed without a fault.
                    Poll::Ready(Err(_)) => {
                        self.pending.swap_remove(index);
                    }
                    Poll::Pending => index += 1,
                }
            }
            if self.pending.is_empty() {
                Poll::Ready(None)
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

/// Called once when a [`FallbackTranscriber`] switches: `(failed remote, local now in use)`.
pub type FallbackNotice = Box<dyn Fn(&ProviderDescriptor, &ProviderDescriptor) + Send + Sync>;

/// Remote transcriber that hands over to a local one after its first failure (FR-045b).
///
/// The failing run still returns its error, so the transcription stage records the lost audio
/// as `gap(provider_failure)` before restarting — now on the local transcriber.
pub struct FallbackTranscriber {
    primary: Arc<dyn Transcriber>,
    fallback: Arc<dyn Transcriber>,
    switched: AtomicBool,
    notice: FallbackNotice,
}

impl FallbackTranscriber {
    /// Wraps `primary` (remote) with `fallback` (local).
    #[must_use]
    pub fn new(
        primary: Arc<dyn Transcriber>,
        fallback: Arc<dyn Transcriber>,
        notice: FallbackNotice,
    ) -> Self {
        Self {
            primary,
            fallback,
            switched: AtomicBool::new(false),
            notice,
        }
    }

    /// Whether the local transcriber took over.
    #[must_use]
    pub fn switched(&self) -> bool {
        self.switched.load(Ordering::SeqCst)
    }
}

impl Transcriber for FallbackTranscriber {
    fn descriptor(&self) -> ProviderDescriptor {
        if self.switched() {
            self.fallback.descriptor()
        } else {
            self.primary.descriptor()
        }
    }

    fn run(
        &self,
        input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            if self.switched() {
                return self.fallback.run(input, out, cancel).await;
            }
            let result = self.primary.run(input, out, cancel).await;
            if let Err(err) = &result
                && *err != ProviderError::Cancelled
                && !self.switched.swap(true, Ordering::SeqCst)
            {
                let (from, to) = (self.primary.descriptor(), self.fallback.descriptor());
                tracing::warn!(from = %from.id, to = %to.id, error = %err, "remote transcriber failed; using local provider");
                (self.notice)(&from, &to);
            }
            result
        })
    }
}

/// What the capture indicator shows (SC-002, SC-003).
#[must_use]
pub fn indicator(active: bool, paused: bool, sources: Vec<AudioSourceKind>) -> SessionEvent {
    SessionEvent::CaptureIndicator {
        active,
        paused,
        sources,
    }
}

#[cfg(test)]
mod tests {
    use devcopilot_domain::audio_source::{Availability, UnavailableReason};

    use super::*;
    use crate::testing::ScriptedAudioSource;

    fn sources(list: &[Arc<ScriptedAudioSource>]) -> Vec<Arc<dyn AudioSource>> {
        list.iter()
            .map(|s| Arc::clone(s) as Arc<dyn AudioSource>)
            .collect()
    }

    #[tokio::test]
    async fn opens_every_source_and_closes_on_drop() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        let system = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::System));
        let cancel = CancellationToken::new();
        let (pipeline, _streams) =
            CapturePipeline::open(&sources(&[mic.clone(), system.clone()]), cancel.clone())
                .unwrap();
        assert_eq!(
            pipeline.active_sources(),
            [AudioSourceKind::Microphone, AudioSourceKind::System]
        );
        assert!(mic.is_capturing() && system.is_capturing());
        drop(pipeline);
        assert!(!mic.is_capturing() && !system.is_capturing());
        assert!(
            !cancel.is_cancelled(),
            "the session token belongs to the service"
        );
    }

    #[tokio::test]
    async fn cancelling_the_session_token_closes_every_source() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        let cancel = CancellationToken::new();
        let (_pipeline, _streams) =
            CapturePipeline::open(&sources(std::slice::from_ref(&mic)), cancel.clone()).unwrap();
        cancel.cancel();
        assert!(!mic.is_capturing());
    }

    #[tokio::test]
    async fn one_failed_source_keeps_the_others() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        let system = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::System));
        system.set_availability(Availability::Unavailable(UnavailableReason::NoDevice));
        let (pipeline, streams) = CapturePipeline::open(
            &sources(&[mic.clone(), system.clone()]),
            CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(pipeline.active_sources(), [AudioSourceKind::Microphone]);
        assert!(mic.is_capturing());
        assert_eq!(streams.failed.len(), 1);
        assert_eq!(streams.failed[0].source, AudioSourceKind::System);
    }

    #[tokio::test]
    async fn fails_when_no_source_opens() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        mic.set_availability(Availability::Unavailable(UnavailableReason::NoDevice));
        let result = CapturePipeline::open(
            &sources(std::slice::from_ref(&mic)),
            CancellationToken::new(),
        );
        assert_eq!(
            result.err().map(|e| e.source),
            Some(AudioSourceKind::Microphone)
        );
        assert!(!mic.is_capturing());
    }

    #[tokio::test]
    async fn closing_one_source_keeps_the_other() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        let system = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::System));
        let (mut pipeline, _streams) = CapturePipeline::open(
            &sources(&[mic.clone(), system.clone()]),
            CancellationToken::new(),
        )
        .unwrap();
        pipeline.close_source(AudioSourceKind::Microphone);
        assert!(!mic.is_capturing());
        assert!(system.is_capturing());
        assert_eq!(pipeline.active_sources(), [AudioSourceKind::System]);
    }

    #[tokio::test]
    async fn fault_stream_yields_faults_then_ends() {
        let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
        let (_pipeline, mut streams) = CapturePipeline::open(
            &sources(std::slice::from_ref(&mic)),
            CancellationToken::new(),
        )
        .unwrap();
        mic.trigger_fault(SourceFault::Lost);
        assert_eq!(
            streams.faults.next().await,
            Some((AudioSourceKind::Microphone, SourceFault::Lost))
        );
        assert_eq!(streams.faults.next().await, None);
    }
}
