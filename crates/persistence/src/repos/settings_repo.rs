//! [`SettingsRepository`] over the `settings` table.

use std::sync::Arc;

use devcopilot_application::ports::{RepositoryError, SettingsRepository};
use rusqlite::{OptionalExtension, params};

use crate::db::{Database, storage_error};

/// Key/value settings stored in the encrypted database.
#[derive(Debug, Clone)]
pub struct SqliteSettingsRepository {
    db: Arc<Database>,
}

impl SqliteSettingsRepository {
    /// Creates the repository.
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

impl SettingsRepository for SqliteSettingsRepository {
    fn get(&self, key: &str) -> Result<Option<String>, RepositoryError> {
        self.db.with_conn(|conn| {
            conn.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| storage_error(&e))
        })
    }

    fn set(&self, key: &str, value: &str) -> Result<(), RepositoryError> {
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(|e| storage_error(&e))
        })
    }
}
