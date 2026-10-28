//! [`SessionRepository`] over the `sessions` and `session_sources` tables.
//!
//! Rows are mapped back through [`Session::restore`], so stored data that breaks a domain
//! invariant surfaces as [`RepositoryError::Corrupt`] instead of a half-valid entity.

use std::sync::Arc;

use devcopilot_application::ports::{RepositoryError, SessionRepository};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, Session, SessionSnapshot, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::GapReason;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::db::{Database, storage_error};

const COLUMNS: &str =
    "id, title, language, state, failure, created_at, started_at, ended_at, active_duration_ms";

/// Raw row; mapped to [`Session`] through domain validation.
struct SessionRow {
    id: String,
    title: String,
    language: String,
    state: String,
    failure: Option<String>,
    created_at: i64,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    active_duration_ms: i64,
}

impl SessionRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            language: row.get(2)?,
            state: row.get(3)?,
            failure: row.get(4)?,
            created_at: row.get(5)?,
            started_at: row.get(6)?,
            ended_at: row.get(7)?,
            active_duration_ms: row.get(8)?,
        })
    }

    fn into_session(self, sources: Vec<String>) -> Result<Session, RepositoryError> {
        let corrupt = |_| RepositoryError::Corrupt("sessions");
        let timestamp = |millis: i64| Timestamp::from_unix_millis(millis).map_err(corrupt);
        Session::restore(SessionSnapshot {
            id: SessionId::parse(&self.id).map_err(corrupt)?,
            title: Title::parse(self.title).map_err(corrupt)?,
            language: SessionLanguage::parse(&self.language).map_err(corrupt)?,
            state: SessionState::parse(&self.state).map_err(corrupt)?,
            sources: sources
                .iter()
                .map(|code| AudioSourceKind::parse(code))
                .collect::<Result<_, _>>()
                .map_err(corrupt)?,
            created_at: timestamp(self.created_at)?,
            started_at: self.started_at.map(timestamp).transpose()?,
            ended_at: self.ended_at.map(timestamp).transpose()?,
            active_duration: Duration::from_millis(
                u64::try_from(self.active_duration_ms)
                    .map_err(|_| RepositoryError::Corrupt("sessions"))?,
            ),
            failure: self
                .failure
                .as_deref()
                .map(FailureReason::parse)
                .transpose()
                .map_err(corrupt)?,
        })
        .map_err(corrupt)
    }
}

fn duration_ms(session: &Session) -> i64 {
    i64::try_from(session.active_duration().as_millis()).unwrap_or(i64::MAX)
}

fn sources_of(conn: &Connection, id: &str) -> Result<Vec<String>, RepositoryError> {
    let mut stmt = conn
        .prepare_cached("SELECT kind FROM session_sources WHERE session_id = ?1 ORDER BY kind")
        .map_err(|e| storage_error(&e))?;
    stmt.query_map(params![id], |row| row.get(0))
        .map_err(|e| storage_error(&e))?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| storage_error(&e))
}

fn select(
    conn: &Connection,
    filter: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<Session>, RepositoryError> {
    let rows = {
        let mut stmt = conn
            .prepare(&format!("SELECT {COLUMNS} FROM sessions {filter}"))
            .map_err(|e| storage_error(&e))?;
        stmt.query_map(args, SessionRow::read)
            .map_err(|e| storage_error(&e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| storage_error(&e))?
    };
    rows.into_iter()
        .map(|row| {
            let sources = sources_of(conn, &row.id)?;
            row.into_session(sources)
        })
        .collect()
}

fn update_row(tx: &Transaction<'_>, session: &Session) -> Result<(), RepositoryError> {
    let changed = tx
        .execute(
            "UPDATE sessions SET title = ?2, language = ?3, state = ?4, failure = ?5, \
             started_at = ?6, ended_at = ?7, active_duration_ms = ?8 WHERE id = ?1",
            params![
                session.id().to_string(),
                session.title().as_str(),
                session.language().code(),
                session.state().code(),
                session.failure().map(FailureReason::code),
                session.started_at().map(|t| t.unix_millis()),
                session.ended_at().map(|t| t.unix_millis()),
                duration_ms(session),
            ],
        )
        .map_err(|e| storage_error(&e))?;
    if changed == 0 {
        return Err(RepositoryError::NotFound);
    }
    Ok(())
}

/// Sessions stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteSessionRepository {
    db: Arc<Database>,
}

impl SqliteSessionRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl SessionRepository for SqliteSessionRepository {
    fn create(&self, session: &Session) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            let id = session.id().to_string();
            tx.execute(
                &format!(
                    "INSERT INTO sessions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
                ),
                params![
                    id,
                    session.title().as_str(),
                    session.language().code(),
                    session.state().code(),
                    session.failure().map(FailureReason::code),
                    session.created_at().unix_millis(),
                    session.started_at().map(|t| t.unix_millis()),
                    session.ended_at().map(|t| t.unix_millis()),
                    duration_ms(session),
                ],
            )
            .map_err(|e| storage_error(&e))?;
            for kind in session.sources() {
                tx.execute(
                    "INSERT INTO session_sources (session_id, kind) VALUES (?1, ?2)",
                    params![id, kind.code()],
                )
                .map_err(|e| storage_error(&e))?;
            }
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn get(&self, id: SessionId) -> Result<Session, RepositoryError> {
        self.db.with_conn(|conn| {
            let id = id.to_string();
            let row = conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1"),
                    params![id],
                    SessionRow::read,
                )
                .optional()
                .map_err(|e| storage_error(&e))?
                .ok_or(RepositoryError::NotFound)?;
            let sources = sources_of(conn, &id)?;
            row.into_session(sources)
        })
    }

    fn update(&self, session: &Session) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            update_row(&tx, session)?;
            tx.commit().map_err(|e| storage_error(&e))
        })
    }

    fn list(&self) -> Result<Vec<Session>, RepositoryError> {
        self.db
            .with_conn(|conn| select(conn, "ORDER BY created_at DESC, id DESC", []))
    }

    fn list_active(&self) -> Result<Vec<Session>, RepositoryError> {
        self.db.with_conn(|conn| {
            select(
                conn,
                "WHERE state IN ('starting', 'capturing', 'paused', 'stopping') ORDER BY created_at",
                [],
            )
        })
    }

    fn save_interrupted(&self, session: &Session) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| storage_error(&e))?;
            update_row(&tx, session)?;
            if session.started_at().is_some() {
                // Zero-length marker after the last persisted segment (US2.3).
                tx.execute(
                    "INSERT INTO segments (id, session_id, seq, kind, gap_reason, start_ms, end_ms) \
                     SELECT ?1, ?2, COALESCE(MAX(seq) + 1, 0), 'gap', ?3, \
                            COALESCE(MAX(end_ms), 0), COALESCE(MAX(end_ms), 0) \
                     FROM segments WHERE session_id = ?2",
                    params![
                        SegmentId::new().to_string(),
                        session.id().to_string(),
                        GapReason::Interrupted.code(),
                    ],
                )
                .map_err(|e| storage_error(&e))?;
            }
            tx.commit().map_err(|e| storage_error(&e))
        })
    }
}
