//! Session use cases: create, start, cancel start, stop, and startup recovery (FR-001–FR-012,
//! FR-039a, FR-045a). Pause and resume arrive with US9 (T145).
//!
//! While capturing, the audio queue feeds the session's [`Transcription`], which persists
//! finals and gaps as they arrive (US2).
//!
//! Controls never wait for audio or providers (SC-006): `start` checks the preconditions,
//! moves the session to `starting` and returns while a background task asks for permission and
//! opens the sources; `stop` closes the sources, switches the indicator off and returns while
//! a background task drains in-flight work for at most [`DRAIN_DEADLINE`].
//!
//! At most one session is active at a time (FR-006). The active session lives in memory under
//! one lock together with its pipeline; every state change is persisted and published while
//! that lock is held, so events reflect the order of transitions.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use devcopilot_domain::audio_source::{AudioSourceKind, Availability, PermissionState};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, Session, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::transcript::GapReason;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

use crate::capture_pipeline::{CapturePipeline, FaultStream, OpenError, indicator};
use crate::error::ApplicationError;
use crate::events::{ProviderStage, SessionEvent, SourceStatus};
use crate::ports::{
    AudioError, AudioSource, Clock, EventPublisher, SecretStore, SegmentRepository,
    SessionRepository, SourceFault, Transcriber, TranscriberFactory,
};
use crate::provider_service::ProviderService;
use crate::settings_service::SettingsService;
use crate::transcription::{Transcription, TranscriptionDeps};

/// How long `stop` lets in-flight processing finish before cancelling it (SC-002).
pub const DRAIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// i18n key published with `storage_error` when a state change cannot be persisted.
const STORAGE_ERROR_KEY: &str = "errors.storageError";

/// Collaborators of [`SessionService`].
pub struct SessionDeps {
    /// Session storage.
    pub sessions: Arc<dyn SessionRepository>,
    /// Transcript storage.
    pub segments: Arc<dyn SegmentRepository>,
    /// Builds the transcriber of the resolved provider (FR-045a).
    pub transcribers: Arc<dyn TranscriberFactory>,
    /// Resolves the transcription provider (FR-045a).
    pub providers: Arc<ProviderService>,
    /// Consent-reminder preference (FR-012).
    pub settings: Arc<SettingsService>,
    /// OS credential store (FR-039a).
    pub secrets: Arc<dyn SecretStore>,
    /// Audio sources registered on this platform, at most one per kind.
    pub sources: Vec<Arc<dyn AudioSource>>,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Session event delivery.
    pub events: Arc<dyn EventPublisher>,
}

/// Input of [`SessionService::create`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    /// Title.
    pub title: Title,
    /// Session language.
    pub language: SessionLanguage,
    /// Selected sources.
    pub sources: Vec<AudioSourceKind>,
}

/// A registered source as listed to the user (FR-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceInfo {
    /// Kind.
    pub kind: AudioSourceKind,
    /// Whether it can be selected, and why not.
    pub availability: Availability,
    /// Current OS permission (queried, never stored).
    pub permission: PermissionState,
}

struct Active {
    session: Session,
    cancel: CancellationToken,
    pipeline: Option<CapturePipeline>,
    transcription: Option<Transcription>,
}

struct Inner {
    deps: SessionDeps,
    active: Mutex<Option<Active>>,
}

/// Session lifecycle use cases.
#[derive(Clone)]
pub struct SessionService {
    inner: Arc<Inner>,
}

fn failure_for(error: &AudioError) -> FailureReason {
    match error {
        AudioError::PermissionNotGranted(_) => FailureReason::PermissionDenied,
        AudioError::Unavailable(_) | AudioError::DeviceLost | AudioError::Backend(_) => {
            FailureReason::SourceUnavailable
        }
    }
}

impl SessionService {
    /// Creates the service.
    #[must_use]
    pub fn new(deps: SessionDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                active: Mutex::new(None),
            }),
        }
    }

    /// Registered sources with availability and current permission (FR-008).
    #[must_use]
    pub fn sources(&self) -> Vec<SourceInfo> {
        let mut sources: Vec<_> = self
            .inner
            .deps
            .sources
            .iter()
            .map(|source| SourceInfo {
                kind: source.kind(),
                availability: source.availability(),
                permission: source.permission(),
            })
            .collect();
        sources.sort_by_key(|source| source.kind);
        sources
    }

    /// Shows the OS permission prompt for `kind` where the platform has one (FR-009).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::SourceUnavailable`] when `kind` is not registered or unavailable.
    pub async fn request_permission(
        &self,
        kind: AudioSourceKind,
    ) -> Result<PermissionState, ApplicationError> {
        let source = self.inner.available_source(kind)?;
        Ok(source.request_permission().await)
    }

    /// Creates an idle session; nothing is captured (FR-001).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] for invalid sources;
    /// [`ApplicationError::SourceUnavailable`] when a selected source cannot be used;
    /// [`ApplicationError::Storage`].
    pub fn create(&self, new: NewSession) -> Result<Session, ApplicationError> {
        let session = Session::new(
            SessionId::new(),
            new.title,
            new.language,
            &new.sources,
            self.inner.deps.clock.now(),
        )?;
        for kind in session.sources() {
            self.inner.available_source(*kind)?;
        }
        self.inner.deps.sessions.create(&session)?;
        tracing::info!(session = %session.id(), "session created");
        Ok(session)
    }

    /// The session, including in-memory progress of the active one.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn get(&self, id: SessionId) -> Result<Session, ApplicationError> {
        if let Some(active) = self.inner.lock().as_ref()
            && active.session.id() == id
        {
            return Ok(active.session.clone());
        }
        Ok(self.inner.deps.sessions.get(id)?)
    }

    /// Every session, newest first.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn list(&self) -> Result<Vec<Session>, ApplicationError> {
        Ok(self.inner.deps.sessions.list()?)
    }

    /// Starts capture by explicit user action (FR-002).
    ///
    /// Checks, in order: no other active session (FR-006), the session is idle, the consent
    /// reminder was acknowledged unless dismissed (FR-012), the credential store is available
    /// (FR-039a), a transcriber is usable (FR-045a), every selected source is available and
    /// none has a denied or revoked permission (FR-009). Then returns the session in
    /// `starting`; permission prompts and opening the sources continue in the background and
    /// end in `capturing` or `failed`.
    ///
    /// Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`], [`ApplicationError::InvalidInput`] (field
    /// `consent_acknowledged`), [`ApplicationError::SecretStoreUnavailable`],
    /// [`ApplicationError::NoTranscriber`], [`ApplicationError::SourceUnavailable`],
    /// [`ApplicationError::PermissionDenied`], [`ApplicationError::Storage`]; the session
    /// stays `idle` on every error.
    pub fn start(
        &self,
        id: SessionId,
        consent_acknowledged: bool,
    ) -> Result<Session, ApplicationError> {
        let inner = &self.inner;
        let runtime = Handle::try_current()
            .map_err(|_| ApplicationError::Internal("no async runtime".into()))?;
        let mut active = inner.lock();
        if let Some(current) = active.as_ref() {
            return Err(ApplicationError::InvalidState(
                if current.session.id() == id {
                    "session is already active"
                } else {
                    "another session is active"
                }
                .into(),
            ));
        }
        let mut session = inner.deps.sessions.get(id)?;
        session.start()?;
        if !consent_acknowledged && !inner.deps.settings.get()?.consent_reminder_dismissed {
            return Err(ApplicationError::InvalidInput {
                field: "consent_acknowledged",
                problem: "required",
            });
        }
        inner.deps.secrets.check_available()?;
        let provider = inner.deps.providers.resolve_transcriber()?;
        let transcriber = inner
            .deps
            .transcribers
            .create(&provider, session.language())
            .map_err(|err| {
                tracing::warn!(session = %id, provider = %provider.id(), error = %err, "transcriber unavailable");
                ApplicationError::NoTranscriber
            })?;
        let sources = session
            .sources()
            .iter()
            .map(|kind| inner.available_source(*kind))
            .collect::<Result<Vec<_>, _>>()?;
        if sources.iter().any(|source| {
            matches!(
                source.permission(),
                PermissionState::Denied | PermissionState::Revoked
            )
        }) {
            return Err(ApplicationError::PermissionDenied);
        }
        inner.deps.sessions.update(&session)?;
        let cancel = CancellationToken::new();
        *active = Some(Active {
            session: session.clone(),
            cancel: cancel.clone(),
            pipeline: None,
            transcription: None,
        });
        inner.publish_state(&session);
        drop(active);
        tracing::info!(session = %id, "session starting");
        runtime.spawn(Arc::clone(inner).finish_start(id, cancel, sources, transcriber));
        Ok(session)
    }

    /// Cancels a start in progress: closes anything already opened and returns to `idle`
    /// (FR-003).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] unless the session is `starting`;
    /// [`ApplicationError::NotFound`].
    pub fn cancel_start(&self, id: SessionId) -> Result<Session, ApplicationError> {
        let inner = &self.inner;
        let mut guard = inner.lock();
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            drop(guard);
            return Err(inner.rejected(id, Session::cancel_start));
        };
        active.session.cancel_start()?;
        active.cancel.cancel();
        if let Some(mut pipeline) = active.pipeline.take() {
            pipeline.close_all();
        }
        let session = active.session.clone();
        *guard = None;
        inner.persist(&session);
        inner.publish_state(&session);
        tracing::info!(session = %id, "session start cancelled");
        Ok(session)
    }

    /// Stops capture immediately and returns the session in `stopping` (FR-005).
    ///
    /// Sources are closed and the indicator is switched off before this returns (SC-002); a
    /// background task then drains in-flight work for at most [`DRAIN_DEADLINE`] and moves
    /// the session to `ended`.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] unless the session is `capturing` or `paused`;
    /// [`ApplicationError::NotFound`].
    pub fn stop(&self, id: SessionId) -> Result<Session, ApplicationError> {
        let inner = &self.inner;
        let now = inner.deps.clock.now();
        let mut guard = inner.lock();
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            drop(guard);
            return Err(inner.rejected(id, |s| s.stop(now)));
        };
        active.session.stop(now)?;
        active.cancel.cancel();
        if let Some(mut pipeline) = active.pipeline.take() {
            pipeline.close_all();
        }
        inner.publish(id, indicator(false, false, Vec::new()));
        let session = active.session.clone();
        inner.persist(&session);
        inner.publish_state(&session);
        let transcription = active.transcription.take();
        drop(guard);
        tracing::info!(session = %id, "session stopping");
        match (Handle::try_current(), transcription) {
            (Ok(runtime), transcription) => {
                runtime.spawn(Arc::clone(inner).drain(id, transcription));
            }
            (Err(_), transcription) => {
                if let Some(transcription) = transcription {
                    transcription.abort(Some(GapReason::CancelledOnStop));
                }
                inner.finish_drain(id);
            }
        }
        Ok(session)
    }

    /// Fails every session left active by an unexpected exit as `interrupted`, keeping its
    /// finalized content and appending a trailing `gap(interrupted)` (FR-007, US2.3).
    ///
    /// Run once at startup, before any session can start.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn recover_on_startup(&self) -> Result<Vec<SessionId>, ApplicationError> {
        let now = self.inner.deps.clock.now();
        let mut recovered = Vec::new();
        for mut session in self.inner.deps.sessions.list_active()? {
            session.interrupt(now)?;
            self.inner.deps.sessions.save_interrupted(&session)?;
            recovered.push(session.id());
        }
        if !recovered.is_empty() {
            tracing::warn!(count = recovered.len(), "interrupted sessions recovered");
        }
        Ok(recovered)
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Option<Active>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn available_source(
        &self,
        kind: AudioSourceKind,
    ) -> Result<Arc<dyn AudioSource>, ApplicationError> {
        self.deps
            .sources
            .iter()
            .find(|source| source.kind() == kind && source.availability().is_available())
            .cloned()
            .ok_or(ApplicationError::SourceUnavailable)
    }

    /// Error for a control applied to a session that is not the active one: the stored
    /// session decides (`not_found` or the domain's invalid transition).
    fn rejected(
        &self,
        id: SessionId,
        transition: impl FnOnce(&mut Session) -> Result<(), devcopilot_domain::DomainError>,
    ) -> ApplicationError {
        match self.deps.sessions.get(id) {
            Err(err) => err.into(),
            Ok(mut stored) => match transition(&mut stored) {
                Err(err) => err.into(),
                // Stored as active but not running here: only a crash leaves that behind,
                // and startup recovery fails such sessions.
                Ok(()) => ApplicationError::InvalidState("session is not running".into()),
            },
        }
    }

    fn publish(&self, id: SessionId, event: SessionEvent) {
        self.deps.events.publish(id, event);
    }

    fn publish_state(&self, session: &Session) {
        self.publish(
            session.id(),
            SessionEvent::StateChanged {
                state: session.state(),
                failure: session.failure(),
            },
        );
    }

    /// Stores a state change; a failure is logged and reported to the UI, never hidden.
    fn persist(&self, session: &Session) -> bool {
        match self.deps.sessions.update(session) {
            Ok(()) => true,
            Err(err) => {
                tracing::error!(session = %session.id(), error = %err, "session update failed");
                self.publish(
                    session.id(),
                    SessionEvent::StorageError {
                        message_key: STORAGE_ERROR_KEY,
                    },
                );
                false
            }
        }
    }

    /// Fails the active session `id` from `starting`, `capturing` or `paused`, closing its
    /// sources. The caller holds the lock.
    fn fail_active(&self, guard: &mut Option<Active>, reason: FailureReason) {
        let Some(mut active) = guard.take() else {
            return;
        };
        let id = active.session.id();
        let was_capturing = active.pipeline.is_some();
        active.cancel.cancel();
        if let Some(mut pipeline) = active.pipeline.take() {
            pipeline.close_all();
        }
        if let Some(transcription) = active.transcription.take() {
            // Audio not yet finalized is lost with the source; storage and transcriber
            // failures already recorded what they could.
            let gap = matches!(
                reason,
                FailureReason::SourceUnavailable | FailureReason::PermissionDenied
            )
            .then_some(GapReason::SourceLost);
            transcription.abort(gap);
        }
        if active.session.fail(reason, self.deps.clock.now()).is_err() {
            return;
        }
        if was_capturing {
            self.publish(id, indicator(false, false, Vec::new()));
        }
        self.persist(&active.session);
        self.publish_state(&active.session);
        tracing::warn!(session = %id, reason = reason.code(), "session failed");
    }

    /// Background part of `start`: permission prompts, opening sources, `capturing`.
    async fn finish_start(
        self: Arc<Self>,
        id: SessionId,
        cancel: CancellationToken,
        sources: Vec<Arc<dyn AudioSource>>,
        transcriber: Arc<dyn Transcriber>,
    ) {
        for source in &sources {
            if source.permission().is_granted() {
                continue;
            }
            let answer = tokio::select! {
                () = cancel.cancelled() => return,
                answer = source.request_permission() => answer,
            };
            if !answer.is_granted() {
                self.fail_starting(&cancel, FailureReason::PermissionDenied);
                return;
            }
        }
        if cancel.is_cancelled() {
            return;
        }
        if let Some(faults) = self.open_sources(id, &cancel, &sources, transcriber) {
            Arc::clone(&self).supervise(id, cancel, faults).await;
        }
    }

    /// Opens the sources and moves the session to `capturing`; the fault stream to supervise
    /// on success.
    fn open_sources(
        self: &Arc<Self>,
        id: SessionId,
        cancel: &CancellationToken,
        sources: &[Arc<dyn AudioSource>],
        transcriber: Arc<dyn Transcriber>,
    ) -> Option<FaultStream> {
        // Opening may take a moment on real devices; the lock is not held meanwhile, so a
        // cancel can still land. Anything opened is closed if it did.
        let opened = CapturePipeline::open(sources, cancel.clone());
        let mut guard = self.lock();
        if cancel.is_cancelled() {
            return None; // `opened` drops here, closing every source.
        }
        let (pipeline, streams) = match opened {
            Ok(opened) => opened,
            Err(OpenError { source, error }) => {
                tracing::warn!(session = %id, source = source.code(), error = %error, "source failed to open");
                self.fail_active(&mut guard, failure_for(&error));
                return None;
            }
        };
        let active = guard.as_mut()?;
        if active.session.ready(self.deps.clock.now()).is_err() {
            self.fail_active(&mut guard, FailureReason::Internal);
            return None;
        }
        let first_seq = match self.deps.segments.next_seq(id) {
            Ok(seq) => seq,
            Err(err) => {
                tracing::error!(session = %id, error = %err, "transcript unavailable");
                self.fail_active(&mut guard, FailureReason::StorageError);
                return None;
            }
        };
        let descriptor = transcriber.descriptor();
        let sources = pipeline.active_sources();
        active.pipeline = Some(pipeline);
        active.transcription = Some(Transcription::spawn(
            TranscriptionDeps {
                session_id: id,
                session_sources: active.session.sources().to_vec(),
                // Offsets continue after earlier captures of the session (pause, US9).
                offset_base: active.session.active_duration(),
                first_seq,
                transcriber,
                segments: Arc::clone(&self.deps.segments),
                events: Arc::clone(&self.deps.events),
                on_fatal: self.fatal_handler(id, cancel.clone()),
            },
            streams.chunks,
        ));
        let session = active.session.clone();
        if !self.persist(&session) {
            self.fail_active(&mut guard, FailureReason::StorageError);
            return None;
        }
        self.publish_state(&session);
        self.publish(id, indicator(true, false, sources));
        self.publish(
            id,
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Transcription,
                locality: descriptor.locality,
                provider_id: descriptor.id,
            },
        );
        tracing::info!(session = %id, "session capturing");
        Some(streams.faults)
    }

    /// Lets the transcription fail the session it belongs to, while that session still
    /// captures (a failure during the stop drain leaves the session to end normally).
    fn fatal_handler(
        self: &Arc<Self>,
        id: SessionId,
        cancel: CancellationToken,
    ) -> crate::transcription::FatalHandler {
        let weak: Weak<Self> = Arc::downgrade(self);
        Arc::new(move |reason| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let mut guard = inner.lock();
            let current = guard
                .as_ref()
                .filter(|active| active.session.id() == id)
                .map(|active| active.session.state());
            if !cancel.is_cancelled()
                && matches!(
                    current,
                    Some(SessionState::Capturing | SessionState::Paused)
                )
            {
                inner.fail_active(&mut guard, reason);
            }
        })
    }

    fn fail_starting(&self, cancel: &CancellationToken, reason: FailureReason) {
        let mut guard = self.lock();
        if !cancel.is_cancelled() {
            self.fail_active(&mut guard, reason);
        }
    }

    /// Turns source faults into source status, indicator and failure transitions.
    async fn supervise(
        self: Arc<Self>,
        id: SessionId,
        cancel: CancellationToken,
        mut faults: FaultStream,
    ) {
        loop {
            let next = tokio::select! {
                () = cancel.cancelled() => return,
                next = faults.next() => next,
            };
            let Some((kind, fault)) = next else {
                return;
            };
            self.on_fault(id, &cancel, kind, fault);
        }
    }

    fn on_fault(
        &self,
        id: SessionId,
        cancel: &CancellationToken,
        kind: AudioSourceKind,
        fault: SourceFault,
    ) {
        let mut guard = self.lock();
        if cancel.is_cancelled() {
            return;
        }
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            return;
        };
        let Some(pipeline) = active.pipeline.as_mut() else {
            return;
        };
        // Close the faulty source and never reopen or replace it (FR-010, FR-011).
        pipeline.close_source(kind);
        let remaining = pipeline.active_sources();
        let paused = active.session.state() == SessionState::Paused;
        tracing::warn!(session = %id, source = kind.code(), ?fault, "source fault");
        self.publish(
            id,
            SessionEvent::SourceStatus {
                source: kind,
                status: match fault {
                    SourceFault::Lost => SourceStatus::Lost,
                    SourceFault::PermissionRevoked => SourceStatus::PermissionRevoked,
                },
            },
        );
        if remaining.is_empty() {
            let reason = match fault {
                SourceFault::Lost => FailureReason::SourceUnavailable,
                SourceFault::PermissionRevoked => FailureReason::PermissionDenied,
            };
            self.fail_active(&mut guard, reason);
        } else {
            self.publish(id, indicator(true, paused, remaining));
        }
    }

    /// Background part of `stop`.
    async fn drain(self: Arc<Self>, id: SessionId, transcription: Option<Transcription>) {
        if let Some(transcription) = transcription {
            transcription.drain(DRAIN_DEADLINE).await;
        }
        self.finish_drain(id);
    }

    fn finish_drain(&self, id: SessionId) {
        let mut guard = self.lock();
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            return;
        };
        if active.session.drained(self.deps.clock.now()).is_err() {
            return;
        }
        let session = active.session.clone();
        *guard = None;
        self.persist(&session);
        self.publish_state(&session);
        tracing::info!(session = %id, "session ended");
    }
}
