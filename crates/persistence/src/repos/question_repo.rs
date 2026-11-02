//! [`QuestionRepository`] over `questions` and `question_evidence`.
//!
//! An answer and its evidence are written in one transaction. Evidence rows keep the order the
//! answer cites them in (`rowid`); a segment or record deleted later takes its row with it.

use std::sync::Arc;

use devcopilot_application::ports::{QuestionRepository, RepositoryError};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{ProviderId, QuestionId, RecordId, SegmentId, SessionId};
use devcopilot_domain::question::{
    Certainty, ContextQuestion, EvidenceRef, QuestionSpec, QuestionStatus,
};
use devcopilot_domain::text::{AnswerText, QuestionText};
use devcopilot_domain::time::Timestamp;
use rusqlite::{Connection, ErrorCode, Row, Transaction, params};

use crate::db::{Database, storage_error};

const COLUMNS: &str =
    "id, session_id, question, answer, certainty, status, provider_id, asked_at, answered_at";

fn corrupt<E>(_: E) -> RepositoryError {
    RepositoryError::Corrupt("questions")
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

fn timestamp(millis: i64) -> Result<Timestamp, RepositoryError> {
    Timestamp::from_unix_millis(millis).map_err(corrupt)
}

/// Raw `questions` row; evidence is read separately.
struct QuestionRow {
    id: String,
    session_id: String,
    question: String,
    answer: Option<String>,
    certainty: Option<String>,
    status: String,
    provider_id: Option<String>,
    asked_at: i64,
    answered_at: Option<i64>,
}

impl QuestionRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            session_id: row.get(1)?,
            question: row.get(2)?,
            answer: row.get(3)?,
            certainty: row.get(4)?,
            status: row.get(5)?,
            provider_id: row.get(6)?,
            asked_at: row.get(7)?,
            answered_at: row.get(8)?,
        })
    }

    fn into_question(self, evidence: Vec<EvidenceRef>) -> Result<ContextQuestion, RepositoryError> {
        Ok(ContextQuestion::restore(QuestionSpec {
            id: QuestionId::parse(&self.id).map_err(corrupt)?,
            session_id: SessionId::parse(&self.session_id).map_err(corrupt)?,
            question: Redacted::new(QuestionText::parse(self.question).map_err(corrupt)?),
            answer: self
                .answer
                .map(|answer| AnswerText::parse(answer).map(Redacted::new))
                .transpose()
                .map_err(corrupt)?,
            certainty: self
                .certainty
                .as_deref()
                .map(Certainty::parse)
                .transpose()
                .map_err(corrupt)?,
            evidence,
            status: QuestionStatus::parse(&self.status).map_err(corrupt)?,
            provider_id: self
                .provider_id
                .as_deref()
                .map(ProviderId::parse)
                .transpose()
                .map_err(corrupt)?,
            asked_at: timestamp(self.asked_at)?,
            answered_at: self.answered_at.map(timestamp).transpose()?,
        }))
    }
}

fn evidence_of(conn: &Connection, question: &str) -> Result<Vec<EvidenceRef>, RepositoryError> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT segment_id, record_id FROM question_evidence WHERE question_id = ?1 \
             ORDER BY rowid",
        )
        .map_err(|e| storage_error(&e))?;
    let rows = stmt
        .query_map(params![question], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .map_err(|e| storage_error(&e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_error(&e))?;
    rows.into_iter()
        .map(|row| match row {
            (Some(segment), None) => Ok(EvidenceRef::Segment(
                SegmentId::parse(&segment).map_err(corrupt)?,
            )),
            (None, Some(record)) => Ok(EvidenceRef::Record(
                RecordId::parse(&record).map_err(corrupt)?,
            )),
            _ => Err(RepositoryError::Corrupt("question_evidence")),
        })
        .collect()
}

fn insert_evidence(
    tx: &Transaction<'_>,
    question: &ContextQuestion,
) -> Result<(), RepositoryError> {
    let id = question.id().to_string();
    tx.execute(
        "DELETE FROM question_evidence WHERE question_id = ?1",
        params![id],
    )
    .map_err(|e| storage_error(&e))?;
    for evidence in question.evidence() {
        let (segment, record) = match evidence {
            EvidenceRef::Segment(segment) => (Some(segment.to_string()), None),
            EvidenceRef::Record(record) => (None, Some(record.to_string())),
        };
        tx.execute(
            "INSERT INTO question_evidence (question_id, segment_id, record_id) \
             VALUES (?1, ?2, ?3)",
            params![id, segment, record],
        )
        .map_err(|e| write_error(&e))?;
    }
    Ok(())
}

/// Contextual questions stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteQuestionRepository {
    db: Arc<Database>,
}

impl SqliteQuestionRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn query(&self, filter: &str, param: String) -> Result<Vec<ContextQuestion>, RepositoryError> {
        self.db.with_conn(|conn| {
            let rows = {
                let mut stmt = conn
                    .prepare_cached(&format!(
                        "SELECT {COLUMNS} FROM questions WHERE {filter} ORDER BY asked_at, id"
                    ))
                    .map_err(|e| storage_error(&e))?;
                stmt.query_map(params![param], QuestionRow::read)
                    .map_err(|e| storage_error(&e))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| storage_error(&e))?
            };
            rows.into_iter()
                .map(|row| {
                    let evidence = evidence_of(conn, &row.id)?;
                    row.into_question(evidence)
                })
                .collect()
        })
    }
}

impl QuestionRepository for SqliteQuestionRepository {
    fn insert(&self, question: &ContextQuestion) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            tx.execute(
                &format!(
                    "INSERT INTO questions ({COLUMNS}) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
                ),
                params![
                    question.id().to_string(),
                    question.session_id().to_string(),
                    question.question().expose().as_str(),
                    question.answer_text().map(|a| a.expose().as_str()),
                    question.certainty().map(Certainty::code),
                    question.status().code(),
                    question.provider_id().map(|p| p.to_string()),
                    question.asked_at().unix_millis(),
                    question.answered_at().map(|t| t.unix_millis()),
                ],
            )
            .map_err(|e| write_error(&e))?;
            insert_evidence(&tx, question)?;
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn update(&self, question: &ContextQuestion) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            let updated = tx
                .execute(
                    "UPDATE questions SET answer = ?2, certainty = ?3, status = ?4, \
                     answered_at = ?5 WHERE id = ?1",
                    params![
                        question.id().to_string(),
                        question.answer_text().map(|a| a.expose().as_str()),
                        question.certainty().map(Certainty::code),
                        question.status().code(),
                        question.answered_at().map(|t| t.unix_millis()),
                    ],
                )
                .map_err(|e| storage_error(&e))?;
            if updated == 0 {
                return Err(RepositoryError::NotFound);
            }
            insert_evidence(&tx, question)?;
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn get(&self, id: QuestionId) -> Result<ContextQuestion, RepositoryError> {
        self.query("id = ?1", id.to_string())?
            .pop()
            .ok_or(RepositoryError::NotFound)
    }

    fn list_by_session(&self, session: SessionId) -> Result<Vec<ContextQuestion>, RepositoryError> {
        self.query("session_id = ?1", session.to_string())
    }
}
