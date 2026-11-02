//! [`SecretStore`] over the OS credential store via `keyring` (ADR-004, R9).
//!
//! Keychain on macOS, Credential Manager on Windows, Secret Service on Linux. Error messages
//! never include secret bytes.

use devcopilot_application::ports::{SecretStore, SecretStoreError};
use devcopilot_domain::Redacted;
use devcopilot_domain::provider::CredentialRef;
use keyring::{Entry, Error as KeyringError};

/// Credential-store service name of the desktop app.
pub const SERVICE: &str = "dev.devcopilot.desktop";

/// Entry used to probe availability; never holds a secret.
const PROBE: &str = "availability-probe";

fn map_error(err: &KeyringError) -> SecretStoreError {
    match err {
        KeyringError::NoDefaultStore
        | KeyringError::NoStorageAccess(_)
        | KeyringError::PlatformFailure(_) => SecretStoreError::Unavailable,
        KeyringError::BadEncoding(_) | KeyringError::BadDataFormat(..) => {
            SecretStoreError::Backend("stored secret has an unexpected format".into())
        }
        KeyringError::BadStoreFormat(_) => {
            SecretStoreError::Backend("credential store has an unexpected format".into())
        }
        KeyringError::TooLong(attribute, _) | KeyringError::Invalid(attribute, _) => {
            SecretStoreError::Backend(format!("invalid credential attribute `{attribute}`"))
        }
        KeyringError::Ambiguous(_) => {
            SecretStoreError::Backend("several credentials match one reference".into())
        }
        KeyringError::NotSupportedByStore(_) => {
            SecretStoreError::Backend("operation not supported by the credential store".into())
        }
        _ => SecretStoreError::Backend("unexpected credential store error".into()),
    }
}

/// OS credential store scoped to one service name.
#[derive(Debug, Clone)]
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    /// Store for the desktop app ([`SERVICE`]).
    #[must_use]
    pub fn new() -> Self {
        Self::with_service(SERVICE)
    }

    /// Store under another service name (used to isolate tests).
    #[must_use]
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    fn entry(&self, user: &str) -> Result<Entry, SecretStoreError> {
        Entry::new(&self.service, user).map_err(|err| map_error(&err))
    }
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringSecretStore {
    fn check_available(&self) -> Result<(), SecretStoreError> {
        Entry::store_status().as_ref().map_err(map_error)?;
        match self.entry(PROBE)?.get_secret() {
            Ok(_) | Err(KeyringError::NoEntry) => Ok(()),
            Err(err) => Err(map_error(&err)),
        }
    }

    fn get(
        &self,
        reference: &CredentialRef,
    ) -> Result<Option<Redacted<Vec<u8>>>, SecretStoreError> {
        match self.entry(reference.as_str())?.get_secret() {
            Ok(secret) => Ok(Some(Redacted::new(secret))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(err) => Err(map_error(&err)),
        }
    }

    fn set(
        &self,
        reference: &CredentialRef,
        secret: &Redacted<Vec<u8>>,
    ) -> Result<(), SecretStoreError> {
        self.entry(reference.as_str())?
            .set_secret(secret.expose())
            .map_err(|err| map_error(&err))
    }

    fn delete(&self, reference: &CredentialRef) -> Result<(), SecretStoreError> {
        match self.entry(reference.as_str())?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(err) => Err(map_error(&err)),
        }
    }
}
