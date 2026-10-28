//! Transcript segments round-trip through the encrypted database and survive an abrupt exit
//! (T063, T069; FR-017, US2.3).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ports::{RepositoryError, SegmentRepository, SessionRepository};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::Session;
use devcopilot_domain::text::{SegmentText, Title};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_persistence::{Database, SqliteSegmentRepository, SqliteSessionRepository};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;
const SYSTEM: AudioSourceKind = AudioSourceKind::System;

struct Store {
    sessions: SqliteSessionRepository,
    segments: SqliteSegmentRepository,
}

fn open(dir: &std::path::Path, secrets: &ToggleableSecretStore) -> Store {
    let db = Arc::new(Database::open_in(dir, secrets).unwrap());
    Store {
        sessions: SqliteSessionRepository::new(Arc::clone(&db)),
        segments: SqliteSegmentRepository::new(db),
    }
}

fn session(store: &Store, sources: &[AudioSourceKind]) -> Session {
    let session = Session::new(
        SessionId::new(),
        Title::parse("Revisão da API").unwrap(),
        SessionLanguage::PtBr,
        sources,
        Timestamp::from_unix_millis(1_000).unwrap(),
    )
    .unwrap();
    store.sessions.create(&session).unwrap();
    session
}

fn final_segment(
    session: &Session,
    seq: u64,
    source: Option<AudioSourceKind>,
    start_ms: u64,
    end_ms: u64,
    text: &str,
) -> TranscriptSegment {
    TranscriptSegment::new(
        SegmentSpec {
            id: SegmentId::new(),
            session_id: session.id(),
            seq,
            kind: SegmentKind::Final {
                text: Redacted::new(SegmentText::parse(text).unwrap()),
            },
            source,
            start: Duration::from_millis(start_ms),
            end: Duration::from_millis(end_ms),
        },
        session.sources(),
    )
    .unwrap()
}

fn gap(
    session: &Session,
    seq: u64,
    reason: GapReason,
    start_ms: u64,
    end_ms: u64,
) -> TranscriptSegment {
    TranscriptSegment::new(
        SegmentSpec {
            id: SegmentId::new(),
            session_id: session.id(),
            seq,
            kind: SegmentKind::Gap { reason },
            source: None,
            start: Duration::from_millis(start_ms),
            end: Duration::from_millis(end_ms),
        },
        session.sources(),
    )
    .unwrap()
}

#[test]
fn finals_and_gaps_round_trip_in_seq_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let s = session(&store, &[MIC, SYSTEM]);
    assert_eq!(store.segments.next_seq(s.id()).unwrap(), 0);

    let written = vec![
        final_segment(&s, 0, Some(MIC), 0, 2_100, "precisamos de paginação"),
        gap(&s, 1, GapReason::Overload, 2_100, 17_100),
        final_segment(&s, 2, Some(SYSTEM), 17_100, 19_000, "e cache"),
    ];
    // Inserted out of order: listing follows `seq`.
    for segment in [&written[2], &written[0], &written[1]] {
        store.segments.append(segment).unwrap();
    }
    assert_eq!(store.segments.list_by_session(s.id()).unwrap(), written);
    assert_eq!(store.segments.next_seq(s.id()).unwrap(), 3);
    assert!(
        store
            .segments
            .list_by_session(SessionId::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn duplicate_seq_is_rejected_and_unknown_session_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let s = session(&store, &[MIC]);
    store
        .segments
        .append(&final_segment(&s, 0, None, 0, 1, "a"))
        .unwrap();
    assert!(matches!(
        store
            .segments
            .append(&final_segment(&s, 0, None, 1, 2, "b")),
        Err(RepositoryError::Storage(_))
    ));

    let orphan = Session::new(
        SessionId::new(),
        Title::parse("x").unwrap(),
        SessionLanguage::En,
        &[MIC],
        Timestamp::from_unix_millis(0).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store
            .segments
            .append(&final_segment(&orphan, 0, None, 0, 1, "c")),
        Err(RepositoryError::NotFound)
    );
}

#[test]
fn appended_finals_survive_an_abrupt_exit() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let (id, written) = {
        let store = open(dir.path(), &secrets);
        let s = session(&store, &[MIC]);
        let written = vec![
            final_segment(&s, 0, None, 0, 1_000, "primeiro trecho"),
            final_segment(&s, 1, None, 1_000, 2_500, "segundo trecho"),
        ];
        for segment in &written {
            store.segments.append(segment).unwrap();
        }
        // Simulate a crash: no clean shutdown or checkpoint runs.
        std::mem::forget(store);
        (s.id(), written)
    };
    let store = open(dir.path(), &secrets);
    assert_eq!(store.segments.list_by_session(id).unwrap(), written);
    assert_eq!(store.segments.next_seq(id).unwrap(), 2);
}

#[test]
fn startup_recovery_gap_follows_the_last_final() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let mut s = session(&store, &[MIC]);
    s.start().unwrap();
    s.ready(Timestamp::from_unix_millis(2_000).unwrap())
        .unwrap();
    store.sessions.update(&s).unwrap();
    store
        .segments
        .append(&final_segment(&s, 0, None, 0, 4_000, "antes da queda"))
        .unwrap();
    s.interrupt(Timestamp::from_unix_millis(9_000).unwrap())
        .unwrap();
    store.sessions.save_interrupted(&s).unwrap();

    let segments = store.segments.list_by_session(s.id()).unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].gap_reason(), Some(GapReason::Interrupted));
    assert_eq!(
        (segments[1].seq(), segments[1].start()),
        (1, Duration::from_secs(4))
    );
}

#[test]
fn deleting_the_session_removes_its_segments() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let db = Arc::new(Database::open_in(dir.path(), &secrets).unwrap());
    let store = Store {
        sessions: SqliteSessionRepository::new(Arc::clone(&db)),
        segments: SqliteSegmentRepository::new(Arc::clone(&db)),
    };
    let s = session(&store, &[MIC]);
    store
        .segments
        .append(&final_segment(&s, 0, None, 0, 1, "x"))
        .unwrap();
    db.with_conn(|conn| {
        conn.execute("DELETE FROM sessions WHERE id = ?1", [s.id().to_string()])
            .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(store.segments.list_by_session(s.id()).unwrap().is_empty());
}
