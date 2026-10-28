//! User actions on technical records (FR-024–FR-026, FR-032).
//!
//! Every change here is made by the user and stored together with its history entry. The AI
//! never reaches these use cases: analysis only creates proposals (`analysis_service`).

use std::sync::{Arc, Mutex, PoisonError};

use devcopilot_domain::ids::{RecordId, SessionId};
use devcopilot_domain::record::{Actor, RecordStatus, TechnicalRecord};
use devcopilot_domain::record_change::RecordChange;

use crate::error::ApplicationError;
use crate::ports::{Clock, RecordRepository};

/// Record review use cases.
pub struct RecordService {
    records: Arc<dyn RecordRepository>,
    clock: Arc<dyn Clock>,
    /// Serializes read-modify-write of records.
    writes: Mutex<()>,
}

impl RecordService {
    /// Creates the service.
    #[must_use]
    pub fn new(records: Arc<dyn RecordRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            records,
            clock,
            writes: Mutex::new(()),
        }
    }

    /// Changes a record's status by explicit user action and records it in the history.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::InvalidState`] for transitions
    /// outside the table (data-model.md); [`ApplicationError::Storage`].
    pub fn set_status(
        &self,
        id: RecordId,
        status: RecordStatus,
    ) -> Result<TechnicalRecord, ApplicationError> {
        let _guard = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let mut record = self.records.get(id)?;
        let change = record.set_status(Actor::User, status, self.clock.now())?;
        self.records.update(&record, &[change])?;
        tracing::info!(record = %id, status = status.code(), "record status changed");
        Ok(record)
    }

    /// Every record of `session`, oldest first.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn list(&self, session: SessionId) -> Result<Vec<TechnicalRecord>, ApplicationError> {
        Ok(self.records.list_by_session(session)?)
    }

    /// History of `record`, oldest first (FR-026).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn history(&self, record: RecordId) -> Result<Vec<RecordChange>, ApplicationError> {
        self.records.get(record)?;
        Ok(self.records.history(record)?)
    }
}

#[cfg(test)]
mod tests {
    use devcopilot_domain::Redacted;
    use devcopilot_domain::ids::{AnalysisRunId, ProviderId, SegmentId};
    use devcopilot_domain::record::{Nature, Origin, RecordFlags, RecordKind, RecordSpec};
    use devcopilot_domain::record_change::{ChangeActor, ChangeField};
    use devcopilot_domain::text::{Rationale, RecordText};
    use devcopilot_domain::time::{Duration, Timestamp};

    use super::*;
    use crate::ports::RepositoryError;
    use crate::testing::{FakeClock, InMemoryRecordRepository};

    fn proposed(session: SessionId) -> TechnicalRecord {
        let at = Timestamp::from_unix_millis(1_000).unwrap();
        TechnicalRecord::new(RecordSpec {
            id: RecordId::new(),
            session_id: session,
            kind: RecordKind::Decision,
            text: Redacted::new(RecordText::parse("Usar Postgres").unwrap()),
            nature: Nature::Observed,
            status: RecordStatus::Proposed,
            rationale: Redacted::new(Rationale::default()),
            origin: Origin::Ai {
                provider_id: ProviderId::new(),
                analysis_run_id: AnalysisRunId::new(),
            },
            evidence: vec![SegmentId::new()],
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            flags: RecordFlags::default(),
            created_at: at,
            updated_at: at,
        })
        .unwrap()
    }

    fn fixture() -> (RecordService, Arc<InMemoryRecordRepository>, Arc<FakeClock>) {
        let repo = Arc::new(InMemoryRecordRepository::default());
        let clock = Arc::new(FakeClock::at(1_000));
        (RecordService::new(repo.clone(), clock.clone()), repo, clock)
    }

    #[test]
    fn confirming_records_history_with_time() {
        let (service, repo, clock) = fixture();
        let record = proposed(SessionId::new());
        repo.seed(record.clone());
        clock.advance(Duration::from_secs(60));
        let updated = service
            .set_status(record.id(), RecordStatus::Confirmed)
            .unwrap();
        assert_eq!(updated.status(), RecordStatus::Confirmed);
        assert_eq!(
            repo.get(record.id()).unwrap().status(),
            RecordStatus::Confirmed
        );
        let history = service.history(record.id()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].field(), ChangeField::Created);
        assert!(matches!(history[0].actor(), ChangeActor::Ai { .. }));
        assert_eq!(history[1].field(), ChangeField::Status);
        assert_eq!(history[1].actor(), ChangeActor::User);
        assert_eq!(history[1].at().unix_millis(), 61_000);
    }

    #[test]
    fn invalid_transitions_change_nothing() {
        let (service, repo, _) = fixture();
        let record = proposed(SessionId::new());
        repo.seed(record.clone());
        let err = service
            .set_status(record.id(), RecordStatus::Proposed)
            .unwrap_err();
        assert!(matches!(err, ApplicationError::InvalidState(_)));
        assert_eq!(repo.changes().len(), 1);
    }

    #[test]
    fn unknown_records_are_not_found() {
        let (service, _, _) = fixture();
        assert_eq!(
            service.set_status(RecordId::new(), RecordStatus::Confirmed),
            Err(ApplicationError::NotFound)
        );
        assert_eq!(
            service.history(RecordId::new()),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn storage_failure_is_reported() {
        let (service, repo, _) = fixture();
        let record = proposed(SessionId::new());
        repo.seed(record.clone());
        repo.fail_writes_with(Some(RepositoryError::Storage("disk full".into())));
        assert!(matches!(
            service.set_status(record.id(), RecordStatus::Rejected),
            Err(ApplicationError::Storage(_))
        ));
        assert_eq!(
            repo.get(record.id()).unwrap().status(),
            RecordStatus::Proposed
        );
    }
}
