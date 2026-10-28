//! Questions, answers and their evidence round-trip through the encrypted database (T113;
//! FR-038, FR-039).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ports::{
    QuestionRepository, RecordRepository, RepositoryError, SegmentRepository, SessionRepository,
};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, QuestionId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::question::{
    Answer, Certainty, ContextQuestion, EvidenceRef, QuestionStatus,
};
use devcopilot_domain::record::{RecordKind, RecordStatus, TechnicalRecord, UserRecord};
use devcopilot_domain::session::Session;
use devcopilot_domain::text::{
    AnswerText, QuestionText, Rationale, RecordText, SegmentText, Title,
};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_persistence::{
    Database, SqliteQuestionRepository, SqliteRecordRepository, SqliteSegmentRepository,
    SqliteSessionRepository,
};

struct Store {
    sessions: SqliteSessionRepository,
    segments: SqliteSegmentRepository,
    records: SqliteRecordRepository,
    questions: SqliteQuestionRepository,
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
        questions: SqliteQuestionRepository::new(db),
    }
}

/// A session with two finals and one record.
fn seed(store: &Store) -> (SessionId, Vec<SegmentId>, RecordId) {
    let session = Session::new(
        SessionId::new(),
        Title::parse("Autenticação").unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone],
        at(1_000),
    )
    .unwrap();
    store.sessions.create(&session).unwrap();
    let segments: Vec<SegmentId> = ["Ficou decidido usar OAuth2", "We agreed on OAuth2 too"]
        .iter()
        .enumerate()
        .map(|(seq, text)| {
            let segment = TranscriptSegment::restore(SegmentSpec {
                id: SegmentId::new(),
                session_id: session.id(),
                seq: seq as u64,
                kind: SegmentKind::Final {
                    text: Redacted::new(SegmentText::parse(*text).unwrap()),
                },
                source: None,
                start: Duration::from_secs(seq as u64 * 3),
                end: Duration::from_secs(seq as u64 * 3 + 3),
            });
            store.segments.append(&segment).unwrap();
            segment.id()
        })
        .collect();
    let record = TechnicalRecord::create_by_user(
        RecordId::new(),
        session.id(),
        UserRecord {
            kind: RecordKind::Decision,
            text: Redacted::new(RecordText::parse("Usar OAuth2").unwrap()),
            rationale: Redacted::new(Rationale::default()),
            status: RecordStatus::Confirmed,
        },
        at(2_000),
    )
    .unwrap();
    store.records.insert(&record, &record.creation()).unwrap();
    (session.id(), segments, record.id())
}

fn pending(session: SessionId, text: &str, asked: i64) -> ContextQuestion {
    ContextQuestion::ask(
        QuestionId::new(),
        session,
        Redacted::new(QuestionText::parse(text).unwrap()),
        Some(ProviderId::new()),
        at(asked),
    )
}

#[test]
fn answered_question_round_trips_with_ordered_evidence_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let (session, segments, record, question) = {
        let store = open(dir.path(), &secrets);
        let (session, segments, record) = seed(&store);
        let mut question = pending(session, "O que foi decidido sobre autenticação?", 3_000);
        store.questions.insert(&question).unwrap();
        question
            .answer(
                Answer {
                    text: Redacted::new(AnswerText::parse("Usar OAuth2.").unwrap()),
                    certainty: Certainty::Supported,
                    evidence: vec![
                        EvidenceRef::Record(record),
                        EvidenceRef::Segment(segments[1]),
                        EvidenceRef::Segment(segments[0]),
                    ],
                },
                at(4_000),
            )
            .unwrap();
        store.questions.update(&question).unwrap();
        (session, segments, record, question)
    };

    let store = open(dir.path(), &secrets);
    let stored = store.questions.get(question.id()).unwrap();
    assert_eq!(stored, question);
    assert_eq!(
        stored.evidence(),
        [
            EvidenceRef::Record(record),
            EvidenceRef::Segment(segments[1]),
            EvidenceRef::Segment(segments[0]),
        ]
    );
    assert_eq!(
        store.questions.list_by_session(session).unwrap(),
        [question]
    );
}

#[test]
fn pending_and_failed_questions_round_trip_in_ask_order() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let (session, _, _) = seed(&store);
    let mut failed = pending(session, "Primeira", 3_000);
    let waiting = pending(session, "Segunda", 5_000);
    store.questions.insert(&waiting).unwrap();
    store.questions.insert(&failed).unwrap();
    failed.fail(at(3_500)).unwrap();
    store.questions.update(&failed).unwrap();

    let listed = store.questions.list_by_session(session).unwrap();
    assert_eq!(listed, [failed.clone(), waiting.clone()]);
    assert_eq!(listed[0].status(), QuestionStatus::Failed);
    assert!(listed[0].answer_text().is_none());
    assert!(listed[0].certainty().is_none());
    assert_eq!(listed[1].status(), QuestionStatus::Pending);
    assert!(
        store
            .questions
            .list_by_session(SessionId::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_sessions_and_questions_are_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let store = open(dir.path(), &secrets);
    let question = pending(SessionId::new(), "Sessão inexistente?", 1_000);
    assert_eq!(
        store.questions.insert(&question),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        store.questions.update(&question),
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        store.questions.get(question.id()),
        Err(RepositoryError::NotFound)
    );
}

#[test]
fn content_is_not_stored_in_plain_text() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    {
        let store = open(dir.path(), &secrets);
        let (session, _, _) = seed(&store);
        store
            .questions
            .insert(&pending(session, "pergunta-sigilosa-xyz", 3_000))
            .unwrap();
    }
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let needle = b"pergunta-sigilosa-xyz";
        assert!(!bytes.windows(needle.len()).any(|window| window == needle));
    }
}
