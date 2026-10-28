//! Transcription stage of the capture pipeline (research R3/R4, FR-015–FR-020).
//!
//! ```text
//! audio queue ─▶ backlog (≤ 30 s, drop oldest) ─▶ transcriber ─▶ partial / final / gap
//! ```
//!
//! - Partials are only published; finals and gaps are persisted on arrival, each in its own
//!   transaction, then published (FR-017).
//! - The backlog is bounded by [`BACKLOG_LIMIT`]. When it overflows, the oldest
//!   [`OVERLOAD_DROP`] of audio is discarded at once and recorded as `gap(overload)` (FR-019),
//!   so the transcriber catches up instead of trailing by the limit forever.
//! - Lag is the audio captured but not yet handed to the transcriber; above
//!   [`LAG_THRESHOLD`] a `transcription_lag` event is published, and `lag = 0` once it falls
//!   to [`LAG_RECOVERED`] (US2.4).
//! - A transcriber failure records `gap(provider_failure)` for the audio it had not finalized
//!   and restarts it; after [`MAX_CONSECUTIVE_FAILURES`] without a final in between, the
//!   session fails as `no_transcriber`.
//! - A storage failure publishes `storage_error` and fails the session as `storage_error`;
//!   what was already stored stays (US2.3).
//!
//! Offsets are relative to the session start: chunks are shifted by the capture's offset base
//! before the transcriber sees them, so its updates already carry session offsets.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{SegmentId, SessionId};
use devcopilot_domain::session::FailureReason;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::events::{FinalSegment, SessionEvent};
use crate::ports::{
    AudioChunk, AudioChunkStream, EventPublisher, ProviderError, SAMPLE_RATE_HZ, SegmentRepository,
    Transcriber, TranscriptUpdate,
};

/// Most audio waiting for the transcriber (research R3: ~30 s of chunks).
pub const BACKLOG_LIMIT: Duration = Duration::from_secs(30);
/// Oldest audio discarded at once when the backlog overflows.
pub const OVERLOAD_DROP: Duration = Duration::from_secs(15);
/// Lag above which `transcription_lag` is published (SC-005).
pub const LAG_THRESHOLD: Duration = Duration::from_secs(15);
/// Lag at or below which the lag indicator is cleared.
pub const LAG_RECOVERED: Duration = Duration::from_secs(5);
/// Transcriber failures in a row, without a final in between, that fail the session.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Chunks queued between the backlog and the transcriber; small, so lag stays measurable.
const TRANSCRIBER_INPUT_CAPACITY: usize = 4;
/// Updates queued from the transcriber.
const TRANSCRIPT_QUEUE_CAPACITY: usize = 64;
/// i18n key published with `storage_error`.
const STORAGE_ERROR_KEY: &str = "errors.storageError";

/// Called once when the stage cannot continue; the session service fails the session.
pub type FatalHandler = Arc<dyn Fn(FailureReason) + Send + Sync>;

/// Collaborators of one session's transcription.
pub struct TranscriptionDeps {
    /// Session being transcribed.
    pub session_id: SessionId,
    /// Sources selected for the session; finals are tagged when there are two (FR-016).
    pub session_sources: Vec<AudioSourceKind>,
    /// Session offset at which this capture started.
    pub offset_base: Duration,
    /// `seq` of the first segment written.
    pub first_seq: u64,
    /// Speech to text.
    pub transcriber: Arc<dyn Transcriber>,
    /// Segment storage.
    pub segments: Arc<dyn SegmentRepository>,
    /// Event delivery.
    pub events: Arc<dyn EventPublisher>,
    /// Fails the session.
    pub on_fatal: FatalHandler,
}

fn chunk_duration(chunk: &AudioChunk) -> Duration {
    let samples = u64::try_from(chunk.samples.len()).unwrap_or(u64::MAX);
    Duration::from_millis(samples.saturating_mul(1_000) / u64::from(SAMPLE_RATE_HZ))
}

fn chunk_end(chunk: &AudioChunk) -> Duration {
    chunk.start.saturating_add(chunk_duration(chunk))
}

#[derive(Debug, Default)]
struct Progress {
    next_seq: u64,
    /// End of the latest audio received from the sources.
    captured_end: Duration,
    /// End of the latest audio handed to the transcriber.
    consumed_end: Duration,
    /// End of the latest final or gap written.
    finalized_end: Duration,
    lag_reported: bool,
    /// No more writes: storage failed or the stage was aborted.
    closed: bool,
}

/// Writes segments and publishes events; shared by the task and its [`Transcription`] handle.
struct Recorder {
    session_id: SessionId,
    two_sources: bool,
    session_sources: Vec<AudioSourceKind>,
    segments: Arc<dyn SegmentRepository>,
    events: Arc<dyn EventPublisher>,
    on_fatal: FatalHandler,
    progress: Mutex<Progress>,
}

impl Recorder {
    fn lock(&self) -> MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, event: SessionEvent) {
        self.events.publish(self.session_id, event);
    }

    fn tag(&self, source: AudioSourceKind) -> Option<AudioSourceKind> {
        self.two_sources.then_some(source)
    }

    /// Persists and publishes one segment. `false` once nothing more can be written.
    fn write(
        &self,
        kind: SegmentKind,
        source: Option<AudioSourceKind>,
        start: Duration,
        end: Duration,
    ) -> bool {
        let mut progress = self.lock();
        if progress.closed {
            return false;
        }
        let segment = match TranscriptSegment::new(
            SegmentSpec {
                id: SegmentId::new(),
                session_id: self.session_id,
                seq: progress.next_seq,
                kind,
                source,
                start,
                end,
            },
            &self.session_sources,
        ) {
            Ok(segment) => segment,
            Err(err) => {
                // A transcriber bug; drop the segment rather than the session.
                tracing::warn!(session = %self.session_id, error = %err, "segment rejected");
                return true;
            }
        };
        if let Err(err) = self.segments.append(&segment) {
            progress.closed = true;
            drop(progress);
            tracing::error!(session = %self.session_id, error = %err, "segment append failed");
            self.publish(SessionEvent::StorageError {
                message_key: STORAGE_ERROR_KEY,
            });
            (self.on_fatal)(FailureReason::StorageError);
            return false;
        }
        progress.next_seq += 1;
        progress.finalized_end = progress.finalized_end.max(segment.end());
        let event = match segment.kind() {
            SegmentKind::Final { text } => SessionEvent::FinalSegment(FinalSegment {
                id: segment.id(),
                seq: segment.seq(),
                source: segment.source(),
                start: segment.start(),
                end: segment.end(),
                text: text.clone(),
            }),
            SegmentKind::Gap { reason } => SessionEvent::Gap {
                reason: *reason,
                start: segment.start(),
                duration: segment.duration(),
            },
        };
        // Published under the lock so events follow `seq` order.
        self.publish(event);
        true
    }

    fn gap(&self, reason: GapReason, start: Duration, end: Duration) -> bool {
        if end <= start {
            return true;
        }
        self.write(SegmentKind::Gap { reason }, None, start, end)
    }

    /// Gap over the audio received but not finalized.
    fn gap_unfinalized(&self, reason: GapReason) -> bool {
        let (start, end) = {
            let progress = self.lock();
            (progress.finalized_end, progress.captured_end)
        };
        self.gap(reason, start, end)
    }

    /// Gap over the audio handed to a transcriber that failed before finalizing it.
    fn gap_unconsumed(&self, reason: GapReason) -> bool {
        let (start, end) = {
            let progress = self.lock();
            (progress.finalized_end, progress.consumed_end)
        };
        self.gap(reason, start, end)
    }

    /// Applies a transcriber update; `false` once nothing more can be written.
    fn apply(&self, update: TranscriptUpdate) -> bool {
        match update {
            TranscriptUpdate::Partial {
                source,
                start,
                text,
            } => {
                if !self.lock().closed {
                    self.publish(SessionEvent::PartialSegment {
                        source: self.tag(source),
                        start,
                        text,
                    });
                }
                true
            }
            TranscriptUpdate::Final {
                source,
                start,
                end,
                text,
            } => self.write(SegmentKind::Final { text }, self.tag(source), start, end),
        }
    }

    fn captured(&self, end: Duration) {
        let mut progress = self.lock();
        progress.captured_end = progress.captured_end.max(end);
    }

    fn consumed(&self, end: Duration) {
        let mut progress = self.lock();
        progress.consumed_end = progress.consumed_end.max(end);
    }

    /// Publishes lag changes (US2.4).
    fn check_lag(&self) {
        let mut progress = self.lock();
        if progress.closed {
            return;
        }
        let lag = progress.captured_end.saturating_sub(progress.consumed_end);
        if !progress.lag_reported && lag > LAG_THRESHOLD {
            progress.lag_reported = true;
            self.publish(SessionEvent::TranscriptionLag { lag });
        } else if progress.lag_reported && lag <= LAG_RECOVERED {
            progress.lag_reported = false;
            self.publish(SessionEvent::TranscriptionLag {
                lag: Duration::ZERO,
            });
        }
    }

    fn close(&self) {
        self.lock().closed = true;
    }

    fn fatal(&self, reason: FailureReason) {
        self.close();
        (self.on_fatal)(reason);
    }
}

/// Audio waiting for the transcriber, bounded by [`BACKLOG_LIMIT`].
#[derive(Default)]
struct Backlog {
    chunks: VecDeque<AudioChunk>,
    duration: Duration,
}

impl Backlog {
    /// Queues `chunk`; on overflow drops the oldest audio and returns the dropped range.
    fn push(&mut self, chunk: AudioChunk) -> Option<(Duration, Duration)> {
        self.duration = self.duration.saturating_add(chunk_duration(&chunk));
        self.chunks.push_back(chunk);
        if self.duration <= BACKLOG_LIMIT {
            return None;
        }
        let keep = BACKLOG_LIMIT.saturating_sub(OVERLOAD_DROP);
        let mut dropped: Option<(Duration, Duration)> = None;
        while self.duration > keep {
            let Some(chunk) = self.pop() else { break };
            let (start, end) = (chunk.start, chunk_end(&chunk));
            dropped = Some(dropped.map_or((start, end), |(s, e)| (s.min(start), e.max(end))));
        }
        dropped
    }

    fn pop(&mut self) -> Option<AudioChunk> {
        let chunk = self.chunks.pop_front()?;
        self.duration = self.duration.saturating_sub(chunk_duration(&chunk));
        Some(chunk)
    }

    fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// Handle to one session's running transcription.
pub struct Transcription {
    handle: JoinHandle<()>,
    cancel: CancellationToken,
    recorder: Arc<Recorder>,
}

impl Transcription {
    /// Starts transcribing `chunks`. Must be called within a Tokio runtime.
    #[must_use]
    pub fn spawn(deps: TranscriptionDeps, chunks: AudioChunkStream) -> Self {
        let recorder = Arc::new(Recorder {
            session_id: deps.session_id,
            two_sources: deps.session_sources.len() > 1,
            session_sources: deps.session_sources,
            segments: deps.segments,
            events: deps.events,
            on_fatal: deps.on_fatal,
            progress: Mutex::new(Progress {
                next_seq: deps.first_seq,
                captured_end: deps.offset_base,
                consumed_end: deps.offset_base,
                finalized_end: deps.offset_base,
                ..Progress::default()
            }),
        });
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(run(
            Arc::clone(&recorder),
            chunks,
            deps.transcriber,
            deps.offset_base,
            cancel.clone(),
        ));
        Self {
            handle,
            cancel,
            recorder,
        }
    }

    /// Lets in-flight audio finish after the sources closed, for at most `deadline`; then
    /// cancels the transcriber and records the rest as `gap(cancelled_on_stop)` (SC-002).
    pub async fn drain(mut self, deadline: std::time::Duration) {
        if tokio::time::timeout(deadline, &mut self.handle)
            .await
            .is_err()
        {
            tracing::warn!(session = %self.recorder.session_id, "transcription drain deadline reached");
            self.stop(Some(GapReason::CancelledOnStop));
        }
    }

    /// Stops transcribing now; with `reason`, records the unfinalized audio as a gap.
    pub fn abort(self, reason: Option<GapReason>) {
        self.stop(reason);
    }

    fn stop(&self, reason: Option<GapReason>) {
        self.cancel.cancel();
        self.handle.abort();
        if let Some(reason) = reason {
            self.recorder.gap_unfinalized(reason);
        }
        self.recorder.close();
    }
}

/// Owned send slot on the transcriber input, or never when the input is closed.
async fn reserve(
    tx: Option<mpsc::Sender<AudioChunk>>,
) -> Result<mpsc::OwnedPermit<AudioChunk>, ()> {
    match tx {
        Some(tx) => tx.reserve_owned().await.map_err(|_| ()),
        None => std::future::pending().await,
    }
}

async fn run(
    recorder: Arc<Recorder>,
    mut chunks: AudioChunkStream,
    transcriber: Arc<dyn Transcriber>,
    offset_base: Duration,
    cancel: CancellationToken,
) {
    let mut backlog = Backlog::default();
    let mut sources_open = true;
    let mut failures = 0_u32;
    loop {
        let (input_tx, input_rx) = mpsc::channel(TRANSCRIBER_INPUT_CAPACITY);
        let (out_tx, mut out_rx) = mpsc::channel(TRANSCRIPT_QUEUE_CAPACITY);
        let mut input_tx = Some(input_tx);
        let run = transcriber.run(input_rx, out_tx, cancel.child_token());
        tokio::pin!(run);
        let mut result: Option<Result<(), ProviderError>> = None;
        let mut updates_open = true;
        while result.is_none() || updates_open {
            if !sources_open && backlog.is_empty() {
                // Everything was handed over: let the transcriber flush its last window.
                input_tx = None;
            }
            tokio::select! {
                chunk = chunks.recv(), if sources_open => match chunk {
                    Some(mut chunk) => {
                        chunk.start = offset_base.saturating_add(chunk.start);
                        recorder.captured(chunk_end(&chunk));
                        if let Some((start, end)) = backlog.push(chunk) {
                            tracing::warn!(session = %recorder.session_id, "transcription overloaded; oldest audio dropped");
                            if !recorder.gap(GapReason::Overload, start, end) {
                                return;
                            }
                        }
                        recorder.check_lag();
                    }
                    None => sources_open = false,
                },
                permit = reserve(input_tx.clone()), if !backlog.is_empty() => match permit {
                    Ok(permit) => {
                        if let Some(chunk) = backlog.pop() {
                            recorder.consumed(chunk_end(&chunk));
                            permit.send(chunk);
                        }
                        recorder.check_lag();
                    }
                    // The transcriber dropped its input; its run is ending.
                    Err(()) => input_tx = None,
                },
                update = out_rx.recv(), if updates_open => match update {
                    Some(update) => {
                        if matches!(update, TranscriptUpdate::Final { .. }) {
                            failures = 0;
                        }
                        if !recorder.apply(update) {
                            return;
                        }
                    }
                    None => updates_open = false,
                },
                finished = &mut run, if result.is_none() => result = Some(finished),
            }
        }
        let drained = !sources_open && backlog.is_empty() && input_tx.is_none();
        match result {
            Some(Ok(())) if drained => return,
            Some(Err(ProviderError::Cancelled)) => return,
            Some(outcome) => {
                tracing::warn!(
                    session = %recorder.session_id,
                    error = ?outcome.err(),
                    "transcriber stopped unexpectedly"
                );
                if !recorder.gap_unconsumed(GapReason::ProviderFailure) {
                    return;
                }
                failures += 1;
                if failures >= MAX_CONSECUTIVE_FAILURES {
                    recorder.fatal(FailureReason::NoTranscriber);
                    return;
                }
            }
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(start_ms: u64, secs: usize) -> AudioChunk {
        AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_millis(start_ms),
            samples: vec![0.0; 16_000 * secs],
        }
    }

    #[test]
    fn backlog_drops_the_oldest_half_on_overflow() {
        let mut backlog = Backlog::default();
        for second in 0..30 {
            assert_eq!(backlog.push(chunk(second * 1_000, 1)), None);
        }
        assert_eq!(
            backlog.push(chunk(30_000, 1)),
            Some((Duration::ZERO, Duration::from_secs(16)))
        );
        assert_eq!(backlog.duration, Duration::from_secs(15));
        assert_eq!(
            backlog.pop().map(|c| c.start),
            Some(Duration::from_secs(16))
        );
    }

    #[test]
    fn chunk_duration_uses_the_sample_rate() {
        assert_eq!(chunk_duration(&chunk(0, 2)), Duration::from_secs(2));
        assert_eq!(chunk_end(&chunk(500, 1)), Duration::from_millis(1_500));
    }
}
