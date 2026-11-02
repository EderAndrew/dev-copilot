//! `settings_get` and `settings_update`.
//!
//! 002 adds the `updates.*` settings (contracts/ipc-updates.md): `updateCheckEnabled` is
//! writable here and goes through the update use case, so switching it off cancels a check
//! or download in progress; the other two fields are read-only.

use devcopilot_application::settings_service::{AnalysisLimits, AppSettings, SettingsUpdate};
use devcopilot_domain::language::UiLanguage;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::IpcError;
use crate::composition::AppState;
use crate::dto::Code;

/// Analysis limits (FR-028).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct AnalysisLimitsDto {
    pub trigger_finals: u32,
    pub trigger_speech_secs: u32,
    pub min_interval_secs: u32,
    pub max_calls_per_hour: u32,
    pub max_tokens_per_session: u32,
}

/// User settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SettingsDto {
    pub ui_language: Code<UiLanguage>,
    pub consent_reminder_dismissed: bool,
    pub analysis_limits: AnalysisLimitsDto,
    pub update_check_enabled: bool,
    pub update_notice_acknowledged: bool,
    /// RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_last_checked_at: Option<String>,
}

/// `Partial<SettingsDto>`; limits are replaced as a whole.
///
/// The read-only fields are accepted by the parser only so `settings_update` can answer them
/// with `invalid_input` instead of a generic deserialization error.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SettingsUpdateDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_language: Option<Code<UiLanguage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent_reminder_dismissed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_limits: Option<AnalysisLimitsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_check_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_notice_acknowledged: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_last_checked_at: Option<serde_json::Value>,
}

impl SettingsUpdateDto {
    /// Refuses the read-only fields.
    ///
    /// # Errors
    ///
    /// `invalid_input` with `problem: read_only`.
    pub fn check_writable(&self) -> Result<(), IpcError> {
        if self.update_notice_acknowledged.is_some() {
            return Err(IpcError::invalid_input(
                "updateNoticeAcknowledged",
                "read_only",
            ));
        }
        if self.update_last_checked_at.is_some() {
            return Err(IpcError::invalid_input("updateLastCheckedAt", "read_only"));
        }
        Ok(())
    }
}

impl From<AnalysisLimitsDto> for AnalysisLimits {
    fn from(dto: AnalysisLimitsDto) -> Self {
        Self {
            trigger_finals: dto.trigger_finals,
            trigger_speech_secs: dto.trigger_speech_secs,
            min_interval_secs: dto.min_interval_secs,
            max_calls_per_hour: dto.max_calls_per_hour,
            max_tokens_per_session: dto.max_tokens_per_session,
        }
    }
}

impl From<AnalysisLimits> for AnalysisLimitsDto {
    fn from(limits: AnalysisLimits) -> Self {
        Self {
            trigger_finals: limits.trigger_finals,
            trigger_speech_secs: limits.trigger_speech_secs,
            min_interval_secs: limits.min_interval_secs,
            max_calls_per_hour: limits.max_calls_per_hour,
            max_tokens_per_session: limits.max_tokens_per_session,
        }
    }
}

impl From<AppSettings> for SettingsDto {
    fn from(settings: AppSettings) -> Self {
        Self {
            ui_language: Code(settings.ui_language),
            consent_reminder_dismissed: settings.consent_reminder_dismissed,
            analysis_limits: settings.analysis_limits.into(),
            update_check_enabled: settings.update_check_enabled,
            update_notice_acknowledged: settings.update_notice_acknowledged,
            update_last_checked_at: settings.update_last_checked_at.map(|at| at.to_rfc3339()),
        }
    }
}

impl From<SettingsUpdateDto> for SettingsUpdate {
    fn from(dto: SettingsUpdateDto) -> Self {
        Self {
            ui_language: dto.ui_language.map(|c| c.0),
            consent_reminder_dismissed: dto.consent_reminder_dismissed,
            analysis_limits: dto.analysis_limits.map(Into::into),
        }
    }
}

/// Current settings.
///
/// # Errors
///
/// `secret_store_unavailable` when storage could not be opened; `storage_error`.
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<SettingsDto, IpcError> {
    Ok(state.core()?.settings.get()?.into())
}

/// Applies a partial update and returns the resulting settings.
///
/// # Errors
///
/// `invalid_input` for out-of-range limits or a read-only field; see [`settings_get`].
#[tauri::command]
pub async fn settings_update(
    state: State<'_, AppState>,
    request: SettingsUpdateDto,
) -> Result<SettingsDto, IpcError> {
    request.check_writable()?;
    let core = state.core()?;
    let check_enabled = request.update_check_enabled;
    let mut settings = core.settings.update(request.into())?;
    if let Some(enabled) = check_enabled {
        settings = state.updates()?.set_check_enabled(enabled)?;
    }
    Ok(settings.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_rejects_unknown_fields_and_codes() {
        assert!(serde_json::from_str::<SettingsUpdateDto>(r#"{"theme":"dark"}"#).is_err());
        assert!(serde_json::from_str::<SettingsUpdateDto>(r#"{"uiLanguage":"fr"}"#).is_err());
        let partial: SettingsUpdateDto = serde_json::from_str(r#"{"uiLanguage":"en"}"#).unwrap();
        assert_eq!(
            SettingsUpdate::from(partial).ui_language,
            Some(UiLanguage::En)
        );
    }

    #[test]
    fn read_only_update_fields_are_invalid_input() {
        for json in [
            r#"{"updateNoticeAcknowledged":true}"#,
            r#"{"updateLastCheckedAt":"2026-11-01T00:00:00Z"}"#,
        ] {
            let request: SettingsUpdateDto = serde_json::from_str(json).unwrap();
            let err = request.check_writable().unwrap_err();
            assert_eq!(err.code, "invalid_input");
            assert_eq!(
                err.details.get("problem").map(String::as_str),
                Some("read_only")
            );
        }
        let toggle: SettingsUpdateDto =
            serde_json::from_str(r#"{"updateCheckEnabled":false}"#).unwrap();
        assert!(toggle.check_writable().is_ok());
        assert_eq!(toggle.update_check_enabled, Some(false));
    }

    #[test]
    fn limits_require_every_field() {
        let json = r#"{"analysisLimits":{"triggerFinals":6}}"#;
        assert!(serde_json::from_str::<SettingsUpdateDto>(json).is_err());
    }
}
