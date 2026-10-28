//! Composition root: builds adapters and services and owns the app-wide state.
//!
//! If the OS credential store or the encrypted database is unavailable, the app still starts;
//! every command that needs storage then fails with a typed error the UI explains (FR-039a).

use std::path::Path;
use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::ports::SecretStore;
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::settings_service::SettingsService;
use devcopilot_persistence::{
    Database, KeyringSecretStore, SqliteProviderRepository, SqliteSettingsRepository,
};
use tauri::{AppHandle, Manager, Runtime};
use tracing_appender::non_blocking::WorkerGuard;

use crate::commands::IpcError;
use crate::events::ChannelEventPublisher;

/// Services that need the encrypted database.
pub struct Core {
    /// Settings use cases.
    pub settings: SettingsService,
    /// Provider use cases.
    pub providers: ProviderService,
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

/// Opens the encrypted database in `data_dir` and wires the services.
///
/// # Errors
///
/// `SecretStoreUnavailable` without a usable credential store; `Storage` when the database
/// cannot be opened or migrated.
pub fn compose_core(data_dir: &Path, secrets: &dyn SecretStore) -> Result<Core, ApplicationError> {
    let db = Arc::new(Database::open_in(data_dir, secrets)?);
    let settings_repo = Arc::new(SqliteSettingsRepository::new(Arc::clone(&db)));
    let provider_repo = Arc::new(SqliteProviderRepository::new(db));
    Ok(Core {
        settings: SettingsService::new(settings_repo.clone()),
        providers: ProviderService::new(provider_repo, settings_repo),
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
    let core = app
        .path()
        .app_data_dir()
        .map_err(|_| ApplicationError::Internal("app data directory unavailable".into()))
        .and_then(|dir| compose_core(&dir, &KeyringSecretStore::new()));
    match &core {
        Ok(_) => tracing::info!("storage ready"),
        Err(err) => {
            tracing::error!(code = err.code().as_str(), error = %err, "storage unavailable")
        }
    }
    AppState {
        core,
        events: Arc::new(ChannelEventPublisher::default()),
        _log_guard: log_guard,
    }
}
