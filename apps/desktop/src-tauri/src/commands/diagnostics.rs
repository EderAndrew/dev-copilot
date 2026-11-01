//! `diagnostics_report_csp_violation` (002 T020/T023, research R1, contracts/ipc-updates.md).
//!
//! The webview reports a blocked resource with three fields only: the directive, the blocked
//! scheme (or `inline`/`eval`) and the disposition. The core logs
//! `csp_violation directive=… scheme=… disposition=…` at `warn`, at most
//! [`MAX_REPORTS_PER_WINDOW`] lines per [`WINDOW`]. Reports beyond that are dropped and counted;
//! the count is logged in one summary line when the next window opens with a new report.
//!
//! Nothing in a report can carry session content: the scheme is checked against a short
//! lowercase pattern and an invalid one is rejected without echoing it (Art. IV.7).

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;

use super::IpcError;

/// Longest accepted `blockedScheme`.
pub const MAX_SCHEME_LEN: usize = 16;
/// Log lines allowed per [`WINDOW`].
pub const MAX_REPORTS_PER_WINDOW: u32 = 20;
/// Rate-limit window.
pub const WINDOW: Duration = Duration::from_secs(60);

/// CSP directive that blocked the resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(missing_docs)]
pub enum CspDirective {
    StyleSrc,
    StyleSrcElem,
    StyleSrcAttr,
    ScriptSrc,
    ScriptSrcElem,
    ImgSrc,
    FontSrc,
    ConnectSrc,
    DefaultSrc,
    Other,
}

impl CspDirective {
    /// Contract value, as logged.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StyleSrc => "style-src",
            Self::StyleSrcElem => "style-src-elem",
            Self::StyleSrcAttr => "style-src-attr",
            Self::ScriptSrc => "script-src",
            Self::ScriptSrcElem => "script-src-elem",
            Self::ImgSrc => "img-src",
            Self::FontSrc => "font-src",
            Self::ConnectSrc => "connect-src",
            Self::DefaultSrc => "default-src",
            Self::Other => "other",
        }
    }
}

/// Whether the policy blocked the resource or only reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(missing_docs)]
pub enum CspDisposition {
    Enforce,
    Report,
}

impl CspDisposition {
    /// Contract value, as logged.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Report => "report",
        }
    }
}

/// `diagnostics_report_csp_violation` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct CspViolationDto {
    pub effective_directive: CspDirective,
    pub blocked_scheme: String,
    pub disposition: CspDisposition,
}

/// Empty object output (`{}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyDto {}

/// Whether `scheme` matches `^[a-z][a-z0-9+.-]{0,15}$|^inline$|^eval$`.
#[must_use]
pub fn valid_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    scheme.len() <= MAX_SCHEME_LEN
        && first.is_ascii_lowercase()
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
}

impl CspViolationDto {
    /// Checks the scheme; the error never contains it.
    ///
    /// # Errors
    ///
    /// `invalid_input` on `blockedScheme`.
    pub fn validate(&self) -> Result<(), IpcError> {
        if valid_scheme(&self.blocked_scheme) {
            Ok(())
        } else {
            Err(IpcError::invalid_input("blockedScheme", "format"))
        }
    }
}

/// What to do with one report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    /// Log the report.
    pub log: bool,
    /// Reports dropped in the previous window, to log in a summary line first.
    pub dropped_before: Option<u64>,
}

/// Fixed-window limiter of [`MAX_REPORTS_PER_WINDOW`] per [`WINDOW`].
#[derive(Debug, Default)]
pub struct RateLimiter {
    window_start: Option<Instant>,
    logged: u32,
    dropped: u64,
}

impl RateLimiter {
    /// Admits or drops a report arriving at `now`.
    pub fn admit(&mut self, now: Instant) -> Admission {
        let mut dropped_before = None;
        let expired = self
            .window_start
            .is_none_or(|start| now.saturating_duration_since(start) >= WINDOW);
        if expired {
            if self.dropped > 0 {
                dropped_before = Some(self.dropped);
            }
            *self = Self {
                window_start: Some(now),
                logged: 0,
                dropped: 0,
            };
        }
        let log = self.logged < MAX_REPORTS_PER_WINDOW;
        if log {
            self.logged += 1;
        } else {
            self.dropped += 1;
        }
        Admission {
            log,
            dropped_before,
        }
    }
}

/// Managed state of the CSP report command.
#[derive(Debug, Default)]
pub struct CspReports {
    limiter: Mutex<RateLimiter>,
}

impl CspReports {
    /// Validates and logs `report` (or counts it as dropped) as of `now`.
    ///
    /// # Errors
    ///
    /// `invalid_input` when the scheme does not match the contract.
    pub fn record(&self, report: &CspViolationDto, now: Instant) -> Result<Admission, IpcError> {
        report.validate()?;
        let admission = self
            .limiter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admit(now);
        if let Some(dropped) = admission.dropped_before {
            tracing::warn!(dropped, "csp_violation reports dropped by the rate limit");
        }
        if admission.log {
            tracing::warn!(
                directive = report.effective_directive.as_str(),
                scheme = %report.blocked_scheme,
                disposition = report.disposition.as_str(),
                "csp_violation"
            );
        }
        Ok(admission)
    }
}

/// Logs a CSP violation seen by the webview.
///
/// # Errors
///
/// `invalid_input` when `blockedScheme` does not match the contract (never echoed).
#[tauri::command]
pub async fn diagnostics_report_csp_violation(
    reports: State<'_, CspReports>,
    request: CspViolationDto,
) -> Result<EmptyDto, IpcError> {
    reports.record(&request, Instant::now())?;
    Ok(EmptyDto {})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(scheme: &str) -> CspViolationDto {
        CspViolationDto {
            effective_directive: CspDirective::StyleSrcAttr,
            blocked_scheme: scheme.to_owned(),
            disposition: CspDisposition::Enforce,
        }
    }

    #[test]
    fn dto_follows_the_contract_and_rejects_unknown_fields() {
        let json = r#"{"effectiveDirective":"style-src-elem","blockedScheme":"inline","disposition":"report"}"#;
        let parsed: CspViolationDto = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.effective_directive, CspDirective::StyleSrcElem);
        assert_eq!(parsed.disposition, CspDisposition::Report);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), json);

        for bad in [
            r#"{"effectiveDirective":"style-src","blockedScheme":"inline","disposition":"enforce","sample":"body{}"}"#,
            r#"{"effectiveDirective":"style-src","blockedScheme":"inline","disposition":"enforce","blockedURI":"https://x"}"#,
            r#"{"effectiveDirective":"frame-src","blockedScheme":"inline","disposition":"enforce"}"#,
            r#"{"effectiveDirective":"style-src","blockedScheme":"inline","disposition":"block"}"#,
            r#"{"effectiveDirective":"style-src","disposition":"enforce"}"#,
        ] {
            assert!(
                serde_json::from_str::<CspViolationDto>(bad).is_err(),
                "{bad}"
            );
        }
        assert_eq!(serde_json::to_string(&EmptyDto {}).unwrap(), "{}");
    }

    #[test]
    fn directive_codes_match_serde() {
        for directive in [
            CspDirective::StyleSrc,
            CspDirective::StyleSrcElem,
            CspDirective::StyleSrcAttr,
            CspDirective::ScriptSrc,
            CspDirective::ScriptSrcElem,
            CspDirective::ImgSrc,
            CspDirective::FontSrc,
            CspDirective::ConnectSrc,
            CspDirective::DefaultSrc,
            CspDirective::Other,
        ] {
            assert_eq!(serde_json::to_value(directive).unwrap(), directive.as_str());
        }
    }

    #[test]
    fn scheme_pattern() {
        for ok in [
            "inline",
            "eval",
            "https",
            "data",
            "blob",
            "asset",
            "a",
            "web+app",
            "x-y.z",
            "abcdefghijklmnop",
        ] {
            assert!(valid_scheme(ok), "{ok}");
        }
        for bad in [
            "",
            "HTTPS",
            "1http",
            "+x",
            "https:",
            "https://evil.example/a.css",
            "abcdefghijklmnopq",
            "in line",
            "ünicode",
            "eval\n",
        ] {
            assert!(!valid_scheme(bad), "{bad:?}");
        }
    }

    #[test]
    fn invalid_scheme_is_rejected_without_echo() {
        let secret = "https://evil.example/SECRET";
        let err = dto(secret).validate().unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(
            err.details.get("field").map(String::as_str),
            Some("blockedScheme")
        );
        let json = serde_json::to_string(&err).unwrap();
        assert!(!json.contains("SECRET") && !json.contains("evil"), "{json}");

        let reports = CspReports::default();
        assert!(reports.record(&dto(secret), Instant::now()).is_err());
        assert!(reports.record(&dto("inline"), Instant::now()).is_ok());
    }

    #[test]
    fn at_most_20_lines_per_minute_then_a_summary() {
        let reports = CspReports::default();
        let start = Instant::now();
        let admissions: Vec<Admission> = (0..25)
            .map(|i| {
                reports
                    .record(&dto("inline"), start + Duration::from_millis(i * 100))
                    .unwrap()
            })
            .collect();
        assert_eq!(admissions.iter().filter(|a| a.log).count(), 20);
        assert!(admissions[..20].iter().all(|a| a.log));
        assert!(admissions[20..].iter().all(|a| !a.log));
        assert!(admissions.iter().all(|a| a.dropped_before.is_none()));

        // Still inside the window: dropped, no summary yet.
        let late = reports
            .record(&dto("inline"), start + Duration::from_secs(59))
            .unwrap();
        assert!(!late.log && late.dropped_before.is_none());

        // The next window logs again, after a summary of the 6 dropped reports.
        let next = reports.record(&dto("inline"), start + WINDOW).unwrap();
        assert_eq!(
            next,
            Admission {
                log: true,
                dropped_before: Some(6)
            }
        );
        // The summary is logged once.
        let after = reports
            .record(&dto("inline"), start + WINDOW + Duration::from_secs(1))
            .unwrap();
        assert_eq!(after.dropped_before, None);
    }

    #[test]
    fn invalid_reports_do_not_use_the_budget() {
        let reports = CspReports::default();
        let now = Instant::now();
        for _ in 0..50 {
            assert!(reports.record(&dto("BAD"), now).is_err());
        }
        assert!(reports.record(&dto("inline"), now).unwrap().log);
    }

    #[test]
    fn quiet_window_without_drops_has_no_summary() {
        let mut limiter = RateLimiter::default();
        let start = Instant::now();
        assert!(limiter.admit(start).log);
        assert_eq!(limiter.admit(start + WINDOW * 3).dropped_before, None);
    }
}
