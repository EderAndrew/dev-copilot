//! Sessions and their sources round-trip through the encrypted database (T043, T048).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ports::{RepositoryError, SessionRepository};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, Session, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_persistence::{Database, SqliteSessionRepository};

fn ts(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

fn session(created_ms: i64, sources: &[AudioSourceKind]) -> Session {
    Session::new(
        SessionId::new(),
        Title::parse("Arquitetura do módulo de pagamentos").unwrap(),
        SessionLanguage::PtBr,
        sources,
        ts(created_ms),
    )
    .unwrap()
}

fn open(
    dir: &std::path::Path,
    secrets: &ToggleableSecretStore,
) -> (Arc<Database>, SqliteSessionRepository) {
    let db = Arc::new(Database::open_in(dir, secrets).unwrap());
    (Arc::clone(&db), SqliteSessionRepository::new(db))
}

/// (seq, kind, gap_reason, start_ms, end_ms) of every segment of `id`.
fn segments(db: &Database, id: SessionId) -> Vec<(i64, String, Option<String>, i64, i64)> {
    db.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT seq, kind, gap_reason, start_ms, end_ms FROM segments WHERE session_id = ?1 ORDER BY seq")
            .unwrap();
        Ok(stmt
            .query_map([id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap())
    })
    .unwrap()
}

#[test]
fn create_and_get_round_trip_with_sources() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let both = session(
        1_000,
        &[AudioSourceKind::System, AudioSourceKind::Microphone],
    );
    repo.create(&both).unwrap();
    let back = repo.get(both.id()).unwrap();
    assert_eq!(back, both);
    assert_eq!(
        back.sources(),
        [AudioSourceKind::Microphone, AudioSourceKind::System]
    );
    assert_eq!(repo.get(SessionId::new()), Err(RepositoryError::NotFound));
}

#[test]
fn update_persists_lifecycle_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let mut s = session(1_000, &[AudioSourceKind::Microphone]);
    {
        let (_db, repo) = open(dir.path(), &secrets);
        repo.create(&s).unwrap();
        s.start().unwrap();
        s.ready(ts(2_000)).unwrap();
        s.stop(ts(5_000)).unwrap();
        s.drained(ts(6_000)).unwrap();
        s.rename(Title::parse("Renomeada").unwrap());
        repo.update(&s).unwrap();
    }
    let (_db, repo) = open(dir.path(), &secrets);
    let back = repo.get(s.id()).unwrap();
    assert_eq!(back.state(), SessionState::Ended);
    assert_eq!(back.title().as_str(), "Renomeada");
    assert_eq!(back.started_at(), Some(ts(2_000)));
    assert_eq!(back.ended_at(), Some(ts(6_000)));
    assert_eq!(back.active_duration(), Duration::from_millis(3_000));
}

#[test]
fn failure_reason_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let mut s = session(1_000, &[AudioSourceKind::Microphone]);
    repo.create(&s).unwrap();
    s.start().unwrap();
    s.fail(FailureReason::PermissionDenied, ts(1_500)).unwrap();
    repo.update(&s).unwrap();
    let back = repo.get(s.id()).unwrap();
    assert_eq!(back.failure(), Some(FailureReason::PermissionDenied));
    assert_eq!(back.ended_at(), None);
}

#[test]
fn update_of_unknown_session_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let s = session(1_000, &[AudioSourceKind::Microphone]);
    assert_eq!(repo.update(&s), Err(RepositoryError::NotFound));
    assert_eq!(repo.save_interrupted(&s), Err(RepositoryError::NotFound));
}

#[test]
fn list_is_newest_first_and_list_active_filters() {
    let dir = tempfile::tempdir().unwrap();
    let (_db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let old = session(1_000, &[AudioSourceKind::Microphone]);
    let mut capturing = session(2_000, &[AudioSourceKind::Microphone]);
    let new = session(3_000, &[AudioSourceKind::Microphone]);
    for s in [&old, &capturing, &new] {
        repo.create(s).unwrap();
    }
    capturing.start().unwrap();
    capturing.ready(ts(2_500)).unwrap();
    repo.update(&capturing).unwrap();

    let ids: Vec<_> = repo.list().unwrap().iter().map(Session::id).collect();
    assert_eq!(ids, [new.id(), capturing.id(), old.id()]);
    let active: Vec<_> = repo
        .list_active()
        .unwrap()
        .iter()
        .map(Session::id)
        .collect();
    assert_eq!(active, [capturing.id()]);
}

#[test]
fn save_interrupted_appends_trailing_gap_after_last_segment() {
    let dir = tempfile::tempdir().unwrap();
    let (db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let mut s = session(1_000, &[AudioSourceKind::Microphone]);
    repo.create(&s).unwrap();
    s.start().unwrap();
    s.ready(ts(2_000)).unwrap();
    repo.update(&s).unwrap();
    // Two finals persisted before the crash (segment repository arrives with US2, T069).
    db.with_conn(|conn| {
        for (seq, start, end) in [(0, 0, 4_000), (1, 4_000, 9_500)] {
            conn.execute(
                "INSERT INTO segments (id, session_id, seq, kind, text, start_ms, end_ms) \
                 VALUES (?1, ?2, ?3, 'final', 'texto', ?4, ?5)",
                rusqlite::params![
                    devcopilot_domain::ids::SegmentId::new().to_string(),
                    s.id().to_string(),
                    seq,
                    start,
                    end
                ],
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();

    s.interrupt(ts(20_000)).unwrap();
    repo.save_interrupted(&s).unwrap();

    let back = repo.get(s.id()).unwrap();
    assert_eq!(back.state(), SessionState::Failed);
    assert_eq!(back.failure(), Some(FailureReason::Interrupted));
    let rows = segments(&db, s.id());
    assert_eq!(rows.len(), 3, "finals must be kept");
    assert_eq!(
        rows[2],
        (2, "gap".into(), Some("interrupted".into()), 9_500, 9_500)
    );
}

#[test]
fn save_interrupted_without_capture_adds_no_gap() {
    let dir = tempfile::tempdir().unwrap();
    let (db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let mut s = session(1_000, &[AudioSourceKind::Microphone]);
    repo.create(&s).unwrap();
    s.start().unwrap();
    repo.update(&s).unwrap();
    s.interrupt(ts(3_000)).unwrap();
    repo.save_interrupted(&s).unwrap();
    assert!(segments(&db, s.id()).is_empty());

    // A session that captured but had no finals gets the gap at offset zero.
    let mut empty = session(2_000, &[AudioSourceKind::Microphone]);
    repo.create(&empty).unwrap();
    empty.start().unwrap();
    empty.ready(ts(2_100)).unwrap();
    empty.interrupt(ts(3_000)).unwrap();
    repo.save_interrupted(&empty).unwrap();
    assert_eq!(
        segments(&db, empty.id()),
        [(0, "gap".into(), Some("interrupted".into()), 0, 0)]
    );
}

#[test]
fn corrupt_rows_are_reported_not_half_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let (db, repo) = open(dir.path(), &ToggleableSecretStore::available());
    let s = session(1_000, &[AudioSourceKind::Microphone]);
    repo.create(&s).unwrap();
    db.with_conn(|conn| {
        conn.execute("DELETE FROM session_sources", []).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(repo.get(s.id()), Err(RepositoryError::Corrupt("sessions")));
}
