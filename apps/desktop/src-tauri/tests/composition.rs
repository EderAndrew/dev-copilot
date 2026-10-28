//! The composition root wires storage-backed services or reports why it cannot (FR-039a).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::ports::{EventPublisher, SecretStore};
use devcopilot_application::settings_service::{AppSettings, SettingsUpdate};
use devcopilot_application::testing::{
    FakeClock, RecordingEventPublisher, ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory,
    ScriptedTranscriber, ScriptedTranscriberFactory, ToggleableSecretStore,
};
use devcopilot_desktop_lib::composition::{Platform, compose_core, platform_sources};
use devcopilot_domain::language::UiLanguage;

fn platform(secrets: Arc<ToggleableSecretStore>) -> Platform {
    Platform {
        secrets: secrets as Arc<dyn SecretStore>,
        sources: platform_sources(),
        clock: Arc::new(FakeClock::default()),
        events: Arc::new(RecordingEventPublisher::default()) as Arc<dyn EventPublisher>,
        transcribers: Arc::new(ScriptedTranscriberFactory::new(Arc::new(
            ScriptedTranscriber::local(Vec::new()),
        ))),
        gate: Arc::default(),
        analyzers: Arc::new(ScriptedAnalysisProviderFactory::new(Arc::new(
            ScriptedAnalysisProvider::local(),
        ))),
    }
}

#[test]
fn unavailable_secret_store_blocks_storage() {
    let dir = tempfile::tempdir().unwrap();
    let result = compose_core(
        dir.path(),
        platform(Arc::new(ToggleableSecretStore::unavailable())),
    );
    assert!(matches!(
        result,
        Err(ApplicationError::SecretStoreUnavailable)
    ));
}

#[test]
fn settings_persist_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(ToggleableSecretStore::available());
    {
        let core = compose_core(dir.path(), platform(secrets.clone())).unwrap();
        assert_eq!(core.settings.get().unwrap(), AppSettings::default());
        core.settings
            .update(SettingsUpdate {
                ui_language: Some(UiLanguage::En),
                ..SettingsUpdate::default()
            })
            .unwrap();
    }
    let core = compose_core(dir.path(), platform(secrets)).unwrap();
    assert_eq!(core.settings.get().unwrap().ui_language, UiLanguage::En);
    assert!(core.providers.list().unwrap().is_empty());
}
