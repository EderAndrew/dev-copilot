//! Provider and settings repositories round-trip through the encrypted database (T028).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ports::{ProviderRepository, RepositoryError, SettingsRepository};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    CredentialRef, DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_persistence::{Database, SqliteProviderRepository, SqliteSettingsRepository};

fn ollama(id: ProviderId, model: &str) -> Provider {
    Provider::new(ProviderSpec {
        id,
        role: ProviderRole::Analysis,
        adapter: ProviderAdapterKind::Ollama,
        endpoint: Some("http://127.0.0.1:11434/api".into()),
        model: model.into(),
        credential_ref: Some(CredentialRef::for_provider(id)),
        data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Questions]),
    })
    .unwrap()
}

fn whisper(id: ProviderId) -> Provider {
    Provider::new(ProviderSpec {
        id,
        role: ProviderRole::Transcription,
        adapter: ProviderAdapterKind::WhisperLocal,
        endpoint: None,
        model: "ggml-base".into(),
        credential_ref: None,
        data_categories: BTreeSet::from([DataCategory::Audio]),
    })
    .unwrap()
}

#[test]
fn providers_round_trip_in_insertion_order() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let first = whisper(ProviderId::new());
    let second_id = ProviderId::new();
    {
        let db = Arc::new(Database::open_in(dir.path(), &secrets).unwrap());
        let repo = SqliteProviderRepository::new(db);
        repo.upsert(&first).unwrap();
        repo.upsert(&ollama(second_id, "llama3")).unwrap();
        repo.upsert(&ollama(second_id, "qwen3")).unwrap();
    }

    let db = Arc::new(Database::open_in(dir.path(), &secrets).unwrap());
    let repo = SqliteProviderRepository::new(db);
    let listed = repo.list().unwrap();
    assert_eq!(listed, vec![first, ollama(second_id, "qwen3")]);
    assert_eq!(repo.get(second_id).unwrap().model(), "qwen3");
    assert_eq!(repo.get(ProviderId::new()), Err(RepositoryError::NotFound));
}

#[test]
fn corrupt_rows_are_reported_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap());
    let id = ProviderId::new();
    db.with_conn(|conn| {
        // A "local" provider pointing off-machine must never be loaded.
        conn.execute(
            "INSERT INTO providers (id, role, adapter, endpoint, model, data_categories) \
             VALUES (?1, 'analysis', 'ollama', 'http://10.0.0.5:11434', 'm', 'transcript')",
            [id.to_string()],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let repo = SqliteProviderRepository::new(db);
    assert_eq!(repo.get(id), Err(RepositoryError::Corrupt("providers")));
    assert_eq!(repo.list(), Err(RepositoryError::Corrupt("providers")));
}

#[test]
fn settings_round_trip_and_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap());
    let repo = SqliteSettingsRepository::new(db);
    assert_eq!(repo.get("ui_language").unwrap(), None);
    repo.set("ui_language", "pt-BR").unwrap();
    repo.set("ui_language", "en").unwrap();
    assert_eq!(repo.get("ui_language").unwrap().as_deref(), Some("en"));
}
