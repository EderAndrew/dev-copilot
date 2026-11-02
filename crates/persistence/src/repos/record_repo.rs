//! [`RecordRepository`] over `records`, `record_evidence` and `record_changes`.
//!
//! A record is always written together with its history entries in one transaction, so the
//! history never misses a change (FR-026). Acceptance criteria and alternatives are stored as
//! JSON arrays of strings.

use std::sync::Arc;

use devcopilot_application::ports::{RecordRepository, RepositoryError};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{AnalysisRunId, ChangeId, ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::record::{
    Nature, Origin, RecordFlags, RecordKind, RecordSpec, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::record_change::{ChangeActor, ChangeField, RecordChange, RecordChangeSpec};
use devcopilot_domain::text::{ListItem, Rationale, RecordText};
use devcopilot_domain::time::Timestamp;
use rusqlite::{Connection, ErrorCode, Row, Transaction, params};

use crate::db::{Database, storage_error};

const COLUMNS: &str = "id, session_id, kind, text, nature, status, rationale, origin, \
    origin_provider_id, origin_analysis_run_id, acceptance_criteria, alternatives, \
    conflicts_with, duplicate_of, created_at, updated_at";

fn corrupt<E>(_: E) -> RepositoryError {
    RepositoryError::Corrupt("records")
}

fn write_error(err: &rusqlite::Error) -> RepositoryError {
    match err.sqlite_error() {
        Some(e)
            if e.code == ErrorCode::ConstraintViolation
                && e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
        {
            RepositoryError::NotFound
        }
        _ => storage_error(err),
    }
}

fn list_json(items: &[Redacted<ListItem>]) -> String {
    let values: Vec<&str> = items.iter().map(|item| item.expose().as_str()).collect();
    serde_json::to_string(&values).unwrap_or_else(|_| "[]".to_owned())
}

fn parse_list(raw: &str) -> Result<Vec<Redacted<ListItem>>, RepositoryError> {
    let values: Vec<String> = serde_json::from_str(raw).map_err(corrupt)?;
    values
        .into_iter()
        .map(|value| ListItem::parse(value).map(Redacted::new).map_err(corrupt))
        .collect()
}

fn timestamp(millis: i64) -> Result<Timestamp, RepositoryError> {
    Timestamp::from_unix_millis(millis).map_err(corrupt)
}

/// Raw `records` row; evidence is read separately.
struct RecordRow {
    id: String,
    session_id: String,
    kind: String,
    text: String,
    nature: String,
    status: String,
    rationale: String,
    origin: String,
    origin_provider_id: Option<String>,
    origin_analysis_run_id: Option<String>,
    acceptance_criteria: String,
    alternatives: String,
    conflicts_with: Option<String>,
    duplicate_of: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl RecordRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            session_id: row.get(1)?,
            kind: row.get(2)?,
            text: row.get(3)?,
            nature: row.get(4)?,
            status: row.get(5)?,
            rationale: row.get(6)?,
            origin: row.get(7)?,
            origin_provider_id: row.get(8)?,
            origin_analysis_run_id: row.get(9)?,
            acceptance_criteria: row.get(10)?,
            alternatives: row.get(11)?,
            conflicts_with: row.get(12)?,
            duplicate_of: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    }

    fn into_record(self, evidence: Vec<SegmentId>) -> Result<TechnicalRecord, RepositoryError> {
        let record_id = |raw: Option<String>| {
            raw.as_deref()
                .map(RecordId::parse)
                .transpose()
                .map_err(corrupt)
        };
        let origin = match (
            self.origin.as_str(),
            self.origin_provider_id.as_deref(),
            self.origin_analysis_run_id.as_deref(),
        ) {
            ("user", _, _) => Origin::User,
            ("ai", Some(provider), Some(run)) => Origin::Ai {
                provider_id: ProviderId::parse(provider).map_err(corrupt)?,
                analysis_run_id: AnalysisRunId::parse(run).map_err(corrupt)?,
            },
            _ => return Err(RepositoryError::Corrupt("records")),
        };
        Ok(TechnicalRecord::restore(RecordSpec {
            id: RecordId::parse(&self.id).map_err(corrupt)?,
            session_id: SessionId::parse(&self.session_id).map_err(corrupt)?,
            kind: RecordKind::parse(&self.kind).map_err(corrupt)?,
            text: Redacted::new(RecordText::parse(self.text).map_err(corrupt)?),
            nature: Nature::parse(&self.nature).map_err(corrupt)?,
            status: RecordStatus::parse(&self.status).map_err(corrupt)?,
            rationale: Redacted::new(Rationale::parse(self.rationale).map_err(corrupt)?),
            origin,
            evidence,
            acceptance_criteria: parse_list(&self.acceptance_criteria)?,
            alternatives: parse_list(&self.alternatives)?,
            flags: RecordFlags {
                conflicts_with: record_id(self.conflicts_with)?,
                duplicate_of: record_id(self.duplicate_of)?,
            },
            created_at: timestamp(self.created_at)?,
            updated_at: timestamp(self.updated_at)?,
        }))
    }
}

fn evidence_of(conn: &Connection, record: &str) -> Result<Vec<SegmentId>, RepositoryError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT e.segment_id FROM record_evidence e JOIN segments s ON s.id = e.segment_id \
             WHERE e.record_id = ?1 ORDER BY s.seq",
        )
        .map_err(|e| storage_error(&e))?;
    let ids = stmt
        .query_map(params![record], |row| row.get::<_, String>(0))
        .map_err(|e| storage_error(&e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_error(&e))?;
    ids.iter()
        .map(|id| SegmentId::parse(id).map_err(corrupt))
        .collect()
}

fn insert_changes(tx: &Transaction<'_>, changes: &[RecordChange]) -> Result<(), RepositoryError> {
    for change in changes {
        let run = match change.actor() {
            ChangeActor::Ai { analysis_run_id } => Some(analysis_run_id.to_string()),
            ChangeActor::User => None,
        };
        tx.execute(
            "INSERT INTO record_changes \
             (id, record_id, actor, actor_analysis_run_id, field, old_value, new_value, at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                change.id().to_string(),
                change.record_id().to_string(),
                change.actor().code(),
                run,
                change.field().code(),
                change.old_value().map(|v| v.expose().as_str()),
                change.new_value().map(|v| v.expose().as_str()),
                change.at().unix_millis(),
            ],
        )
        .map_err(|e| write_error(&e))?;
    }
    Ok(())
}

/// Technical records stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteRecordRepository {
    db: Arc<Database>,
}

impl SqliteRecordRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn query(&self, filter: &str, param: String) -> Result<Vec<TechnicalRecord>, RepositoryError> {
        self.db.with_conn(|conn| {
            let rows = {
                let mut stmt = conn
                    .prepare_cached(&format!(
                        "SELECT {COLUMNS} FROM records WHERE {filter} ORDER BY created_at, id"
                    ))
                    .map_err(|e| storage_error(&e))?;
                stmt.query_map(params![param], RecordRow::read)
                    .map_err(|e| storage_error(&e))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| storage_error(&e))?
            };
            rows.into_iter()
                .map(|row| {
                    let evidence = evidence_of(conn, &row.id)?;
                    row.into_record(evidence)
                })
                .collect()
        })
    }
}

impl RecordRepository for SqliteRecordRepository {
    fn insert(
        &self,
        record: &TechnicalRecord,
        creation: &RecordChange,
    ) -> Result<(), RepositoryError> {
        let (provider, run) = match record.origin() {
            Origin::Ai {
                provider_id,
                analysis_run_id,
            } => (
                Some(provider_id.to_string()),
                Some(analysis_run_id.to_string()),
            ),
            Origin::User => (None, None),
        };
        let flags = record.flags();
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            let id = record.id().to_string();
            tx.execute(
                &format!(
                    "INSERT INTO records ({COLUMNS}) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"
                ),
                params![
                    id,
                    record.session_id().to_string(),
                    record.kind().code(),
                    record.text().expose().as_str(),
                    record.nature().code(),
                    record.status().code(),
                    record.rationale().expose().as_str(),
                    record.origin().code(),
                    provider,
                    run,
                    list_json(record.acceptance_criteria()),
                    list_json(record.alternatives()),
                    flags.conflicts_with.map(|r| r.to_string()),
                    flags.duplicate_of.map(|r| r.to_string()),
                    record.created_at().unix_millis(),
                    record.updated_at().unix_millis(),
                ],
            )
            .map_err(|e| write_error(&e))?;
            for segment in record.evidence() {
                tx.execute(
                    "INSERT INTO record_evidence (record_id, segment_id) VALUES (?1, ?2)",
                    params![id, segment.to_string()],
                )
                .map_err(|e| write_error(&e))?;
            }
            insert_changes(&tx, std::slice::from_ref(creation))?;
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn update(
        &self,
        record: &TechnicalRecord,
        changes: &[RecordChange],
    ) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            let updated = tx
                .execute(
                    "UPDATE records SET text = ?2, status = ?3, rationale = ?4, \
                     acceptance_criteria = ?5, alternatives = ?6, updated_at = ?7 WHERE id = ?1",
                    params![
                        record.id().to_string(),
                        record.text().expose().as_str(),
                        record.status().code(),
                        record.rationale().expose().as_str(),
                        list_json(record.acceptance_criteria()),
                        list_json(record.alternatives()),
                        record.updated_at().unix_millis(),
                    ],
                )
                .map_err(|e| storage_error(&e))?;
            if updated == 0 {
                return Err(RepositoryError::NotFound);
            }
            insert_changes(&tx, changes)?;
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn get(&self, id: RecordId) -> Result<TechnicalRecord, RepositoryError> {
        self.query("id = ?1", id.to_string())?
            .pop()
            .ok_or(RepositoryError::NotFound)
    }

    fn list_by_session(&self, session: SessionId) -> Result<Vec<TechnicalRecord>, RepositoryError> {
        self.query("session_id = ?1", session.to_string())
    }

    fn history(&self, record: RecordId) -> Result<Vec<RecordChange>, RepositoryError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT id, record_id, actor, actor_analysis_run_id, field, old_value, \
                     new_value, at FROM record_changes WHERE record_id = ?1 ORDER BY at, rowid",
                )
                .map_err(|e| storage_error(&e))?;
            let rows = stmt
                .query_map(params![record.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                })
                .map_err(|e| storage_error(&e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| storage_error(&e))?;
            rows.into_iter()
                .map(|(id, record_id, actor, run, field, old, new, at)| {
                    let actor = match (actor.as_str(), run) {
                        ("user", _) => ChangeActor::User,
                        ("ai", Some(run)) => ChangeActor::Ai {
                            analysis_run_id: AnalysisRunId::parse(&run).map_err(corrupt)?,
                        },
                        _ => return Err(RepositoryError::Corrupt("record_changes")),
                    };
                    Ok(RecordChange::restore(RecordChangeSpec {
                        id: ChangeId::parse(&id).map_err(corrupt)?,
                        record_id: RecordId::parse(&record_id).map_err(corrupt)?,
                        actor,
                        field: ChangeField::parse(&field).map_err(corrupt)?,
                        old_value: old.map(Redacted::new),
                        new_value: new.map(Redacted::new),
                        at: timestamp(at)?,
                    }))
                })
                .collect()
        })
    }
}
