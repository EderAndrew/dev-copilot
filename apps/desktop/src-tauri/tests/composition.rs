//! The composition root wires storage-backed services or reports why it cannot (FR-039a).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use devcopilot_application::ApplicationError;
use devcopilot_application::settings_service::{AppSettings, SettingsUpdate};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_desktop_lib::composition::compose_core;
use devcopilot_domain::language::UiLanguage;

#[test]
fn unavailable_secret_store_blocks_storage() {
    let dir = tempfile::tempdir().unwrap();
    let result = compose_core(dir.path(), &ToggleableSecretStore::unavailable());
    assert!(matches!(
        result,
        Err(ApplicationError::SecretStoreUnavailable)
    ));
}

#[test]
fn settings_persist_across_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    {
        let core = compose_core(dir.path(), &secrets).unwrap();
        assert_eq!(core.settings.get().unwrap(), AppSettings::default());
        core.settings
            .update(SettingsUpdate {
                ui_language: Some(UiLanguage::En),
                ..SettingsUpdate::default()
            })
            .unwrap();
    }
    let core = compose_core(dir.path(), &secrets).unwrap();
    assert_eq!(core.settings.get().unwrap().ui_language, UiLanguage::En);
    assert!(core.providers.list().unwrap().is_empty());
}
