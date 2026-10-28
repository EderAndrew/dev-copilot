//! Application settings (data-model.md "AppSettings") and analysis limits (research R7).
//!
//! Settings are non-sensitive and stored as key/value strings. Invalid stored values fall back
//! to defaults so a damaged row can never block the app.

use std::sync::Arc;

use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::language::UiLanguage;

use crate::error::ApplicationError;
use crate::ports::SettingsRepository;

const UI_LANGUAGE: &str = "ui_language";
const CONSENT_DISMISSED: &str = "consent_reminder_dismissed";
const TRIGGER_FINALS: &str = "analysis.trigger_finals";
const TRIGGER_SPEECH_SECS: &str = "analysis.trigger_speech_secs";
const MIN_INTERVAL_SECS: &str = "analysis.min_interval_secs";
const MAX_CALLS_PER_HOUR: &str = "analysis.max_calls_per_hour";
const MAX_TOKENS_PER_SESSION: &str = "analysis.max_tokens_per_session";
const PROVIDER_TOKEN_CAP_PREFIX: &str = "analysis.max_tokens_per_session.";

/// Inclusive bounds of a numeric limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitRange {
    /// Smallest accepted value.
    pub min: u32,
    /// Largest accepted value.
    pub max: u32,
}

/// Limits on incremental analysis cost and frequency (FR-028, R7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisLimits {
    /// Run after this many new final segments.
    pub trigger_finals: u32,
    /// Or after this much new speech, in seconds.
    pub trigger_speech_secs: u32,
    /// Minimum seconds between two runs of the same session.
    pub min_interval_secs: u32,
    /// Maximum runs per hour across sessions.
    pub max_calls_per_hour: u32,
    /// Default token budget per session and provider.
    pub max_tokens_per_session: u32,
}

impl AnalysisLimits {
    /// Accepted range of `trigger_finals`.
    pub const TRIGGER_FINALS: LimitRange = LimitRange { min: 1, max: 100 };
    /// Accepted range of `trigger_speech_secs`.
    pub const TRIGGER_SPEECH_SECS: LimitRange = LimitRange {
        min: 10,
        max: 3_600,
    };
    /// Accepted range of `min_interval_secs`.
    pub const MIN_INTERVAL_SECS: LimitRange = LimitRange { min: 5, max: 3_600 };
    /// Accepted range of `max_calls_per_hour`.
    pub const MAX_CALLS_PER_HOUR: LimitRange = LimitRange { min: 1, max: 1_000 };
    /// Accepted range of token budgets.
    pub const MAX_TOKENS_PER_SESSION: LimitRange = LimitRange {
        min: 1_000,
        max: 10_000_000,
    };
}

impl Default for AnalysisLimits {
    /// Research R7: 6 finals or 90 s, at least 30 s apart, at most 40 calls per hour.
    fn default() -> Self {
        Self {
            trigger_finals: 6,
            trigger_speech_secs: 90,
            min_interval_secs: 30,
            max_calls_per_hour: 40,
            max_tokens_per_session: 200_000,
        }
    }
}

/// User settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppSettings {
    /// Interface language.
    pub ui_language: UiLanguage,
    /// The user chose "do not show again" on the consent reminder (FR-012).
    pub consent_reminder_dismissed: bool,
    /// Analysis limits.
    pub analysis_limits: AnalysisLimits,
}

/// Partial update; `None` keeps the current value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsUpdate {
    /// New interface language.
    pub ui_language: Option<UiLanguage>,
    /// New consent-reminder flag.
    pub consent_reminder_dismissed: Option<bool>,
    /// New analysis limits (validated as a whole).
    pub analysis_limits: Option<AnalysisLimits>,
}

/// Reads and updates settings.
pub struct SettingsService {
    repo: Arc<dyn SettingsRepository>,
}

impl SettingsService {
    /// Creates the service.
    #[must_use]
    pub fn new(repo: Arc<dyn SettingsRepository>) -> Self {
        Self { repo }
    }

    /// Current settings, with defaults for missing or invalid values.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when the repository fails.
    pub fn get(&self) -> Result<AppSettings, ApplicationError> {
        let defaults = AppSettings::default();
        let limits = defaults.analysis_limits;
        Ok(AppSettings {
            ui_language: self
                .read(UI_LANGUAGE)?
                .and_then(|v| UiLanguage::parse(&v).ok())
                .unwrap_or(defaults.ui_language),
            consent_reminder_dismissed: self
                .read(CONSENT_DISMISSED)?
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.consent_reminder_dismissed),
            analysis_limits: AnalysisLimits {
                trigger_finals: self.read_limit(
                    TRIGGER_FINALS,
                    AnalysisLimits::TRIGGER_FINALS,
                    limits.trigger_finals,
                )?,
                trigger_speech_secs: self.read_limit(
                    TRIGGER_SPEECH_SECS,
                    AnalysisLimits::TRIGGER_SPEECH_SECS,
                    limits.trigger_speech_secs,
                )?,
                min_interval_secs: self.read_limit(
                    MIN_INTERVAL_SECS,
                    AnalysisLimits::MIN_INTERVAL_SECS,
                    limits.min_interval_secs,
                )?,
                max_calls_per_hour: self.read_limit(
                    MAX_CALLS_PER_HOUR,
                    AnalysisLimits::MAX_CALLS_PER_HOUR,
                    limits.max_calls_per_hour,
                )?,
                max_tokens_per_session: self.read_limit(
                    MAX_TOKENS_PER_SESSION,
                    AnalysisLimits::MAX_TOKENS_PER_SESSION,
                    limits.max_tokens_per_session,
                )?,
            },
        })
    }

    /// Applies `update` and returns the resulting settings. Nothing is written when any value
    /// is out of range.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] for out-of-range limits;
    /// [`ApplicationError::Storage`] when the repository fails.
    pub fn update(&self, update: SettingsUpdate) -> Result<AppSettings, ApplicationError> {
        if let Some(limits) = &update.analysis_limits {
            validate_limits(limits)?;
        }
        if let Some(language) = update.ui_language {
            self.repo.set(UI_LANGUAGE, language.code())?;
        }
        if let Some(dismissed) = update.consent_reminder_dismissed {
            self.repo.set(CONSENT_DISMISSED, &dismissed.to_string())?;
        }
        if let Some(limits) = update.analysis_limits {
            self.repo
                .set(TRIGGER_FINALS, &limits.trigger_finals.to_string())?;
            self.repo
                .set(TRIGGER_SPEECH_SECS, &limits.trigger_speech_secs.to_string())?;
            self.repo
                .set(MIN_INTERVAL_SECS, &limits.min_interval_secs.to_string())?;
            self.repo
                .set(MAX_CALLS_PER_HOUR, &limits.max_calls_per_hour.to_string())?;
            self.repo.set(
                MAX_TOKENS_PER_SESSION,
                &limits.max_tokens_per_session.to_string(),
            )?;
        }
        self.get()
    }

    /// Token budget per session for `provider`: its override, or the default limit.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::Storage`] when the repository fails.
    pub fn max_tokens_per_session(&self, provider: ProviderId) -> Result<u32, ApplicationError> {
        let default = self.get()?.analysis_limits.max_tokens_per_session;
        self.read_limit(
            &provider_cap_key(provider),
            AnalysisLimits::MAX_TOKENS_PER_SESSION,
            default,
        )
    }

    /// Sets a provider-specific token budget per session.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidInput`] when out of range;
    /// [`ApplicationError::Storage`] when the repository fails.
    pub fn set_provider_token_cap(
        &self,
        provider: ProviderId,
        tokens: u32,
    ) -> Result<(), ApplicationError> {
        check_range(
            "max_tokens_per_session",
            tokens,
            AnalysisLimits::MAX_TOKENS_PER_SESSION,
        )?;
        self.repo
            .set(&provider_cap_key(provider), &tokens.to_string())?;
        Ok(())
    }

    fn read(&self, key: &str) -> Result<Option<String>, ApplicationError> {
        Ok(self.repo.get(key)?)
    }

    fn read_limit(
        &self,
        key: &str,
        range: LimitRange,
        default: u32,
    ) -> Result<u32, ApplicationError> {
        Ok(self
            .read(key)?
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| (range.min..=range.max).contains(v))
            .unwrap_or(default))
    }
}

fn provider_cap_key(provider: ProviderId) -> String {
    format!("{PROVIDER_TOKEN_CAP_PREFIX}{provider}")
}

fn check_range(field: &'static str, value: u32, range: LimitRange) -> Result<(), ApplicationError> {
    if (range.min..=range.max).contains(&value) {
        Ok(())
    } else {
        Err(ApplicationError::InvalidInput {
            field,
            problem: "out_of_range",
        })
    }
}

fn validate_limits(limits: &AnalysisLimits) -> Result<(), ApplicationError> {
    check_range(
        "trigger_finals",
        limits.trigger_finals,
        AnalysisLimits::TRIGGER_FINALS,
    )?;
    check_range(
        "trigger_speech_secs",
        limits.trigger_speech_secs,
        AnalysisLimits::TRIGGER_SPEECH_SECS,
    )?;
    check_range(
        "min_interval_secs",
        limits.min_interval_secs,
        AnalysisLimits::MIN_INTERVAL_SECS,
    )?;
    check_range(
        "max_calls_per_hour",
        limits.max_calls_per_hour,
        AnalysisLimits::MAX_CALLS_PER_HOUR,
    )?;
    check_range(
        "max_tokens_per_session",
        limits.max_tokens_per_session,
        AnalysisLimits::MAX_TOKENS_PER_SESSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemorySettingsRepository;

    fn service() -> (SettingsService, Arc<InMemorySettingsRepository>) {
        let repo = Arc::new(InMemorySettingsRepository::default());
        (SettingsService::new(repo.clone()), repo)
    }

    #[test]
    fn defaults_follow_research_r7() {
        let (service, _) = service();
        let settings = service.get().unwrap();
        assert_eq!(settings.ui_language, UiLanguage::PtBr);
        assert!(!settings.consent_reminder_dismissed);
        let limits = settings.analysis_limits;
        assert_eq!(
            (
                limits.trigger_finals,
                limits.trigger_speech_secs,
                limits.min_interval_secs,
                limits.max_calls_per_hour
            ),
            (6, 90, 30, 40)
        );
    }

    #[test]
    fn partial_update_keeps_other_values() {
        let (service, _) = service();
        let updated = service
            .update(SettingsUpdate {
                ui_language: Some(UiLanguage::En),
                ..SettingsUpdate::default()
            })
            .unwrap();
        assert_eq!(updated.ui_language, UiLanguage::En);
        let updated = service
            .update(SettingsUpdate {
                consent_reminder_dismissed: Some(true),
                ..SettingsUpdate::default()
            })
            .unwrap();
        assert_eq!(updated.ui_language, UiLanguage::En);
        assert!(updated.consent_reminder_dismissed);
    }

    #[test]
    fn out_of_range_limits_write_nothing() {
        let (service, repo) = service();
        let limits = AnalysisLimits {
            min_interval_secs: 1,
            ..AnalysisLimits::default()
        };
        let err = service
            .update(SettingsUpdate {
                ui_language: Some(UiLanguage::En),
                analysis_limits: Some(limits),
                ..SettingsUpdate::default()
            })
            .unwrap_err();
        assert_eq!(
            err,
            ApplicationError::InvalidInput {
                field: "min_interval_secs",
                problem: "out_of_range"
            }
        );
        assert!(repo.snapshot().is_empty());
    }

    #[test]
    fn invalid_stored_values_fall_back_to_defaults() {
        let (service, repo) = service();
        repo.set(UI_LANGUAGE, "klingon").unwrap();
        repo.set(MAX_CALLS_PER_HOUR, "0").unwrap();
        repo.set(TRIGGER_FINALS, "abc").unwrap();
        assert_eq!(service.get().unwrap(), AppSettings::default());
    }

    #[test]
    fn provider_token_cap_overrides_default() {
        let (service, _) = service();
        let provider = ProviderId::new();
        assert_eq!(service.max_tokens_per_session(provider).unwrap(), 200_000);
        service.set_provider_token_cap(provider, 50_000).unwrap();
        assert_eq!(service.max_tokens_per_session(provider).unwrap(), 50_000);
        assert_eq!(
            service.max_tokens_per_session(ProviderId::new()).unwrap(),
            200_000
        );
        assert!(service.set_provider_token_cap(provider, 10).is_err());
    }
}
