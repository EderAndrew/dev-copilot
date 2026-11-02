//! A finished session survives closing and reopening the encrypted database intact: session,
//! transcript (finals and gaps), records with their history, and analysis runs (T100, US4
//! independent test, SC-007).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ports::{
    AnalysisRunRepository, RecordRepository, SegmentRepository, SessionRepository,
};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::{AnalysisOutcome, AnalysisRun, AnalysisTrigger};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::record::{
    Actor, Nature, Origin, RecordEdit, RecordFlags, RecordKind, RecordSpec, RecordStatus,
    TechnicalRecord, UserRecord,
};
use devcopilot_domain::session::{FailureReason, Session};
use devcopilot_domain::text::{ListItem, Rationale, RecordText, SegmentText, Title};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
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

fn open(dir: &std::path::Path, secrets: &ToggleableSecretStore) -> Store {
    let db = Arc::new(Database::open_in(dir, secrets).unwrap());
    Store {
        sessions: SqliteSessionRepository::new(Arc::clone(&db)),
        segments: SqliteSegmentRepository::new(Arc::clone(&db)),
        records: SqliteRecordRepository::new(Arc::clone(&db)),
        runs: SqliteAnalysisRunRepository::new(db),
    }
}

fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

fn segment(session: SessionId, seq: u64, kind: SegmentKind) -> TranscriptSegment {
    TranscriptSegment::restore(SegmentSpec {
        id: SegmentId::new(),
        session_id: session,
        seq,
        kind,
        source: None,
        start: Duration::from_secs(seq * 5),
        end: Duration::from_secs(seq * 5 + 4),
    })
}

struct Written {
    session: Session,
    segments: Vec<TranscriptSegment>,
    records: Vec<TechnicalRecord>,
    run: AnalysisRun,
}

fn write(store: &Store, fail: bool) -> Written {
    let mut session = Session::new(
        SessionId::new(),
        Title::parse("Planejamento do export").unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone],
        at(1_000),
    )
    .unwrap();
    store.sessions.create(&session).unwrap();
    session.start().unwrap();
    session.ready(at(2_000)).unwrap();
    store.sessions.update(&session).unwrap();
    let id = session.id();
    let segments = vec![
        segment(
            id,
            0,
            SegmentKind::Final {
                text: Redacted::new(SegmentText::parse("O export precisa suportar CSV").unwrap()),
            },
        ),
        segment(
            id,
            1,
            SegmentKind::Gap {
                reason: GapReason::Overload,
            },
        ),
        segment(
            id,
            2,
            SegmentKind::Final {
                text: Redacted::new(SegmentText::parse("Vamos usar Postgres").unwrap()),
            },
        ),
    ];
    for segment in &segments {
        store.segments.append(segment).unwrap();
    }
    let mut run = AnalysisRun::start(
        AnalysisRunId::new(),
        id,
        session.state(),
        ProviderId::new(),
        AnalysisTrigger::Incremental,
        (0, 2),
        at(3_000),
    )
    .unwrap();
    store.runs.insert(&run).unwrap();
    run.finish(AnalysisOutcome::Ok { created: 1 }, at(4_000))
        .unwrap();
    store.runs.update(&run).unwrap();

    let mut proposed = TechnicalRecord::new(RecordSpec {
        id: RecordId::new(),
        session_id: id,
        kind: RecordKind::Task,
        text: Redacted::new(RecordText::parse("Implementar export CSV").unwrap()),
        nature: Nature::Observed,
        status: RecordStatus::Proposed,
        rationale: Redacted::new(Rationale::default()),
        origin: Origin::Ai {
            provider_id: run.provider_id(),
            analysis_run_id: run.id(),
        },
        evidence: vec![segments[0].id()],
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at(4_000),
        updated_at: at(4_000),
    })
    .unwrap();
    store
        .records
        .insert(&proposed, &proposed.creation())
        .unwrap();
    let mut changes = vec![
        proposed
            .set_status(Actor::User, RecordStatus::Confirmed, at(5_000))
            .unwrap(),
    ];
    changes.extend(
        proposed
            .edit(
                Actor::User,
                RecordEdit {
                    acceptance_criteria: Some(vec![Redacted::new(
                        ListItem::parse("Exporta 10 mil linhas").unwrap(),
                    )]),
                    ..RecordEdit::default()
                },
                at(6_000),
            )
            .unwrap(),
    );
    store.records.update(&proposed, &changes).unwrap();
    let mine = TechnicalRecord::create_by_user(
        RecordId::new(),
        id,
        UserRecord {
            kind: RecordKind::Assumption,
            text: Redacted::new(RecordText::parse("Equipe de três pessoas").unwrap()),
            rationale: Redacted::new(Rationale::default()),
            status: RecordStatus::Confirmed,
        },
        at(7_000),
    )
    .unwrap();
    store.records.insert(&mine, &mine.creation()).unwrap();

    if fail {
        session
            .fail(FailureReason::SourceUnavailable, at(8_000))
            .unwrap();
    } else {
        session.stop(at(8_000)).unwrap();
        session.drained(at(8_500)).unwrap();
    }
    session.rename(Title::parse("Export — revisado").unwrap());
    store.sessions.update(&session).unwrap();
    Written {
        session,
        segments,
        records: vec![proposed, mine],
        run,
    }
}

fn assert_intact(store: &Store, written: &Written) {
    let id = written.session.id();
    assert_eq!(store.sessions.get(id).unwrap(), written.session);
    assert_eq!(
        store.segments.list_by_session(id).unwrap(),
        written.segments
    );
    assert_eq!(store.records.list_by_session(id).unwrap(), written.records);
    let history: Vec<_> = store
        .records
        .history(written.records[0].id())
        .unwrap()
        .iter()
        .map(|c| c.field().code())
        .collect();
    assert_eq!(history, ["created", "status", "acceptance_criteria"]);
    assert_eq!(store.runs.list_by_session(id).unwrap(), vec![written.run]);
}

#[test]
fn ended_and_failed_sessions_survive_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let (ended, failed) = {
        let store = open(dir.path(), &secrets);
        (write(&store, false), write(&store, true))
    };
    let store = open(dir.path(), &secrets);
    assert_intact(&store, &ended);
    assert_intact(&store, &failed);
    assert_eq!(
        store.sessions.get(failed.session.id()).unwrap().failure(),
        Some(FailureReason::SourceUnavailable)
    );
    assert_eq!(store.sessions.list().unwrap().len(), 2);
}
