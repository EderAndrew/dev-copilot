//! One call to an analysis provider (data-model.md "AnalysisRun").
//!
//! Runs record provenance of AI proposals and let the application bound cost and frequency
//! (FR-028).

use crate::error::{DomainError, InputProblem};
use crate::ids::{AnalysisRunId, ProviderId, SessionId};
use crate::session::SessionState;
use crate::time::Timestamp;

/// Why a run was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalysisTrigger {
    /// Scheduled while the session captures (FR-021).
    Incremental,
    /// Requested by the user on an ended or failed session (FR-033).
    Final,
}

code_enum!(AnalysisTrigger, "trigger", {
    Incremental => "incremental",
    Final => "final",
});

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalysisOutcome {
    /// Output accepted; `created` proposals were stored.
    Ok {
        /// Proposals created.
        created: u32,
    },
    /// Output failed validation and was discarded (FR-030).
    InvalidOutput,
    /// The provider failed, timed out or was cancelled.
    ProviderError,
    /// Not sent: frequency or token limits reached (FR-028).
    RateLimited,
}

impl AnalysisOutcome {
    /// Stable code, without the created count.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ok { .. } => "ok",
            Self::InvalidOutput => "invalid_output",
            Self::ProviderError => "provider_error",
            Self::RateLimited => "rate_limited",
        }
    }

    /// Rebuilds an outcome from its code and created count.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] on `outcome` for unknown codes.
    pub fn parse(code: &str, created: u32) -> Result<Self, DomainError> {
        match code {
            "ok" => Ok(Self::Ok { created }),
            "invalid_output" => Ok(Self::InvalidOutput),
            "provider_error" => Ok(Self::ProviderError),
            "rate_limited" => Ok(Self::RateLimited),
            _ => Err(DomainError::invalid("outcome", InputProblem::Malformed)),
        }
    }

    /// Proposals created (zero unless `Ok`).
    #[must_use]
    pub const fn created(self) -> u32 {
        match self {
            Self::Ok { created } => created,
            _ => 0,
        }
    }
}

/// Fields of a run, also used to restore stored rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRunSpec {
    /// Identifier.
    pub id: AnalysisRunId,
    /// Analysed session.
    pub session_id: SessionId,
    /// Provider called.
    pub provider_id: ProviderId,
    /// Why.
    pub trigger: AnalysisTrigger,
    /// `seq` of the first segment analysed.
    pub seq_from: u64,
    /// `seq` of the last segment analysed.
    pub seq_to: u64,
    /// How it ended; `None` while in flight.
    pub outcome: Option<AnalysisOutcome>,
    /// When it started.
    pub started_at: Timestamp,
    /// When it ended.
    pub finished_at: Option<Timestamp>,
}

/// An analysis run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRun {
    spec: AnalysisRunSpec,
}

impl AnalysisRun {
    /// Starts a run over segments `seq_from..=seq_to` of a session in `session_state`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] on `segment_range` when `seq_to < seq_from`;
    /// [`DomainError::InvalidTransition`] for a final run of a session that has not ended or
    /// failed (FR-033), or an incremental run of one that has never captured or is over.
    pub fn start(
        id: AnalysisRunId,
        session_id: SessionId,
        session_state: SessionState,
        provider_id: ProviderId,
        trigger: AnalysisTrigger,
        (seq_from, seq_to): (u64, u64),
        started_at: Timestamp,
    ) -> Result<Self, DomainError> {
        if seq_to < seq_from {
            return Err(DomainError::invalid(
                "segment_range",
                InputProblem::Inconsistent,
            ));
        }
        let allowed = match trigger {
            AnalysisTrigger::Final => session_state.is_terminal(),
            AnalysisTrigger::Incremental => matches!(
                session_state,
                SessionState::Capturing | SessionState::Paused | SessionState::Stopping
            ),
        };
        if !allowed {
            return Err(DomainError::InvalidTransition {
                from: session_state.code(),
                action: trigger.code(),
            });
        }
        Ok(Self::restore(AnalysisRunSpec {
            id,
            session_id,
            provider_id,
            trigger,
            seq_from,
            seq_to,
            outcome: None,
            started_at,
            finished_at: None,
        }))
    }

    /// Rebuilds a stored run.
    #[must_use]
    pub const fn restore(spec: AnalysisRunSpec) -> Self {
        Self { spec }
    }

    /// Records how the run ended.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] when it already ended.
    pub fn finish(&mut self, outcome: AnalysisOutcome, at: Timestamp) -> Result<(), DomainError> {
        if let Some(previous) = self.spec.outcome {
            return Err(DomainError::InvalidTransition {
                from: previous.code(),
                action: "finish",
            });
        }
        self.spec.outcome = Some(outcome);
        self.spec.finished_at = Some(at.max(self.spec.started_at));
        Ok(())
    }

    /// All fields.
    #[must_use]
    pub const fn spec(&self) -> &AnalysisRunSpec {
        &self.spec
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> AnalysisRunId {
        self.spec.id
    }

    /// Analysed session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.spec.session_id
    }

    /// Provider called.
    #[must_use]
    pub const fn provider_id(&self) -> ProviderId {
        self.spec.provider_id
    }

    /// Why.
    #[must_use]
    pub const fn trigger(&self) -> AnalysisTrigger {
        self.spec.trigger
    }

    /// `seq` range analysed.
    #[must_use]
    pub const fn segment_range(&self) -> (u64, u64) {
        (self.spec.seq_from, self.spec.seq_to)
    }

    /// How it ended.
    #[must_use]
    pub const fn outcome(&self) -> Option<AnalysisOutcome> {
        self.spec.outcome
    }

    /// When it started.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.spec.started_at
    }

    /// When it ended.
    #[must_use]
    pub const fn finished_at(&self) -> Option<Timestamp> {
        self.spec.finished_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).unwrap()
    }

    fn start(
        state: SessionState,
        trigger: AnalysisTrigger,
        range: (u64, u64),
    ) -> Result<AnalysisRun, DomainError> {
        AnalysisRun::start(
            AnalysisRunId::new(),
            SessionId::new(),
            state,
            ProviderId::new(),
            trigger,
            range,
            at(1_000),
        )
    }

    #[test]
    fn incremental_runs_need_a_live_session() {
        for state in [
            SessionState::Capturing,
            SessionState::Paused,
            SessionState::Stopping,
        ] {
            assert!(start(state, AnalysisTrigger::Incremental, (0, 5)).is_ok());
        }
        for state in [
            SessionState::Idle,
            SessionState::Starting,
            SessionState::Ended,
            SessionState::Failed,
        ] {
            assert!(start(state, AnalysisTrigger::Incremental, (0, 5)).is_err());
        }
    }

    #[test]
    fn final_runs_need_an_ended_or_failed_session() {
        assert!(start(SessionState::Ended, AnalysisTrigger::Final, (0, 0)).is_ok());
        assert!(start(SessionState::Failed, AnalysisTrigger::Final, (0, 0)).is_ok());
        assert_eq!(
            start(SessionState::Capturing, AnalysisTrigger::Final, (0, 0)),
            Err(DomainError::InvalidTransition {
                from: "capturing",
                action: "final"
            })
        );
    }

    #[test]
    fn range_must_be_ordered() {
        assert!(
            start(
                SessionState::Capturing,
                AnalysisTrigger::Incremental,
                (4, 3)
            )
            .is_err()
        );
    }

    #[test]
    fn finishes_once() {
        let mut run = start(
            SessionState::Capturing,
            AnalysisTrigger::Incremental,
            (0, 5),
        )
        .unwrap();
        run.finish(AnalysisOutcome::Ok { created: 2 }, at(2_000))
            .unwrap();
        assert_eq!(run.outcome(), Some(AnalysisOutcome::Ok { created: 2 }));
        assert_eq!(run.finished_at(), Some(at(2_000)));
        assert!(
            run.finish(AnalysisOutcome::ProviderError, at(3_000))
                .is_err()
        );
    }

    #[test]
    fn outcome_codes_round_trip() {
        for outcome in [
            AnalysisOutcome::Ok { created: 4 },
            AnalysisOutcome::InvalidOutput,
            AnalysisOutcome::ProviderError,
            AnalysisOutcome::RateLimited,
        ] {
            assert_eq!(
                AnalysisOutcome::parse(outcome.code(), outcome.created()),
                Ok(outcome)
            );
        }
        assert!(AnalysisOutcome::parse("deleted", 0).is_err());
    }
}
