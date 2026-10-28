//! Session use cases: create, start, cancel start, pause, resume, stop, delete, and startup
//! recovery (FR-001–FR-012, FR-039a, FR-042–FR-045b).
//!
//! While capturing, the audio queue feeds the session's [`Transcription`], which persists
//! finals and gaps as they arrive (US2) and announces finals to the session's incremental
//! analysis (US3). Analysis never holds up capture: it starts with capture when an analysis
//! provider is usable, stops scheduling when capture ends, and a run still in flight gets
//! [`DRAIN_DEADLINE`] before it is cancelled.
//!
//! Controls never wait for audio or providers (SC-006): `start` checks the preconditions,
//! moves the session to `starting` and returns while a background task asks for permission and
//! opens the sources; `stop` closes the sources, switches the indicator off and returns while
//! a background task drains in-flight work for at most [`DRAIN_DEADLINE`].
//!
//! At most one session is active at a time (FR-006). The active session lives in memory under
//! one lock together with its pipeline; every state change is persisted and published while
//! that lock is held, so events reflect the order of transitions.
//!
//! Pause (FR-006a) closes every source at once, so nothing is captured, and lets the audio
//! captured before it finish transcribing in the background. Resume re-checks the transcriber,
//! the sources and their permissions; sources that became unusable during the pause are
//! reported and left closed (US9.4). The paused interval is stored as `gap(paused)` before the
//! new capture writes anything. Segment offsets are wall-clock time since the session first
//! captured, so the gap has the length of the pause; the active duration excludes it.
//!
//! Deleting a session is refused while it is active (FR-044). Otherwise its analysis runs and
//! question still in flight are cancelled and awaited first, so nothing is written for it once
//! its rows are gone (FR-043).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use devcopilot_domain::audio_source::{AudioSourceKind, Availability, PermissionState};
use devcopilot_domain::ids::SegmentId;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{Locality, Provider, ProviderRole};
use devcopilot_domain::session::{FailureReason, Session, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{GapReason, TranscriptSegment};
use devcopilot_domain::transcript::{SegmentKind, SegmentSpec};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::analysis_service::{AnalysisService, SessionAnalysis};
use crate::capture_pipeline::{
    CapturePipeline, FallbackNotice, FallbackTranscriber, FaultStream, OpenError, indicator,
};
use crate::error::ApplicationError;
use crate::events::{ProviderStage, SessionEvent, SourceStatus};
use crate::ports::{
    AudioError, AudioSource, Clock, EventPublisher, SecretStore, SegmentRepository,
    SessionRepository, SourceFault, Transcriber, TranscriberFactory,
};
use crate::provider_service::ProviderService;
use crate::question_service::QuestionService;
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
    /// Incremental analysis; `None` runs capture without it.
    pub analysis: Option<AnalysisService>,
    /// Contextual questions, cancelled when their session is deleted.
    pub questions: Option<QuestionService>,
}

/// Largest page of [`SessionService::list_page`].
pub const MAX_PAGE_SIZE: usize = 100;

/// One page of sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    /// Sessions, newest first.
    pub items: Vec<Session>,
    /// Cursor of the next page, when there is one.
    pub next_cursor: Option<SessionId>,
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
    /// Child of `cancel` for the current capture; cancelled when it pauses or ends, so the
    /// fault supervisor of a closed pipeline never touches a later one.
    capture: CancellationToken,
    pipeline: Option<CapturePipeline>,
    transcription: Option<Transcription>,
    analysis: Option<SessionAnalysis>,
    paused: Option<PausedCapture>,
}

/// What a pause leaves behind for the resume.
struct PausedCapture {
    /// Session offset where the paused interval begins.
    gap_start: Duration,
    /// Transcription of the audio captured before the pause, finishing in the background.
    drain: Option<JoinHandle<()>>,
}

/// Wall-clock offset of `now` from the first capture of `session`.
fn offset_at(session: &Session, now: Timestamp) -> Duration {
    session.started_at().map_or(Duration::ZERO, |started| {
        now.saturating_duration_since(started)
    })
}

struct Inner {
    deps: SessionDeps,
    active: Mutex<Option<Active>>,
    /// Sessions being deleted; they cannot start.
    deleting: Mutex<HashSet<SessionId>>,
}

/// Removes a session from [`Inner::deleting`] when its deletion ends, however it ends.
struct Deleting<'a> {
    inner: &'a Inner,
    id: SessionId,
}

impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        self.inner.deleting_set().remove(&self.id);
    }
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
                deleting: Mutex::new(HashSet::new()),
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

    /// One page of sessions, newest first, starting after `cursor` (the last id of the
    /// previous page).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] when `limit` is outside `1..=`[`MAX_PAGE_SIZE`] or
    /// `cursor` names no session; [`ApplicationError::Storage`].
    pub fn list_page(
        &self,
        cursor: Option<SessionId>,
        limit: usize,
    ) -> Result<SessionPage, ApplicationError> {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(ApplicationError::InvalidInput {
                field: "limit",
                problem: "out_of_range",
            });
        }
        let sessions = self.list()?;
        let start = match cursor {
            None => 0,
            Some(cursor) => {
                sessions
                    .iter()
                    .position(|session| session.id() == cursor)
                    .ok_or(ApplicationError::InvalidInput {
                        field: "cursor",
                        problem: "unknown",
                    })?
                    + 1
            }
        };
        let items: Vec<Session> = sessions.into_iter().skip(start).take(limit + 1).collect();
        let more = items.len() > limit;
        let items: Vec<Session> = items.into_iter().take(limit).collect();
        let next_cursor = more.then(|| items.last().map(Session::id)).flatten();
        Ok(SessionPage { items, next_cursor })
    }

    /// Renames a session, active or not.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn rename(&self, id: SessionId, title: Title) -> Result<Session, ApplicationError> {
        let mut guard = self.inner.lock();
        if let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) {
            let mut renamed = active.session.clone();
            renamed.rename(title);
            self.inner.deps.sessions.update(&renamed)?;
            active.session = renamed.clone();
            return Ok(renamed);
        }
        drop(guard);
        let mut session = self.inner.deps.sessions.get(id)?;
        session.rename(title);
        self.inner.deps.sessions.update(&session)?;
        Ok(session)
    }

    /// Deletes a session and all of its local content after the user confirmed (FR-042).
    ///
    /// Refused while the session is active, including while it starts or stops (FR-044).
    /// Analysis runs and the question still in flight for it are cancelled and awaited for up
    /// to [`DRAIN_DEADLINE`] before its rows are removed, so their results are discarded
    /// (FR-043).
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::InvalidState`] while the session is
    /// active or already being deleted; [`ApplicationError::Storage`].
    pub async fn delete(&self, id: SessionId) -> Result<(), ApplicationError> {
        let inner = &self.inner;
        if !inner.deleting_set().insert(id) {
            return Err(ApplicationError::InvalidState(
                "session is being deleted".into(),
            ));
        }
        let _deleting = Deleting { inner, id };
        // Checked after joining `deleting`, which `start` reads under the active lock: either
        // the start is refused or the session is seen active here.
        let session = self.get(id)?;
        if session.state().is_active()
            || inner.lock().as_ref().is_some_and(|a| a.session.id() == id)
        {
            return Err(ApplicationError::InvalidState(
                "stop the session before deleting it".into(),
            ));
        }
        let mut settled = true;
        if let Some(analysis) = &inner.deps.analysis {
            settled &= analysis.cancel_session(id, DRAIN_DEADLINE).await;
        }
        if let Some(questions) = &inner.deps.questions {
            settled &= questions.cancel_session(id, DRAIN_DEADLINE).await;
        }
        if !settled {
            // Late writes fail once the session row is gone; nothing of it survives.
            tracing::warn!(session = %id, "work still running when the session was deleted");
        }
        inner.deps.sessions.delete(id)?;
        tracing::info!(session = %id, "session deleted");
        Ok(())
    }

    /// Persisted finals and gaps of a session, in `seq` order.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`]; [`ApplicationError::Storage`].
    pub fn transcript(&self, id: SessionId) -> Result<Vec<TranscriptSegment>, ApplicationError> {
        self.get(id)?;
        Ok(self.inner.deps.segments.list_by_session(id)?)
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
        if inner.deleting_set().contains(&id) {
            return Err(ApplicationError::InvalidState(
                "session is being deleted".into(),
            ));
        }
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
        let transcriber = inner.transcriber(id, &provider, session.language())?;
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
        let capture = cancel.child_token();
        *active = Some(Active {
            session: session.clone(),
            cancel: cancel.clone(),
            capture: capture.clone(),
            pipeline: None,
            transcription: None,
            analysis: None,
            paused: None,
        });
        inner.publish_state(&session);
        drop(active);
        tracing::info!(session = %id, "session starting");
        runtime.spawn(Arc::clone(inner).finish_start(id, cancel, capture, sources, transcriber));
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

    /// Pauses capture (FR-006a): every source closes before this returns, the indicator shows
    /// "paused", and audio captured before the pause finishes transcribing in the background.
    ///
    /// Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] unless the session is `capturing` with its sources
    /// open; [`ApplicationError::NotFound`].
    pub fn pause(&self, id: SessionId) -> Result<Session, ApplicationError> {
        let inner = &self.inner;
        let runtime = Handle::try_current()
            .map_err(|_| ApplicationError::Internal("no async runtime".into()))?;
        let now = inner.deps.clock.now();
        let mut guard = inner.lock();
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            drop(guard);
            return Err(inner.rejected(id, |s| s.pause(now)));
        };
        if active.session.state() == SessionState::Capturing && active.pipeline.is_none() {
            return Err(ApplicationError::InvalidState(
                "session is still resuming".into(),
            ));
        }
        let wall = offset_at(&active.session, now);
        active.session.pause(now)?;
        active.capture.cancel();
        if let Some(mut pipeline) = active.pipeline.take() {
            pipeline.close_all();
        }
        let transcription = active.transcription.take();
        let captured = transcription
            .as_ref()
            .map_or(Duration::ZERO, Transcription::captured_end);
        active.paused = Some(PausedCapture {
            gap_start: wall.max(captured),
            drain: transcription.map(|t| runtime.spawn(t.drain(DRAIN_DEADLINE))),
        });
        let session = active.session.clone();
        inner.publish(id, indicator(true, true, session.sources().to_vec()));
        inner.persist(&session);
        inner.publish_state(&session);
        drop(guard);
        tracing::info!(session = %id, "session paused");
        Ok(session)
    }

    /// Resumes a paused session (FR-006a). Checks again that the credential store is
    /// available, a transcriber is usable and each source is available with its permission
    /// granted. A source that fails is reported with `source_status` and stays closed
    /// (US9.4); with none left the session stays paused. Then returns the session in
    /// `capturing` while the sources reopen in the background, after the paused interval is
    /// stored as `gap(paused)`.
    ///
    /// Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] unless paused; [`ApplicationError::NotFound`];
    /// [`ApplicationError::SecretStoreUnavailable`], [`ApplicationError::NoTranscriber`],
    /// [`ApplicationError::AuthorizationRequired`]; [`ApplicationError::PermissionDenied`] or
    /// [`ApplicationError::SourceUnavailable`] when no source can capture.
    pub fn resume(&self, id: SessionId) -> Result<Session, ApplicationError> {
        let inner = &self.inner;
        let runtime = Handle::try_current()
            .map_err(|_| ApplicationError::Internal("no async runtime".into()))?;
        let now = inner.deps.clock.now();
        let mut guard = inner.lock();
        let Some(active) = guard.as_mut().filter(|a| a.session.id() == id) else {
            drop(guard);
            return Err(inner.rejected(id, |s| s.resume(now)));
        };
        let mut resumed = active.session.clone();
        resumed.resume(now)?;
        inner.deps.secrets.check_available()?;
        let provider = inner.deps.providers.resolve_transcriber()?;
        let transcriber = inner.transcriber(id, &provider, resumed.language())?;
        let mut sources = Vec::new();
        let mut problem = None;
        for kind in resumed.sources() {
            let status = match inner.available_source(*kind) {
                Ok(source) if source.permission().is_granted() => {
                    sources.push(source);
                    continue;
                }
                Ok(_) => {
                    problem.get_or_insert(ApplicationError::PermissionDenied);
                    SourceStatus::PermissionRevoked
                }
                Err(_) => {
                    problem.get_or_insert(ApplicationError::SourceUnavailable);
                    SourceStatus::Lost
                }
            };
            tracing::warn!(session = %id, source = kind.code(), "source not resumed");
            inner.publish(
                id,
                SessionEvent::SourceStatus {
                    source: *kind,
                    status,
                },
            );
        }
        if sources.is_empty() {
            return Err(problem.unwrap_or(ApplicationError::SourceUnavailable));
        }
        let capture = active.cancel.child_token();
        active.capture = capture.clone();
        active.session = resumed;
        let paused = active.paused.take();
        let session = active.session.clone();
        inner.persist(&session);
        inner.publish_state(&session);
        drop(guard);
        tracing::info!(session = %id, "session resuming");
        runtime.spawn(Arc::clone(inner).finish_resume(id, capture, sources, transcriber, paused));
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
        let analysis = active.analysis.take();
        let paused_drain = active.paused.take().and_then(|paused| paused.drain);
        drop(guard);
        tracing::info!(session = %id, "session stopping");
        match (Handle::try_current(), transcription) {
            (Ok(runtime), transcription) => {
                runtime.spawn(Arc::clone(inner).drain(id, paused_drain, transcription, analysis));
            }
            (Err(_), transcription) => {
                if let Some(transcription) = transcription {
                    transcription.abort(Some(GapReason::CancelledOnStop));
                }
                if let Some(analysis) = analysis {
                    analysis.finish(DRAIN_DEADLINE);
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

    fn deleting_set(&self) -> MutexGuard<'_, HashSet<SessionId>> {
        self.deleting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Transcriber of `provider`; a remote one falls back to a local provider when one can
    /// transcribe (FR-045b).
    fn transcriber(
        &self,
        id: SessionId,
        provider: &Provider,
        language: SessionLanguage,
    ) -> Result<Arc<dyn Transcriber>, ApplicationError> {
        let primary = self
            .deps
            .transcribers
            .create(provider, language)
            .map_err(|err| {
                tracing::warn!(session = %id, provider = %provider.id(), error = %err, "transcriber unavailable");
                ApplicationError::NoTranscriber
            })?;
        if provider.locality() != Locality::Remote {
            return Ok(primary);
        }
        let fallback = match self
            .deps
            .providers
            .resolve_local_fallback(ProviderRole::Transcription)
        {
            Ok(Some(local)) => self.deps.transcribers.create(&local, language).ok(),
            Ok(None) => None,
            Err(err) => {
                tracing::warn!(session = %id, error = %err, "local fallback unresolved");
                None
            }
        };
        let Some(fallback) = fallback else {
            return Ok(primary);
        };
        let events = Arc::clone(&self.deps.events);
        let notice: FallbackNotice = Box::new(move |from, to| {
            events.publish(
                id,
                SessionEvent::ProviderFallback {
                    stage: ProviderStage::Transcription,
                    from_provider_id: from.id,
                    to_provider_id: to.id,
                },
            );
            events.publish(
                id,
                SessionEvent::ProviderInUse {
                    stage: ProviderStage::Transcription,
                    locality: to.locality,
                    provider_id: to.id,
                },
            );
        });
        Ok(Arc::new(FallbackTranscriber::new(
            primary, fallback, notice,
        )))
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
        if let Some(analysis) = active.analysis.take() {
            analysis.finish(DRAIN_DEADLINE);
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
        capture: CancellationToken,
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
        if let Some(faults) = self.open_sources(id, &capture, &sources, transcriber, None) {
            Arc::clone(&self).supervise(id, capture, faults).await;
        }
    }

    /// Background part of `resume`: waits for the pre-pause transcription, then stores
    /// `gap(paused)` and reopens the sources.
    async fn finish_resume(
        self: Arc<Self>,
        id: SessionId,
        capture: CancellationToken,
        sources: Vec<Arc<dyn AudioSource>>,
        transcriber: Arc<dyn Transcriber>,
        paused: Option<PausedCapture>,
    ) {
        let gap_start = match paused {
            Some(PausedCapture { gap_start, drain }) => {
                if let Some(drain) = drain {
                    // Bounded by its own deadline; segments must be written in `seq` order.
                    let _ = drain.await;
                }
                Some(gap_start)
            }
            None => None,
        };
        if capture.is_cancelled() {
            return;
        }
        let gap = Some(gap_start.unwrap_or(Duration::ZERO));
        if let Some(faults) = self.open_sources(id, &capture, &sources, transcriber, gap) {
            Arc::clone(&self).supervise(id, capture, faults).await;
        }
    }

    /// Stores the paused interval `[start, end]` as `gap(paused)`; `false` on storage failure.
    fn write_pause_gap(&self, session: &Session, seq: u64, start: Duration, end: Duration) -> bool {
        let id = session.id();
        let segment = match TranscriptSegment::new(
            SegmentSpec {
                id: SegmentId::new(),
                session_id: id,
                seq,
                kind: SegmentKind::Gap {
                    reason: GapReason::Paused,
                },
                source: None,
                start,
                end,
            },
            session.sources(),
        ) {
            Ok(segment) => segment,
            Err(err) => {
                tracing::warn!(session = %id, error = %err, "pause gap rejected");
                return true;
            }
        };
        if let Err(err) = self.deps.segments.append(&segment) {
            tracing::error!(session = %id, error = %err, "pause gap append failed");
            return false;
        }
        self.publish(
            id,
            SessionEvent::Gap {
                reason: GapReason::Paused,
                start,
                duration: segment.duration(),
            },
        );
        true
    }

    /// Opens the sources and moves the session to `capturing`; the fault stream to supervise
    /// on success.
    ///
    /// On resume, `pause_gap` is where the paused interval began: the session is already
    /// `capturing`, and `gap(paused)` is stored before the new capture's first segment.
    fn open_sources(
        self: &Arc<Self>,
        id: SessionId,
        cancel: &CancellationToken,
        sources: &[Arc<dyn AudioSource>],
        transcriber: Arc<dyn Transcriber>,
        pause_gap: Option<Duration>,
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
        let now = self.deps.clock.now();
        if pause_gap.is_none() && active.session.ready(now).is_err() {
            self.fail_active(&mut guard, FailureReason::Internal);
            return None;
        }
        let mut offset_base = offset_at(&active.session, now);
        let first_seq = match self.deps.segments.next_seq(id) {
            Ok(seq) => seq,
            Err(err) => {
                tracing::error!(session = %id, error = %err, "transcript unavailable");
                self.fail_active(&mut guard, FailureReason::StorageError);
                return None;
            }
        };
        let mut first_seq = first_seq;
        if let Some(gap_start) = pause_gap {
            offset_base = offset_base.max(gap_start);
            if !self.write_pause_gap(&active.session, first_seq, gap_start, offset_base) {
                self.publish(
                    id,
                    SessionEvent::StorageError {
                        message_key: STORAGE_ERROR_KEY,
                    },
                );
                self.fail_active(&mut guard, FailureReason::StorageError);
                return None;
            }
            first_seq += 1;
        }
        let active = guard.as_mut()?;
        let descriptor = transcriber.descriptor();
        let sources = pipeline.active_sources();
        active.pipeline = Some(pipeline);
        if active.analysis.is_none() {
            active.analysis = self
                .deps
                .analysis
                .as_ref()
                .and_then(|analysis| analysis.attach(&active.session));
        }
        active.transcription = Some(Transcription::spawn(
            TranscriptionDeps {
                session_id: id,
                session_sources: active.session.sources().to_vec(),
                // Wall-clock offset since the first capture, so pauses keep their length.
                offset_base,
                first_seq,
                transcriber,
                segments: Arc::clone(&self.deps.segments),
                events: Arc::clone(&self.deps.events),
                on_fatal: self.fatal_handler(id, cancel.clone()),
                analysis: active.analysis.as_ref().map(SessionAnalysis::feed),
            },
            streams.chunks,
        ));
        let session = active.session.clone();
        if pause_gap.is_none() {
            if !self.persist(&session) {
                self.fail_active(&mut guard, FailureReason::StorageError);
                return None;
            }
            self.publish_state(&session);
        }
        self.publish(id, indicator(true, false, sources));
        self.publish(
            id,
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Transcription,
                locality: descriptor.locality,
                provider_id: descriptor.id,
            },
        );
        tracing::info!(session = %id, resumed = pause_gap.is_some(), "session capturing");
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

    /// Background part of `stop`. Analysis keeps receiving the finals of the drain, then
    /// stops scheduling.
    async fn drain(
        self: Arc<Self>,
        id: SessionId,
        paused_drain: Option<JoinHandle<()>>,
        transcription: Option<Transcription>,
        analysis: Option<SessionAnalysis>,
    ) {
        if let Some(paused_drain) = paused_drain {
            // Already bounded by its own deadline.
            let _ = paused_drain.await;
        }
        if let Some(transcription) = transcription {
            transcription.drain(DRAIN_DEADLINE).await;
        }
        if let Some(analysis) = analysis {
            analysis.finish(DRAIN_DEADLINE);
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
