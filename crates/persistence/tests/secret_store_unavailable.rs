//! FR-039a: without the OS credential store, the database is neither created nor opened.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use devcopilot_application::ports::{RepositoryError, SecretStore};
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::provider::CredentialRef;
use devcopilot_persistence::Database;
use devcopilot_persistence::db::{DB_FILE_NAME, DB_KEY_REF};

#[test]
fn unavailable_store_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let secrets = ToggleableSecretStore::unavailable();

    let result = Database::open_in(&data_dir, &secrets);

    assert_eq!(result.err(), Some(RepositoryError::SecretStoreUnavailable));
    assert!(!data_dir.join(DB_FILE_NAME).exists());
}

#[test]
fn existing_database_refuses_to_open_when_store_becomes_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    drop(Database::open_in(dir.path(), &secrets).unwrap());

    secrets.set_available(false);

    assert_eq!(
        Database::open_in(dir.path(), &secrets).err(),
        Some(RepositoryError::SecretStoreUnavailable)
    );
}

#[test]
fn existing_database_without_its_key_is_never_rekeyed() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    drop(Database::open_in(dir.path(), &secrets).unwrap());
    let key_ref = CredentialRef::parse(DB_KEY_REF).unwrap();
    secrets.delete(&key_ref).unwrap();

    assert_eq!(
        Database::open_in(dir.path(), &secrets).err(),
        Some(RepositoryError::SecretStoreUnavailable)
    );
    assert!(!secrets.contains(&key_ref), "no replacement key generated");
}

#[test]
fn first_open_generates_and_stores_a_256_bit_key() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    drop(Database::open_in(dir.path(), &secrets).unwrap());

    let key = secrets
        .get(&CredentialRef::parse(DB_KEY_REF).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(key.expose().len(), 32);
    assert_ne!(key.expose(), &vec![0u8; 32]);
}

#[test]
fn database_with_a_different_key_cannot_be_opened() {
    let dir = tempfile::tempdir().unwrap();
    drop(Database::open_in(dir.path(), &ToggleableSecretStore::available()).unwrap());

    let other = ToggleableSecretStore::available();
    other
        .set(
            &CredentialRef::parse(DB_KEY_REF).unwrap(),
            &vec![7u8; 32].into(),
        )
        .unwrap();

    assert!(matches!(
        Database::open_in(dir.path(), &other),
        Err(RepositoryError::Storage(_))
    ));
}

#[cfg(unix)]
#[test]
fn files_are_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let db = Database::open_in(&data_dir, &ToggleableSecretStore::available()).unwrap();

    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&data_dir), 0o700);
    assert_eq!(mode(db.path()), 0o600);
}
