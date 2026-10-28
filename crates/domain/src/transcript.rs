//! Transcript vocabulary shared by ports and events.
//!
//! The `TranscriptSegment` entity is added with US2 (T068).

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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
