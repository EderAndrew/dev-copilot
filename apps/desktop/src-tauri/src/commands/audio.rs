//! `audio_sources_list`, `audio_permission_request` and `audio_permission_help`
//! (contracts/ipc-commands.md "Fontes e permissões").

use devcopilot_application::session_service::SourceInfo;
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::IpcError;
use crate::composition::{AppState, Core};
use crate::dto::Code;

/// `availability` of an [`AudioSourceDto`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum AvailabilityDto {
    Available,
    Unavailable,
}

/// A source as listed to the user (FR-008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct AudioSourceDto {
    pub kind: Code<AudioSourceKind>,
    pub availability: AvailabilityDto,
    /// Present iff `availability` is `unavailable` (FR-014).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<Code<UnavailableReason>>,
    pub permission: Code<PermissionState>,
}

impl From<SourceInfo> for AudioSourceDto {
    fn from(info: SourceInfo) -> Self {
        let (availability, unavailable_reason) = match info.availability {
            Availability::Available => (AvailabilityDto::Available, None),
            Availability::Unavailable(reason) => (AvailabilityDto::Unavailable, Some(Code(reason))),
        };
        Self {
            kind: Code(info.kind),
            availability,
            unavailable_reason,
            permission: Code(info.permission),
        }
    }
}

/// Input naming a source kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SourceKindDto {
    pub kind: Code<AudioSourceKind>,
}

/// `audio_permission_help` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct PermissionHelpDto {
    pub message_key: String,
}

/// Platform segment of permission-help i18n keys.
#[must_use]
pub const fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// i18n key explaining how to grant `kind` on this OS (FR-010).
#[must_use]
pub fn help_key(kind: AudioSourceKind) -> String {
    format!("permissionHelp.{}.{}", platform(), kind.code())
}

/// Registered sources.
#[must_use]
pub fn list(core: &Core) -> Vec<AudioSourceDto> {
    core.sessions
        .sources()
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Every audio source with availability and permission (FR-008).
///
/// # Errors
///
/// `secret_store_unavailable` or `storage_error` when storage could not be opened.
#[tauri::command]
pub async fn audio_sources_list(
    state: State<'_, AppState>,
) -> Result<Vec<AudioSourceDto>, IpcError> {
    Ok(list(state.core()?))
}

/// Shows the OS permission prompt where the platform has one (FR-009).
///
/// # Errors
///
/// `source_unavailable` for sources that cannot be used.
#[tauri::command]
pub async fn audio_permission_request(
    state: State<'_, AppState>,
    request: SourceKindDto,
) -> Result<Code<PermissionState>, IpcError> {
    Ok(Code(
        state
            .core()?
            .sessions
            .request_permission(request.kind.0)
            .await?,
    ))
}

/// How to change the permission in the OS settings (FR-010).
///
/// # Errors
///
/// Never fails; the signature keeps the IPC convention.
#[tauri::command]
pub async fn audio_permission_help(request: SourceKindDto) -> Result<PermissionHelpDto, IpcError> {
    Ok(PermissionHelpDto {
        message_key: help_key(request.kind.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_sources_carry_their_reason() {
        let dto = AudioSourceDto::from(SourceInfo {
            kind: AudioSourceKind::System,
            availability: Availability::Unavailable(UnavailableReason::NotValidatedOnPlatform),
            permission: PermissionState::NotRequested,
        });
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "kind": "system",
                "availability": "unavailable",
                "unavailableReason": "not_validated_on_platform",
                "permission": "not_requested"
            })
        );
    }

    #[test]
    fn help_key_names_platform_and_kind() {
        let key = help_key(AudioSourceKind::Microphone);
        assert!(key.starts_with("permissionHelp."));
        assert!(key.ends_with(".microphone"));
        assert!(serde_json::from_str::<SourceKindDto>(r#"{"kind":"camera"}"#).is_err());
    }
}
