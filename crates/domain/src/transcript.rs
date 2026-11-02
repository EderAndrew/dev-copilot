//! Transcript segments (data-model.md "TranscriptSegment").
//!
//! Only finals and gaps are entities: partial text lives in memory and in UI events and is
//! never persisted (FR-017).

use crate::Redacted;
use crate::audio_source::AudioSourceKind;
use crate::error::{DomainError, InputProblem};
use crate::ids::{SegmentId, SessionId};
use crate::text::SegmentText;
use crate::time::Duration;

/// Why the transcript has a gap (data-model.md "TranscriptSegment").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GapReason {
    /// The user paused capture.
    Paused,
    /// Audio was dropped because the pipeline was overloaded (FR-019).
    Overload,
    /// The transcription provider failed.
    ProviderFailure,
    /// Transcription still running when stop's drain deadline expired.
    CancelledOnStop,
    /// A source stopped delivering audio.
    SourceLost,
    /// The app stopped during the session (appended on startup recovery).
    Interrupted,
}

code_enum!(GapReason, "reason", {
    Paused => "paused",
    Overload => "overload",
    ProviderFailure => "provider_failure",
    CancelledOnStop => "cancelled_on_stop",
    SourceLost => "source_lost",
    Interrupted => "interrupted",
});

/// What a segment holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentKind {
    /// Finalized speech.
    Final {
        /// Recognized text (session content).
        text: Redacted<SegmentText>,
    },
    /// Audio that has no transcript; its length is `end - start`.
    Gap {
        /// Why.
        reason: GapReason,
    },
}

/// Fields of a segment before validation, also used to restore stored rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentSpec {
    /// Identifier.
    pub id: SegmentId,
    /// Owning session.
    pub session_id: SessionId,
    /// Monotonic position within the session.
    pub seq: u64,
    /// Final text or gap.
    pub kind: SegmentKind,
    /// Source of the audio.
    pub source: Option<AudioSourceKind>,
    /// Offset of the start from the session start.
    pub start: Duration,
    /// Offset of the end from the session start.
    pub end: Duration,
}

/// A persisted final segment or gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSegment {
    id: SegmentId,
    session_id: SessionId,
    seq: u64,
    kind: SegmentKind,
    source: Option<AudioSourceKind>,
    start: Duration,
    end: Duration,
}

impl TranscriptSegment {
    /// Validates a segment of a session captured from `session_sources`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] when `end < start` (field `end_offset`), when a final of
    /// a two-source session has no source (FR-016) or the source is not one of the session's
    /// (field `source`).
    pub fn new(
        spec: SegmentSpec,
        session_sources: &[AudioSourceKind],
    ) -> Result<Self, DomainError> {
        if spec.end < spec.start {
            return Err(DomainError::invalid(
                "end_offset",
                InputProblem::Inconsistent,
            ));
        }
        match spec.source {
            Some(source) if !session_sources.contains(&source) => {
                return Err(DomainError::invalid("source", InputProblem::Inconsistent));
            }
            None if session_sources.len() > 1 && matches!(spec.kind, SegmentKind::Final { .. }) => {
                return Err(DomainError::invalid("source", InputProblem::Empty));
            }
            _ => {}
        }
        Ok(Self::restore(spec))
    }

    /// Rebuilds a stored segment without the session-dependent checks of [`Self::new`].
    #[must_use]
    pub fn restore(spec: SegmentSpec) -> Self {
        Self {
            id: spec.id,
            session_id: spec.session_id,
            seq: spec.seq,
            kind: spec.kind,
            source: spec.source,
            start: spec.start,
            end: spec.end.max(spec.start),
        }
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> SegmentId {
        self.id
    }

    /// Owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Position within the session.
    #[must_use]
    pub const fn seq(&self) -> u64 {
        self.seq
    }

    /// Final text or gap.
    #[must_use]
    pub const fn kind(&self) -> &SegmentKind {
        &self.kind
    }

    /// Source of the audio, if known.
    #[must_use]
    pub const fn source(&self) -> Option<AudioSourceKind> {
        self.source
    }

    /// Start offset.
    #[must_use]
    pub const fn start(&self) -> Duration {
        self.start
    }

    /// End offset.
    #[must_use]
    pub const fn end(&self) -> Duration {
        self.end
    }

    /// Length (`end - start`).
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Text of a final segment.
    #[must_use]
    pub const fn text(&self) -> Option<&Redacted<SegmentText>> {
        match &self.kind {
            SegmentKind::Final { text } => Some(text),
            SegmentKind::Gap { .. } => None,
        }
    }

    /// Reason of a gap.
    #[must_use]
    pub const fn gap_reason(&self) -> Option<GapReason> {
        match self.kind {
            SegmentKind::Gap { reason } => Some(reason),
            SegmentKind::Final { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIC: AudioSourceKind = AudioSourceKind::Microphone;
    const SYSTEM: AudioSourceKind = AudioSourceKind::System;

    fn text(value: &str) -> SegmentKind {
        SegmentKind::Final {
            text: Redacted::new(SegmentText::parse(value).unwrap()),
        }
    }

    fn spec(
        kind: SegmentKind,
        source: Option<AudioSourceKind>,
        start: u64,
        end: u64,
    ) -> SegmentSpec {
        SegmentSpec {
            id: SegmentId::new(),
            session_id: SessionId::new(),
            seq: 3,
            kind,
            source,
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
        }
    }

    #[test]
    fn codes_round_trip() {
        for reason in [
            GapReason::Paused,
            GapReason::Overload,
            GapReason::ProviderFailure,
            GapReason::CancelledOnStop,
            GapReason::SourceLost,
            GapReason::Interrupted,
        ] {
            assert_eq!(GapReason::parse(reason.code()), Ok(reason));
        }
        assert!(GapReason::parse("partial").is_err());
    }

    #[test]
    fn final_segment_exposes_text_and_offsets() {
        let segment =
            TranscriptSegment::new(spec(text("olá"), Some(MIC), 1_000, 2_500), &[MIC]).unwrap();
        assert_eq!(segment.seq(), 3);
        assert_eq!(segment.text().map(|t| t.expose().as_str()), Some("olá"));
        assert_eq!(segment.gap_reason(), None);
        assert_eq!(segment.duration(), Duration::from_millis(1_500));
    }

    #[test]
    fn gap_has_reason_and_no_text() {
        let gap = TranscriptSegment::new(
            spec(
                SegmentKind::Gap {
                    reason: GapReason::Overload,
                },
                None,
                0,
                4_000,
            ),
            &[MIC, SYSTEM],
        )
        .unwrap();
        assert_eq!(gap.gap_reason(), Some(GapReason::Overload));
        assert!(gap.text().is_none());
        assert_eq!(gap.duration(), Duration::from_secs(4));
    }

    #[test]
    fn zero_length_segments_are_allowed() {
        assert!(
            TranscriptSegment::new(
                spec(
                    SegmentKind::Gap {
                        reason: GapReason::Interrupted
                    },
                    None,
                    5,
                    5
                ),
                &[MIC]
            )
            .is_ok()
        );
    }

    #[test]
    fn end_before_start_is_rejected() {
        assert_eq!(
            TranscriptSegment::new(spec(text("x"), Some(MIC), 2_000, 1_999), &[MIC]),
            Err(DomainError::invalid(
                "end_offset",
                InputProblem::Inconsistent
            ))
        );
    }

    #[test]
    fn source_is_required_for_finals_with_two_sources() {
        assert_eq!(
            TranscriptSegment::new(spec(text("x"), None, 0, 1), &[MIC, SYSTEM]),
            Err(DomainError::invalid("source", InputProblem::Empty))
        );
        assert!(
            TranscriptSegment::new(spec(text("x"), Some(SYSTEM), 0, 1), &[MIC, SYSTEM]).is_ok()
        );
        // One source: the source is implied and may be omitted.
        assert!(TranscriptSegment::new(spec(text("x"), None, 0, 1), &[MIC]).is_ok());
    }

    #[test]
    fn source_must_belong_to_the_session() {
        assert_eq!(
            TranscriptSegment::new(spec(text("x"), Some(SYSTEM), 0, 1), &[MIC]),
            Err(DomainError::invalid("source", InputProblem::Inconsistent))
        );
    }

    #[test]
    fn debug_output_hides_text() {
        let segment =
            TranscriptSegment::new(spec(text("segredo"), Some(MIC), 0, 1), &[MIC]).unwrap();
        assert!(!format!("{segment:?}").contains("segredo"));
    }
}
