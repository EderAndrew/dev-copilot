//! Records, their history and analysis runs round-trip through the encrypted database
//! (T084, T088; FR-022, FR-026, SC-007).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ports::{
    AnalysisRunRepository, RecordRepository, RepositoryError, SegmentRepository, SessionRepository,
};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::{AnalysisOutcome, AnalysisRun, AnalysisTrigger};
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::record::{
    Actor, Nature, Origin, RecordFlags, RecordKind, RecordSpec, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::record_change::{ChangeActor, ChangeField};
use devcopilot_domain::session::{Session, SessionState};
use devcopilot_domain::text::{ListItem, Rationale, RecordText, SegmentText, Title};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_persistence::{
    Database, SqliteAnalysisRunRepository, SqliteRecordRepository, SqliteSegmentRepository,
    SqliteSessionRepository,
};

struct Store {
    sessions: SqliteSessionRepository,
    segments: SqliteSegmentRepository,
    records: SqliteRecordRepository,
    runs: SqliteAnalysisRunRepository,
}

fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

fn open(dir: &std::path::Path, secrets: &ToggleableSecretStore) -> Store {
    let db = Arc::new(Database::open_in(dir, secrets).unwrap());
    Store {
        sessions: SqliteSessionRepository::new(Arc::clone(&db)),
        segments: SqliteSegmentRepository::new(Arc::clone(&db)),
        records: SqliteRecordRepository::new(Arc::clone(&db)),
        runs: SqliteAnalysisRunRepository::new(db),
    }
}

/// A session with one final segment and one finished run.
fn seed(store: &Store) -> (SessionId, SegmentId, AnalysisRun) {
    let session = Session::new(
        SessionId::new(),
        Title::parse("Planejamento").unwrap(),
        SessionLanguage::PtBr,
        &[devcopilot_domain::audio_source::AudioSourceKind::Microphone],
        at(1_000),
    )
    .unwrap();
    store.sessions.create(&session).unwrap();
    let segment = TranscriptSegment::restore(SegmentSpec {
        id: SegmentId::new(),
        session_id: session.id(),
        seq: 0,
        kind: SegmentKind::Final {
            text: Redacted::new(SegmentText::parse("O export precisa suportar CSV").unwrap()),
        },
        source: None,
        start: Duration::ZERO,
        end: Duration::from_secs(3),
    });
    store.segments.append(&segment).unwrap();
    let mut run = AnalysisRun::start(
        AnalysisRunId::new(),
        session.id(),
        SessionState::Capturing,
        ProviderId::new(),
        AnalysisTrigger::Incremental,
        (0, 0),
        at(2_000),
    )
    .unwrap();
    store.runs.insert(&run).unwrap();
    run.finish(AnalysisOutcome::Ok { created: 1 }, at(3_000))
        .unwrap();
    store.runs.update(&run).unwrap();
    (session.id(), segment.id(), run)
}

fn proposed(session: SessionId, segment: SegmentId, run: &AnalysisRun) -> TechnicalRecord {
    TechnicalRecord::new(RecordSpec {
        id: RecordId::new(),
        session_id: session,
        kind: RecordKind::Task,
        text: Redacted::new(RecordText::parse("Implementar export CSV").unwrap()),
        nature: Nature::Observed,
        status: RecordStatus::Proposed,
        rationale: Redacted::new(Rationale::parse("Pedido do cliente").unwrap()),
        origin: Origin::Ai {
            provider_id: run.provider_id(),
            analysis_run_id: run.id(),
        },
        evidence: vec![segment],
        acceptance_criteria: vec![Redacted::new(
            ListItem::parse("Exporta 10 mil linhas").unwrap(),
        )],
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at(3_000),
        updated_at: at(3_000),
    })
    .unwrap()
}

#[test]
fn proposed_record_round_trips_with_creation_history() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, segment, run) = seed(&store);
    let record = proposed(session, segment, &run);
    store.records.insert(&record, &record.creation()).unwrap();

    assert_eq!(store.records.get(record.id()).unwrap(), record);
    assert_eq!(
        store.records.list_by_session(session).unwrap(),
        vec![record.clone()]
    );
    let history = store.records.history(record.id()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].field(), ChangeField::Created);
    assert_eq!(
        history[0].actor(),
        ChangeActor::Ai {
            analysis_run_id: run.id()
        }
    );
}

#[test]
fn user_change_updates_record_and_appends_history() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, segment, run) = seed(&store);
    let mut record = proposed(session, segment, &run);
    store.records.insert(&record, &record.creation()).unwrap();
    let change = record
        .set_status(Actor::User, RecordStatus::Confirmed, at(9_000))
        .unwrap();
    store
        .records
        .update(&record, std::slice::from_ref(&change))
        .unwrap();

    let stored = store.records.get(record.id()).unwrap();
    assert_eq!(stored.status(), RecordStatus::Confirmed);
    assert_eq!(stored.updated_at(), at(9_000));
    let history = store.records.history(record.id()).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1], change);
}

#[test]
fn records_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let (record, run) = {
        let store = open(dir.path(), &secrets);
        let (session, segment, run) = seed(&store);
        let record = proposed(session, segment, &run);
        store.records.insert(&record, &record.creation()).unwrap();
        (record, run)
    };
    let store = open(dir.path(), &secrets);
    assert_eq!(store.records.get(record.id()).unwrap(), record);
    assert_eq!(
        store.runs.list_by_session(record.session_id()).unwrap(),
        vec![run]
    );
}

#[test]
fn evidence_and_sessions_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, _, run) = seed(&store);
    let foreign = proposed(session, SegmentId::new(), &run);
    assert_eq!(
        store.records.insert(&foreign, &foreign.creation()),
        Err(RepositoryError::NotFound)
    );
    // Nothing of the failed insert remains.
    assert!(store.records.list_by_session(session).unwrap().is_empty());
    assert!(store.records.history(foreign.id()).unwrap().is_empty());
    assert_eq!(
        store.records.update(&foreign, &[]),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        store.records.get(RecordId::new()),
        Err(RepositoryError::NotFound)
    );
}

#[test]
fn flags_and_user_records_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, segment, run) = seed(&store);
    let original = proposed(session, segment, &run);
    store
        .records
        .insert(&original, &original.creation())
        .unwrap();
    let mut spec = proposed(session, segment, &run).spec().clone();
    spec.id = RecordId::new();
    spec.origin = Origin::User;
    spec.status = RecordStatus::Confirmed;
    spec.nature = Nature::Suggested;
    spec.evidence.clear();
    spec.flags.conflicts_with = Some(original.id());
    let user = TechnicalRecord::new(spec).unwrap();
    store.records.insert(&user, &user.creation()).unwrap();
    assert_eq!(store.records.get(user.id()).unwrap(), user);
    assert_eq!(store.records.list_by_session(session).unwrap().len(), 2);
}

#[test]
fn run_outcomes_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, _, run) = seed(&store);
    let mut limited = AnalysisRun::start(
        AnalysisRunId::new(),
        session,
        SessionState::Capturing,
        run.provider_id(),
        AnalysisTrigger::Incremental,
        (1, 4),
        at(4_000),
    )
    .unwrap();
    store.runs.insert(&limited).unwrap();
    limited
        .finish(AnalysisOutcome::RateLimited, at(4_000))
        .unwrap();
    store.runs.update(&limited).unwrap();
    assert_eq!(
        store.runs.list_by_session(session).unwrap(),
        vec![run, limited]
    );
    assert_eq!(
        store.runs.insert(
            &AnalysisRun::start(
                AnalysisRunId::new(),
                SessionId::new(),
                SessionState::Capturing,
                ProviderId::new(),
                AnalysisTrigger::Incremental,
                (0, 0),
                at(1),
            )
            .unwrap()
        ),
        Err(RepositoryError::NotFound)
    );
}
