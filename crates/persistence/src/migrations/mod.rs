//! Forward-only schema migrations tracked by `PRAGMA user_version`.

use devcopilot_application::ports::RepositoryError;
use rusqlite::Connection;

use crate::db::storage_error;

/// Migrations in order; index + 1 is the resulting `user_version`.
const MIGRATIONS: &[&str] = &[include_str!("v1.sql")];

/// Schema version written by the latest migration.
pub const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

/// Current `user_version`.
///
/// # Errors
///
/// [`RepositoryError::Storage`] when the pragma cannot be read.
pub fn current_version(conn: &Connection) -> Result<i64, RepositoryError> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| storage_error(&e))
}

/// Applies every pending migration, each in its own transaction. Idempotent.
///
/// # Errors
///
/// [`RepositoryError::Storage`] when the database was written by a newer app version or a
/// migration fails (the failing migration is rolled back).
pub fn migrate(conn: &Connection) -> Result<(), RepositoryError> {
    let current = current_version(conn)?;
    if current > LATEST_VERSION {
        return Err(RepositoryError::Storage(format!(
            "database schema v{current} is newer than this app (v{LATEST_VERSION})"
        )));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = i64::try_from(index).unwrap_or(i64::MAX).saturating_add(1);
        if version <= current {
            continue;
        }
        let script = format!("BEGIN IMMEDIATE;\n{sql}\nPRAGMA user_version = {version};\nCOMMIT;");
        if let Err(err) = conn.execute_batch(&script) {
            // Best effort: the transaction may already be gone if BEGIN itself failed.
            let _ = conn.execute_batch("ROLLBACK;");
            return Err(RepositoryError::Storage(format!(
                "migration v{version} failed: {err}"
            )));
        }
    }
    Ok(())
}
