//! Contextual questions (T109, US5; FR-034–FR-038, FR-053b, SC-011).
//!
//! Answers are grounded in the session (foreign or unknown evidence is dropped, leaving
//! `no_evidence`), never change records, become a record only by explicit request, and run
//! beside capture without holding it up.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::{ProviderStage, SessionEvent};
use devcopilot_application::ports::{
    AnswerOutput, ProviderError, QuestionRepository, QuestionRequest, RecordRepository,
    TranscriptUpdate,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{QuestionId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::question::{
    Answer, Certainty, ContextQuestion, EvidenceRef, QuestionStatus,
};
use devcopilot_domain::record::{Nature, Origin, RecordKind, RecordStatus, UserRecord};
use devcopilot_domain::record_change::ChangeField;
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{AnswerText, QuestionText, Rationale, RecordText, SegmentText};
use devcopilot_domain::time::Duration;
use support::{Fixture, fixture, wait_until};

fn question(text: &str) -> Redacted<QuestionText> {
    Redacted::new(QuestionText::parse(text).unwrap())
}

fn reply(text: &str, certainty: Certainty, evidence: Vec<EvidenceRef>) -> AnswerOutput {
    AnswerOutput {
        answer: Answer {
            text: Redacted::new(AnswerText::parse(text).unwrap()),
            certainty,
            evidence,
        },
        tokens_used: 20,
    }
}

fn emit(f: &Fixture, start: u64, line: &str) {
    f.transcriber.emit(TranscriptUpdate::Final {
        source: AudioSourceKind::Microphone,
        start: Duration::from_secs(start),
        end: Duration::from_secs(start + 3),
        text: Redacted::new(SegmentText::parse(line).unwrap()),
    });
}

/// A capturing session with the given finals persisted; returns its id and segment ids.
async fn capturing_with(f: &Fixture, lines: &[&str]) -> (SessionId, Vec<SegmentId>) {
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    for (i, line) in lines.iter().enumerate() {
        emit(f, i as u64 * 3, line);
    }
    wait_until(|| f.service.transcript(id).unwrap().len() == lines.len()).await;
    let segments = f
        .service
        .transcript(id)
        .unwrap()
        .iter()
        .map(|segment| segment.id())
        .collect();
    (id, segments)
}

/// [`capturing_with`], then stopped and ended.
async fn ended_with(f: &Fixture, lines: &[&str]) -> (SessionId, Vec<SegmentId>) {
    let (id, segments) = capturing_with(f, lines).await;
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    (id, segments)
}

fn confirmed_decision(f: &Fixture, session: SessionId, text: &str) -> RecordId {
    f.record_service
        .create(
            session,
            UserRecord {
                kind: RecordKind::Decision,
                text: Redacted::new(RecordText::parse(text).unwrap()),
                rationale: Redacted::new(Rationale::default()),
                status: RecordStatus::Confirmed,
            },
        )
        .unwrap()
        .id()
}

async fn finished(f: &Fixture, id: QuestionId) -> ContextQuestion {
    wait_until(|| f.questions.get(id).unwrap().status() != QuestionStatus::Pending).await;
    f.questions.get(id).unwrap()
}

fn answered_events(f: &Fixture, session: SessionId) -> Vec<ContextQuestion> {
    f.events(session)
        .into_iter()
        .filter_map(|event| match event {
            SessionEvent::QuestionAnswered(question) => Some(*question),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn grounded_answers_cite_session_segments_and_records() {
    let f = fixture();
    let provider = f.add_analyzer();
    let (id, segments) = ended_with(
        &f,
        &[
            "Bom dia a todos",
            "Ficou decidido usar OAuth2 na autenticação",
        ],
    )
    .await;
    let decision = confirmed_decision(&f, id, "Usar OAuth2");
    f.analyzer.push_answer(Ok(reply(
        "Foi decidido usar OAuth2.",
        Certainty::Supported,
        vec![
            EvidenceRef::Segment(segments[1]),
            EvidenceRef::Record(decision),
        ],
    )));

    let pending = f
        .question_service
        .ask(id, question("O que foi decidido sobre autenticação?"))
        .unwrap();
    assert_eq!(pending.status(), QuestionStatus::Pending);
    assert_eq!(pending.provider_id(), Some(provider));

    let done = finished(&f, pending.id()).await;
    assert_eq!(done.status(), QuestionStatus::Answered);
    assert_eq!(done.certainty(), Some(Certainty::Supported));
    assert_eq!(
        done.evidence(),
        [
            EvidenceRef::Segment(segments[1]),
            EvidenceRef::Record(decision)
        ]
    );
    assert_eq!(
        done.answer_text().unwrap().expose().as_str(),
        "Foi decidido usar OAuth2."
    );
    // Persisted with the session (FR-038) and delivered as an event.
    assert_eq!(
        f.question_service.list(id).unwrap(),
        std::slice::from_ref(&done)
    );
    assert_eq!(answered_events(&f, id), [done]);
    assert!(f.events(id).iter().any(|event| matches!(
        event,
        SessionEvent::ProviderInUse {
            stage: ProviderStage::Questions,
            provider_id,
            ..
        } if *provider_id == provider
    )));
    // The provider saw the question with this session's content only (FR-035).
    let request = f.analyzer.questions().pop().unwrap();
    assert_eq!(request.session_id, id);
    assert_eq!(
        request.question.expose().as_str(),
        "O que foi decidido sobre autenticação?"
    );
    assert_eq!(
        request.segments.iter().map(|s| s.id).collect::<Vec<_>>(),
        segments
    );
    assert_eq!(request.records.len(), 1);
    assert_eq!(request.records[0].id, decision);
}

#[tokio::test]
async fn invalid_evidence_is_dropped_leaving_no_evidence() {
    let f = fixture();
    f.add_analyzer();
    let (other, other_segments) = ended_with(&f, &["Outra reunião sobre filas"]).await;
    let foreign_record = confirmed_decision(&f, other, "Usar RabbitMQ");
    let (id, _) = ended_with(&f, &["Falamos só do cronograma"]).await;
    f.analyzer.push_answer(Ok(reply(
        "Decidiram usar RabbitMQ.",
        Certainty::Supported,
        vec![
            EvidenceRef::Segment(other_segments[0]),
            EvidenceRef::Segment(SegmentId::new()),
            EvidenceRef::Record(foreign_record),
            EvidenceRef::Record(RecordId::new()),
        ],
    )));

    let asked = f
        .question_service
        .ask(id, question("Qual fila foi escolhida?"))
        .unwrap();
    let done = finished(&f, asked.id()).await;
    assert_eq!(done.status(), QuestionStatus::Answered);
    assert_eq!(done.certainty(), Some(Certainty::NoEvidence));
    assert!(done.evidence().is_empty());
    // The other session's content never reached the provider.
    let request = f.analyzer.questions().pop().unwrap();
    assert!(request.segments.iter().all(|s| s.id != other_segments[0]));
    assert!(request.records.is_empty());
}

#[tokio::test]
async fn partial_evidence_keeps_only_what_is_in_the_session() {
    let f = fixture();
    f.add_analyzer();
    let (id, segments) = ended_with(&f, &["O cache expira em uma hora"]).await;
    f.analyzer.push_answer(Ok(reply(
        "O cache expira em uma hora; a invalidação não foi discutida.",
        Certainty::Partial,
        vec![
            EvidenceRef::Segment(SegmentId::new()),
            EvidenceRef::Segment(segments[0]),
            EvidenceRef::Segment(segments[0]),
        ],
    )));
    let asked = f
        .question_service
        .ask(id, question("Como funciona o cache?"))
        .unwrap();
    let done = finished(&f, asked.id()).await;
    assert_eq!(done.certainty(), Some(Certainty::Partial));
    assert_eq!(done.evidence(), [EvidenceRef::Segment(segments[0])]);
}

#[tokio::test]
async fn answers_never_change_records_until_converted() {
    let f = fixture();
    f.add_analyzer();
    let (id, segments) = ended_with(&f, &["Ficou decidido usar OAuth2"]).await;
    let decision = confirmed_decision(&f, id, "Usar OAuth2");
    let before = f.records.all();
    let history_before = f.records.changes();
    f.analyzer.push_answer(Ok(reply(
        "Sugiro trocar OAuth2 por SSO corporativo e rejeitar a decisão atual.",
        Certainty::Partial,
        vec![
            EvidenceRef::Segment(segments[0]),
            EvidenceRef::Record(decision),
        ],
    )));

    let asked = f
        .question_service
        .ask(id, question("Devemos mudar a autenticação?"))
        .unwrap();
    let done = finished(&f, asked.id()).await;
    assert_eq!(done.status(), QuestionStatus::Answered);
    // US5.3: the confirmed decision is untouched and nothing was added.
    assert_eq!(f.records.all(), before);
    assert_eq!(f.records.changes(), history_before);
    assert_eq!(
        f.records.get(decision).unwrap().status(),
        RecordStatus::Confirmed
    );

    // By explicit action the answer becomes a new proposed suggestion.
    let record = f
        .question_service
        .to_record(asked.id(), RecordKind::Decision)
        .unwrap();
    assert_eq!(record.status(), RecordStatus::Proposed);
    assert_eq!(record.nature(), Nature::Suggested);
    assert_eq!(record.origin(), Origin::User);
    assert_eq!(record.session_id(), id);
    assert_eq!(record.evidence(), [segments[0]]);
    assert_eq!(f.records.all().len(), before.len() + 1);
    assert_eq!(f.records.get(decision).unwrap(), before[0]);
    let new_changes: Vec<_> = f
        .records
        .changes()
        .into_iter()
        .skip(history_before.len())
        .collect();
    assert_eq!(new_changes.len(), 1);
    assert_eq!(new_changes[0].field(), ChangeField::Created);
    assert_eq!(new_changes[0].record_id(), record.id());
}

#[tokio::test]
async fn only_answered_questions_become_records() {
    let f = fixture();
    f.add_analyzer();
    let (id, _) = ended_with(&f, &["Nada relevante"]).await;
    f.analyzer.hold();
    let asked = f
        .question_service
        .ask(id, question("Algo foi decidido?"))
        .unwrap();
    assert!(matches!(
        f.question_service.to_record(asked.id(), RecordKind::Task),
        Err(ApplicationError::InvalidState(_))
    ));
    f.analyzer.release();
    finished(&f, asked.id()).await;
    assert_eq!(
        f.question_service
            .to_record(QuestionId::new(), RecordKind::Task),
        Err(ApplicationError::NotFound)
    );
}

#[tokio::test]
async fn questions_run_beside_capture_without_blocking_it() {
    let f = fixture();
    f.add_analyzer();
    let (id, _) = capturing_with(&f, &["Vamos começar pelo login"]).await;
    f.analyzer.hold();

    // Asking returns at once while the provider is still thinking.
    let asked = tokio::time::timeout(std::time::Duration::from_millis(200), async {
        f.question_service
            .ask(id, question("O que foi dito sobre login?"))
    })
    .await
    .unwrap()
    .unwrap();
    wait_until(|| f.analyzer.questions().len() == 1).await;

    // Capture and transcription go on meanwhile.
    emit(&f, 10, "O login usa OAuth2");
    emit(&f, 13, "E o logout invalida o token");
    wait_until(|| f.service.transcript(id).unwrap().len() == 3).await;
    assert_eq!(f.state(id), SessionState::Capturing);
    assert_eq!(
        f.questions.get(asked.id()).unwrap().status(),
        QuestionStatus::Pending
    );

    f.analyzer.release();
    let done = finished(&f, asked.id()).await;
    assert_eq!(done.status(), QuestionStatus::Answered);
    assert_eq!(f.state(id), SessionState::Capturing);
}

#[tokio::test]
async fn provider_failures_fail_the_question_and_leave_capture_alone() {
    let f = fixture();
    f.add_analyzer();
    let (id, _) = capturing_with(&f, &["Primeiro trecho"]).await;
    let before = f.records.all();
    f.analyzer.push_answer(Err(ProviderError::Timeout));

    let asked = f
        .question_service
        .ask(id, question("Qual o prazo?"))
        .unwrap();
    let done = finished(&f, asked.id()).await;
    assert_eq!(done.status(), QuestionStatus::Failed);
    assert!(done.answer_text().is_none());
    assert!(done.answered_at().is_some());
    assert_eq!(answered_events(&f, id), [done]);

    emit(&f, 5, "Segundo trecho");
    wait_until(|| f.service.transcript(id).unwrap().len() == 2).await;
    assert_eq!(f.state(id), SessionState::Capturing);
    assert_eq!(f.records.all(), before);

    // A failed question frees the session for the next one.
    f.analyzer.push_answer(Err(ProviderError::InvalidOutput));
    let again = f.question_service.ask(id, question("E agora?")).unwrap();
    assert_eq!(
        finished(&f, again.id()).await.status(),
        QuestionStatus::Failed
    );
}

#[tokio::test]
async fn cited_evidence_keeps_its_original_language() {
    let f = fixture();
    f.add_analyzer();
    // A pt-BR session where someone spoke English.
    let (id, segments) = ended_with(
        &f,
        &[
            "Vamos falar da API",
            "We agreed on rate limiting at 100 requests per minute",
        ],
    )
    .await;
    f.analyzer
        .answer_with(Arc::new(move |request: &QuestionRequest| {
            Ok(reply(
                "O limite combinado é de 100 requisições por minuto.",
                Certainty::Supported,
                vec![EvidenceRef::Segment(request.segments[1].id)],
            ))
        }));
    let asked = f
        .question_service
        .ask(id, question("Qual o limite de requisições?"))
        .unwrap();
    let done = finished(&f, asked.id()).await;

    // The answer is asked for in the session language (FR-053b)...
    let request = f.analyzer.questions().pop().unwrap();
    assert_eq!(request.language, SessionLanguage::PtBr);
    // ...and evidence is a reference to the stored segment, whose text stays as spoken.
    assert_eq!(done.evidence(), [EvidenceRef::Segment(segments[1])]);
    let cited = f
        .service
        .transcript(id)
        .unwrap()
        .into_iter()
        .find(|segment| segment.id() == segments[1])
        .unwrap();
    assert_eq!(
        cited.text().unwrap().expose().as_str(),
        "We agreed on rate limiting at 100 requests per minute"
    );
}

#[tokio::test]
async fn one_question_per_session_at_a_time() {
    let f = fixture();
    f.add_analyzer();
    let (first, _) = ended_with(&f, &["Trecho A"]).await;
    let (second, _) = ended_with(&f, &["Trecho B"]).await;
    f.analyzer.hold();
    let asked = f
        .question_service
        .ask(first, question("Primeira?"))
        .unwrap();
    assert!(matches!(
        f.question_service.ask(first, question("Segunda?")),
        Err(ApplicationError::InvalidState(_))
    ));
    // Other sessions are independent.
    let other = f
        .question_service
        .ask(second, question("Outra sessão?"))
        .unwrap();
    f.analyzer.release();
    finished(&f, asked.id()).await;
    finished(&f, other.id()).await;
    assert!(
        f.question_service
            .ask(first, question("Agora sim?"))
            .is_ok()
    );
}

#[tokio::test]
async fn questions_need_a_started_session_and_a_provider() {
    let f = fixture();
    let idle = f.create_mic().id();
    f.add_analyzer();
    assert!(matches!(
        f.question_service.ask(idle, question("Algo?")),
        Err(ApplicationError::InvalidState(_))
    ));
    assert_eq!(
        f.question_service.ask(SessionId::new(), question("Algo?")),
        Err(ApplicationError::NotFound)
    );

    let g = fixture();
    let (id, _) = ended_with(&g, &["Sem provedor de análise"]).await;
    assert_eq!(
        g.question_service.ask(id, question("Algo?")),
        Err(ApplicationError::Provider(ProviderError::Unavailable))
    );
    assert!(g.questions.all().is_empty());
}

#[tokio::test]
async fn pending_questions_fail_on_startup() {
    let f = fixture();
    f.add_analyzer();
    let (id, _) = ended_with(&f, &["Trecho"]).await;
    f.analyzer.hold();
    let asked = f
        .question_service
        .ask(id, question("Ficou pendente?"))
        .unwrap();
    // Simulates an exit: the stored question stays pending.
    f.question_service.recover_on_startup().unwrap();
    let recovered = f.questions.get(asked.id()).unwrap();
    assert_eq!(recovered.status(), QuestionStatus::Failed);
    f.analyzer.release();
}
