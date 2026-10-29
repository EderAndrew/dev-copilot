//! Composition root: builds adapters and services and owns the app-wide state.
//!
//! If the OS credential store or the encrypted database is unavailable, the app still starts;
//! every command that needs storage then fails with a typed error the UI explains (FR-039a).

use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use devcopilot_application::ApplicationError;
use devcopilot_application::analysis_service::{AnalysisDeps, AnalysisService, DEFAULT_TICK};
use devcopilot_application::export_service::{ExportDeps, ExportService};
use devcopilot_application::ports::{
    AnalysisProviderFactory, AudioSource, AuthorizationError, Clock, EventPublisher, RemoteGate,
    SecretStore, TranscriberFactory,
};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::question_service::{QuestionDeps, QuestionService};
use devcopilot_application::record_service::RecordService;
use devcopilot_application::session_service::{SessionDeps, SessionService};
use devcopilot_application::settings_service::SettingsService;
use devcopilot_audio::{CpalMicrophone, system_source};
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::DataCategory;
use devcopilot_domain::time::Timestamp;
use devcopilot_intelligence::{AnalysisAdapterFactory, TranscriberAdapterFactory};
use devcopilot_persistence::{
    Database, KeyringSecretStore, SqliteAnalysisRunRepository, SqliteExportRepository,
    SqliteProviderRepository, SqliteQuestionRepository, SqliteRecordRepository,
    SqliteSegmentRepository, SqliteSessionRepository, SqliteSettingsRepository,
};
use tauri::{AppHandle, Manager, Runtime};
use tracing_appender::non_blocking::WorkerGuard;

use crate::commands::IpcError;
use crate::events::ChannelEventPublisher;
use crate::models::LocalModels;

/// Services that need the encrypted database.
pub struct Core {
    /// Settings use cases.
    pub settings: Arc<SettingsService>,
    /// Provider use cases.
    pub providers: Arc<ProviderService>,
    /// Session lifecycle use cases.
    pub sessions: SessionService,
    /// Record review use cases.
    pub records: Arc<RecordService>,
    /// Incremental and final analysis.
    pub analysis: AnalysisService,
    /// Contextual questions.
    pub questions: QuestionService,
    /// Session export.
    pub export: ExportService,
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
    /// Builds transcribers for configured providers.
    pub transcribers: Arc<dyn TranscriberFactory>,
    /// Builds analysis adapters for configured providers.
    pub analyzers: Arc<dyn AnalysisProviderFactory>,
    /// Gate handed to the remote adapters; connected to the provider service by
    /// [`compose_core`].
    pub gate: Arc<DeferredGate>,
}

/// [`RemoteGate`] that the adapter factories hold before storage exists: it refuses every
/// request until [`compose_core`] connects it to the provider service, and stays refusing if
/// storage never opens (research R9: deny by default).
#[derive(Default)]
pub struct DeferredGate {
    inner: OnceLock<Arc<dyn RemoteGate>>,
}

impl DeferredGate {
    /// Connects the real gate; later calls are ignored.
    pub fn connect(&self, gate: Arc<dyn RemoteGate>) {
        let _ = self.inner.set(gate);
    }
}

impl RemoteGate for DeferredGate {
    fn check(
        &self,
        provider: &ProviderId,
        categories: &[DataCategory],
    ) -> Result<(), AuthorizationError> {
        self.inner
            .get()
            .ok_or(AuthorizationError::NotAuthorized)?
            .check(provider, categories)
    }
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

/// Microphone via `cpal`; system audio as registered by the audio crate for this platform,
/// which lists it as not validated until spike S1 approves an adapter (US8, ADR-002).
#[must_use]
pub fn platform_sources() -> Vec<Arc<dyn AudioSource>> {
    vec![Arc::new(CpalMicrophone::new()), system_source()]
}

/// Where downloaded transcription models live: `<app data>/models`. Model files are not
/// session content and are not encrypted.
#[must_use]
pub fn models_dir(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("models")
}

/// Managed Tauri state.
pub struct AppState {
    core: Result<Core, ApplicationError>,
    /// Session event routing; independent of storage.
    pub events: Arc<ChannelEventPublisher>,
    /// Local model catalog and downloads; `None` without an app data directory.
    pub models: Option<Arc<LocalModels>>,
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

/// Opens the encrypted database in `data_dir`, wires the services and fails sessions and
/// questions left active by an unexpected exit (FR-007).
///
/// # Errors
///
/// `SecretStoreUnavailable` without a usable credential store; `Storage` when the database
/// cannot be opened, migrated or recovered.
pub fn compose_core(data_dir: &Path, platform: Platform) -> Result<Core, ApplicationError> {
    let db = Arc::new(Database::open_in(data_dir, platform.secrets.as_ref())?);
    let settings_repo = Arc::new(SqliteSettingsRepository::new(Arc::clone(&db)));
    let provider_repo = Arc::new(SqliteProviderRepository::new(Arc::clone(&db)));
    let session_repo = Arc::new(SqliteSessionRepository::new(Arc::clone(&db)));
    let segment_repo = Arc::new(SqliteSegmentRepository::new(Arc::clone(&db)));
    let record_repo = Arc::new(SqliteRecordRepository::new(Arc::clone(&db)));
    let question_repo = Arc::new(SqliteQuestionRepository::new(Arc::clone(&db)));
    let export_repo = Arc::new(SqliteExportRepository::new(Arc::clone(&db)));
    let run_repo = Arc::new(SqliteAnalysisRunRepository::new(db));
    let settings = Arc::new(SettingsService::new(settings_repo.clone()));
    let providers = Arc::new(ProviderService::new(
        provider_repo,
        settings_repo,
        Arc::clone(&platform.secrets),
        Arc::clone(&platform.clock),
    ));
    platform.gate.connect(providers.gate());
    let questions = QuestionService::new(QuestionDeps {
        providers: Arc::clone(&providers),
        analyzers: Arc::clone(&platform.analyzers),
        questions: question_repo.clone(),
        records: record_repo.clone(),
        segments: segment_repo.clone(),
        sessions: session_repo.clone(),
        clock: Arc::clone(&platform.clock),
        events: Arc::clone(&platform.events),
    });
    let analysis = AnalysisService::new(AnalysisDeps {
        providers: Arc::clone(&providers),
        analyzers: platform.analyzers,
        records: record_repo.clone(),
        runs: run_repo,
        segments: segment_repo.clone(),
        settings: Arc::clone(&settings),
        clock: Arc::clone(&platform.clock),
        events: Arc::clone(&platform.events),
        tick: DEFAULT_TICK,
    });
    let export = ExportService::new(ExportDeps {
        sessions: session_repo.clone(),
        segments: segment_repo.clone(),
        records: record_repo.clone(),
        questions: question_repo,
        exports: export_repo,
        settings: Arc::clone(&settings),
        clock: Arc::clone(&platform.clock),
    });
    let records = Arc::new(RecordService::new(
        record_repo,
        session_repo.clone(),
        Arc::clone(&platform.clock),
    ));
    let sessions = SessionService::new(SessionDeps {
        sessions: session_repo,
        segments: segment_repo,
        transcribers: platform.transcribers,
        providers: Arc::clone(&providers),
        settings: Arc::clone(&settings),
        secrets: platform.secrets,
        sources: platform.sources,
        clock: platform.clock,
        events: platform.events,
        analysis: Some(analysis.clone()),
        questions: Some(questions.clone()),
    });
    sessions.recover_on_startup()?;
    questions.recover_on_startup()?;
    Ok(Core {
        settings,
        providers,
        sessions,
        records,
        analysis,
        questions,
        export,
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
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| ApplicationError::Internal("app data directory unavailable".into()));
    let models = data_dir
        .as_ref()
        .ok()
        .map(|dir| Arc::new(LocalModels::new(models_dir(dir))));
    let core = data_dir.and_then(|dir| {
        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore::new());
        let gate = Arc::new(DeferredGate::default());
        let platform = Platform {
            sources: platform_sources(),
            clock: Arc::new(SystemClock),
            events: Arc::clone(&events) as Arc<dyn EventPublisher>,
            transcribers: Arc::new(TranscriberAdapterFactory::new(
                models_dir(&dir),
                Arc::clone(&gate) as Arc<dyn RemoteGate>,
                Arc::clone(&secrets),
            )),
            analyzers: Arc::new(AnalysisAdapterFactory::new(
                Arc::clone(&gate) as Arc<dyn RemoteGate>,
                Arc::clone(&secrets),
            )),
            secrets,
            gate,
        };
        compose_core(&dir, platform)
    });
    match &core {
        Ok(_) => tracing::info!("storage ready"),
        Err(err) => {
            tracing::error!(code = err.code().as_str(), error = %err, "storage unavailable")
        }
    }
    AppState {
        core,
        events,
        models,
        _log_guard: log_guard,
    }
}
