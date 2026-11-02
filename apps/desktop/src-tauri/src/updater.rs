//! `TauriUpdater`: the [`AppUpdater`] port over `tauri-plugin-updater` (002 research R5).
//!
//! The plugin is used from the core only; the webview has no updater permission. It verifies
//! each package with minisign against `plugins.updater.pubkey` and only offers versions newer
//! than the installed one. This adapter adds the rules the plugin leaves open:
//!
//! - Endpoints and the manifest's package URL must be HTTPS. The plugin only warns about a
//!   package URL, and accepts plain-HTTP endpoints in debug builds.
//! - No `dangerous*` transport option, and a well-formed public key, before any request.
//! - Release notes are capped at [`MAX_UPDATE_NOTES_CHARS`] characters.
//! - A download stops when cancelled, and only a package it verified is installed.
//! - Plugin errors become [`UpdateFailure`] codes; plugin types never leave this module.

use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine as _;
use devcopilot_application::ports::{
    AppUpdater, AvailableUpdate, BoxFuture, MAX_UPDATE_NOTES_CHARS, UpdateFailure,
};
use devcopilot_domain::time::Timestamp;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Error as PluginError, Update, UpdaterExt};
use tokio_util::sync::CancellationToken;

/// Update offered by the last check, with its package once downloaded and verified.
struct Pending {
    update: Update,
    package: Option<Vec<u8>>,
}

/// Signed updates through the Tauri updater plugin.
pub struct TauriUpdater<R: Runtime> {
    app: AppHandle<R>,
    pending: Mutex<Option<Pending>>,
    allow_plain_http: bool,
}

impl<R: Runtime> TauriUpdater<R> {
    /// Uses the plugin registered on `app` with the `plugins.updater` configuration.
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            pending: Mutex::new(None),
            allow_plain_http: false,
        }
    }

    /// Accepts plain-HTTP package URLs, so contract tests can serve packages from a local mock
    /// server. Never available in the shipped app.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn allowing_plain_http(mut self) -> Self {
        self.allow_plain_http = true;
        self
    }

    fn pending(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The plugin update matching `update`, as offered by the last check.
    fn offered(&self, update: &AvailableUpdate) -> Result<Update, UpdateFailure> {
        self.pending()
            .as_ref()
            .filter(|pending| pending.update.version == update.version)
            .map(|pending| pending.update.clone())
            .ok_or(UpdateFailure::ManifestInvalid)
    }

    async fn check_inner(&self) -> Result<Option<AvailableUpdate>, UpdateFailure> {
        // The plugin accepts plain-HTTP endpoints in debug builds; this adapter never does.
        if !self.allow_plain_http {
            let config = self.app.config().plugins.0.get("updater");
            if let Err(err) = validate_config(config.unwrap_or(&serde_json::Value::Null)) {
                tracing::warn!(error = %err, "updater configuration rejected; no request made");
                return Err(UpdateFailure::ManifestInvalid);
            }
        }
        let updater = self.app.updater().map_err(|err| check_failure(&err))?;
        let found = updater.check().await.map_err(|err| check_failure(&err))?;
        let Some(update) = found else {
            *self.pending() = None;
            return Ok(None);
        };
        let available = to_available(&update, self.allow_plain_http)?;
        *self.pending() = Some(Pending {
            update,
            package: None,
        });
        Ok(Some(available))
    }

    async fn download_inner(
        &self,
        update: &AvailableUpdate,
        progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
        cancel: CancellationToken,
    ) -> Result<(), UpdateFailure> {
        let plugin_update = self.offered(update)?;
        if let Some(size) = self
            .pending()
            .as_ref()
            .and_then(|pending| pending.package.as_ref())
            .map(|package| package.len() as u64)
        {
            // Already downloaded and verified for this version.
            progress(size, Some(size));
            return Ok(());
        }
        let mut received: u64 = 0;
        let download = plugin_update.download(
            |chunk, total| {
                received = received.saturating_add(chunk as u64);
                progress(received, total);
            },
            || {},
        );
        let package = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(UpdateFailure::DownloadInterrupted),
            result = download => result.map_err(|err| download_failure(&err))?,
        };
        // `download` verified the signature (and the signed version) before returning.
        let mut pending = self.pending();
        match pending.as_mut() {
            Some(current) if current.update.version == update.version => {
                current.package = Some(package);
                Ok(())
            }
            _ => Err(UpdateFailure::ManifestInvalid),
        }
    }
}

impl<R: Runtime> AppUpdater for TauriUpdater<R> {
    fn check(&self) -> BoxFuture<'_, Result<Option<AvailableUpdate>, UpdateFailure>> {
        Box::pin(self.check_inner())
    }

    fn download<'a>(
        &'a self,
        update: &'a AvailableUpdate,
        progress: &'a (dyn Fn(u64, Option<u64>) + Send + Sync),
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<(), UpdateFailure>> {
        Box::pin(self.download_inner(update, progress, cancel))
    }

    fn install(&self, update: &AvailableUpdate) -> Result<(), UpdateFailure> {
        let pending = self.pending();
        let Some((plugin_update, package)) = pending
            .as_ref()
            .filter(|pending| pending.update.version == update.version)
            .and_then(|pending| Some((&pending.update, pending.package.as_ref()?)))
        else {
            return Err(UpdateFailure::SignatureInvalid);
        };
        // `install` verifies the bytes again before touching the installed app.
        plugin_update
            .install(package)
            .map_err(|err| install_failure(&err))
    }

    fn restart(&self) {
        self.app.restart();
    }
}

/// Converts a plugin update, enforcing HTTPS for the package URL and capping the notes.
///
/// # Errors
///
/// [`UpdateFailure::ManifestInvalid`] for a non-HTTPS package URL (unless `allow_plain_http`)
/// or an empty version.
pub fn to_available(
    update: &Update,
    allow_plain_http: bool,
) -> Result<AvailableUpdate, UpdateFailure> {
    let scheme = update.download_url.scheme();
    if scheme != "https" && !(allow_plain_http && scheme == "http") {
        tracing::warn!(scheme, "update package URL rejected: not HTTPS");
        return Err(UpdateFailure::ManifestInvalid);
    }
    if update.version.trim().is_empty() {
        return Err(UpdateFailure::ManifestInvalid);
    }
    Ok(AvailableUpdate {
        version: update.version.clone(),
        notes: truncate_notes(update.body.as_deref().unwrap_or_default()),
        published_at: update.date.map(Timestamp::from_datetime),
    })
}

/// The first [`MAX_UPDATE_NOTES_CHARS`] characters of `notes`.
#[must_use]
pub fn truncate_notes(notes: &str) -> String {
    match notes.char_indices().nth(MAX_UPDATE_NOTES_CHARS) {
        Some((cut, _)) => notes[..cut].to_owned(),
        None => notes.to_owned(),
    }
}

fn is_signature_error(err: &PluginError) -> bool {
    matches!(
        err,
        PluginError::Minisign(_)
            | PluginError::Base64(_)
            | PluginError::SignatureUtf8(_)
            | PluginError::SignedVersionMismatch { .. }
            | PluginError::MissingSignedVersion
    )
}

/// Failure of a manifest request.
#[must_use]
pub fn check_failure(err: &PluginError) -> UpdateFailure {
    let failure = match err {
        PluginError::Reqwest(inner) if inner.is_decode() => UpdateFailure::ManifestInvalid,
        // `ReleaseNotFound`: every endpoint answered with an error status.
        PluginError::Reqwest(_)
        | PluginError::Network(_)
        | PluginError::Io(_)
        | PluginError::ReleaseNotFound => UpdateFailure::Network,
        _ => UpdateFailure::ManifestInvalid,
    };
    tracing::warn!(reason = failure.code(), error = %err, "update check failed");
    failure
}

/// Failure of a package download, which includes signature verification.
#[must_use]
pub fn download_failure(err: &PluginError) -> UpdateFailure {
    let failure = if is_signature_error(err) {
        UpdateFailure::SignatureInvalid
    } else {
        UpdateFailure::DownloadInterrupted
    };
    tracing::warn!(reason = failure.code(), error = %err, "update download failed");
    failure
}

fn install_failure(err: &PluginError) -> UpdateFailure {
    let failure = if is_signature_error(err) {
        UpdateFailure::SignatureInvalid
    } else {
        UpdateFailure::InstallFailed
    };
    tracing::warn!(reason = failure.code(), error = %err, "update install failed");
    failure
}

/// Problems with the `plugins.updater` configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdaterConfigError {
    /// The section is missing or not an object.
    #[error("plugins.updater missing")]
    Missing,
    /// No endpoint is configured.
    #[error("no updater endpoint")]
    NoEndpoint,
    /// An endpoint does not use HTTPS.
    #[error("updater endpoint is not HTTPS")]
    InsecureEndpoint,
    /// A `dangerous*` transport option is switched on.
    #[error("dangerous updater transport option enabled")]
    DangerousOption,
    /// `pubkey` is not a minisign public key.
    #[error("updater public key malformed")]
    MalformedPubkey,
}

/// Checks the `plugins.updater` section: HTTPS endpoints only, no `dangerous*` option, and
/// a well-formed public key. Returns the key id of the public key.
///
/// # Errors
///
/// See [`UpdaterConfigError`].
pub fn validate_config(updater: &serde_json::Value) -> Result<String, UpdaterConfigError> {
    let section = updater.as_object().ok_or(UpdaterConfigError::Missing)?;
    for option in [
        "dangerousInsecureTransportProtocol",
        "dangerousAcceptInvalidCerts",
        "dangerousAcceptInvalidHostnames",
        "allowDowngrades",
    ] {
        if section.get(option).and_then(serde_json::Value::as_bool) == Some(true) {
            return Err(UpdaterConfigError::DangerousOption);
        }
    }
    let endpoints = section
        .get("endpoints")
        .and_then(serde_json::Value::as_array)
        .filter(|endpoints| !endpoints.is_empty())
        .ok_or(UpdaterConfigError::NoEndpoint)?;
    for endpoint in endpoints {
        let url = endpoint
            .as_str()
            .and_then(|raw| tauri::Url::parse(raw).ok())
            .ok_or(UpdaterConfigError::InsecureEndpoint)?;
        if url.scheme() != "https" {
            return Err(UpdaterConfigError::InsecureEndpoint);
        }
    }
    section
        .get("pubkey")
        .and_then(serde_json::Value::as_str)
        .and_then(pubkey_key_id)
        .ok_or(UpdaterConfigError::MalformedPubkey)
}

/// Key id (16 uppercase hex digits, as `minisign` prints it) of a Tauri updater public key:
/// base64 of the minisign public key file.
#[must_use]
pub fn pubkey_key_id(pubkey: &str) -> Option<String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let file = String::from_utf8(engine.decode(pubkey.trim()).ok()?).ok()?;
    let key_line = file
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))?;
    let key = engine.decode(key_line).ok()?;
    // Signature algorithm "Ed", 8-byte key id (little endian), 32-byte Ed25519 key.
    if key.len() != 42 || &key[..2] != b"Ed" {
        return None;
    }
    Some(
        key[2..10]
            .iter()
            .rev()
            .map(|b| format!("{b:02X}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PUBKEY: &str = include_str!("../tests/fixtures/updater/test.key.pub");

    #[test]
    fn notes_are_capped_by_characters() {
        let long = "ç".repeat(MAX_UPDATE_NOTES_CHARS + 1);
        assert_eq!(
            truncate_notes(&long).chars().count(),
            MAX_UPDATE_NOTES_CHARS
        );
        assert_eq!(truncate_notes("<b>x</b>"), "<b>x</b>");
    }

    #[test]
    fn key_id_matches_minisign() {
        assert_eq!(
            pubkey_key_id(TEST_PUBKEY).as_deref(),
            Some("5F8BE192457A18B8")
        );
        assert_eq!(pubkey_key_id("not base64!"), None);
        assert_eq!(pubkey_key_id(""), None);
    }

    fn config(endpoint: &str) -> serde_json::Value {
        serde_json::json!({ "pubkey": TEST_PUBKEY, "endpoints": [endpoint] })
    }

    #[test]
    fn config_requires_https_and_no_dangerous_option() {
        assert_eq!(
            validate_config(&config("https://example.com/latest.json")).as_deref(),
            Ok("5F8BE192457A18B8")
        );
        assert_eq!(
            validate_config(&config("http://example.com/latest.json")),
            Err(UpdaterConfigError::InsecureEndpoint)
        );
        assert_eq!(
            validate_config(&config("not a url")),
            Err(UpdaterConfigError::InsecureEndpoint)
        );
        let mut dangerous = config("https://example.com/latest.json");
        dangerous["dangerousAcceptInvalidCerts"] = true.into();
        assert_eq!(
            validate_config(&dangerous),
            Err(UpdaterConfigError::DangerousOption)
        );
        assert_eq!(
            validate_config(&serde_json::json!({ "pubkey": TEST_PUBKEY, "endpoints": [] })),
            Err(UpdaterConfigError::NoEndpoint)
        );
        assert_eq!(
            validate_config(&serde_json::Value::Null),
            Err(UpdaterConfigError::Missing)
        );
    }
}
