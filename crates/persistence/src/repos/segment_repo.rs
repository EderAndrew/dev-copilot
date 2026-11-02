//! [`SegmentRepository`] over the `segments` table.
//!
//! Each append is one autocommitted statement, durable once it returns (WAL), so finals
//! written before a crash or a later failure are kept (FR-017, US2.3).

use std::sync::Arc;

use devcopilot_application::ports::{RepositoryError, SegmentRepository};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{SegmentId, SessionId};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
use rusqlite::{ErrorCode, Row, params};

use crate::db::{Database, storage_error};

/// Raw row; mapped to [`TranscriptSegment`] through domain parsing.
struct SegmentRow {
    id: String,
    session_id: String,
    seq: i64,
    kind: String,
    text: Option<String>,
    gap_reason: Option<String>,
    source: Option<String>,
    start_ms: i64,
    end_ms: i64,
}

impl SegmentRow {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            session_id: row.get(1)?,
            seq: row.get(2)?,
            kind: row.get(3)?,
            text: row.get(4)?,
            gap_reason: row.get(5)?,
            source: row.get(6)?,
            start_ms: row.get(7)?,
            end_ms: row.get(8)?,
        })
    }

    fn into_segment(self) -> Result<TranscriptSegment, RepositoryError> {
        let corrupt = |_| RepositoryError::Corrupt("segments");
        let millis = |value: i64| {
            u64::try_from(value)
                .map(Duration::from_millis)
                .map_err(|_| RepositoryError::Corrupt("segments"))
        };
        let kind = match (self.kind.as_str(), self.text, self.gap_reason.as_deref()) {
            ("final", Some(text), None) => SegmentKind::Final {
                text: Redacted::new(SegmentText::parse(text).map_err(corrupt)?),
            },
            ("gap", None, Some(reason)) => SegmentKind::Gap {
                reason: GapReason::parse(reason).map_err(corrupt)?,
            },
            _ => return Err(RepositoryError::Corrupt("segments")),
        };
        Ok(TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::parse(&self.id).map_err(corrupt)?,
            session_id: SessionId::parse(&self.session_id).map_err(corrupt)?,
            seq: u64::try_from(self.seq).map_err(|_| RepositoryError::Corrupt("segments"))?,
            kind,
            source: self
                .source
                .as_deref()
                .map(AudioSourceKind::parse)
                .transpose()
                .map_err(corrupt)?,
            start: millis(self.start_ms)?,
            end: millis(self.end_ms)?,
        }))
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Transcript segments stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteSegmentRepository {
    db: Arc<Database>,
}

impl SqliteSegmentRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl SegmentRepository for SqliteSegmentRepository {
    fn append(&self, segment: &TranscriptSegment) -> Result<(), RepositoryError> {
        let (kind, text, gap_reason) = match segment.kind() {
            SegmentKind::Final { text } => ("final", Some(text.expose().as_str()), None),
            SegmentKind::Gap { reason } => ("gap", None, Some(reason.code())),
        };
        self.db.with_conn(|conn| {
            conn.prepare_cached(
                "INSERT INTO segments \
                 (id, session_id, seq, kind, text, gap_reason, source, start_ms, end_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .and_then(|mut stmt| {
                stmt.execute(params![
                    segment.id().to_string(),
                    segment.session_id().to_string(),
                    to_i64(segment.seq()),
                    kind,
                    text,
                    gap_reason,
                    segment.source().map(AudioSourceKind::code),
                    to_i64(segment.start().as_millis()),
                    to_i64(segment.end().as_millis()),
                ])
            })
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

    fn list_by_session(
        &self,
        session: SessionId,
    ) -> Result<Vec<TranscriptSegment>, RepositoryError> {
        self.db.with_conn(|conn| {
            let rows = {
                let mut stmt = conn
                    .prepare_cached(
                        "SELECT id, session_id, seq, kind, text, gap_reason, source, start_ms, end_ms \
                         FROM segments WHERE session_id = ?1 ORDER BY seq",
                    )
                    .map_err(|e| storage_error(&e))?;
                stmt.query_map(params![session.to_string()], SegmentRow::read)
                    .map_err(|e| storage_error(&e))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| storage_error(&e))?
            };
            rows.into_iter().map(SegmentRow::into_segment).collect()
        })
    }

    fn next_seq(&self, session: SessionId) -> Result<u64, RepositoryError> {
        self.db.with_conn(|conn| {
            let next: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(seq) + 1, 0) FROM segments WHERE session_id = ?1",
                    params![session.to_string()],
                    |row| row.get(0),
                )
                .map_err(|e| storage_error(&e))?;
            u64::try_from(next).map_err(|_| RepositoryError::Corrupt("segments"))
        })
    }
}
