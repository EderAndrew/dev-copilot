//! Session deletion in the encrypted database (T120; FR-042, FR-043, SC-012).
//!
//! Deleting a session removes every row it owns, leaves other sessions untouched, and
//! checkpoints the write-ahead log so the deleted pages do not linger in it.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use devcopilot_application::ports::{
    AnalysisRunRepository, ExportRepository, QuestionRepository, RecordRepository, RepositoryError,
    SegmentRepository, SessionRepository,
};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::{AnalysisOutcome, AnalysisRun, AnalysisTrigger};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat, ExportRecord};
use devcopilot_domain::ids::{
    AnalysisRunId, ExportId, ProviderId, QuestionId, RecordId, SegmentId, SessionId,
};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::question::{Answer, Certainty, ContextQuestion, EvidenceRef};
use devcopilot_domain::record::{
    Nature, Origin, Proposal, RecordFlags, RecordKind, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::record_change::RecordChange;
use devcopilot_domain::session::{Session, SessionState};
use devcopilot_domain::text::{
    AnswerText, QuestionText, Rationale, RecordText, SegmentText, Title,
};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_persistence::db::DB_FILE_NAME;
use devcopilot_persistence::{
    Database, SqliteAnalysisRunRepository, SqliteExportRepository, SqliteQuestionRepository,
    SqliteRecordRepository, SqliteSegmentRepository, SqliteSessionRepository,
};

/// Tables holding session content, with the column that ties a row to its session (directly
/// or through a parent).
const OWNED: [(&str, &str); 9] = [
    ("sessions", "SELECT count(*) FROM sessions WHERE id = ?1"),
    (
        "session_sources",
        "SELECT count(*) FROM session_sources WHERE session_id = ?1",
    ),
    (
        "segments",
        "SELECT count(*) FROM segments WHERE session_id = ?1",
    ),
    (
        "analysis_runs",
        "SELECT count(*) FROM analysis_runs WHERE session_id = ?1",
    ),
    (
        "records",
        "SELECT count(*) FROM records WHERE session_id = ?1",
    ),
    (
        "record_evidence",
        "SELECT count(*) FROM record_evidence WHERE record_id IN \
         (SELECT id FROM records WHERE session_id = ?1)",
    ),
    (
        "record_changes",
        "SELECT count(*) FROM record_changes WHERE record_id IN \
         (SELECT id FROM records WHERE session_id = ?1)",
    ),
    (
        "questions",
        "SELECT count(*) FROM questions WHERE session_id = ?1",
    ),
    (
        "exports",
        "SELECT count(*) FROM exports WHERE session_id = ?1",
    ),
];

struct Store {
    db: Arc<Database>,
    sessions: SqliteSessionRepository,
    segments: SqliteSegmentRepository,
    records: SqliteRecordRepository,
    runs: SqliteAnalysisRunRepository,
    questions: SqliteQuestionRepository,
    exports: SqliteExportRepository,
}

fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

fn open(dir: &Path, secrets: &ToggleableSecretStore) -> Store {
    let db = Arc::new(Database::open_in(dir, secrets).unwrap());
    Store {
        sessions: SqliteSessionRepository::new(Arc::clone(&db)),
        segments: SqliteSegmentRepository::new(Arc::clone(&db)),
        records: SqliteRecordRepository::new(Arc::clone(&db)),
        runs: SqliteAnalysisRunRepository::new(Arc::clone(&db)),
        questions: SqliteQuestionRepository::new(Arc::clone(&db)),
        exports: SqliteExportRepository::new(Arc::clone(&db)),
        db,
    }
}

/// Rows owned by `session` in each table.
fn counts(store: &Store, session: SessionId) -> Vec<(&'static str, i64)> {
    store
        .db
        .with_conn(|conn| {
            Ok(OWNED
                .iter()
                .map(|(table, sql)| {
                    let n = conn
                        .query_row(sql, [session.to_string()], |row| row.get(0))
                        .unwrap();
                    (*table, n)
                })
                .collect())
        })
        .unwrap()
}

fn total(store: &Store, table: &str) -> i64 {
    store
        .db
        .with_conn(|conn| {
            Ok(conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap())
        })
        .unwrap()
}

/// An ended session with a final, a gap, an AI record (evidence + a user status change), a
/// run, an answered question citing both, and an export.
fn seed(store: &Store, title: &str) -> SessionId {
    let mut session = Session::new(
        SessionId::new(),
        Title::parse(title).unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone, AudioSourceKind::System],
        at(1_000),
    )
    .unwrap();
    let id = session.id();
    store.sessions.create(&session).unwrap();
    session.start().unwrap();
    session.ready(at(2_000)).unwrap();
    session.stop(at(60_000)).unwrap();
    session.drained(at(61_000)).unwrap();
    assert_eq!(session.state(), SessionState::Ended);
    store.sessions.update(&session).unwrap();

    let segment = SegmentId::new();
    store
        .segments
        .append(&TranscriptSegment::restore(SegmentSpec {
            id: segment,
            session_id: id,
            seq: 0,
            kind: SegmentKind::Final {
                text: Redacted::new(SegmentText::parse("Conteúdo confidencial").unwrap()),
            },
            source: Some(AudioSourceKind::Microphone),
            start: Duration::from_secs(0),
            end: Duration::from_secs(3),
        }))
        .unwrap();
    store
        .segments
        .append(&TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::new(),
            session_id: id,
            seq: 1,
            kind: SegmentKind::Gap {
                reason: GapReason::Paused,
            },
            source: None,
            start: Duration::from_secs(3),
            end: Duration::from_secs(10),
        }))
        .unwrap();

    let provider = ProviderId::new();
    let mut run = AnalysisRun::start(
        AnalysisRunId::new(),
        id,
        SessionState::Ended,
        provider,
        AnalysisTrigger::Final,
        (0, 1),
        at(62_000),
    )
    .unwrap();
    store.runs.insert(&run).unwrap();
    run.finish(AnalysisOutcome::Ok { created: 1 }, at(63_000))
        .unwrap();
    store.runs.update(&run).unwrap();

    let mut record = TechnicalRecord::propose(
        RecordId::new(),
        id,
        Proposal {
            kind: RecordKind::Requirement,
            text: Redacted::new(RecordText::parse("Requisito confidencial").unwrap()),
            nature: Nature::Observed,
            rationale: Redacted::new(Rationale::parse("").unwrap()),
            evidence: vec![segment],
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            relates_to: None,
        },
        Origin::Ai {
            provider_id: provider,
            analysis_run_id: run.id(),
        },
        RecordFlags::default(),
        at(63_000),
    )
    .unwrap();
    store.records.insert(&record, &record.creation()).unwrap();
    let change: RecordChange = record
        .set_status(
            devcopilot_domain::record::Actor::User,
            RecordStatus::Confirmed,
            at(64_000),
        )
        .unwrap();
    store.records.update(&record, &[change]).unwrap();

    let mut question = ContextQuestion::ask(
        QuestionId::new(),
        id,
        Redacted::new(QuestionText::parse("O que é confidencial?").unwrap()),
        Some(provider),
        at(65_000),
    );
    store.questions.insert(&question).unwrap();
    question
        .answer(
            Answer {
                text: Redacted::new(AnswerText::parse("O requisito.").unwrap()),
                certainty: Certainty::Supported,
                evidence: vec![
                    EvidenceRef::Segment(segment),
                    EvidenceRef::Record(record.id()),
                ],
            },
            at(66_000),
        )
        .unwrap();
    store.questions.update(&question).unwrap();

    store
        .exports
        .insert(
            &ExportRecord::new(
                ExportId::new(),
                id,
                BTreeSet::from([ExportFormat::Markdown, ExportFormat::Json]),
                BTreeSet::from([ExportContent::Plan]),
                at(67_000),
            )
            .unwrap(),
        )
        .unwrap();
    id
}

#[test]
fn delete_cascades_to_every_owned_row_and_spares_other_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let doomed = seed(&store, "Sessão a excluir");
    let kept = seed(&store, "Sessão mantida");
    assert!(counts(&store, doomed).iter().all(|(_, n)| *n > 0));
    let kept_before = counts(&store, kept);

    store.sessions.delete(doomed).unwrap();

    for (table, n) in counts(&store, doomed) {
        assert_eq!(n, 0, "{table} still holds rows of the deleted session");
    }
    assert_eq!(counts(&store, kept), kept_before);
    // No orphans anywhere: child tables only hold the kept session's rows.
    assert_eq!(total(&store, "question_evidence"), 2);
    assert_eq!(total(&store, "record_evidence"), 1);

    assert!(matches!(
        store.sessions.get(doomed),
        Err(RepositoryError::NotFound)
    ));
    assert!(
        store
            .sessions
            .list()
            .unwrap()
            .iter()
            .all(|s| s.id() != doomed)
    );
    assert!(store.segments.list_by_session(doomed).unwrap().is_empty());
    assert!(store.records.list_by_session(doomed).unwrap().is_empty());
    assert!(store.runs.list_by_session(doomed).unwrap().is_empty());
    assert!(store.questions.list_by_session(doomed).unwrap().is_empty());
    assert!(store.exports.list_by_session(doomed).unwrap().is_empty());
    assert_eq!(store.exports.list_by_session(kept).unwrap().len(), 1);
}

#[test]
fn delete_checkpoints_the_write_ahead_log() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let id = seed(&store, "Sessão a excluir");
    let wal = dir.path().join(format!("{DB_FILE_NAME}-wal"));
    assert!(std::fs::metadata(&wal).unwrap().len() > 0);

    store.sessions.delete(id).unwrap();

    assert_eq!(std::fs::metadata(&wal).map_or(0, |m| m.len()), 0);
}

#[test]
fn deletion_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let id = {
        let store = open(dir.path(), &secrets);
        let id = seed(&store, "Sessão a excluir");
        store.sessions.delete(id).unwrap();
        id
    };
    let store = open(dir.path(), &secrets);
    assert!(counts(&store, id).iter().all(|(_, n)| *n == 0));
}

#[test]
fn unknown_session_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    assert!(matches!(
        store.sessions.delete(SessionId::new()),
        Err(RepositoryError::NotFound)
    ));
}

#[test]
fn exports_round_trip_without_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path(), &ToggleableSecretStore::available());
    let id = seed(&store, "Sessão exportada");
    let exports = store.exports.list_by_session(id).unwrap();
    assert_eq!(exports.len(), 1);
    assert_eq!(
        exports[0].formats(),
        &BTreeSet::from([ExportFormat::Markdown, ExportFormat::Json])
    );
    assert_eq!(exports[0].content(), &BTreeSet::from([ExportContent::Plan]));
    assert_eq!(exports[0].at(), at(67_000));
    let columns: Vec<String> = store
        .db
        .with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT name FROM pragma_table_info('exports')")
                .unwrap();
            Ok(stmt
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap())
        })
        .unwrap();
    assert_eq!(columns, ["id", "session_id", "formats", "content", "at"]);
    assert!(matches!(
        store.exports.insert(
            &ExportRecord::new(
                ExportId::new(),
                SessionId::new(),
                BTreeSet::from([ExportFormat::Json]),
                BTreeSet::from([ExportContent::Transcript]),
                at(1),
            )
            .unwrap()
        ),
        Err(RepositoryError::NotFound)
    ));
}
