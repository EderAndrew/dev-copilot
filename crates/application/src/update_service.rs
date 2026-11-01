//! In-app updates (002 US8, data-model §1.3, contracts/ipc-updates.md).
//!
//! The core alone decides when to check and whether to install: checks happen only once the
//! first-run notice was dismissed and while checks are enabled; an install needs an explicit
//! confirmation of the version last checked and never runs while a session could be
//! capturing. The [`AppUpdater`] adapter verifies the package signature; this use case never
//! installs a package the adapter did not report as downloaded and verified.
//!
//! One operation runs at a time (check, or download and install). Its completion is matched
//! against a generation number, so the result of a cancelled operation is dropped.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration as StdDuration;

use devcopilot_domain::time::Timestamp;
use tokio::runtime::Handle;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::ports::{
    AppUpdater, AvailableUpdate, Clock, MAX_UPDATE_NOTES_CHARS, SessionActivity, UpdateFailure,
};
use crate::settings_service::{AppSettings, SettingsService};

/// Time between two automatic checks while the app stays open (research R5).
pub const CHECK_INTERVAL: StdDuration = StdDuration::from_secs(24 * 60 * 60);

/// Without a known package size, progress is published every this many bytes.
const PROGRESS_STEP_UNKNOWN_TOTAL: u64 = 1024 * 1024;

/// Update state as shown to the user (contracts/ipc-updates.md `UpdateStatusDto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Checks are switched off.
    Disabled,
    /// The first-run notice was not dismissed yet; nothing is requested.
    NoticePending,
    /// Nothing in progress.
    Idle {
        /// Last completed check.
        last_checked_at: Option<Timestamp>,
    },
    /// A check is in progress.
    Checking,
    /// The last check found no newer version.
    UpToDate {
        /// When that check completed.
        last_checked_at: Timestamp,
    },
    /// A newer version can be installed after confirmation.
    Available(AvailableUpdate),
    /// The confirmed install waits for the active session to end.
    BlockedBySession {
        /// Confirmed version.
        version: String,
    },
    /// The package is being downloaded and verified.
    Downloading {
        /// Version being downloaded.
        version: String,
        /// Bytes received so far.
        received_bytes: u64,
        /// Package size, when the server announced it.
        total_bytes: Option<u64>,
    },
    /// The verified package is being installed; the app restarts next.
    Installing {
        /// Version being installed.
        version: String,
    },
    /// The last operation failed; the installed version is unchanged.
    Failed(UpdateFailure),
}

/// Where the state machine is, before the settings gates are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    Checking,
    UpToDate(Timestamp),
    Available,
    BlockedBySession,
    Downloading {
        received_bytes: u64,
        total_bytes: Option<u64>,
    },
    Installing,
    Failed(UpdateFailure),
}

#[derive(Debug)]
struct State {
    phase: Phase,
    /// Update found by the last successful check; the only version `install` accepts.
    offered: Option<AvailableUpdate>,
    /// Cancellation of the operation in progress, if any.
    op: Option<CancellationToken>,
    /// Incremented whenever an operation starts or is abandoned.
    generation: u64,
}

/// Dependencies of [`UpdateService`].
pub struct UpdateDeps {
    /// Signed update adapter.
    pub updater: Arc<dyn AppUpdater>,
    /// Session activity, to block installs during sessions.
    pub sessions: Arc<dyn SessionActivity>,
    /// `updates.*` settings.
    pub settings: Arc<SettingsService>,
    /// Wall clock, for `last_checked_at`.
    pub clock: Arc<dyn Clock>,
}

struct Inner {
    deps: UpdateDeps,
    state: Mutex<State>,
    status: watch::Sender<UpdateStatus>,
    /// Wakes the scheduler when checks become allowed.
    wake: Notify,
}

/// Update use cases.
#[derive(Clone)]
pub struct UpdateService {
    inner: Arc<Inner>,
}

impl UpdateService {
    /// Creates the service. Nothing is requested until [`UpdateService::check`] or
    /// [`UpdateService::run_scheduler`] runs.
    #[must_use]
    pub fn new(deps: UpdateDeps) -> Self {
        let (status, _) = watch::channel(UpdateStatus::NoticePending);
        let service = Self {
            inner: Arc::new(Inner {
                deps,
                state: Mutex::new(State {
                    phase: Phase::Idle,
                    offered: None,
                    op: None,
                    generation: 0,
                }),
                status,
                wake: Notify::new(),
            }),
        };
        // Seed subscribers with the real initial state; a storage failure keeps the
        // conservative `NoticePending` until the next transition.
        let _ = service.status();
        service
    }

    /// Current state. Also publishes it, so a session that ended since an install was
    /// blocked turns `blocked_by_session` back into `available`.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when the settings cannot be read.
    pub fn status(&self) -> Result<UpdateStatus, ApplicationError> {
        let state = self.inner.lock();
        self.inner.publish(&state)
    }

    /// Receives every state transition and download progress in steps of at least 5 %.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<UpdateStatus> {
        self.inner.status.subscribe()
    }

    /// Starts a check in the background and returns `checking`.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::UpdatesDisabled`]; [`ApplicationError::NoticePending`];
    /// [`ApplicationError::InvalidState`] while another operation runs or without a Tokio
    /// runtime; [`ApplicationError::Storage`].
    pub fn check(&self) -> Result<UpdateStatus, ApplicationError> {
        let runtime = runtime()?;
        let mut state = self.inner.lock();
        self.inner.ensure_allowed()?;
        if state.op.is_some() {
            return Err(busy());
        }
        let (token, generation) = start_op(&mut state);
        state.phase = Phase::Checking;
        let status = self.inner.publish(&state)?;
        drop(state);

        let inner = Arc::clone(&self.inner);
        runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                () = token.cancelled() => return,
                result = inner.deps.updater.check() => result,
            };
            inner.finish_check(generation, result);
        });
        tracing::info!("update check started");
        Ok(status)
    }

    /// Confirms the install of `version`, which must be the version of the last check.
    /// Returns `downloading`, or `blocked_by_session` while a session is active.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::UpdatesDisabled`]; [`ApplicationError::NoticePending`];
    /// [`ApplicationError::InvalidState`] when no update is available or another operation
    /// runs; [`ApplicationError::VersionMismatch`]; [`ApplicationError::Storage`].
    pub fn install(&self, version: &str) -> Result<UpdateStatus, ApplicationError> {
        let runtime = runtime()?;
        let mut state = self.inner.lock();
        self.inner.ensure_allowed()?;
        if state.op.is_some() {
            return Err(busy());
        }
        if !matches!(state.phase, Phase::Available | Phase::BlockedBySession) {
            return Err(ApplicationError::InvalidState("no update available".into()));
        }
        let Some(offered) = state.offered.clone() else {
            return Err(ApplicationError::InvalidState("no update available".into()));
        };
        if offered.version != version {
            return Err(ApplicationError::VersionMismatch);
        }
        if self.inner.deps.sessions.has_active_session() {
            state.phase = Phase::BlockedBySession;
            tracing::info!("update install blocked by an active session");
            return self.inner.publish(&state);
        }
        let (token, generation) = start_op(&mut state);
        state.phase = Phase::Downloading {
            received_bytes: 0,
            total_bytes: None,
        };
        let status = self.inner.publish(&state)?;
        drop(state);

        let inner = Arc::clone(&self.inner);
        runtime.spawn(async move { inner.download_and_install(generation, offered, token).await });
        tracing::info!("update download started");
        Ok(status)
    }

    /// Cancels the check or download in progress. A check goes back to `idle`, a download to
    /// `available`. An install cannot be cancelled.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] when nothing can be cancelled;
    /// [`ApplicationError::Storage`].
    pub fn cancel(&self) -> Result<UpdateStatus, ApplicationError> {
        let mut state = self.inner.lock();
        let next = match state.phase {
            Phase::Checking => Phase::Idle,
            Phase::Downloading { .. } => Phase::Available,
            _ => return Err(ApplicationError::InvalidState("nothing to cancel".into())),
        };
        abandon_op(&mut state);
        state.phase = next;
        tracing::info!("update operation cancelled");
        self.inner.publish(&state)
    }

    /// Switches checks on or off and returns the resulting settings. Switching off cancels
    /// a check or download in progress (data-model §1.3); switching on wakes the scheduler.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn set_check_enabled(&self, enabled: bool) -> Result<AppSettings, ApplicationError> {
        let mut state = self.inner.lock();
        self.inner.deps.settings.set_update_check_enabled(enabled)?;
        if enabled {
            self.inner.wake.notify_one();
        } else if state.phase != Phase::Installing {
            abandon_op(&mut state);
            state.phase = Phase::Idle;
            state.offered = None;
        }
        self.inner.publish(&state)?;
        self.inner.deps.settings.get()
    }

    /// Records that the first-run notice was dismissed and returns the resulting settings.
    /// Checks become possible, and the scheduler runs its first one.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`].
    pub fn acknowledge_notice(&self) -> Result<AppSettings, ApplicationError> {
        let state = self.inner.lock();
        self.inner.deps.settings.acknowledge_update_notice()?;
        self.inner.wake.notify_one();
        self.inner.publish(&state)?;
        self.inner.deps.settings.get()
    }

    /// Checks at startup and then every [`CHECK_INTERVAL`], only while checks are enabled
    /// and the notice was dismissed, until `shutdown` fires. Enabling checks or dismissing
    /// the notice triggers a check right away.
    pub async fn run_scheduler(&self, shutdown: CancellationToken) {
        loop {
            match self.inner.ensure_allowed() {
                Ok(()) => match self.check() {
                    Ok(_) | Err(ApplicationError::InvalidState(_)) => {}
                    Err(err) => {
                        tracing::warn!(
                            code = err.code().as_str(),
                            "scheduled update check skipped"
                        );
                    }
                },
                Err(ApplicationError::UpdatesDisabled | ApplicationError::NoticePending) => {}
                Err(err) => {
                    tracing::warn!(code = err.code().as_str(), "update settings unreadable");
                }
            }
            tokio::select! {
                biased;
                () = shutdown.cancelled() => break,
                () = self.inner.wake.notified() => {}
                () = tokio::time::sleep(CHECK_INTERVAL) => {}
            }
        }
        let mut state = self.inner.lock();
        if state.phase != Phase::Installing {
            abandon_op(&mut state);
        }
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn ensure_allowed(&self) -> Result<(), ApplicationError> {
        if !self.deps.settings.update_check_enabled()? {
            return Err(ApplicationError::UpdatesDisabled);
        }
        if !self.deps.settings.update_notice_acknowledged()? {
            return Err(ApplicationError::NoticePending);
        }
        Ok(())
    }

    /// State with the settings gates applied, published when it changed.
    fn publish(&self, state: &State) -> Result<UpdateStatus, ApplicationError> {
        let status = self.effective(state)?;
        self.status.send_if_modified(|current| {
            if *current == status {
                false
            } else {
                current.clone_from(&status);
                true
            }
        });
        Ok(status)
    }

    fn effective(&self, state: &State) -> Result<UpdateStatus, ApplicationError> {
        match self.ensure_allowed() {
            Err(ApplicationError::UpdatesDisabled) => return Ok(UpdateStatus::Disabled),
            Err(ApplicationError::NoticePending) => return Ok(UpdateStatus::NoticePending),
            other => other?,
        }
        let version = || {
            state
                .offered
                .as_ref()
                .map(|offered| offered.version.clone())
                .unwrap_or_default()
        };
        Ok(match &state.phase {
            Phase::Idle => UpdateStatus::Idle {
                last_checked_at: self.deps.settings.update_last_checked_at()?,
            },
            Phase::Checking => UpdateStatus::Checking,
            Phase::UpToDate(at) => UpdateStatus::UpToDate {
                last_checked_at: *at,
            },
            Phase::Available | Phase::BlockedBySession => match &state.offered {
                Some(offered)
                    if state.phase == Phase::Available
                        || !self.deps.sessions.has_active_session() =>
                {
                    UpdateStatus::Available(offered.clone())
                }
                Some(offered) => UpdateStatus::BlockedBySession {
                    version: offered.version.clone(),
                },
                None => UpdateStatus::Idle {
                    last_checked_at: self.deps.settings.update_last_checked_at()?,
                },
            },
            Phase::Downloading {
                received_bytes,
                total_bytes,
            } => UpdateStatus::Downloading {
                version: version(),
                received_bytes: *received_bytes,
                total_bytes: *total_bytes,
            },
            Phase::Installing => UpdateStatus::Installing { version: version() },
            Phase::Failed(reason) => UpdateStatus::Failed(*reason),
        })
    }

    fn finish_check(
        &self,
        generation: u64,
        result: Result<Option<AvailableUpdate>, UpdateFailure>,
    ) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        state.op = None;
        let now = self.deps.clock.now();
        match result {
            Ok(found) => {
                if let Err(err) = self.deps.settings.set_update_last_checked_at(now) {
                    tracing::warn!(code = err.code().as_str(), "last update check not stored");
                }
                match found.map(sanitize) {
                    Some(Ok(update)) => {
                        tracing::info!(version = %update.version, "update available");
                        state.offered = Some(update);
                        state.phase = Phase::Available;
                    }
                    Some(Err(reason)) => fail(&mut state, reason),
                    None => {
                        tracing::info!("app is up to date");
                        state.offered = None;
                        state.phase = Phase::UpToDate(now);
                    }
                }
            }
            Err(reason) => fail(&mut state, reason),
        }
        self.publish_or_log(&state);
    }

    async fn download_and_install(
        self: Arc<Self>,
        generation: u64,
        update: AvailableUpdate,
        token: CancellationToken,
    ) {
        let published = AtomicU64::new(0);
        let progress = |received: u64, total: Option<u64>| {
            self.on_progress(generation, &published, received, total);
        };
        let result = self
            .deps
            .updater
            .download(&update, &progress, token.clone())
            .await;

        if !self.begin_install(generation, &token, result) {
            return;
        }

        tracing::info!(version = %update.version, "installing update");
        let updater = Arc::clone(&self.deps.updater);
        let installed = tokio::task::spawn_blocking(move || updater.install(&update))
            .await
            .unwrap_or(Err(UpdateFailure::InstallFailed));
        match installed {
            Ok(()) => {
                tracing::info!("update installed; restarting");
                self.deps.updater.restart();
            }
            Err(reason) => {
                let mut state = self.lock();
                state.op = None;
                fail(&mut state, reason);
                self.publish_or_log(&state);
            }
        }
    }

    /// Moves a finished download to `installing`, or records why it must not install.
    /// Returns whether to install.
    fn begin_install(
        &self,
        generation: u64,
        token: &CancellationToken,
        downloaded: Result<(), UpdateFailure>,
    ) -> bool {
        let mut state = self.lock();
        if state.generation != generation || token.is_cancelled() {
            return false;
        }
        if let Err(reason) = downloaded {
            state.op = None;
            fail(&mut state, reason);
        } else if self.deps.sessions.has_active_session() {
            // Checked again right before installing: a session may have started meanwhile.
            state.op = None;
            state.phase = Phase::BlockedBySession;
            tracing::info!("update install blocked by a session started during the download");
        } else {
            state.phase = Phase::Installing;
        }
        self.publish_or_log(&state);
        state.phase == Phase::Installing
    }

    fn on_progress(
        &self,
        generation: u64,
        published: &AtomicU64,
        received: u64,
        total: Option<u64>,
    ) {
        let mut state = self.lock();
        if state.generation != generation || !matches!(state.phase, Phase::Downloading { .. }) {
            return;
        }
        state.phase = Phase::Downloading {
            received_bytes: received,
            total_bytes: total,
        };
        let last = published.load(Ordering::Relaxed);
        let step = total.map_or(PROGRESS_STEP_UNKNOWN_TOTAL, |total| (total / 20).max(1));
        let done = total.is_some_and(|total| received >= total);
        if received.saturating_sub(last) >= step || done {
            published.store(received, Ordering::Relaxed);
            self.publish_or_log(&state);
        }
    }

    fn publish_or_log(&self, state: &State) {
        if let Err(err) = self.publish(state) {
            tracing::warn!(code = err.code().as_str(), "update status not published");
        }
    }
}

fn runtime() -> Result<Handle, ApplicationError> {
    Handle::try_current().map_err(|_| ApplicationError::Internal("no async runtime".into()))
}

fn busy() -> ApplicationError {
    ApplicationError::InvalidState("update operation in progress".into())
}

fn start_op(state: &mut State) -> (CancellationToken, u64) {
    let token = CancellationToken::new();
    state.op = Some(token.clone());
    state.generation = state.generation.wrapping_add(1);
    (token, state.generation)
}

fn abandon_op(state: &mut State) {
    if let Some(token) = state.op.take() {
        token.cancel();
    }
    state.generation = state.generation.wrapping_add(1);
}

fn fail(state: &mut State, reason: UpdateFailure) {
    tracing::warn!(
        reason = reason.code(),
        "update failed; installed version unchanged"
    );
    state.offered = None;
    state.phase = Phase::Failed(reason);
}

/// Second line of defence after the adapter: a version is required and notes are capped.
fn sanitize(mut update: AvailableUpdate) -> Result<AvailableUpdate, UpdateFailure> {
    if update.version.trim().is_empty() {
        return Err(UpdateFailure::ManifestInvalid);
    }
    if let Some((cut, _)) = update.notes.char_indices().nth(MAX_UPDATE_NOTES_CHARS) {
        update.notes.truncate(cut);
    }
    Ok(update)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(version: &str, notes: &str) -> AvailableUpdate {
        AvailableUpdate {
            version: version.into(),
            notes: notes.into(),
            published_at: None,
        }
    }

    #[test]
    fn sanitize_caps_notes_by_characters() {
        let long = "é".repeat(MAX_UPDATE_NOTES_CHARS + 10);
        let clean = sanitize(update("0.2.0", &long)).unwrap();
        assert_eq!(clean.notes.chars().count(), MAX_UPDATE_NOTES_CHARS);
        assert_eq!(sanitize(update("0.2.0", "ok")).unwrap().notes, "ok");
    }

    #[test]
    fn sanitize_requires_a_version() {
        assert_eq!(
            sanitize(update("  ", "x")).unwrap_err(),
            UpdateFailure::ManifestInvalid
        );
    }
}
