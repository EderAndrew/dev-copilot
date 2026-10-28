//! Session events (contracts/ipc-events.md), expressed with domain types.
//!
//! The composition root assigns `seq` and maps these to the IPC JSON shape.
//! `question_answered` is added with US5 (T116).

use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, SegmentId};
use devcopilot_domain::provider::Locality;
use devcopilot_domain::record::TechnicalRecord;
use devcopilot_domain::session::{FailureReason, SessionState};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::GapReason;

/// Health of one capture source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    /// Delivering audio.
    Ok,
    /// Stopped delivering audio.
    Lost,
    /// The OS permission was withdrawn (FR-010).
    PermissionRevoked,
}

/// Outcome reported by `analysis_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisStatus {
    /// In flight.
    Running,
    /// Finished; `created` proposals were added.
    Ok {
        /// Number of proposals created.
        created: u32,
    },
    /// Output failed validation and was discarded (FR-030).
    InvalidOutput,
    /// Provider failed; existing records untouched.
    ProviderError,
    /// Skipped by rate or token limits (FR-028).
    RateLimited,
}

/// Stage shown in `provider_in_use` (FR-050).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderStage {
    /// Speech to text.
    Transcription,
    /// Incremental and final analysis.
    Analysis,
    /// Contextual questions.
    Questions,
}

/// A persisted final segment as carried by `final_segment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalSegment {
    /// Segment id.
    pub id: SegmentId,
    /// Monotonic position within the session.
    pub seq: u64,
    /// Source, when the session has two sources.
    pub source: Option<AudioSourceKind>,
    /// Start offset.
    pub start: Duration,
    /// End offset.
    pub end: Duration,
    /// Text.
    pub text: Redacted<SegmentText>,
}

/// Event emitted by the core for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// The session moved to `state`.
    StateChanged {
        /// New state.
        state: SessionState,
        /// Present iff `state` is `failed`.
        failure: Option<FailureReason>,
    },
    /// What the capture indicator must show; derived from core state (SC-002).
    CaptureIndicator {
        /// Capture is on.
        active: bool,
        /// Capture is paused.
        paused: bool,
        /// Sources currently captured.
        sources: Vec<AudioSourceKind>,
    },
    /// Provisional text; never persisted.
    PartialSegment {
        /// Source of the audio.
        source: Option<AudioSourceKind>,
        /// Start offset.
        start: Duration,
        /// Provisional text.
        text: Redacted<String>,
    },
    /// A final segment was persisted.
    FinalSegment(FinalSegment),
    /// A gap was recorded.
    Gap {
        /// Why.
        reason: GapReason,
        /// Start offset.
        start: Duration,
        /// Length.
        duration: Duration,
    },
    /// A source changed health.
    SourceStatus {
        /// Which source.
        source: AudioSourceKind,
        /// New status.
        status: SourceStatus,
    },
    /// Analysis progress.
    AnalysisStatus {
        /// Run.
        run_id: AnalysisRunId,
        /// Status.
        status: AnalysisStatus,
    },
    /// Analysis created a proposed record.
    RecordProposed(Box<TechnicalRecord>),
    /// Provider used for a stage (emitted at start and on every change).
    ProviderInUse {
        /// Stage.
        stage: ProviderStage,
        /// Local or remote.
        locality: Locality,
        /// Provider.
        provider_id: ProviderId,
    },
    /// Transcription lag above 15 s, or zero once recovered.
    TranscriptionLag {
        /// Current lag.
        lag: Duration,
    },
    /// Persisting content failed.
    StorageError {
        /// i18n key describing the failure.
        message_key: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_text() {
        let event = SessionEvent::PartialSegment {
            source: None,
            start: Duration::ZERO,
            text: Redacted::new("conteúdo sigiloso".into()),
        };
        assert!(!format!("{event:?}").contains("sigiloso"));
    }
}
