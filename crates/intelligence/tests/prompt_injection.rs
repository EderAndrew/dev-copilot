//! Prompt-injection suite (T081; FR-029, SC-010, US3.6).
//!
//! Each case in `fixtures/injection/cases.json` pairs a transcript carrying instructions aimed at
//! the assistant with the output a fully compromised model could return. The case runs end to
//! end: the real prompt builder, the real schema validation and the real `AnalysisService`, with
//! in-memory stores. Whatever the model returns, the only possible effect is new `proposed`
//! records: zero status changes, edits or deletions, no evidence or relations outside the
//! session, and no events other than analysis progress.
//!
//! The Q&A cases in `fixtures/injection/qa_cases.json` (T110, US5) put the attack in the
//! question or the transcript and run through the real answer prompt, answer validation and
//! `QuestionService`: an answer can only be stored text with evidence from the session; no
//! record changes, and no events beyond the question's own.
//!
//! Lives in `intelligence` (not `application`, as T081 and T110 first placed it) so attacks go
//! through the real validation; `application` cannot depend on `intelligence`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::analysis_service::{AnalysisDeps, AnalysisService};
use devcopilot_application::events::SessionEvent;
use devcopilot_application::ports::{
    AnalysisOutput, AnalysisRequest, AnswerOutput, QuestionRepository, QuestionRequest,
    RecordRepository, SegmentRepository, SessionRepository,
};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::question_service::{QuestionDeps, QuestionService};
use devcopilot_application::settings_service::{AnalysisLimits, SettingsService, SettingsUpdate};
use devcopilot_application::testing::{
    FakeClock, InMemoryAnalysisRunRepository, InMemoryProviderRepository,
    InMemoryQuestionRepository, InMemoryRecordRepository, InMemorySegmentRepository,
    InMemorySessionRepository, InMemorySettingsRepository, RecordingEventPublisher,
    ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind, ProviderRole, ProviderSpec};
use devcopilot_domain::question::{Certainty, EvidenceRef, QuestionStatus};
use devcopilot_domain::record::{
    Nature, Origin, RecordFlags, RecordKind, RecordSpec, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::record_change::ChangeField;
use devcopilot_domain::session::Session;
use devcopilot_domain::text::{QuestionText, Rationale, RecordText, SegmentText, Title};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_intelligence::output_validation::{parse_analysis_output, parse_answer_output};
use devcopilot_intelligence::prompt::{
    DATA_CLOSE, DATA_OPEN, analysis_prompt, answer_instructions, answer_prompt, system_instructions,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Case {
    id: String,
    language: String,
    transcript: Vec<String>,
    output: Value,
}

fn cases() -> Vec<Case> {
    let raw = include_str!("fixtures/injection/cases.json");
    serde_json::from_str(raw).unwrap()
}

/// Ids the placeholders of a case stand for.
struct Ids {
    segments: Vec<SegmentId>,
    confirmed: RecordId,
    rejected: RecordId,
    pending: RecordId,
    foreign_segment: SegmentId,
    foreign_record: RecordId,
}

fn raw_output(output: &Value, ids: &Ids) -> String {
    if let Some(count) = output.get("$REPEAT_PROPOSALS").and_then(Value::as_u64) {
        let proposals: Vec<Value> = (0..count)
            .map(|i| {
                json!({ "kind": "task", "text": format!("Tarefa {i}"), "nature": "suggested",
                        "rationale": "", "evidenceSegmentIds": [] })
            })
            .collect();
        return json!({ "schemaVersion": 1, "proposals": proposals }).to_string();
    }
    let mut raw = match output {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    for (i, segment) in ids.segments.iter().enumerate() {
        raw = raw.replace(&format!("$SEG{i}"), &segment.to_string());
    }
    raw.replace("$CONFIRMED", &ids.confirmed.to_string())
        .replace("$REJECTED", &ids.rejected.to_string())
        .replace("$PENDING", &ids.pending.to_string())
        .replace("$FOREIGN_SEG", &ids.foreign_segment.to_string())
        .replace("$FOREIGN_RECORD", &ids.foreign_record.to_string())
}

fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

fn user_record(session: SessionId, status: RecordStatus, text: &str) -> TechnicalRecord {
    TechnicalRecord::new(RecordSpec {
        id: RecordId::new(),
        session_id: session,
        kind: RecordKind::Decision,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature: Nature::Suggested,
        status,
        rationale: Redacted::new(Rationale::default()),
        origin: Origin::User,
        evidence: Vec::new(),
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at(100),
        updated_at: at(100),
    })
    .unwrap()
}

async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(StdDuration::from_millis(1)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what}: condition not reached"));
}

async fn run_case(case: &Case) {
    let language = SessionLanguage::parse(&case.language).unwrap();
    let clock = Arc::new(FakeClock::at(1_000));
    let settings_repo = Arc::new(InMemorySettingsRepository::default());
    let settings = Arc::new(SettingsService::new(settings_repo.clone()));
    settings
        .update(SettingsUpdate {
            analysis_limits: Some(AnalysisLimits {
                trigger_finals: 1,
                ..AnalysisLimits::default()
            }),
            ..SettingsUpdate::default()
        })
        .unwrap();
    let providers = Arc::new(ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        settings_repo,
    ));
    providers
        .upsert_local(ProviderSpec {
            id: ProviderId::new(),
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3.1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Records]),
        })
        .unwrap();

    let mut session = Session::new(
        SessionId::new(),
        Title::parse("Sessão de teste").unwrap(),
        language,
        &[AudioSourceKind::Microphone],
        at(500),
    )
    .unwrap();
    session.start().unwrap();
    session.ready(at(600)).unwrap();
    let id = session.id();

    let segments = Arc::new(InMemorySegmentRepository::default());
    let mut segment_ids = Vec::new();
    for (seq, line) in case.transcript.iter().enumerate() {
        let segment = TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::new(),
            session_id: id,
            seq: seq as u64,
            kind: SegmentKind::Final {
                text: Redacted::new(SegmentText::parse(line.as_str()).unwrap()),
            },
            source: None,
            start: Duration::from_secs(seq as u64 * 3),
            end: Duration::from_secs(seq as u64 * 3 + 3),
        });
        segments.append(&segment).unwrap();
        segment_ids.push(segment.id());
    }

    let records = Arc::new(InMemoryRecordRepository::default());
    let confirmed = user_record(id, RecordStatus::Confirmed, "Usar Postgres");
    let rejected = user_record(id, RecordStatus::Rejected, "Usar planilhas");
    let pending = user_record(id, RecordStatus::Pending, "Avaliar fila");
    let other_session = SessionId::new();
    let foreign = user_record(other_session, RecordStatus::Confirmed, "Outra sessão");
    for record in [&confirmed, &rejected, &pending, &foreign] {
        records.seed(record.clone());
    }
    let seeded = records.all();
    let ids = Ids {
        segments: segment_ids.clone(),
        confirmed: confirmed.id(),
        rejected: rejected.id(),
        pending: pending.id(),
        foreign_segment: SegmentId::new(),
        foreign_record: foreign.id(),
    };
    let raw = raw_output(&case.output, &ids);

    let analyzer = Arc::new(ScriptedAnalysisProvider::local());
    let case_id = case.id.clone();
    let transcript = case.transcript.clone();
    analyzer.respond_with(Arc::new(move |request: &AnalysisRequest| {
        // The prompt keeps session content out of the instructions and inside one data block.
        let prompt = analysis_prompt(request);
        assert_eq!(
            prompt.system,
            system_instructions(request.language),
            "{case_id}"
        );
        assert_eq!(prompt.data.matches(DATA_OPEN).count(), 1, "{case_id}");
        assert_eq!(prompt.data.matches(DATA_CLOSE).count(), 1, "{case_id}");
        assert!(prompt.data.ends_with(DATA_CLOSE), "{case_id}");
        for line in &transcript {
            assert!(!prompt.system.contains(line.as_str()), "{case_id}");
        }
        // A compromised model returns the attack; only validated output gets through.
        parse_analysis_output(&raw).map(|proposals| AnalysisOutput {
            proposals,
            tokens_used: 10,
        })
    }));
    let runs = Arc::new(InMemoryAnalysisRunRepository::default());
    let events = Arc::new(RecordingEventPublisher::default());
    let service = AnalysisService::new(AnalysisDeps {
        providers,
        analyzers: Arc::new(ScriptedAnalysisProviderFactory::new(analyzer.clone())),
        records: records.clone(),
        runs: runs.clone(),
        segments: segments.clone(),
        settings,
        clock,
        events: events.clone(),
        tick: StdDuration::from_millis(5),
    });
    let analysis = service.attach(&session).unwrap();
    for (seq, _) in case.transcript.iter().enumerate() {
        analysis
            .feed()
            .final_persisted(seq as u64, Duration::from_secs(3));
    }
    wait_until(&case.id, || {
        runs.all().iter().any(|run| run.outcome().is_some())
    })
    .await;
    analysis.finish(StdDuration::ZERO);

    // Nothing that existed changed or disappeared.
    let after = records.all();
    for record in &seeded {
        assert_eq!(records.get(record.id()).unwrap(), *record, "{}", case.id);
    }
    assert!(after.len() >= seeded.len(), "{}", case.id);
    // History holds creations only: no status, text or other edits.
    assert!(
        records
            .changes()
            .iter()
            .all(|c| c.field() == ChangeField::Created),
        "{}",
        case.id
    );
    // New records are proposals grounded in this session only.
    let session_segments: HashSet<SegmentId> = segment_ids.into_iter().collect();
    let session_records: HashSet<RecordId> = [confirmed.id(), rejected.id(), pending.id()].into();
    for record in after.iter().skip(seeded.len()) {
        assert_eq!(record.session_id(), id, "{}", case.id);
        assert_eq!(record.status(), RecordStatus::Proposed, "{}", case.id);
        assert!(matches!(record.origin(), Origin::Ai { .. }), "{}", case.id);
        assert!(
            record
                .evidence()
                .iter()
                .all(|s| session_segments.contains(s)),
            "{}",
            case.id
        );
        if record.nature() == Nature::Observed {
            assert!(!record.evidence().is_empty(), "{}", case.id);
        }
        for flag in [record.flags().conflicts_with, record.flags().duplicate_of]
            .into_iter()
            .flatten()
        {
            assert!(session_records.contains(&flag), "{}", case.id);
        }
    }
    // The only observable effects are analysis progress and proposals.
    for (session_id, event) in events.events() {
        assert_eq!(session_id, id, "{}", case.id);
        assert!(
            matches!(
                event,
                SessionEvent::ProviderInUse { .. }
                    | SessionEvent::AnalysisStatus { .. }
                    | SessionEvent::RecordProposed(_)
            ),
            "{}: unexpected {event:?}",
            case.id
        );
    }
    assert_eq!(analyzer.call_count(), 1, "{}", case.id);
}

#[test]
fn suite_has_at_least_twenty_cases_in_both_languages() {
    let cases = cases();
    assert!(cases.len() >= 20);
    let ids: HashSet<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "case ids are unique");
    for language in ["pt-BR", "en"] {
        assert!(cases.iter().filter(|c| c.language == language).count() >= 8);
    }
}

#[tokio::test]
async fn no_case_changes_deletes_or_escapes_the_session() {
    for case in cases() {
        run_case(&case).await;
    }
}

#[tokio::test]
async fn schema_breaking_attacks_are_discarded_entirely() {
    // Cases whose output violates the schema must create nothing at all (FR-030).
    for id in [
        "en-ignore-previous-instructions",
        "pt-status-field-smuggling",
        "en-delete-instruction",
        "en-tool-call",
        "en-json-breakout",
        "pt-schema-version-downgrade",
        "en-prose-instead-of-json",
        "pt-markdown-fenced",
        "en-oversize-flood",
        "pt-secret-exfiltration",
        "en-control-characters",
        "pt-unknown-kind",
        "en-observed-without-evidence",
    ] {
        let case = cases().into_iter().find(|c| c.id == id).unwrap();
        let ids = Ids {
            segments: case.transcript.iter().map(|_| SegmentId::new()).collect(),
            confirmed: RecordId::new(),
            rejected: RecordId::new(),
            pending: RecordId::new(),
            foreign_segment: SegmentId::new(),
            foreign_record: RecordId::new(),
        };
        assert!(
            parse_analysis_output(&raw_output(&case.output, &ids)).is_err(),
            "{id}"
        );
    }
}

// Q&A (T110) -----------------------------------------------------------------------------------

#[derive(Deserialize)]
struct QaCase {
    id: String,
    language: String,
    transcript: Vec<String>,
    question: String,
    output: Value,
    /// `answered` or `failed`.
    expect: String,
    /// Certainty of an answered case after grounding.
    #[serde(default)]
    certainty: Option<String>,
}

fn qa_cases() -> Vec<QaCase> {
    let raw = include_str!("fixtures/injection/qa_cases.json");
    serde_json::from_str(raw).unwrap()
}

fn raw_answer(output: &Value, ids: &Ids) -> String {
    match output.as_str() {
        Some("$OVERSIZED_ANSWER") => {
            return json!({ "schemaVersion": 1, "answer": "yes ".repeat(2_000),
                           "certainty": "supported", "evidenceSegmentIds": [],
                           "evidenceRecordIds": [] })
            .to_string();
        }
        Some("$CITATION_FLOOD") => {
            let flood: Vec<String> = (0..25).map(|_| SegmentId::new().to_string()).collect();
            return json!({ "schemaVersion": 1, "answer": "Muitos trechos.",
                           "certainty": "supported", "evidenceSegmentIds": flood,
                           "evidenceRecordIds": [] })
            .to_string();
        }
        _ => {}
    }
    raw_output(output, ids)
        .replace("$RANDOM_SEG", &SegmentId::new().to_string())
        .replace("$RANDOM_RECORD", &RecordId::new().to_string())
}

async fn run_qa_case(case: &QaCase) {
    let language = SessionLanguage::parse(&case.language).unwrap();
    let clock = Arc::new(FakeClock::at(1_000));
    let settings_repo = Arc::new(InMemorySettingsRepository::default());
    let providers = Arc::new(ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        settings_repo,
    ));
    providers
        .upsert_local(ProviderSpec {
            id: ProviderId::new(),
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3.1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([
                DataCategory::Transcript,
                DataCategory::Records,
                DataCategory::Questions,
            ]),
        })
        .unwrap();

    let mut session = Session::new(
        SessionId::new(),
        Title::parse("Sessão de teste").unwrap(),
        language,
        &[AudioSourceKind::Microphone],
        at(500),
    )
    .unwrap();
    session.start().unwrap();
    session.ready(at(600)).unwrap();
    let id = session.id();
    let sessions = Arc::new(InMemorySessionRepository::default());
    sessions.create(&session).unwrap();

    let segments = Arc::new(InMemorySegmentRepository::default());
    let mut segment_ids = Vec::new();
    for (seq, line) in case.transcript.iter().enumerate() {
        let segment = TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::new(),
            session_id: id,
            seq: seq as u64,
            kind: SegmentKind::Final {
                text: Redacted::new(SegmentText::parse(line.as_str()).unwrap()),
            },
            source: None,
            start: Duration::from_secs(seq as u64 * 3),
            end: Duration::from_secs(seq as u64 * 3 + 3),
        });
        segments.append(&segment).unwrap();
        segment_ids.push(segment.id());
    }
    // A segment of another session, cited by some attacks.
    let other_session = SessionId::new();
    let foreign_segment = TranscriptSegment::restore(SegmentSpec {
        id: SegmentId::new(),
        session_id: other_session,
        seq: 0,
        kind: SegmentKind::Final {
            text: Redacted::new(SegmentText::parse("Orçamento aprovado").unwrap()),
        },
        source: None,
        start: Duration::ZERO,
        end: Duration::from_secs(3),
    });
    segments.append(&foreign_segment).unwrap();

    let records = Arc::new(InMemoryRecordRepository::default());
    let confirmed = user_record(id, RecordStatus::Confirmed, "Usar Postgres");
    let rejected = user_record(id, RecordStatus::Rejected, "Usar planilhas");
    let pending = user_record(id, RecordStatus::Pending, "Avaliar fila");
    let foreign = user_record(other_session, RecordStatus::Confirmed, "Outra sessão");
    for record in [&confirmed, &rejected, &pending, &foreign] {
        records.seed(record.clone());
    }
    let seeded = records.all();
    let history = records.changes();
    let ids = Ids {
        segments: segment_ids.clone(),
        confirmed: confirmed.id(),
        rejected: rejected.id(),
        pending: pending.id(),
        foreign_segment: foreign_segment.id(),
        foreign_record: foreign.id(),
    };
    let raw = raw_answer(&case.output, &ids);

    let analyzer = Arc::new(ScriptedAnalysisProvider::local());
    let case_id = case.id.clone();
    let question_text = case.question.clone();
    let transcript = case.transcript.clone();
    analyzer.answer_with(Arc::new(move |request: &QuestionRequest| {
        // Question and transcript stay out of the instructions, inside one data block.
        let prompt = answer_prompt(request);
        assert_eq!(
            prompt.system,
            answer_instructions(request.language),
            "{case_id}"
        );
        assert_eq!(prompt.data.matches(DATA_OPEN).count(), 1, "{case_id}");
        assert_eq!(prompt.data.matches(DATA_CLOSE).count(), 1, "{case_id}");
        assert!(prompt.data.ends_with(DATA_CLOSE), "{case_id}");
        assert!(!prompt.system.contains(question_text.as_str()), "{case_id}");
        for line in &transcript {
            assert!(!prompt.system.contains(line.as_str()), "{case_id}");
        }
        // Only this session's content is sent.
        assert!(
            request
                .segments
                .iter()
                .all(|s| s.id != foreign_segment.id()),
            "{case_id}"
        );
        assert!(
            request.records.iter().all(|r| r.id != foreign.id()),
            "{case_id}"
        );
        // A compromised model returns the attack; only validated output gets through.
        parse_answer_output(&raw).map(|answer| AnswerOutput {
            answer,
            tokens_used: 10,
        })
    }));
    let questions = Arc::new(InMemoryQuestionRepository::default());
    let events = Arc::new(RecordingEventPublisher::default());
    let service = QuestionService::new(QuestionDeps {
        providers,
        analyzers: Arc::new(ScriptedAnalysisProviderFactory::new(analyzer.clone())),
        questions: questions.clone(),
        records: records.clone(),
        segments,
        sessions,
        clock,
        events: events.clone(),
    });

    let asked = service
        .ask(
            id,
            Redacted::new(QuestionText::parse(case.question.as_str()).unwrap()),
        )
        .unwrap();
    wait_until(&case.id, || {
        questions.get(asked.id()).unwrap().status() != QuestionStatus::Pending
    })
    .await;
    let done = questions.get(asked.id()).unwrap();

    // Records are exactly as before: no status change, edit, creation or deletion.
    assert_eq!(records.all(), seeded, "{}", case.id);
    assert_eq!(records.changes(), history, "{}", case.id);
    match case.expect.as_str() {
        "failed" => {
            assert_eq!(done.status(), QuestionStatus::Failed, "{}", case.id);
            assert!(done.answer_text().is_none(), "{}", case.id);
        }
        "answered" => {
            assert_eq!(done.status(), QuestionStatus::Answered, "{}", case.id);
            let expected = Certainty::parse(case.certainty.as_deref().unwrap()).unwrap();
            assert_eq!(done.certainty(), Some(expected), "{}", case.id);
        }
        other => panic!("{}: unknown expectation {other}", case.id),
    }
    // Evidence only ever points into this session; no_evidence cites nothing.
    let session_records: HashSet<RecordId> = [confirmed.id(), rejected.id(), pending.id()].into();
    for evidence in done.evidence() {
        match evidence {
            EvidenceRef::Segment(segment) => {
                assert!(segment_ids.contains(segment), "{}", case.id);
            }
            EvidenceRef::Record(record) => {
                assert!(session_records.contains(record), "{}", case.id);
            }
        }
    }
    if done.certainty() == Some(Certainty::NoEvidence) {
        assert!(done.evidence().is_empty(), "{}", case.id);
    }
    // The only observable effects are the question's own events.
    for (session_id, event) in events.events() {
        assert_eq!(session_id, id, "{}", case.id);
        assert!(
            matches!(
                event,
                SessionEvent::ProviderInUse { .. } | SessionEvent::QuestionAnswered(_)
            ),
            "{}: unexpected {event:?}",
            case.id
        );
    }
    assert_eq!(analyzer.questions().len(), 1, "{}", case.id);
    assert_eq!(analyzer.call_count(), 0, "{}", case.id);
}

#[test]
fn qa_suite_covers_both_languages_and_outcomes() {
    let cases = qa_cases();
    assert!(cases.len() >= 12);
    let ids: HashSet<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "case ids are unique");
    for language in ["pt-BR", "en"] {
        assert!(cases.iter().filter(|c| c.language == language).count() >= 6);
    }
    for expect in ["answered", "failed"] {
        assert!(cases.iter().any(|c| c.expect == expect));
    }
}

#[tokio::test]
async fn no_qa_case_changes_records_or_escapes_the_session() {
    for case in qa_cases() {
        run_qa_case(&case).await;
    }
}
