//! Encrypted database open/create (ADR-004).
//!
//! The 256-bit key lives only in the OS credential store. Without the store, nothing is opened
//! or written (FR-039a).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use devcopilot_application::ports::{RepositoryError, SecretStore, SecretStoreError};
use devcopilot_domain::Redacted;
use devcopilot_domain::provider::CredentialRef;
use rusqlite::Connection;

use crate::migrations;

/// Credential-store entry holding the database key.
pub const DB_KEY_REF: &str = "db:key";

/// File name of the database inside the app data directory.
pub const DB_FILE_NAME: &str = "devcopilot.db";

const KEY_LEN: usize = 32;

pub(crate) fn storage_error(err: &rusqlite::Error) -> RepositoryError {
    // rusqlite messages describe SQLite failures, never bound parameter values.
    RepositoryError::Storage(err.to_string())
}

fn key_ref() -> Result<CredentialRef, RepositoryError> {
    CredentialRef::parse(DB_KEY_REF).map_err(|_| RepositoryError::Storage("invalid key ref".into()))
}

fn from_secret_error(err: SecretStoreError) -> RepositoryError {
    match err {
        SecretStoreError::Unavailable => RepositoryError::SecretStoreUnavailable,
        SecretStoreError::Backend(detail) => RepositoryError::Storage(detail),
    }
}

/// Obtains the key, generating and storing one only for a database that does not exist yet.
fn database_key(
    secrets: &dyn SecretStore,
    db_exists: bool,
) -> Result<Redacted<Vec<u8>>, RepositoryError> {
    secrets.check_available().map_err(from_secret_error)?;
    let reference = key_ref()?;
    match secrets.get(&reference).map_err(from_secret_error)? {
        Some(key) if key.expose().len() == KEY_LEN => Ok(key),
        Some(_) => Err(RepositoryError::Storage(
            "stored database key has an unexpected length".into(),
        )),
        // An existing database without its key is unreadable; never replace the key.
        None if db_exists => Err(RepositoryError::SecretStoreUnavailable),
        None => {
            let mut key = vec![0u8; KEY_LEN];
            getrandom::fill(&mut key)
                .map_err(|_| RepositoryError::Storage("OS random generator failed".into()))?;
            let key = Redacted::new(key);
            secrets.set(&reference, &key).map_err(from_secret_error)?;
            Ok(key)
        }
    }
}

#[cfg(unix)]
fn restrict_to_user(path: &Path, mode: u32) -> Result<(), RepositoryError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|err| RepositoryError::Storage(format!("cannot set permissions: {}", err.kind())))
}

// Windows: the per-user app data directory is already private to the account.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn restrict_to_user(_path: &Path, _mode: u32) -> Result<(), RepositoryError> {
    Ok(())
}

/// Encrypted SQLite database shared by the repositories.
#[derive(Debug)]
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    /// Opens (or creates) `<data_dir>/devcopilot.db`, applies the key and pragmas, and runs
    /// pending migrations.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::SecretStoreUnavailable`] when the credential store is unavailable or
    /// the key of an existing database is missing; [`RepositoryError::Storage`] when the file
    /// cannot be created or decrypted, or a migration fails.
    pub fn open_in(data_dir: &Path, secrets: &dyn SecretStore) -> Result<Self, RepositoryError> {
        Self::open(&data_dir.join(DB_FILE_NAME), secrets)
    }

    /// Opens (or creates) the database at `path`. See [`Database::open_in`].
    ///
    /// # Errors
    ///
    /// See [`Database::open_in`].
    pub fn open(path: &Path, secrets: &dyn SecretStore) -> Result<Self, RepositoryError> {
        let db_exists = path.exists();
        // Resolve the key before touching the file system: no store, no file (FR-039a).
        let key = database_key(secrets, db_exists)?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|err| {
                RepositoryError::Storage(format!("cannot create data dir: {}", err.kind()))
            })?;
            // Also covers the WAL and SHM files SQLite creates next to the database.
            restrict_to_user(dir, 0o700)?;
        }
        let conn = Connection::open(path).map_err(|e| storage_error(&e))?;
        restrict_to_user(path, 0o600)?;
        apply_key(&conn, &key)?;
        configure(&conn)?;
        migrations::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: path.to_owned(),
        })
    }

    /// Location of the database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` with exclusive access to the connection.
    ///
    /// # Errors
    ///
    /// Whatever `f` returns.
    pub fn with_conn<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, RepositoryError>,
    ) -> Result<T, RepositoryError> {
        let mut conn: MutexGuard<'_, Connection> =
            self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut conn)
    }
}

fn apply_key(conn: &Connection, key: &Redacted<Vec<u8>>) -> Result<(), RepositoryError> {
    let hex: String = key.expose().iter().map(|b| format!("{b:02x}")).collect();
    // Raw key via execute_batch: `pragma_update` re-quotes the value and derives a different
    // key (ADR-004, spike S3).
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))
        .map_err(|e| storage_error(&e))?;
    // The first read fails if the key does not match the file.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| RepositoryError::Storage("database cannot be decrypted".into()))?;
    Ok(())
}

fn configure(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch("PRAGMA secure_delete = ON; PRAGMA foreign_keys = ON;")
        .map_err(|e| storage_error(&e))?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|e| storage_error(&e))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(RepositoryError::Storage(format!(
            "journal mode `{mode}` instead of WAL"
        )));
    }
    Ok(())
}
