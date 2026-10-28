//! Migrations apply idempotently on fresh and existing databases (T027).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use devcopilot_application::ports::RepositoryError;
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_persistence::Database;
use devcopilot_persistence::migrations::{LATEST_VERSION, current_version, migrate};

const TABLES: [&str; 13] = [
    "sessions",
    "session_sources",
    "segments",
    "records",
    "record_evidence",
    "record_changes",
    "analysis_runs",
    "questions",
    "question_evidence",
    "providers",
    "provider_authorizations",
    "exports",
    "settings",
];

fn table_names(db: &Database) -> Vec<String> {
    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        Ok(stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap())
    })
    .unwrap()
}

#[test]
fn fresh_database_gets_every_table_at_latest_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap();

    let names = table_names(&db);
    for table in TABLES {
        assert!(names.iter().any(|n| n == table), "missing table {table}");
    }
    let version = db.with_conn(|conn| current_version(conn)).unwrap();
    assert_eq!(version, LATEST_VERSION);
}

#[test]
fn rerunning_migrations_is_a_no_op_and_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    {
        let db = Database::open_in(dir.path(), &secrets).unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', 'v')", [])
                .unwrap();
            migrate(conn)
        })
        .unwrap();
    }

    let db = Database::open_in(dir.path(), &secrets).unwrap();
    let (version, value): (i64, String) = db
        .with_conn(|conn| {
            Ok((
                current_version(conn)?,
                conn.query_row("SELECT value FROM settings WHERE key = 'k'", [], |row| {
                    row.get(0)
                })
                .unwrap(),
            ))
        })
        .unwrap();
    assert_eq!(version, LATEST_VERSION);
    assert_eq!(value, "v");
}

#[test]
fn database_from_a_newer_app_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    {
        let db = Database::open_in(dir.path(), &secrets).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch(&format!("PRAGMA user_version = {}", LATEST_VERSION + 1))
                .unwrap();
            Ok(())
        })
        .unwrap();
    }

    assert!(matches!(
        Database::open_in(dir.path(), &secrets),
        Err(RepositoryError::Storage(message)) if message.contains("newer")
    ));
}

#[test]
fn deleting_a_session_cascades() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap();
    db.with_conn(|conn| {
        conn.execute_batch(
            "INSERT INTO sessions (id, title, language, state, created_at) VALUES ('s', 't', 'en', 'ended', 0);
             INSERT INTO session_sources (session_id, kind) VALUES ('s', 'microphone');
             INSERT INTO segments (id, session_id, seq, kind, text, start_ms, end_ms) VALUES ('g', 's', 0, 'final', 'x', 0, 1);
             INSERT INTO records (id, session_id, kind, text, nature, status, origin, created_at, updated_at)
                 VALUES ('r', 's', 'task', 'x', 'observed', 'proposed', 'user', 0, 0);
             INSERT INTO record_evidence (record_id, segment_id) VALUES ('r', 'g');
             INSERT INTO questions (id, session_id, question, status, asked_at) VALUES ('q', 's', 'x', 'pending', 0);
             INSERT INTO question_evidence (question_id, segment_id) VALUES ('q', 'g');
             DELETE FROM sessions WHERE id = 's';",
        )
        .unwrap();
        for table in ["session_sources", "segments", "records", "record_evidence", "questions", "question_evidence"] {
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table} not emptied");
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn schema_constraints_reject_inconsistent_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap();
    db.with_conn(|conn| {
        // failed without failure reason
        assert!(conn
            .execute("INSERT INTO sessions (id, title, language, state, created_at) VALUES ('a', 't', 'en', 'failed', 0)", [])
            .is_err());
        // orphan segment (foreign keys enforced)
        assert!(conn
            .execute("INSERT INTO segments (id, session_id, seq, kind, gap_reason, start_ms, end_ms) VALUES ('g', 'missing', 0, 'gap', 'paused', 0, 1)", [])
            .is_err());
        Ok(())
    })
    .unwrap();
}
