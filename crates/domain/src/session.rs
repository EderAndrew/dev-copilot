//! Session vocabulary shared by ports and events.
//!
//! The `Session` entity and its transition functions are added with US1 (T046); the states and
//! failure reasons below are the contract vocabulary (data-model.md, ipc-events.md).

/// Lifecycle state of a session (data-model.md state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Created, not started.
    Idle,
    /// Checking permissions and opening sources.
    Starting,
    /// Capturing and transcribing.
    Capturing,
    /// Capture and processing halted by the user.
    Paused,
    /// Sources closed; draining in-flight transcription.
    Stopping,
    /// Finished normally.
    Ended,
    /// Finished with a [`FailureReason`].
    Failed,
}

code_enum!(SessionState, "state", {
    Idle => "idle",
    Starting => "starting",
    Capturing => "capturing",
    Paused => "paused",
    Stopping => "stopping",
    Ended => "ended",
    Failed => "failed",
});

impl SessionState {
    /// States counted by the single-active-session invariant (FR-006).
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Capturing | Self::Paused | Self::Stopping
        )
    }

    /// States in which capture can no longer happen.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Failed)
    }
}

/// Why a session ended in [`SessionState::Failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Microphone or system-audio permission denied or revoked.
    PermissionDenied,
    /// Every selected source became unavailable.
    SourceUnavailable,
    /// No usable transcription provider (FR-045a).
    NoTranscriber,
    /// Writing to the encrypted database failed.
    StorageError,
    /// The OS credential store is unavailable (FR-039a).
    SecretStoreUnavailable,
    /// The app stopped while the session was active (FR-007).
    Interrupted,
    /// Unexpected internal error.
    Internal,
}

code_enum!(FailureReason, "failure", {
    PermissionDenied => "permission_denied",
    SourceUnavailable => "source_unavailable",
    NoTranscriber => "no_transcriber",
    StorageError => "storage_error",
    SecretStoreUnavailable => "secret_store_unavailable",
    Interrupted => "interrupted",
    Internal => "internal",
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_and_terminal_sets() {
        use SessionState::*;
        let active: Vec<_> = [Idle, Starting, Capturing, Paused, Stopping, Ended, Failed]
            .into_iter()
            .filter(|s| s.is_active())
            .collect();
        assert_eq!(active, [Starting, Capturing, Paused, Stopping]);
        assert!(Ended.is_terminal() && Failed.is_terminal() && !Idle.is_terminal());
    }

    #[test]
    fn codes_round_trip() {
        assert_eq!(SessionState::parse("paused"), Ok(SessionState::Paused));
        assert_eq!(
            FailureReason::parse(FailureReason::SecretStoreUnavailable.code()),
            Ok(FailureReason::SecretStoreUnavailable)
        );
        assert!(SessionState::parse("Paused").is_err());
    }
}
