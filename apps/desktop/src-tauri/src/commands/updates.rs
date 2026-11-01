//! `update_status_get`, `update_check`, `update_install`, `update_cancel`,
//! `update_notice_acknowledge` and the `update_subscribe` channel
//! (specs/002-desktop-release-readiness/contracts/ipc-updates.md).
//!
//! The webview has no updater permission: these commands are its only way in, and every rule
//! (gates, confirmation of the checked version, session blocking) is enforced by
//! [`UpdateService`], whatever the frontend shows (Art. IV.11).

use devcopilot_application::ports::UpdateFailure;
use devcopilot_application::update_service::{UpdateService, UpdateStatus};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use super::IpcError;
use super::settings::SettingsDto;
use crate::composition::AppState;

/// `failed.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum UpdateFailureDto {
    Network,
    SignatureInvalid,
    ManifestInvalid,
    DownloadInterrupted,
    InstallFailed,
}

impl From<UpdateFailure> for UpdateFailureDto {
    fn from(failure: UpdateFailure) -> Self {
        match failure {
            UpdateFailure::Network => Self::Network,
            UpdateFailure::SignatureInvalid => Self::SignatureInvalid,
            UpdateFailure::ManifestInvalid => Self::ManifestInvalid,
            UpdateFailure::DownloadInterrupted => Self::DownloadInterrupted,
            UpdateFailure::InstallFailed => Self::InstallFailed,
        }
    }
}

/// Update state (`UpdateStatusDto`). Instants are RFC 3339; `notes` is plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(missing_docs)]
pub enum UpdateStatusDto {
    Disabled,
    NoticePending,
    Idle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_checked_at: Option<String>,
    },
    Checking,
    UpToDate {
        last_checked_at: String,
    },
    Available {
        version: String,
        notes: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        published_at: Option<String>,
    },
    BlockedBySession {
        version: String,
    },
    Downloading {
        version: String,
        received_bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
    },
    Installing {
        version: String,
    },
    Failed {
        reason: UpdateFailureDto,
    },
}

impl From<UpdateStatus> for UpdateStatusDto {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::Disabled => Self::Disabled,
            UpdateStatus::NoticePending => Self::NoticePending,
            UpdateStatus::Idle { last_checked_at } => Self::Idle {
                last_checked_at: last_checked_at.map(|at| at.to_rfc3339()),
            },
            UpdateStatus::Checking => Self::Checking,
            UpdateStatus::UpToDate { last_checked_at } => Self::UpToDate {
                last_checked_at: last_checked_at.to_rfc3339(),
            },
            UpdateStatus::Available(update) => Self::Available {
                version: update.version,
                notes: update.notes,
                published_at: update.published_at.map(|at| at.to_rfc3339()),
            },
            UpdateStatus::BlockedBySession { version } => Self::BlockedBySession { version },
            UpdateStatus::Downloading {
                version,
                received_bytes,
                total_bytes,
            } => Self::Downloading {
                version,
                received_bytes,
                total_bytes,
            },
            UpdateStatus::Installing { version } => Self::Installing { version },
            UpdateStatus::Failed(reason) => Self::Failed {
                reason: reason.into(),
            },
        }
    }
}

/// `update_install` input: the version shown in the confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInstallDto {
    /// Version to install; must equal the version of the last check.
    pub version: String,
}

/// Event of the `update_subscribe` channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpdateEvent {
    /// Emitted on every transition and every ≥ 5 % of download progress.
    UpdateStatus {
        /// New state.
        status: UpdateStatusDto,
    },
}

/// Longest accepted `version` input; semver strings are far shorter.
const MAX_VERSION_LEN: usize = 64;

/// Validates the install request and delegates to the use case.
///
/// # Errors
///
/// `invalid_input` for an empty or oversized version; see [`UpdateService::install`].
pub fn install(
    updates: &UpdateService,
    request: &UpdateInstallDto,
) -> Result<UpdateStatusDto, IpcError> {
    let version = request.version.trim();
    if version.is_empty() {
        return Err(IpcError::invalid_input("version", "empty"));
    }
    if version.len() > MAX_VERSION_LEN {
        return Err(IpcError::invalid_input("version", "too_long"));
    }
    Ok(updates.install(version)?.into())
}

/// Current update state.
///
/// # Errors
///
/// `secret_store_unavailable` or `storage_error` when storage could not be opened.
#[tauri::command]
pub async fn update_status_get(state: State<'_, AppState>) -> Result<UpdateStatusDto, IpcError> {
    Ok(state.updates()?.status()?.into())
}

/// Starts a check; returns `checking`.
///
/// # Errors
///
/// `invalid_state`, `updates_disabled`, `notice_pending`.
#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> Result<UpdateStatusDto, IpcError> {
    Ok(state.updates()?.check()?.into())
}

/// Confirms the install of the checked version; returns `downloading` or
/// `blocked_by_session`.
///
/// # Errors
///
/// `invalid_input`, `invalid_state`, `version_mismatch`, `updates_disabled`.
#[tauri::command]
pub async fn update_install(
    state: State<'_, AppState>,
    request: UpdateInstallDto,
) -> Result<UpdateStatusDto, IpcError> {
    install(state.updates()?, &request)
}

/// Cancels the check or download in progress.
///
/// # Errors
///
/// `invalid_state` when nothing can be cancelled.
#[tauri::command]
pub async fn update_cancel(state: State<'_, AppState>) -> Result<UpdateStatusDto, IpcError> {
    Ok(state.updates()?.cancel()?.into())
}

/// Dismisses the first-run update notice; returns the settings.
///
/// # Errors
///
/// `storage_error`.
#[tauri::command]
pub async fn update_notice_acknowledge(
    state: State<'_, AppState>,
) -> Result<SettingsDto, IpcError> {
    Ok(state.updates()?.acknowledge_notice()?.into())
}

/// Streams update states to `channel` until the webview drops it; returns the current state.
///
/// # Errors
///
/// See [`update_status_get`].
#[tauri::command]
pub async fn update_subscribe(
    state: State<'_, AppState>,
    channel: Channel<UpdateEvent>,
) -> Result<UpdateStatusDto, IpcError> {
    let updates = state.updates()?;
    let mut receiver = updates.subscribe();
    let current = updates.status()?;
    receiver.mark_unchanged();
    tauri::async_runtime::spawn(async move {
        while receiver.changed().await.is_ok() {
            let status = receiver.borrow_and_update().clone();
            let event = UpdateEvent::UpdateStatus {
                status: status.into(),
            };
            if channel.send(event).is_err() {
                break;
            }
        }
    });
    Ok(current.into())
}

#[cfg(test)]
mod tests {
    use devcopilot_application::ports::AvailableUpdate;
    use devcopilot_domain::time::Timestamp;

    use super::*;

    #[test]
    fn status_serializes_per_contract() {
        let at = Timestamp::from_unix_millis(1_793_059_200_000).unwrap();
        let cases = [
            (
                UpdateStatus::Disabled,
                serde_json::json!({ "state": "disabled" }),
            ),
            (
                UpdateStatus::Idle {
                    last_checked_at: None,
                },
                serde_json::json!({ "state": "idle" }),
            ),
            (
                UpdateStatus::UpToDate {
                    last_checked_at: at,
                },
                serde_json::json!({ "state": "up_to_date", "lastCheckedAt": "2026-10-27T00:00:00Z" }),
            ),
            (
                UpdateStatus::Available(AvailableUpdate {
                    version: "0.2.0".into(),
                    notes: "<b>x</b>".into(),
                    published_at: None,
                }),
                serde_json::json!({ "state": "available", "version": "0.2.0", "notes": "<b>x</b>" }),
            ),
            (
                UpdateStatus::Failed(UpdateFailure::SignatureInvalid),
                serde_json::json!({ "state": "failed", "reason": "signature_invalid" }),
            ),
        ];
        for (status, json) in cases {
            assert_eq!(
                serde_json::to_value(UpdateStatusDto::from(status)).unwrap(),
                json
            );
        }
    }

    #[test]
    fn install_request_rejects_unknown_fields() {
        assert!(
            serde_json::from_str::<UpdateInstallDto>(r#"{"version":"0.2.0","force":true}"#)
                .is_err()
        );
    }
}
