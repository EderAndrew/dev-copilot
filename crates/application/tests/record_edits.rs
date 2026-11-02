//! User edits, manual records and final analysis (T099, US4.2; FR-026, FR-032, FR-033), plus
//! session history listing and rename (US4.3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::{AnalysisStatus, SessionEvent};
use devcopilot_application::ports::{
    AnalysisOutput, AnalysisRequest, RecordRepository, SessionRepository, TranscriptUpdate,
};
use devcopilot_application::session_service::MAX_PAGE_SIZE;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::{AnalysisOutcome, AnalysisTrigger};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{RecordId, SessionId};
use devcopilot_domain::record::{
    Nature, Origin, Proposal, RecordEdit, RecordKind, RecordStatus, UserRecord,
};
use devcopilot_domain::record_change::{ChangeActor, ChangeField};
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{ListItem, Rationale, RecordText, SegmentText, Title};
use devcopilot_domain::time::Duration;
use support::{Fixture, fixture, wait_until};

fn draft(kind: RecordKind, text: &str, status: RecordStatus) -> UserRecord {
    UserRecord {
        kind,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        rationale: Redacted::new(Rationale::parse("decidido pelo time").unwrap()),
        status,
    }
}

fn text(value: &str) -> Option<Redacted<RecordText>> {
    Some(Redacted::new(RecordText::parse(value).unwrap()))
}

/// A session that captured two finals and ended.
async fn ended_session(f: &Fixture) -> SessionId {
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    for (start, line) in [
        (0, "O export precisa suportar CSV"),
        (3, "Vamos usar Postgres"),
    ] {
        f.transcriber.emit(TranscriptUpdate::Final {
            source: AudioSourceKind::Microphone,
            start: Duration::from_secs(start),
            end: Duration::from_secs(start + 3),
            text: Redacted::new(SegmentText::parse(line).unwrap()),
        });
    }
    wait_until(|| f.service.transcript(id).unwrap().len() == 2).await;
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    id
}

#[test]
fn user_records_are_created_with_history_and_any_status() {
    let f = fixture();
    let session = f.create_mic().id();
    let record = f
        .record_service
        .create(
            session,
            draft(
                RecordKind::Assumption,
                "Equipe de três pessoas",
                RecordStatus::Confirmed,
            ),
        )
        .unwrap();
    assert_eq!(record.origin(), Origin::User);
    assert_eq!(record.status(), RecordStatus::Confirmed);
    assert_eq!(record.nature(), Nature::Suggested);
    let history = f.record_service.history(record.id()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actor(), ChangeActor::User);
    assert_eq!(
        f.record_service.create(
            SessionId::new(),
            draft(RecordKind::Risk, "x", RecordStatus::Pending)
        ),
        Err(ApplicationError::NotFound)
    );
}

#[test]
fn edits_are_saved_and_recorded_per_field() {
    let f = fixture();
    let session = f.create_mic().id();
    let task = f
        .record_service
        .create(
            session,
            draft(RecordKind::Task, "Criar endpoint", RecordStatus::Confirmed),
        )
        .unwrap();
    f.clock.advance(Duration::from_secs(30));
    let edited = f
        .record_service
        .edit(
            task.id(),
            RecordEdit {
                text: text("Criar endpoint de export"),
                acceptance_criteria: Some(vec![Redacted::new(
                    ListItem::parse("Pagina de 100 em 100").unwrap(),
                )]),
                ..RecordEdit::default()
            },
        )
        .unwrap();
    assert_eq!(edited.text().expose().as_str(), "Criar endpoint de export");
    assert_eq!(
        edited.status(),
        RecordStatus::Confirmed,
        "editing keeps the status"
    );
    assert_eq!(f.records.get(task.id()).unwrap(), edited);
    let fields: Vec<ChangeField> = f
        .record_service
        .history(task.id())
        .unwrap()
        .iter()
        .map(|c| c.field())
        .collect();
    assert_eq!(
        fields,
        [
            ChangeField::Created,
            ChangeField::Text,
            ChangeField::AcceptanceCriteria
        ]
    );
}

#[test]
fn invalid_edits_change_nothing() {
    let f = fixture();
    let session = f.create_mic().id();
    let risk = f
        .record_service
        .create(
            session,
            draft(RecordKind::Risk, "Prazo curto", RecordStatus::Pending),
        )
        .unwrap();
    assert!(matches!(
        f.record_service.edit(risk.id(), RecordEdit::default()),
        Err(ApplicationError::InvalidInput { field: "edit", .. })
    ));
    assert!(matches!(
        f.record_service.edit(
            risk.id(),
            RecordEdit {
                alternatives: Some(vec![Redacted::new(ListItem::parse("x").unwrap())]),
                ..RecordEdit::default()
            }
        ),
        Err(ApplicationError::InvalidInput {
            field: "alternatives",
            ..
        })
    ));
    assert_eq!(f.records.get(risk.id()).unwrap(), risk);
    assert_eq!(
        f.record_service.edit(
            RecordId::new(),
            RecordEdit {
                text: text("x"),
                ..RecordEdit::default()
            }
        ),
        Err(ApplicationError::NotFound)
    );
}

#[tokio::test]
async fn final_analysis_runs_only_on_ended_or_failed_sessions() {
    let f = fixture();
    f.add_analyzer();
    let idle = f.create_mic();
    assert!(matches!(
        f.analysis.run_final(&idle),
        Err(ApplicationError::InvalidState(_))
    ));

    let id = ended_session(&f).await;
    let confirmed = f
        .record_service
        .create(
            id,
            draft(
                RecordKind::Decision,
                "Usar Postgres",
                RecordStatus::Confirmed,
            ),
        )
        .unwrap();
    f.analyzer.respond_with(Arc::new(|req: &AnalysisRequest| {
        Ok(AnalysisOutput {
            proposals: vec![Proposal {
                kind: RecordKind::Requirement,
                text: Redacted::new(RecordText::parse("Export em CSV").unwrap()),
                nature: Nature::Observed,
                rationale: Redacted::new(Rationale::default()),
                evidence: vec![req.segments[0].id],
                acceptance_criteria: Vec::new(),
                alternatives: Vec::new(),
                relates_to: None,
            }],
            tokens_used: 50,
        })
    }));
    let session = f.service.get(id).unwrap();
    let run_id = f.analysis.run_final(&session).unwrap();
    wait_until(|| {
        f.events(id).iter().any(|e| {
            matches!(e, SessionEvent::AnalysisStatus { run_id: r, status: AnalysisStatus::Ok { created: 1 } } if *r == run_id)
        })
    })
    .await;
    let run = f.runs.all().into_iter().find(|r| r.id() == run_id).unwrap();
    assert_eq!(run.trigger(), AnalysisTrigger::Final);
    assert_eq!(run.outcome(), Some(AnalysisOutcome::Ok { created: 1 }));
    // Every final of the session was sent, and existing records were summarized.
    let request = f.analyzer.requests().pop().unwrap();
    assert_eq!(request.segments.len(), 2);
    assert_eq!(request.records[0].id, confirmed.id());
    // Same rules as incremental analysis: only a new proposal; the confirmed record is intact.
    assert_eq!(f.records.get(confirmed.id()).unwrap(), confirmed);
    let records = f.records.list_by_session(id).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1].status(), RecordStatus::Proposed);
}

#[tokio::test]
async fn one_final_analysis_at_a_time_and_a_provider_is_required() {
    let f = fixture();
    let id = ended_session(&f).await;
    let session = f.service.get(id).unwrap();
    assert!(matches!(
        f.analysis.run_final(&session),
        Err(ApplicationError::Provider(_))
    ));
    f.add_analyzer();
    f.analyzer.hold();
    f.analysis.run_final(&session).unwrap();
    assert!(matches!(
        f.analysis.run_final(&session),
        Err(ApplicationError::InvalidState(_))
    ));
    f.analyzer.release();
    wait_until(|| f.runs.all().iter().all(|r| r.outcome().is_some())).await;
    wait_until(|| f.analysis.run_final(&session).is_ok()).await;
}

#[test]
fn history_pages_newest_first_and_renames() {
    let f = fixture();
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(f.create_mic().id());
        f.clock.advance(Duration::from_secs(1));
    }
    let first = f.service.list_page(None, 2).unwrap();
    assert_eq!(
        first.items.iter().map(|s| s.id()).collect::<Vec<_>>(),
        [ids[4], ids[3]]
    );
    let second = f.service.list_page(first.next_cursor, 2).unwrap();
    assert_eq!(
        second.items.iter().map(|s| s.id()).collect::<Vec<_>>(),
        [ids[2], ids[1]]
    );
    let last = f.service.list_page(second.next_cursor, 2).unwrap();
    assert_eq!(last.items.len(), 1);
    assert_eq!(last.next_cursor, None);
    for limit in [0, MAX_PAGE_SIZE + 1] {
        assert!(f.service.list_page(None, limit).is_err());
    }
    assert!(f.service.list_page(Some(SessionId::new()), 2).is_err());

    let renamed = f
        .service
        .rename(ids[0], Title::parse("Retro").unwrap())
        .unwrap();
    assert_eq!(renamed.title().as_str(), "Retro");
    assert_eq!(f.sessions.get(ids[0]).unwrap().title().as_str(), "Retro");
}

#[tokio::test]
async fn renaming_an_active_session_keeps_it_capturing() {
    let f = fixture();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    f.service
        .rename(id, Title::parse("Planning ao vivo").unwrap())
        .unwrap();
    assert_eq!(f.state(id), SessionState::Capturing);
    assert_eq!(
        f.service.get(id).unwrap().title().as_str(),
        "Planning ao vivo"
    );
    assert_eq!(
        f.sessions.get(id).unwrap().title().as_str(),
        "Planning ao vivo"
    );
}
