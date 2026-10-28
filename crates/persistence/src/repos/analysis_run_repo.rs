//! [`AnalysisRunRepository`] over the `analysis_runs` table.

use std::sync::Arc;

use devcopilot_application::ports::{AnalysisRunRepository, RepositoryError};
use devcopilot_domain::analysis_run::{
    AnalysisOutcome, AnalysisRun, AnalysisRunSpec, AnalysisTrigger,
};
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, SessionId};
use devcopilot_domain::time::Timestamp;
use rusqlite::{ErrorCode, params};

use crate::db::{Database, storage_error};

fn corrupt<E>(_: E) -> RepositoryError {
    RepositoryError::Corrupt("analysis_runs")
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Analysis runs stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteAnalysisRunRepository {
    db: Arc<Database>,
}

impl SqliteAnalysisRunRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl AnalysisRunRepository for SqliteAnalysisRunRepository {
    fn insert(&self, run: &AnalysisRun) -> Result<(), RepositoryError> {
        let (from, to) = run.segment_range();
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO analysis_runs (id, session_id, provider_id, trigger, seq_from, \
                 seq_to, outcome, created_count, started_at, finished_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    run.id().to_string(),
                    run.session_id().to_string(),
                    run.provider_id().to_string(),
                    run.trigger().code(),
                    to_i64(from),
                    to_i64(to),
                    run.outcome().map(AnalysisOutcome::code),
                    run.outcome().map_or(0, AnalysisOutcome::created),
                    run.started_at().unix_millis(),
                    run.finished_at().map(|t| t.unix_millis()),
                ],
            )
            .map_err(|err| match err.sqlite_error() {
                Some(e)
                    if e.code == ErrorCode::ConstraintViolation
                        && e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
                {
                    RepositoryError::NotFound
                }
                _ => storage_error(&err),
            })?;
            Ok(())
        })
    }

    fn update(&self, run: &AnalysisRun) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let updated = conn
                .execute(
                    "UPDATE analysis_runs SET outcome = ?2, created_count = ?3, finished_at = ?4 \
                     WHERE id = ?1",
                    params![
                        run.id().to_string(),
                        run.outcome().map(AnalysisOutcome::code),
                        run.outcome().map_or(0, AnalysisOutcome::created),
                        run.finished_at().map(|t| t.unix_millis()),
                    ],
                )
                .map_err(|e| storage_error(&e))?;
            if updated == 0 {
                return Err(RepositoryError::NotFound);
            }
            Ok(())
        })
    }

    fn list_by_session(&self, session: SessionId) -> Result<Vec<AnalysisRun>, RepositoryError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT id, session_id, provider_id, trigger, seq_from, seq_to, outcome, \
                     created_count, started_at, finished_at FROM analysis_runs \
                     WHERE session_id = ?1 ORDER BY started_at, id",
                )
                .map_err(|e| storage_error(&e))?;
            let rows = stmt
                .query_map(params![session.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                    ))
                })
                .map_err(|e| storage_error(&e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| storage_error(&e))?;
            rows.into_iter()
                .map(
                    |(
                        id,
                        session,
                        provider,
                        trigger,
                        from,
                        to,
                        outcome,
                        created,
                        started,
                        finished,
                    )| {
                        let created = u32::try_from(created).map_err(corrupt)?;
                        Ok(AnalysisRun::restore(AnalysisRunSpec {
                            id: AnalysisRunId::parse(&id).map_err(corrupt)?,
                            session_id: SessionId::parse(&session).map_err(corrupt)?,
                            provider_id: ProviderId::parse(&provider).map_err(corrupt)?,
                            trigger: AnalysisTrigger::parse(&trigger).map_err(corrupt)?,
                            seq_from: u64::try_from(from).map_err(corrupt)?,
                            seq_to: u64::try_from(to).map_err(corrupt)?,
                            outcome: outcome
                                .map(|code| AnalysisOutcome::parse(&code, created))
                                .transpose()
                                .map_err(corrupt)?,
                            started_at: Timestamp::from_unix_millis(started).map_err(corrupt)?,
                            finished_at: finished
                                .map(Timestamp::from_unix_millis)
                                .transpose()
                                .map_err(corrupt)?,
                        }))
                    },
                )
                .collect()
        })
    }
}
