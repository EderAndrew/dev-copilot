//! Session export (T119, US6; FR-041, FR-053b, contracts/export-format.md).
//!
//! The JSON matches the v1 example fixture, the Markdown matches its snapshots, content
//! selection is honoured, nothing identifying providers, credentials or paths is written, and an
//! export taken while an analysis runs holds only what was persisted.
//!
//! Set `DEVCOPILOT_UPDATE_SNAPSHOTS=1` to rewrite the fixtures after an intended change, then
//! review the diff.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;

use devcopilot_application::ApplicationError;
use devcopilot_application::export_service::{ExportDocument, ExportRequest};
use devcopilot_application::ports::{AnalysisOutput, RecordRepository, TranscriptUpdate};
use devcopilot_application::settings_service::SettingsUpdate;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::{
    AnalysisRunId, ChangeId, ProviderId, QuestionId, RecordId, SegmentId, SessionId,
};
use devcopilot_domain::language::{SessionLanguage, UiLanguage};
use devcopilot_domain::question::{
    Certainty, ContextQuestion, EvidenceRef, QuestionSpec, QuestionStatus,
};
use devcopilot_domain::record::{
    Nature, Origin, Proposal, RecordFlags, RecordKind, RecordSpec, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::record_change::{ChangeActor, ChangeField, RecordChange, RecordChangeSpec};
use devcopilot_domain::session::{Session, SessionSnapshot, SessionState};
use devcopilot_domain::text::{
    AnswerText, ListItem, QuestionText, Rationale, RecordText, SegmentText, Title,
};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, SegmentKind, SegmentSpec, TranscriptSegment};
use serde_json::Value;
use support::{Fixture, fixture, wait_until};

const SESSION: &str = "0192f3a1-0000-7000-8000-000000000001";
const SEG_CSV: &str = "0192f3a1-0000-7000-8000-000000000011";
const SEG_GAP: &str = "0192f3a1-0000-7000-8000-000000000012";
const SEG_QUEUE: &str = "0192f3a1-0000-7000-8000-000000000013";
const REC_REQUIREMENT: &str = "0192f3a1-0000-7000-8000-000000000021";
const REC_DECISION: &str = "0192f3a1-0000-7000-8000-000000000022";
const REC_TASK: &str = "0192f3a1-0000-7000-8000-000000000023";
const RUN: &str = "0192f3a1-0000-7000-8000-000000000031";
const PROVIDER: &str = "0192f3a1-0000-7000-8000-0000000000aa";
const QUESTION_ANSWERED: &str = "0192f3a1-0000-7000-8000-000000000041";
const QUESTION_FAILED: &str = "0192f3a1-0000-7000-8000-000000000042";

/// 2026-10-27T17:00:00Z.
const START_MS: i64 = 1_793_120_400_000;

fn at(minutes_after_start: i64) -> Timestamp {
    Timestamp::from_unix_millis(START_MS + minutes_after_start * 60_000).unwrap()
}

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/001-meeting-sessions/contracts/examples")
}

fn items(values: &[&str]) -> Vec<Redacted<ListItem>> {
    values
        .iter()
        .map(|v| Redacted::new(ListItem::parse(*v).unwrap()))
        .collect()
}

fn record(
    id: &str,
    kind: RecordKind,
    text: &str,
    nature: Nature,
    status: RecordStatus,
    origin: Origin,
    evidence: &[&str],
) -> RecordSpec {
    RecordSpec {
        id: RecordId::parse(id).unwrap(),
        session_id: SessionId::parse(SESSION).unwrap(),
        kind,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature,
        status,
        rationale: Redacted::new(Rationale::parse("").unwrap()),
        origin,
        evidence: evidence
            .iter()
            .map(|id| SegmentId::parse(id).unwrap())
            .collect(),
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at(2),
        updated_at: at(2),
    }
}

/// An ended session with a gap, three records (one confirmed by the user), an answered and a
/// failed question.
fn seed(f: &Fixture) -> SessionId {
    let session = SessionId::parse(SESSION).unwrap();
    f.sessions.seed(
        Session::restore(SessionSnapshot {
            id: session,
            title: Title::parse("Revisão da API de exportação").unwrap(),
            language: SessionLanguage::PtBr,
            state: SessionState::Ended,
            sources: vec![AudioSourceKind::Microphone],
            created_at: at(-5),
            started_at: Some(at(0)),
            ended_at: Some(at(45)),
            active_duration: Duration::from_secs(2_580),
            failure: None,
        })
        .unwrap(),
    );
    let segment = |id: &str, seq: u64, kind: SegmentKind, start: u64, end: u64| {
        TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::parse(id).unwrap(),
            session_id: session,
            seq,
            source: matches!(kind, SegmentKind::Final { .. })
                .then_some(AudioSourceKind::Microphone),
            kind,
            start: Duration::from_millis(start),
            end: Duration::from_millis(end),
        })
    };
    let text = |t: &str| SegmentKind::Final {
        text: Redacted::new(SegmentText::parse(t).unwrap()),
    };
    use devcopilot_application::ports::SegmentRepository as _;
    for segment in [
        segment(SEG_CSV, 0, text("O export precisa suportar CSV."), 0, 4_200),
        segment(
            SEG_GAP,
            1,
            SegmentKind::Gap {
                reason: GapReason::Paused,
            },
            4_200,
            64_200,
        ),
        segment(
            SEG_QUEUE,
            2,
            text("Vamos usar a fila existente, sem <b>HTML</b> nem [links](http://exemplo)."),
            64_200,
            70_000,
        ),
    ] {
        f.segments.append(&segment).unwrap();
    }

    let ai = Origin::Ai {
        provider_id: ProviderId::parse(PROVIDER).unwrap(),
        analysis_run_id: AnalysisRunId::parse(RUN).unwrap(),
    };
    let mut requirement = record(
        REC_REQUIREMENT,
        RecordKind::Requirement,
        "O export precisa suportar CSV",
        Nature::Observed,
        RecordStatus::Proposed,
        ai,
        &[SEG_CSV],
    );
    requirement.rationale = Redacted::new(Rationale::parse("Pedido explícito.").unwrap());
    f.records
        .seed(TechnicalRecord::restore(requirement.clone()));
    requirement.status = RecordStatus::Confirmed;
    requirement.updated_at = at(30);
    f.records
        .update(
            &TechnicalRecord::restore(requirement),
            &[RecordChange::restore(RecordChangeSpec {
                id: ChangeId::new(),
                record_id: RecordId::parse(REC_REQUIREMENT).unwrap(),
                actor: ChangeActor::User,
                field: ChangeField::Status,
                old_value: Some(Redacted::new("proposed".into())),
                new_value: Some(Redacted::new("confirmed".into())),
                at: at(30),
            })],
        )
        .unwrap();
    let mut decision = record(
        REC_DECISION,
        RecordKind::Decision,
        "Usar a fila existente",
        Nature::Inferred,
        RecordStatus::Proposed,
        ai,
        &[SEG_QUEUE],
    );
    decision.alternatives = items(&["Criar uma fila nova"]);
    decision.created_at = at(3);
    decision.updated_at = at(3);
    f.records.seed(TechnicalRecord::restore(decision));
    let mut task = record(
        REC_TASK,
        RecordKind::Task,
        "Implementar exportação CSV",
        Nature::Suggested,
        RecordStatus::Pending,
        Origin::User,
        &[],
    );
    task.acceptance_criteria = items(&["Gera um arquivo .csv válido", "Inclui cabeçalho"]);
    task.created_at = at(40);
    task.updated_at = at(40);
    f.records.seed(TechnicalRecord::restore(task));

    use devcopilot_application::ports::QuestionRepository as _;
    let question = |id: &str, text: &str| QuestionSpec {
        id: QuestionId::parse(id).unwrap(),
        session_id: session,
        question: Redacted::new(QuestionText::parse(text).unwrap()),
        answer: None,
        certainty: None,
        evidence: Vec::new(),
        status: QuestionStatus::Failed,
        provider_id: Some(ProviderId::parse(PROVIDER).unwrap()),
        asked_at: at(50),
        answered_at: Some(at(51)),
    };
    f.questions
        .insert(&ContextQuestion::restore(QuestionSpec {
            answer: Some(Redacted::new(
                AnswerText::parse("CSV, conforme pedido na reunião.").unwrap(),
            )),
            certainty: Some(Certainty::Supported),
            evidence: vec![
                EvidenceRef::Segment(SegmentId::parse(SEG_CSV).unwrap()),
                EvidenceRef::Record(RecordId::parse(REC_REQUIREMENT).unwrap()),
            ],
            status: QuestionStatus::Answered,
            ..question(QUESTION_ANSWERED, "Qual formato de exportação foi pedido?")
        }))
        .unwrap();
    f.questions
        .insert(&ContextQuestion::restore(question(
            QUESTION_FAILED,
            "Quem aprova o prazo?",
        )))
        .unwrap();
    // exportedAt: 2026-10-27T18:00:00Z.
    f.clock.set(at(60));
    session
}

fn request(
    session: SessionId,
    formats: &[ExportFormat],
    content: &[ExportContent],
) -> ExportRequest {
    ExportRequest {
        session_id: session,
        formats: formats.iter().copied().collect(),
        content: content.iter().copied().collect(),
    }
}

fn everything(session: SessionId) -> ExportRequest {
    request(
        session,
        &[ExportFormat::Markdown, ExportFormat::Json],
        &[ExportContent::Plan, ExportContent::Transcript],
    )
}

fn document(docs: &[ExportDocument], format: ExportFormat) -> &ExportDocument {
    docs.iter().find(|doc| doc.format == format).unwrap()
}

fn json_of(docs: &[ExportDocument]) -> Value {
    serde_json::from_str(document(docs, ExportFormat::Json).contents.expose()).unwrap()
}

/// Compares `actual` with the example file `name`, or rewrites it when asked to.
fn assert_snapshot(name: &str, actual: &str) {
    let path = examples_dir().join(name);
    if std::env::var_os("DEVCOPILOT_UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    let expected =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    if name.ends_with(".json") {
        let expected: Value = serde_json::from_str(&expected).unwrap();
        let actual: Value = serde_json::from_str(actual).unwrap();
        assert_eq!(actual, expected, "{name} differs from the example");
    } else {
        assert_eq!(actual, expected, "{name} differs from the snapshot");
    }
}

#[test]
fn json_matches_the_v1_example() {
    let f = fixture();
    let session = seed(&f);
    let docs = f.export_service.render(&everything(session)).unwrap();
    let json = document(&docs, ExportFormat::Json);
    assert_eq!(
        json.file_name,
        "revisao-da-api-de-exportacao-2026-10-27.json"
    );
    assert_snapshot("session-export.v1.json", json.contents.expose());
}

#[test]
fn markdown_matches_snapshots_with_labels_in_the_interface_language() {
    let f = fixture();
    let session = seed(&f);
    let docs = f.export_service.render(&everything(session)).unwrap();
    let markdown = document(&docs, ExportFormat::Markdown);
    assert_eq!(
        markdown.file_name,
        "revisao-da-api-de-exportacao-2026-10-27.md"
    );
    assert_snapshot("session-export.pt-BR.md", markdown.contents.expose());

    f.settings
        .update(SettingsUpdate {
            ui_language: Some(UiLanguage::En),
            ..SettingsUpdate::default()
        })
        .unwrap();
    let docs = f.export_service.render(&everything(session)).unwrap();
    let markdown = document(&docs, ExportFormat::Markdown).contents.expose();
    assert_snapshot("session-export.en.md", markdown);
    // Labels change; the content stays in the session language (FR-053b).
    assert!(markdown.contains("## Plan") && markdown.contains("O export precisa suportar CSV"));
}

#[test]
fn markdown_escapes_session_text() {
    let f = fixture();
    let session = seed(&f);
    let docs = f.export_service.render(&everything(session)).unwrap();
    let markdown = document(&docs, ExportFormat::Markdown).contents.expose();
    assert!(!markdown.contains("<b>"));
    assert!(!markdown.contains("[links](http"));
    assert!(markdown.contains("\\<b\\>HTML\\</b\\>"));
}

#[test]
fn content_selection_is_honoured() {
    let f = fixture();
    let session = seed(&f);

    let docs = f
        .export_service
        .render(&request(
            session,
            &[ExportFormat::Json, ExportFormat::Markdown],
            &[ExportContent::Transcript],
        ))
        .unwrap();
    let json = json_of(&docs);
    assert!(json.get("transcript").is_some());
    assert!(json.get("records").is_none() && json.get("questions").is_none());
    let markdown = document(&docs, ExportFormat::Markdown).contents.expose();
    assert!(markdown.contains("## Transcrição"));
    assert!(!markdown.contains("## Plano") && !markdown.contains("## Perguntas e respostas"));

    let docs = f
        .export_service
        .render(&request(
            session,
            &[ExportFormat::Json, ExportFormat::Markdown],
            &[ExportContent::Plan],
        ))
        .unwrap();
    let json = json_of(&docs);
    assert!(json.get("transcript").is_none());
    assert_eq!(json["records"].as_array().unwrap().len(), 3);
    // Only answered questions are exported.
    assert_eq!(json["questions"].as_array().unwrap().len(), 1);
    let markdown = document(&docs, ExportFormat::Markdown).contents.expose();
    assert!(markdown.contains("## Plano") && !markdown.contains("## Transcrição"));
    // Plan items still cite their evidence by timestamp.
    assert!(markdown.contains("evidência: 00:00:00"));

    let docs = f
        .export_service
        .render(&request(
            session,
            &[ExportFormat::Markdown],
            &[ExportContent::Plan],
        ))
        .unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].format, ExportFormat::Markdown);
}

#[test]
fn nothing_identifies_providers_credentials_or_paths() {
    let f = fixture();
    let session = seed(&f);
    let docs = f.export_service.render(&everything(session)).unwrap();
    for doc in &docs {
        let contents = doc.contents.expose();
        assert!(!contents.contains(PROVIDER), "{:?}", doc.format);
        for forbidden in [
            "providerId",
            "provider_id",
            "credential",
            "endpoint",
            "secret",
        ] {
            assert!(
                !contents.contains(forbidden),
                "{forbidden} in {:?}",
                doc.format
            );
        }
        assert!(!contents.contains(env!("CARGO_MANIFEST_DIR")));
        assert!(!doc.file_name.contains('/') && !doc.file_name.contains('\\'));
    }
    let json = json_of(&docs);
    assert_eq!(
        json["records"][0]["origin"],
        serde_json::json!({ "type": "ai", "analysisRunId": RUN })
    );
}

#[test]
fn empty_selection_and_unknown_sessions_are_rejected() {
    let f = fixture();
    let session = seed(&f);
    assert!(matches!(
        f.export_service
            .render(&request(session, &[], &[ExportContent::Plan])),
        Err(ApplicationError::InvalidInput {
            field: "formats",
            ..
        })
    ));
    assert!(matches!(
        f.export_service
            .render(&request(session, &[ExportFormat::Json], &[])),
        Err(ApplicationError::InvalidInput {
            field: "content",
            ..
        })
    ));
    assert!(matches!(
        f.export_service.render(&everything(SessionId::new())),
        Err(ApplicationError::NotFound)
    ));
    assert!(matches!(
        f.export_service
            .file_names(SessionId::new(), &BTreeSet::from([ExportFormat::Json])),
        Err(ApplicationError::NotFound)
    ));
}

#[test]
fn record_keeps_formats_content_and_time_but_no_path() {
    let f = fixture();
    let session = seed(&f);
    let export = f
        .export_service
        .record(&request(
            session,
            &[ExportFormat::Json],
            &[ExportContent::Plan],
        ))
        .unwrap();
    assert_eq!(export.formats(), &BTreeSet::from([ExportFormat::Json]));
    assert_eq!(export.content(), &BTreeSet::from([ExportContent::Plan]));
    assert_eq!(export.at(), at(60));
    assert_eq!(f.export_service.history(session).unwrap(), [export]);
    assert_eq!(f.exports.all().len(), 1);
}

#[tokio::test]
async fn export_during_analysis_holds_only_persisted_content() {
    let f = fixture();
    f.add_analyzer();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    f.transcriber.emit(TranscriptUpdate::Final {
        source: AudioSourceKind::Microphone,
        start: Duration::from_secs(0),
        end: Duration::from_secs(3),
        text: Redacted::new(SegmentText::parse("A fila pode saturar.").unwrap()),
    });
    wait_until(|| f.service.transcript(id).unwrap().len() == 1).await;
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;

    f.analyzer.push(Ok(AnalysisOutput {
        proposals: vec![Proposal {
            kind: RecordKind::Risk,
            text: Redacted::new(RecordText::parse("Saturação da fila").unwrap()),
            nature: Nature::Inferred,
            rationale: Redacted::new(Rationale::parse("").unwrap()),
            evidence: Vec::new(),
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            relates_to: None,
        }],
        tokens_used: 10,
    }));
    f.analyzer.hold();
    f.analysis.run_final(&f.service.get(id).unwrap()).unwrap();
    wait_until(|| f.analyzer.call_count() == 1).await;

    let during = f.export_service.render(&everything(id)).unwrap();
    f.analyzer.release();
    wait_until(|| f.records.list_by_session(id).unwrap().len() == 1).await;

    assert_eq!(json_of(&during)["records"], serde_json::json!([]));
    for doc in &during {
        assert!(!doc.contents.expose().contains("Saturação da fila"));
    }
    let after = f.export_service.render(&everything(id)).unwrap();
    assert_eq!(json_of(&after)["records"].as_array().unwrap().len(), 1);
}
