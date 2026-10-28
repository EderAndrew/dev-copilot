//! Session entity and its lifecycle state machine (data-model.md "Session", FR-003, FR-006a,
//! FR-007).
//!
//! Every transition takes the current instant from the caller; the domain never reads a clock.
//! Transitions outside the table in data-model.md fail with [`DomainError::InvalidTransition`]
//! and leave the session unchanged.

use crate::audio_source::AudioSourceKind;
use crate::error::{DomainError, InputProblem};
use crate::ids::SessionId;
use crate::language::SessionLanguage;
use crate::text::Title;
use crate::time::{Duration, Timestamp};

/// Lifecycle state of a session (data-model.md state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Created, not started.
    Idle,
    /// Checking permissions and opening sources.
    Starting,
    /// Capturing and transcribing.
    Capturing,
    /// Capture and processing halted by the user.
    Paused,
    /// Sources closed; draining in-flight transcription.
    Stopping,
    /// Finished normally.
    Ended,
    /// Finished with a [`FailureReason`].
    Failed,
}

code_enum!(SessionState, "state", {
    Idle => "idle",
    Starting => "starting",
    Capturing => "capturing",
    Paused => "paused",
    Stopping => "stopping",
    Ended => "ended",
    Failed => "failed",
});

impl SessionState {
    /// Every state, in lifecycle order.
    pub const ALL: [Self; 7] = [
        Self::Idle,
        Self::Starting,
        Self::Capturing,
        Self::Paused,
        Self::Stopping,
        Self::Ended,
        Self::Failed,
    ];

    /// States counted by the single-active-session invariant (FR-006).
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Capturing | Self::Paused | Self::Stopping
        )
    }

    /// States in which capture can no longer happen.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Failed)
    }
}

/// Why a session ended in [`SessionState::Failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Microphone or system-audio permission denied or revoked.
    PermissionDenied,
    /// Every selected source became unavailable.
    SourceUnavailable,
    /// No usable transcription provider (FR-045a).
    NoTranscriber,
    /// Writing to the encrypted database failed.
    StorageError,
    /// The OS credential store is unavailable (FR-039a).
    SecretStoreUnavailable,
    /// The app stopped while the session was active (FR-007).
    Interrupted,
    /// Unexpected internal error.
    Internal,
}

code_enum!(FailureReason, "failure", {
    PermissionDenied => "permission_denied",
    SourceUnavailable => "source_unavailable",
    NoTranscriber => "no_transcriber",
    StorageError => "storage_error",
    SecretStoreUnavailable => "secret_store_unavailable",
    Interrupted => "interrupted",
    Internal => "internal",
});

/// Maximum number of sources per session (microphone and system audio).
pub const MAX_SOURCES: usize = 2;

/// Stored fields of a session, used to rebuild it with [`Session::restore`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// Identifier.
    pub id: SessionId,
    /// Title.
    pub title: Title,
    /// Transcription and generated-content language.
    pub language: SessionLanguage,
    /// Lifecycle state.
    pub state: SessionState,
    /// Selected sources.
    pub sources: Vec<AudioSourceKind>,
    /// Creation instant.
    pub created_at: Timestamp,
    /// First instant capture began.
    pub started_at: Option<Timestamp>,
    /// Instant the session reached a terminal state after capturing.
    pub ended_at: Option<Timestamp>,
    /// Time spent capturing, excluding pauses.
    pub active_duration: Duration,
    /// Present iff `state` is `failed`.
    pub failure: Option<FailureReason>,
}

/// A meeting session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    title: Title,
    language: SessionLanguage,
    state: SessionState,
    sources: Vec<AudioSourceKind>,
    created_at: Timestamp,
    started_at: Option<Timestamp>,
    ended_at: Option<Timestamp>,
    active_duration: Duration,
    failure: Option<FailureReason>,
    /// Start of the current capturing interval; in memory only.
    capturing_since: Option<Timestamp>,
}

fn validate_sources(sources: &[AudioSourceKind]) -> Result<Vec<AudioSourceKind>, DomainError> {
    if sources.is_empty() {
        return Err(DomainError::invalid("sources", InputProblem::Empty));
    }
    if sources.len() > MAX_SOURCES {
        return Err(DomainError::invalid("sources", InputProblem::TooLong));
    }
    let mut sorted = sources.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != sources.len() {
        return Err(DomainError::invalid("sources", InputProblem::Inconsistent));
    }
    Ok(sorted)
}

impl Session {
    /// Creates an idle session; nothing is captured until [`Session::start`] (FR-001).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] on `sources` when empty, longer than [`MAX_SOURCES`] or
    /// repeated.
    pub fn new(
        id: SessionId,
        title: Title,
        language: SessionLanguage,
        sources: &[AudioSourceKind],
        now: Timestamp,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            title,
            language,
            state: SessionState::Idle,
            sources: validate_sources(sources)?,
            created_at: now,
            started_at: None,
            ended_at: None,
            active_duration: Duration::ZERO,
            failure: None,
            capturing_since: None,
        })
    }

    /// Rebuilds a stored session, checking its invariants.
    ///
    /// A restored `capturing` session does not accrue active time until it changes state;
    /// stored active sessions only exist after a crash and are failed on startup (FR-007).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] for invalid sources;
    /// [`DomainError::InvariantViolation`] when `failure` and `state` disagree or `ended_at`
    /// precedes `started_at`.
    pub fn restore(snapshot: SessionSnapshot) -> Result<Self, DomainError> {
        let sources = validate_sources(&snapshot.sources)?;
        if (snapshot.state == SessionState::Failed) != snapshot.failure.is_some() {
            return Err(DomainError::InvariantViolation(
                "failure is present iff the session failed",
            ));
        }
        match (snapshot.started_at, snapshot.ended_at) {
            (None, Some(_)) => {
                return Err(DomainError::InvariantViolation(
                    "ended_at requires started_at",
                ));
            }
            (Some(started), Some(ended)) if ended < started => {
                return Err(DomainError::InvariantViolation(
                    "ended_at precedes started_at",
                ));
            }
            _ => {}
        }
        Ok(Self {
            id: snapshot.id,
            title: snapshot.title,
            language: snapshot.language,
            state: snapshot.state,
            sources,
            created_at: snapshot.created_at,
            started_at: snapshot.started_at,
            ended_at: snapshot.ended_at,
            active_duration: snapshot.active_duration,
            failure: snapshot.failure,
            capturing_since: None,
        })
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> SessionId {
        self.id
    }

    /// Title.
    #[must_use]
    pub const fn title(&self) -> &Title {
        &self.title
    }

    /// Session language.
    #[must_use]
    pub const fn language(&self) -> SessionLanguage {
        self.language
    }

    /// Lifecycle state.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Selected sources, sorted.
    #[must_use]
    pub fn sources(&self) -> &[AudioSourceKind] {
        &self.sources
    }

    /// Creation instant.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// First instant capture began.
    #[must_use]
    pub const fn started_at(&self) -> Option<Timestamp> {
        self.started_at
    }

    /// Instant the session ended after capturing.
    #[must_use]
    pub const fn ended_at(&self) -> Option<Timestamp> {
        self.ended_at
    }

    /// Time spent capturing up to the last state change, excluding pauses.
    #[must_use]
    pub const fn active_duration(&self) -> Duration {
        self.active_duration
    }

    /// Present iff the session failed.
    #[must_use]
    pub const fn failure(&self) -> Option<FailureReason> {
        self.failure
    }

    /// Stored fields.
    #[must_use]
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            id: self.id,
            title: self.title.clone(),
            language: self.language,
            state: self.state,
            sources: self.sources.clone(),
            created_at: self.created_at,
            started_at: self.started_at,
            ended_at: self.ended_at,
            active_duration: self.active_duration,
            failure: self.failure,
        }
    }

    /// Changes the title; allowed in every state.
    pub fn rename(&mut self, title: Title) {
        self.title = title;
    }

    fn require(&self, action: &'static str, allowed: &[SessionState]) -> Result<(), DomainError> {
        if allowed.contains(&self.state) {
            Ok(())
        } else {
            Err(DomainError::InvalidTransition {
                from: self.state.code(),
                action,
            })
        }
    }

    /// Closes the current capturing interval, if any.
    fn accrue(&mut self, now: Timestamp) {
        if let Some(since) = self.capturing_since.take() {
            self.active_duration = self
                .active_duration
                .saturating_add(now.saturating_duration_since(since));
        }
    }

    /// Sets `ended_at` when capture ever began (keeps `ended_at ≥ started_at`).
    fn close(&mut self, now: Timestamp) {
        if let Some(started) = self.started_at {
            self.ended_at = Some(now.max(started));
        }
    }

    /// `idle → starting`: the user asked to start (FR-002).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn start(&mut self) -> Result<(), DomainError> {
        self.require("start", &[SessionState::Idle])?;
        self.state = SessionState::Starting;
        Ok(())
    }

    /// `starting → idle`: the user cancelled before capture began.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn cancel_start(&mut self) -> Result<(), DomainError> {
        self.require("cancel", &[SessionState::Starting])?;
        self.state = SessionState::Idle;
        Ok(())
    }

    /// `starting → capturing`: sources are open.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn ready(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require("ready", &[SessionState::Starting])?;
        self.state = SessionState::Capturing;
        self.started_at.get_or_insert(now);
        self.capturing_since = Some(now);
        Ok(())
    }

    /// `capturing → paused` (FR-006a).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn pause(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require("pause", &[SessionState::Capturing])?;
        self.accrue(now);
        self.state = SessionState::Paused;
        Ok(())
    }

    /// `paused → capturing`, once sources and permissions were re-checked (FR-006a).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn resume(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require("resume", &[SessionState::Paused])?;
        self.state = SessionState::Capturing;
        self.capturing_since = Some(now);
        Ok(())
    }

    /// `capturing | paused → stopping`: sources are closed (FR-005).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn stop(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require("stop", &[SessionState::Capturing, SessionState::Paused])?;
        self.accrue(now);
        self.state = SessionState::Stopping;
        Ok(())
    }

    /// `stopping → ended`: in-flight processing finished or was cancelled.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn drained(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require("drained", &[SessionState::Stopping])?;
        self.state = SessionState::Ended;
        self.close(now);
        Ok(())
    }

    /// `starting | capturing | paused → failed(reason)`.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from any other state.
    pub fn fail(&mut self, reason: FailureReason, now: Timestamp) -> Result<(), DomainError> {
        self.require(
            "fail",
            &[
                SessionState::Starting,
                SessionState::Capturing,
                SessionState::Paused,
            ],
        )?;
        self.accrue(now);
        self.state = SessionState::Failed;
        self.failure = Some(reason);
        self.close(now);
        Ok(())
    }

    /// `starting | capturing | paused | stopping → failed(interrupted)`: applied on startup to
    /// sessions left active by a crash (FR-007).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidTransition`] from `idle` and terminal states.
    pub fn interrupt(&mut self, now: Timestamp) -> Result<(), DomainError> {
        self.require(
            "interrupt",
            &[
                SessionState::Starting,
                SessionState::Capturing,
                SessionState::Paused,
                SessionState::Stopping,
            ],
        )?;
        self.accrue(now);
        self.state = SessionState::Failed;
        self.failure = Some(FailureReason::Interrupted);
        self.close(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Action {
        Start,
        CancelStart,
        Ready,
        Pause,
        Resume,
        Stop,
        Drained,
        Fail,
        Interrupt,
    }

    const ACTIONS: [Action; 9] = [
        Action::Start,
        Action::CancelStart,
        Action::Ready,
        Action::Pause,
        Action::Resume,
        Action::Stop,
        Action::Drained,
        Action::Fail,
        Action::Interrupt,
    ];

    /// The data-model.md transition table.
    fn expected(from: SessionState, action: Action) -> Option<SessionState> {
        use Action as A;
        use SessionState as S;
        match (from, action) {
            (S::Idle, A::Start) => Some(S::Starting),
            (S::Starting, A::CancelStart) => Some(S::Idle),
            (S::Starting, A::Ready) | (S::Paused, A::Resume) => Some(S::Capturing),
            (S::Capturing, A::Pause) => Some(S::Paused),
            (S::Capturing | S::Paused, A::Stop) => Some(S::Stopping),
            (S::Stopping, A::Drained) => Some(S::Ended),
            (S::Starting | S::Capturing | S::Paused, A::Fail)
            | (S::Starting | S::Capturing | S::Paused | S::Stopping, A::Interrupt) => {
                Some(S::Failed)
            }
            _ => None,
        }
    }

    fn ts(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).unwrap()
    }

    fn idle() -> Session {
        Session::new(
            SessionId::new(),
            Title::parse("Planejamento").unwrap(),
            SessionLanguage::PtBr,
            &[AudioSourceKind::Microphone],
            ts(1_000),
        )
        .unwrap()
    }

    /// Drives a fresh session into `state` through valid transitions.
    fn in_state(state: SessionState) -> Session {
        let mut session = idle();
        let now = ts(2_000);
        let path: &[Action] = match state {
            SessionState::Idle => &[],
            SessionState::Starting => &[Action::Start],
            SessionState::Capturing => &[Action::Start, Action::Ready],
            SessionState::Paused => &[Action::Start, Action::Ready, Action::Pause],
            SessionState::Stopping => &[Action::Start, Action::Ready, Action::Stop],
            SessionState::Ended => &[Action::Start, Action::Ready, Action::Stop, Action::Drained],
            SessionState::Failed => &[Action::Start, Action::Fail],
        };
        for action in path {
            apply(&mut session, *action, now).unwrap();
        }
        assert_eq!(session.state(), state);
        session
    }

    fn apply(session: &mut Session, action: Action, now: Timestamp) -> Result<(), DomainError> {
        match action {
            Action::Start => session.start(),
            Action::CancelStart => session.cancel_start(),
            Action::Ready => session.ready(now),
            Action::Pause => session.pause(now),
            Action::Resume => session.resume(now),
            Action::Stop => session.stop(now),
            Action::Drained => session.drained(now),
            Action::Fail => session.fail(FailureReason::SourceUnavailable, now),
            Action::Interrupt => session.interrupt(now),
        }
    }

    #[test]
    fn every_state_and_action_follows_the_table() {
        for from in SessionState::ALL {
            for action in ACTIONS {
                let mut session = in_state(from);
                let before = session.clone();
                let result = apply(&mut session, action, ts(3_000));
                match expected(from, action) {
                    Some(to) => {
                        assert_eq!(result, Ok(()), "{from:?} --{action:?}");
                        assert_eq!(session.state(), to, "{from:?} --{action:?}");
                    }
                    None => {
                        assert!(
                            matches!(result, Err(DomainError::InvalidTransition { from: code, .. }) if code == from.code()),
                            "{from:?} --{action:?} should be rejected, got {result:?}"
                        );
                        assert_eq!(session, before, "rejected transition changed the session");
                    }
                }
            }
        }
    }

    #[test]
    fn terminal_states_accept_no_transition() {
        for state in [SessionState::Ended, SessionState::Failed] {
            for action in ACTIONS {
                assert!(apply(&mut in_state(state), action, ts(3_000)).is_err());
            }
        }
    }

    #[test]
    fn active_and_terminal_sets() {
        use SessionState::*;
        let active: Vec<_> = SessionState::ALL
            .into_iter()
            .filter(|s| s.is_active())
            .collect();
        assert_eq!(active, [Starting, Capturing, Paused, Stopping]);
        let terminal: Vec<_> = SessionState::ALL
            .into_iter()
            .filter(|s| s.is_terminal())
            .collect();
        assert_eq!(terminal, [Ended, Failed]);
    }

    #[test]
    fn new_session_is_idle_and_validates_sources() {
        let session = idle();
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.started_at(), None);
        assert_eq!(session.failure(), None);
        let make = |sources: &[AudioSourceKind]| {
            Session::new(
                SessionId::new(),
                Title::parse("t").unwrap(),
                SessionLanguage::En,
                sources,
                ts(0),
            )
        };
        assert_eq!(
            make(&[]).unwrap_err(),
            DomainError::invalid("sources", InputProblem::Empty)
        );
        assert_eq!(
            make(&[AudioSourceKind::Microphone, AudioSourceKind::Microphone]).unwrap_err(),
            DomainError::invalid("sources", InputProblem::Inconsistent)
        );
        assert_eq!(
            make(&[
                AudioSourceKind::System,
                AudioSourceKind::Microphone,
                AudioSourceKind::System
            ])
            .unwrap_err(),
            DomainError::invalid("sources", InputProblem::TooLong)
        );
        let both = make(&[AudioSourceKind::System, AudioSourceKind::Microphone]).unwrap();
        assert_eq!(
            both.sources(),
            [AudioSourceKind::Microphone, AudioSourceKind::System]
        );
    }

    #[test]
    fn cancel_returns_to_idle_without_start_time() {
        let mut session = in_state(SessionState::Starting);
        session.cancel_start().unwrap();
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.started_at(), None);
        session.start().unwrap();
        assert_eq!(session.state(), SessionState::Starting);
    }

    #[test]
    fn active_duration_excludes_pauses() {
        let mut session = idle();
        session.start().unwrap();
        session.ready(ts(10_000)).unwrap();
        session.pause(ts(14_000)).unwrap();
        session.resume(ts(20_000)).unwrap();
        session.stop(ts(23_000)).unwrap();
        assert_eq!(session.active_duration(), Duration::from_millis(7_000));
        session.drained(ts(25_000)).unwrap();
        assert_eq!(session.started_at(), Some(ts(10_000)));
        assert_eq!(session.ended_at(), Some(ts(25_000)));
        assert_eq!(session.active_duration(), Duration::from_millis(7_000));
    }

    #[test]
    fn stop_from_paused_does_not_count_the_pause() {
        let mut session = idle();
        session.start().unwrap();
        session.ready(ts(0)).unwrap();
        session.pause(ts(5_000)).unwrap();
        session.stop(ts(60_000)).unwrap();
        assert_eq!(session.active_duration(), Duration::from_millis(5_000));
    }

    #[test]
    fn failure_records_reason_and_end() {
        let mut session = in_state(SessionState::Capturing);
        session
            .fail(FailureReason::PermissionDenied, ts(9_000))
            .unwrap();
        assert_eq!(session.failure(), Some(FailureReason::PermissionDenied));
        assert_eq!(session.ended_at(), Some(ts(9_000)));
        assert_eq!(session.active_duration(), Duration::from_millis(7_000));

        // Failing before capture began leaves no end time (ended_at requires started_at).
        let mut starting = in_state(SessionState::Starting);
        starting
            .fail(FailureReason::NoTranscriber, ts(9_000))
            .unwrap();
        assert_eq!(starting.ended_at(), None);
        assert_eq!(starting.failure(), Some(FailureReason::NoTranscriber));
    }

    #[test]
    fn interrupt_marks_interrupted_from_every_active_state() {
        for state in SessionState::ALL.into_iter().filter(|s| s.is_active()) {
            let mut session = in_state(state);
            session.interrupt(ts(5_000)).unwrap();
            assert_eq!(session.state(), SessionState::Failed);
            assert_eq!(session.failure(), Some(FailureReason::Interrupted));
        }
    }

    #[test]
    fn end_time_never_precedes_start() {
        let mut session = idle();
        session.start().unwrap();
        session.ready(ts(10_000)).unwrap();
        session.stop(ts(5_000)).unwrap();
        session.drained(ts(4_000)).unwrap();
        assert_eq!(session.ended_at(), Some(ts(10_000)));
        assert_eq!(session.active_duration(), Duration::ZERO);
    }

    #[test]
    fn rename_is_allowed_in_any_state() {
        for state in SessionState::ALL {
            let mut session = in_state(state);
            session.rename(Title::parse("Novo título").unwrap());
            assert_eq!(session.title().as_str(), "Novo título");
        }
    }

    #[test]
    fn snapshot_restores_and_checks_invariants() {
        let mut session = in_state(SessionState::Capturing);
        session.stop(ts(4_000)).unwrap();
        session.drained(ts(5_000)).unwrap();
        let restored = Session::restore(session.snapshot()).unwrap();
        assert_eq!(restored, session);

        let mut failed_without_reason = session.snapshot();
        failed_without_reason.state = SessionState::Failed;
        assert!(matches!(
            Session::restore(failed_without_reason),
            Err(DomainError::InvariantViolation(_))
        ));

        let mut reason_without_failure = session.snapshot();
        reason_without_failure.failure = Some(FailureReason::Internal);
        assert!(Session::restore(reason_without_failure).is_err());

        let mut ended_before_start = session.snapshot();
        ended_before_start.ended_at = Some(ts(1));
        assert!(Session::restore(ended_before_start).is_err());

        let mut ended_never_started = session.snapshot();
        ended_never_started.started_at = None;
        assert!(Session::restore(ended_never_started).is_err());

        let mut no_sources = session.snapshot();
        no_sources.sources.clear();
        assert!(Session::restore(no_sources).is_err());
    }

    #[test]
    fn codes_round_trip() {
        for state in SessionState::ALL {
            assert_eq!(SessionState::parse(state.code()), Ok(state));
        }
        assert_eq!(
            FailureReason::parse(FailureReason::SecretStoreUnavailable.code()),
            Ok(FailureReason::SecretStoreUnavailable)
        );
        assert!(SessionState::parse("Paused").is_err());
    }
}
