//! Composition root: builds adapters and services and owns the app-wide state.
//!
//! If the OS credential store or the encrypted database is unavailable, the app still starts;
//! every command that needs storage then fails with a typed error the UI explains (FR-039a).

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use devcopilot_application::ApplicationError;
use devcopilot_application::ports::{AudioSource, Clock, EventPublisher, SecretStore};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::session_service::{SessionDeps, SessionService};
use devcopilot_application::settings_service::SettingsService;
use devcopilot_audio::{CpalMicrophone, UnavailableSource};
use devcopilot_domain::audio_source::{AudioSourceKind, UnavailableReason};
use devcopilot_domain::time::Timestamp;
use devcopilot_persistence::{
    Database, KeyringSecretStore, SqliteProviderRepository, SqliteSessionRepository,
    SqliteSettingsRepository,
};
use tauri::{AppHandle, Manager, Runtime};
use tracing_appender::non_blocking::WorkerGuard;

use crate::commands::IpcError;
use crate::events::ChannelEventPublisher;

/// Services that need the encrypted database.
pub struct Core {
    /// Settings use cases.
    pub settings: Arc<SettingsService>,
    /// Provider use cases.
    pub providers: Arc<ProviderService>,
    /// Session lifecycle use cases.
    pub sessions: SessionService,
}

/// Platform adapters that do not depend on storage.
pub struct Platform {
    /// OS credential store.
    pub secrets: Arc<dyn SecretStore>,
    /// Audio sources available on this OS, one per kind.
    pub sources: Vec<Arc<dyn AudioSource>>,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Session event delivery.
    pub events: Arc<dyn EventPublisher>,
}

/// Wall clock from the OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Timestamp::from_unix_millis(millis)
            .or_else(|_| Timestamp::from_unix_millis(0))
            .unwrap_or_else(|_| unreachable!("the epoch is always representable"))
    }
}

/// Microphone via `cpal`; system audio listed as not validated until spike S1 (US8).
#[must_use]
pub fn platform_sources() -> Vec<Arc<dyn AudioSource>> {
    vec![
        Arc::new(CpalMicrophone::new()),
        Arc::new(UnavailableSource::new(
            AudioSourceKind::System,
            UnavailableReason::NotValidatedOnPlatform,
        )),
    ]
}

/// Managed Tauri state.
pub struct AppState {
    core: Result<Core, ApplicationError>,
    /// Session event routing; independent of storage.
    pub events: Arc<ChannelEventPublisher>,
    _log_guard: Option<WorkerGuard>,
}

impl AppState {
    /// Storage-backed services, or the startup error as an [`IpcError`].
    ///
    /// # Errors
    ///
    /// The error that prevented opening storage.
    pub fn core(&self) -> Result<&Core, IpcError> {
        self.core
            .as_ref()
            .map_err(|err| IpcError::from(err.clone()))
    }
}

/// Opens the encrypted database in `data_dir`, wires the services and fails sessions left
/// active by an unexpected exit (FR-007).
///
/// # Errors
///
/// `SecretStoreUnavailable` without a usable credential store; `Storage` when the database
/// cannot be opened, migrated or recovered.
pub fn compose_core(data_dir: &Path, platform: Platform) -> Result<Core, ApplicationError> {
    let db = Arc::new(Database::open_in(data_dir, platform.secrets.as_ref())?);
    let settings_repo = Arc::new(SqliteSettingsRepository::new(Arc::clone(&db)));
    let provider_repo = Arc::new(SqliteProviderRepository::new(Arc::clone(&db)));
    let session_repo = Arc::new(SqliteSessionRepository::new(db));
    let settings = Arc::new(SettingsService::new(settings_repo.clone()));
    let providers = Arc::new(ProviderService::new(provider_repo, settings_repo));
    let sessions = SessionService::new(SessionDeps {
        sessions: session_repo,
        providers: Arc::clone(&providers),
        settings: Arc::clone(&settings),
        secrets: platform.secrets,
        sources: platform.sources,
        clock: platform.clock,
        events: platform.events,
    });
    sessions.recover_on_startup()?;
    Ok(Core {
        settings,
        providers,
        sessions,
    })
}

/// Initializes logging to a daily file in `log_dir`.
///
/// Log events carry ids, codes and states only — never session content or secrets (FR-051);
/// content types are wrapped in `Redacted`.
fn init_tracing(log_dir: Option<&Path>) -> Option<WorkerGuard> {
    let filter = tracing_subscriber::EnvFilter::try_from_env("DEVCOPILOT_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let file = log_dir.and_then(|dir| {
        std::fs::create_dir_all(dir).ok()?;
        Some(tracing_appender::rolling::daily(dir, "devcopilot.log"))
    });
    match file {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .try_init()
                .ok()?;
            Some(guard)
        }
        None => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .try_init();
            None
        }
    }
}

/// Builds the managed state for the running app.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> AppState {
    let log_guard = init_tracing(app.path().app_log_dir().ok().as_deref());
    let events = Arc::new(ChannelEventPublisher::default());
    let platform = Platform {
        secrets: Arc::new(KeyringSecretStore::new()),
        sources: platform_sources(),
        clock: Arc::new(SystemClock),
        events: Arc::clone(&events) as Arc<dyn EventPublisher>,
    };
    let core = app
        .path()
        .app_data_dir()
        .map_err(|_| ApplicationError::Internal("app data directory unavailable".into()))
        .and_then(|dir| compose_core(&dir, platform));
    match &core {
        Ok(_) => tracing::info!("storage ready"),
        Err(err) => {
            tracing::error!(code = err.code().as_str(), error = %err, "storage unavailable")
        }
    }
    AppState {
        core,
        events,
        _log_guard: log_guard,
    }
}
