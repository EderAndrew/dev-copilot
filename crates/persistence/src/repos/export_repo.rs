//! [`ExportRepository`] over the `exports` table.
//!
//! Formats and content are stored as comma-separated codes. The file path is never stored
//! (data-model.md "ExportRecord"); rows go with their session (`ON DELETE CASCADE`).

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ports::{ExportRepository, RepositoryError};
use devcopilot_domain::export::{ExportContent, ExportFormat, ExportRecord};
use devcopilot_domain::ids::{ExportId, SessionId};
use devcopilot_domain::time::Timestamp;
use rusqlite::{ErrorCode, params};

use crate::db::{Database, storage_error};

fn corrupt<E>(_: E) -> RepositoryError {
    RepositoryError::Corrupt("exports")
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

fn codes<T: Copy>(values: &BTreeSet<T>, code: fn(T) -> &'static str) -> String {
    values
        .iter()
        .map(|v| code(*v))
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_codes<T: Ord, E>(
    column: &str,
    parse: fn(&str) -> Result<T, E>,
) -> Result<BTreeSet<T>, RepositoryError> {
    column
        .split(',')
        .filter(|code| !code.is_empty())
        .map(|code| parse(code).map_err(corrupt))
        .collect()
}

/// Export events stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteExportRepository {
    db: Arc<Database>,
}

impl SqliteExportRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl ExportRepository for SqliteExportRepository {
    fn insert(&self, export: &ExportRecord) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO exports (id, session_id, formats, content, at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    export.id().to_string(),
                    export.session_id().to_string(),
                    codes(export.formats(), ExportFormat::code),
                    codes(export.content(), ExportContent::code),
                    export.at().unix_millis(),
                ],
            )
            .map_err(|e| write_error(&e))?;
            Ok(())
        })
    }

    fn list_by_session(&self, session: SessionId) -> Result<Vec<ExportRecord>, RepositoryError> {
        self.db.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, formats, content, at FROM exports WHERE session_id = ?1 \
                     ORDER BY at, rowid",
                )
                .map_err(|e| storage_error(&e))?;
            let rows = stmt
                .query_map(params![session.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .map_err(|e| storage_error(&e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| storage_error(&e))?;
            rows.into_iter()
                .map(|(id, formats, content, at)| {
                    ExportRecord::new(
                        ExportId::parse(&id).map_err(corrupt)?,
                        session,
                        parse_codes(&formats, ExportFormat::parse)?,
                        parse_codes(&content, ExportContent::parse)?,
                        Timestamp::from_unix_millis(at).map_err(corrupt)?,
                    )
                    .map_err(corrupt)
                })
                .collect()
        })
    }
}
