//! `KeyringSecretStore` against the real OS credential store.
//!
//! Runs only with `DEVCOPILOT_KEYRING_SPIKE=1` (set by CI where the vault is provisioned).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use devcopilot_application::ports::SecretStore;
use devcopilot_application::testing::ToggleableSecretStore;
use devcopilot_domain::provider::CredentialRef;
use devcopilot_persistence::{Database, KeyringSecretStore};

fn enabled() -> bool {
    let on = std::env::var("DEVCOPILOT_KEYRING_SPIKE").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: set DEVCOPILOT_KEYRING_SPIKE=1 to touch the OS credential store");
    }
    on
}

#[test]
fn secret_round_trip_and_delete() {
    if !enabled() {
        return;
    }
    let store = KeyringSecretStore::with_service("dev.devcopilot.tests");
    let reference = CredentialRef::parse("test:round-trip").unwrap();
    store.check_available().unwrap();
    store.set(&reference, &vec![1u8, 2, 3].into()).unwrap();
    assert_eq!(store.get(&reference).unwrap().unwrap().expose(), &[1, 2, 3]);
    store.delete(&reference).unwrap();
    assert_eq!(store.get(&reference).unwrap(), None);
    // Deleting a missing secret succeeds.
    store.delete(&reference).unwrap();
}

#[test]
fn database_opens_with_key_from_os_store() {
    if !enabled() {
        return;
    }
    let store = KeyringSecretStore::with_service("dev.devcopilot.tests.db");
    let dir = tempfile::tempdir().unwrap();
    drop(Database::open_in(dir.path(), &store).unwrap());
    drop(Database::open_in(dir.path(), &store).unwrap());
    store
        .delete(&CredentialRef::parse(devcopilot_persistence::db::DB_KEY_REF).unwrap())
        .unwrap();
    // Sanity: the fake store is unrelated and cannot open this database.
    assert!(Database::open_in(dir.path(), &ToggleableSecretStore::available()).is_err());
}
