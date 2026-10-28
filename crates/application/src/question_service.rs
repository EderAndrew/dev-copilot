//! Contextual questions about a session (FR-034–FR-038, SC-011).
//!
//! ```text
//! question_ask ─▶ pending question stored ─▶ spawned call ─▶ AnalysisProvider::answer
//!                                                              │
//!                     question_answered ◀─ grounded answer ◀───┘
//! ```
//!
//! - A question is stored `pending` and answered off the caller's path: [`QuestionService::ask`]
//!   returns at once and the answer arrives as a `question_answered` event, so a slow or failing
//!   provider never holds up capture or transcription (US5.4). One question per session is in
//!   flight at a time.
//! - The provider sees only this session's finals and record summary (FR-035). Evidence it cites
//!   outside the session is dropped; an answer left without evidence is stored as
//!   `no_evidence` ([`Answer::grounded`](devcopilot_domain::question::Answer::grounded)).
//! - Answers never touch records (FR-037). [`QuestionService::to_record`] creates a *new*
//!   `proposed` record only on explicit user request.
//! - Deleting a session cancels its question in flight ([`QuestionService::cancel_session`]);
//!   whatever the provider still returns is discarded, neither stored nor published.

use std::collections::HashSet;
use std::sync::Arc;

use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{QuestionId, RecordId, SegmentId, SessionId};
use devcopilot_domain::question::{ContextQuestion, EvidenceRef, QuestionStatus};
use devcopilot_domain::record::{RecordKind, TechnicalRecord};
use devcopilot_domain::session::{Session, SessionState};
use devcopilot_domain::text::QuestionText;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

use crate::analysis_service::summary;
use crate::error::ApplicationError;
use crate::events::{ProviderStage, SessionEvent};
use crate::in_flight::InFlight;
use crate::ports::{
    AnalysisProvider, AnalysisProviderFactory, AnalysisSegment, Clock, EventPublisher,
    ProviderError, QuestionRepository, QuestionRequest, RecordRepository, SegmentRepository,
    SessionRepository,
};
use crate::provider_service::ProviderService;

/// Most final segments sent with one question (the most recent ones win).
pub const MAX_SEGMENTS_PER_QUESTION: usize = 400;

/// Collaborators of [`QuestionService`].
pub struct QuestionDeps {
    /// Resolves the analysis provider, which also answers questions.
    pub providers: Arc<ProviderService>,
    /// Builds the provider's adapter.
    pub analyzers: Arc<dyn AnalysisProviderFactory>,
    /// Question storage.
    pub questions: Arc<dyn QuestionRepository>,
    /// Record storage (read for context; written only by [`QuestionService::to_record`]).
    pub records: Arc<dyn RecordRepository>,
    /// Transcript storage.
    pub segments: Arc<dyn SegmentRepository>,
    /// Session storage.
    pub sessions: Arc<dyn SessionRepository>,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Session event delivery.
    pub events: Arc<dyn EventPublisher>,
}

struct Inner {
    deps: QuestionDeps,
    /// Questions in flight (at most one per session).
    in_flight: InFlight,
}

/// Contextual question use cases.
#[derive(Clone)]
pub struct QuestionService {
    inner: Arc<Inner>,
}

impl QuestionService {
    /// Creates the service.
    #[must_use]
    pub fn new(deps: QuestionDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                in_flight: InFlight::default(),
            }),
        }
    }

    /// Asks `question` about a session that has started, during or after capture (FR-034).
    /// Returns the stored `pending` question; the answer arrives as a `question_answered`
    /// event. Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`] for unknown sessions; [`ApplicationError::InvalidState`]
    /// for a session that never started or while another question about it is in flight;
    /// [`ApplicationError::Provider`] when no analysis provider is usable;
    /// [`ApplicationError::Storage`].
    pub fn ask(
        &self,
        session: SessionId,
        question: Redacted<QuestionText>,
    ) -> Result<ContextQuestion, ApplicationError> {
        let deps = &self.inner.deps;
        let session = deps.sessions.get(session)?;
        let id = session.id();
        if session.state() == SessionState::Idle {
            return Err(ApplicationError::InvalidState(
                "questions need a started session".into(),
            ));
        }
        let runtime = Handle::try_current()
            .map_err(|_| ApplicationError::Internal("no async runtime".into()))?;
        let provider = deps
            .providers
            .resolve_analyzer()?
            .ok_or(ApplicationError::Provider(ProviderError::Unavailable))?;
        let analyzer = deps.analyzers.create(&provider)?;
        let ticket = self.inner.in_flight.register_exclusive(id).ok_or_else(|| {
            ApplicationError::InvalidState("a question about this session is pending".into())
        })?;
        let pending = ContextQuestion::ask(
            QuestionId::new(),
            id,
            question,
            Some(provider.id()),
            deps.clock.now(),
        );
        deps.questions.insert(&pending)?;
        deps.events.publish(
            id,
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Questions,
                locality: provider.locality(),
                provider_id: provider.id(),
            },
        );
        let inner = Arc::clone(&self.inner);
        let question = pending.clone();
        runtime.spawn(async move {
            answer(
                &inner,
                question,
                &session,
                analyzer.as_ref(),
                ticket.token(),
            )
            .await;
            drop(ticket);
        });
        tracing::info!(session = %id, question = %pending.id(), "question asked");
        Ok(pending)
    }

    /// Questions of `session`, oldest first (FR-038).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn list(&self, session: SessionId) -> Result<Vec<ContextQuestion>, ApplicationError> {
        Ok(self.inner.deps.questions.list_by_session(session)?)
    }

    /// Turns an answer into a new `proposed`, `suggested` record of `kind` by explicit user
    /// action (FR-037). Existing records are never changed.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`] for unknown questions; [`ApplicationError::InvalidState`]
    /// unless the question was answered; [`ApplicationError::Storage`].
    pub fn to_record(
        &self,
        question: QuestionId,
        kind: RecordKind,
    ) -> Result<TechnicalRecord, ApplicationError> {
        let deps = &self.inner.deps;
        let question = deps.questions.get(question)?;
        let record = question.to_record(RecordId::new(), kind, deps.clock.now())?;
        deps.records.insert(&record, &record.creation())?;
        tracing::info!(question = %question.id(), record = %record.id(), "answer turned into a record");
        Ok(record)
    }

    /// Cancels the question about `session` still in flight, if any, and waits up to
    /// `deadline` for it to end; its outcome is discarded. `false` when it outlived the
    /// deadline.
    pub async fn cancel_session(&self, session: SessionId, deadline: std::time::Duration) -> bool {
        self.inner.in_flight.cancel(session, deadline).await
    }

    /// Marks questions left `pending` by an unexpected exit as failed, so none waits forever.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn recover_on_startup(&self) -> Result<(), ApplicationError> {
        let deps = &self.inner.deps;
        let now = deps.clock.now();
        for session in deps.sessions.list()? {
            for mut question in deps.questions.list_by_session(session.id())? {
                if question.status() == QuestionStatus::Pending {
                    question.fail(now)?;
                    deps.questions.update(&question)?;
                    tracing::warn!(question = %question.id(), "pending question failed on startup");
                }
            }
        }
        Ok(())
    }
}

/// Calls the provider for `question` and stores and publishes the outcome.
async fn answer(
    inner: &Inner,
    mut question: ContextQuestion,
    session: &Session,
    analyzer: &dyn AnalysisProvider,
    cancel: CancellationToken,
) {
    let deps = &inner.deps;
    let id = session.id();
    let outcome = match context(deps, session, &question) {
        Ok((request, finals, records)) => {
            let reply = analyzer.answer(request, cancel.clone()).await;
            if cancel.is_cancelled() {
                tracing::info!(session = %id, question = %question.id(), "question discarded");
                return;
            }
            match reply {
                Ok(output) => {
                    let grounded = output.answer.grounded(|evidence| match evidence {
                        EvidenceRef::Segment(segment) => finals.contains(&segment),
                        EvidenceRef::Record(record) => records.contains(&record),
                    });
                    question.answer(grounded, deps.clock.now())
                }
                Err(err) => {
                    tracing::warn!(session = %id, question = %question.id(), error = %err, "question provider failed");
                    question.fail(deps.clock.now())
                }
            }
        }
        Err(err) => {
            tracing::error!(session = %id, question = %question.id(), error = %err, "question context unavailable");
            question.fail(deps.clock.now())
        }
    };
    if let Err(err) = outcome {
        tracing::error!(session = %id, question = %question.id(), error = %err, "question outcome rejected");
        return;
    }
    if let Err(err) = deps.questions.update(&question) {
        tracing::error!(session = %id, question = %question.id(), error = %err, "question outcome not stored");
        deps.events.publish(
            id,
            SessionEvent::StorageError {
                message_key: "errors.storageError",
            },
        );
        return;
    }
    tracing::info!(session = %id, question = %question.id(), status = question.status().code(), certainty = question.certainty().map(|c| c.code()), "question finished");
    deps.events
        .publish(id, SessionEvent::QuestionAnswered(Box::new(question)));
}

/// The request for `question` and the ids of every final and record of the session.
fn context(
    deps: &QuestionDeps,
    session: &Session,
    question: &ContextQuestion,
) -> Result<(QuestionRequest, HashSet<SegmentId>, HashSet<RecordId>), ApplicationError> {
    let mut segments: Vec<AnalysisSegment> = deps
        .segments
        .list_by_session(session.id())?
        .iter()
        .filter_map(|segment| {
            Some(AnalysisSegment {
                id: segment.id(),
                source: segment.source(),
                start: segment.start(),
                text: segment.text()?.clone(),
            })
        })
        .collect();
    let finals = segments.iter().map(|segment| segment.id).collect();
    let excess = segments.len().saturating_sub(MAX_SEGMENTS_PER_QUESTION);
    segments.drain(..excess);
    let stored = deps.records.list_by_session(session.id())?;
    let records = stored.iter().map(TechnicalRecord::id).collect();
    Ok((
        QuestionRequest {
            session_id: session.id(),
            language: session.language(),
            question: question.question().clone(),
            segments,
            records: stored.iter().map(summary).collect(),
        },
        finals,
        records,
    ))
}
