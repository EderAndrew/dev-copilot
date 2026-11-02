//! Session deletion (T120, US6; FR-042–FR-044, SC-012).
//!
//! A deleted session leaves nothing behind in lists, questions, analysis or exports; an active
//! session cannot be deleted; analysis runs and questions still in flight are cancelled and
//! their results discarded.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::BTreeSet;
use std::time::Duration as StdDuration;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::SessionEvent;
use devcopilot_application::export_service::ExportRequest;
use devcopilot_application::ports::{
    AnalysisOutput, AnalysisRunRepository, RecordRepository, SegmentRepository, TranscriptUpdate,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::record::{Nature, Proposal, RecordKind, RecordStatus, UserRecord};
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{QuestionText, Rationale, RecordText, SegmentText};
use devcopilot_domain::time::Duration;
use support::{Fixture, fixture, wait_until};

fn emit(f: &Fixture, start: u64, line: &str) {
    f.transcriber.emit(TranscriptUpdate::Final {
        source: AudioSourceKind::Microphone,
        start: Duration::from_secs(start),
        end: Duration::from_secs(start + 3),
        text: Redacted::new(SegmentText::parse(line).unwrap()),
    });
}

/// An ended session with two finals.
async fn ended(f: &Fixture) -> SessionId {
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    emit(f, 0, "O export precisa suportar CSV.");
    emit(f, 3, "Decidimos usar a fila existente.");
    wait_until(|| f.service.transcript(id).unwrap().len() == 2).await;
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    id
}

fn user_record(f: &Fixture, id: SessionId, text: &str) {
    f.record_service
        .create(
            id,
            UserRecord {
                kind: RecordKind::Requirement,
                text: Redacted::new(RecordText::parse(text).unwrap()),
                rationale: Redacted::new(Rationale::parse("").unwrap()),
                status: RecordStatus::Confirmed,
            },
        )
        .unwrap();
}

fn ask(f: &Fixture, id: SessionId, text: &str) {
    f.question_service
        .ask(id, Redacted::new(QuestionText::parse(text).unwrap()))
        .unwrap();
}

fn request(id: SessionId) -> ExportRequest {
    ExportRequest {
        session_id: id,
        formats: BTreeSet::from([ExportFormat::Markdown, ExportFormat::Json]),
        content: BTreeSet::from([ExportContent::Plan, ExportContent::Transcript]),
    }
}

fn proposal(text: &str) -> Proposal {
    Proposal {
        kind: RecordKind::Risk,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature: Nature::Inferred,
        rationale: Redacted::new(Rationale::parse("").unwrap()),
        evidence: Vec::new(),
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        relates_to: None,
    }
}

#[tokio::test]
async fn deleted_content_is_absent_everywhere() {
    let f = fixture();
    f.add_analyzer();
    let id = ended(&f).await;
    let kept = ended(&f).await;
    user_record(&f, id, "Exportar em CSV");
    user_record(&f, kept, "Manter a fila");
    ask(&f, id, "Qual formato de exportação?");
    wait_until(|| {
        f.question_service.list(id).unwrap()[0]
            .answered_at()
            .is_some()
    })
    .await;
    f.analysis.run_final(&f.service.get(id).unwrap()).unwrap();
    wait_until(|| {
        f.runs
            .list_by_session(id)
            .unwrap()
            .iter()
            .all(|run| run.outcome().is_some())
    })
    .await;
    f.export_service.record(&request(id)).unwrap();

    f.service.delete(id).await.unwrap();

    // Lists and lookups.
    assert!(f.service.list().unwrap().iter().all(|s| s.id() != id));
    assert!(matches!(f.service.get(id), Err(ApplicationError::NotFound)));
    assert!(matches!(
        f.service.transcript(id),
        Err(ApplicationError::NotFound)
    ));
    assert!(f.segments.list_by_session(id).unwrap().is_empty());
    // Plan and analysis.
    assert!(f.record_service.list(id).unwrap().is_empty());
    assert!(f.records.all().iter().all(|r| r.session_id() != id));
    assert!(
        f.records
            .changes()
            .iter()
            .all(|c| f.records.get(c.record_id()).is_ok())
    );
    assert!(f.runs.list_by_session(id).unwrap().is_empty());
    // Questions: none listed, and none can be asked.
    assert!(f.question_service.list(id).unwrap().is_empty());
    assert!(matches!(
        f.question_service.ask(
            id,
            Redacted::new(QuestionText::parse("Ainda existe?").unwrap())
        ),
        Err(ApplicationError::NotFound)
    ));
    // Exports: no history, nothing to render.
    assert!(f.export_service.history(id).unwrap().is_empty());
    assert!(matches!(
        f.export_service.render(&request(id)),
        Err(ApplicationError::NotFound)
    ));

    // The other session is untouched.
    assert_eq!(f.service.transcript(kept).unwrap().len(), 2);
    assert_eq!(f.record_service.list(kept).unwrap().len(), 1);
    let rendered = f.export_service.render(&request(kept)).unwrap();
    assert!(
        rendered
            .iter()
            .all(|doc| !doc.contents.expose().contains("Exportar em CSV"))
    );
}

#[tokio::test]
async fn idle_session_can_be_deleted() {
    let f = fixture();
    let id = f.create_mic().id();
    f.service.delete(id).await.unwrap();
    assert_eq!(f.sessions.deleted(), [id]);
    assert!(matches!(f.service.get(id), Err(ApplicationError::NotFound)));
}

#[tokio::test]
async fn unknown_session_is_not_found() {
    let f = fixture();
    assert!(matches!(
        f.service.delete(SessionId::new()).await,
        Err(ApplicationError::NotFound)
    ));
}

#[tokio::test]
async fn active_session_cannot_be_deleted() {
    let f = fixture();
    // Starting: the permission prompt is still open.
    f.mic.hold_permission_prompt();
    let starting = f.create_mic().id();
    f.service.start(starting, true).unwrap();
    assert!(matches!(
        f.service.delete(starting).await,
        Err(ApplicationError::InvalidState(_))
    ));
    f.mic.answer_permission_prompt();
    f.wait_for_state(starting, SessionState::Capturing).await;
    // Capturing.
    assert!(matches!(
        f.service.delete(starting).await,
        Err(ApplicationError::InvalidState(_))
    ));
    assert_eq!(f.state(starting), SessionState::Capturing);
    assert!(f.mic.is_capturing());
    assert!(f.sessions.deleted().is_empty());
    // Stopped: now it can go.
    f.service.stop(starting).unwrap();
    f.wait_for_state(starting, SessionState::Ended).await;
    f.service.delete(starting).await.unwrap();
}

#[tokio::test]
async fn rejected_deletion_leaves_the_session_usable() {
    let f = fixture();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    assert!(f.service.delete(id).await.is_err());
    // The rejected deletion leaves no trace: the session stops and deletes normally.
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    f.service.delete(id).await.unwrap();
    assert!(matches!(
        f.service.delete(id).await,
        Err(ApplicationError::NotFound)
    ));
}

#[tokio::test]
async fn in_flight_final_analysis_is_cancelled_and_discarded() {
    let f = fixture();
    f.add_analyzer();
    let id = ended(&f).await;
    f.analyzer.push(Ok(AnalysisOutput {
        proposals: vec![proposal("Fila pode saturar")],
        tokens_used: 10,
    }));
    f.analyzer.hold();
    f.analysis.run_final(&f.service.get(id).unwrap()).unwrap();
    wait_until(|| f.analyzer.call_count() == 1).await;

    tokio::time::timeout(StdDuration::from_secs(2), f.service.delete(id))
        .await
        .expect("deletion waits only for the cancelled run")
        .unwrap();
    f.analyzer.release();
    tokio::time::sleep(StdDuration::from_millis(20)).await;

    assert!(f.records.all().is_empty());
    assert!(f.runs.all().is_empty());
    let late = f.events(id);
    assert!(
        !late
            .iter()
            .any(|event| matches!(event, SessionEvent::RecordProposed(_)))
    );
}

#[tokio::test]
async fn in_flight_question_is_cancelled_and_discarded() {
    let f = fixture();
    f.add_analyzer();
    let id = ended(&f).await;
    f.analyzer.hold();
    ask(&f, id, "Qual formato de exportação?");
    wait_until(|| f.analyzer.questions().len() == 1).await;

    tokio::time::timeout(StdDuration::from_secs(2), f.service.delete(id))
        .await
        .expect("deletion waits only for the cancelled question")
        .unwrap();
    f.analyzer.release();
    tokio::time::sleep(StdDuration::from_millis(20)).await;

    assert!(f.questions.all().is_empty());
    assert!(
        !f.events(id)
            .iter()
            .any(|event| matches!(event, SessionEvent::QuestionAnswered(_)))
    );
    // Asking about another session is not blocked by the cancelled one.
    let other = ended(&f).await;
    ask(&f, other, "E agora?");
}

#[tokio::test]
async fn draining_incremental_analysis_is_cancelled_before_deletion() {
    let f = fixture();
    f.add_analyzer();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    f.analyzer.push(Ok(AnalysisOutput {
        proposals: vec![proposal("Fila pode saturar")],
        tokens_used: 10,
    }));
    f.analyzer.hold();
    // Six finals trigger an incremental run with the default limits.
    for i in 0..6 {
        emit(&f, i * 3, "A fila atual pode saturar com o volume novo.");
    }
    wait_until(|| f.analyzer.call_count() == 1).await;
    // Stop lets the run drain in the background for up to the drain deadline.
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;

    tokio::time::timeout(StdDuration::from_secs(2), f.service.delete(id))
        .await
        .expect("deletion cancels the draining run instead of waiting it out")
        .unwrap();
    f.analyzer.release();
    tokio::time::sleep(StdDuration::from_millis(20)).await;

    assert!(f.records.all().is_empty());
    assert!(f.runs.all().is_empty());
    assert!(f.segments.all().is_empty());
}
