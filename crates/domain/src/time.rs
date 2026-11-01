//! Time value types. The domain never reads the system clock; callers pass timestamps in
//! (the application `Clock` port provides them).

use std::fmt;

use time::OffsetDateTime;

use crate::error::{DomainError, InputProblem};

/// A UTC instant with millisecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    /// Builds a timestamp from milliseconds since the Unix epoch.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] when the value is outside the supported range.
    pub fn from_unix_millis(millis: i64) -> Result<Self, DomainError> {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
            .map(Self)
            .map_err(|_| DomainError::invalid("timestamp", InputProblem::Malformed))
    }

    /// Milliseconds since the Unix epoch (the storage and IPC representation).
    #[must_use]
    pub fn unix_millis(&self) -> i64 {
        // In range by construction: every value came from an `i64` of milliseconds or from
        // `from_datetime`, which truncates to milliseconds within the `time` crate's range.
        i64::try_from(self.0.unix_timestamp_nanos() / 1_000_000).unwrap_or(i64::MAX)
    }

    /// Converts a UTC date-time, truncating to milliseconds.
    #[must_use]
    pub fn from_datetime(value: OffsetDateTime) -> Self {
        let utc = value.to_offset(time::UtcOffset::UTC);
        let truncated = utc.replace_millisecond(utc.millisecond()).unwrap_or(utc);
        Self(truncated)
    }

    /// The UTC date-time.
    #[must_use]
    pub const fn as_datetime(&self) -> OffsetDateTime {
        self.0
    }

    /// RFC 3339 in UTC (e.g. `2026-11-01T12:00:00Z`, or `…:00.042Z` with milliseconds).
    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        self.0
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| self.unix_millis().to_string())
    }

    /// Parses an RFC 3339 instant, converting it to UTC and truncating to milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidInput`] when `value` is not RFC 3339.
    pub fn parse_rfc3339(value: &str) -> Result<Self, DomainError> {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Self::from_datetime)
            .map_err(|_| DomainError::invalid("timestamp", InputProblem::Malformed))
    }

    /// Time elapsed from `earlier` to `self`, or zero when `earlier` is later.
    #[must_use]
    pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
        let millis = self.unix_millis().saturating_sub(earlier.unix_millis());
        Duration::from_millis(u64::try_from(millis).unwrap_or(0))
    }

    /// The instant `duration` after `self`, saturating at the supported maximum.
    #[must_use]
    pub fn saturating_add(&self, duration: Duration) -> Self {
        let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
        Self::from_unix_millis(self.unix_millis().saturating_add(millis)).unwrap_or(*self)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ms", self.unix_millis())
    }
}

/// A non-negative span of time with millisecond precision (offsets, durations, gaps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(u64);

impl Duration {
    /// Zero length.
    pub const ZERO: Self = Self(0);

    /// Builds a duration from milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Builds a duration from whole seconds, saturating on overflow.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(1_000))
    }

    /// Length in milliseconds.
    #[must_use]
    pub const fn as_millis(&self) -> u64 {
        self.0
    }

    /// Sum, saturating on overflow.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Difference, or zero when `other` is longer.
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl From<std::time::Duration> for Duration {
    fn from(value: std::time::Duration) -> Self {
        Self(u64::try_from(value.as_millis()).unwrap_or(u64::MAX))
    }
}

impl From<Duration> for std::time::Duration {
    fn from(value: Duration) -> Self {
        Self::from_millis(value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_round_trips_millis() {
        let ts = Timestamp::from_unix_millis(1_761_600_000_123).unwrap();
        assert_eq!(ts.unix_millis(), 1_761_600_000_123);
    }

    #[test]
    fn from_datetime_truncates_to_millis() {
        let dt = OffsetDateTime::from_unix_timestamp_nanos(1_000_000_123_456_789).unwrap();
        assert_eq!(Timestamp::from_datetime(dt).unix_millis(), 1_000_000_123);
    }

    #[test]
    fn duration_since_saturates_at_zero() {
        let earlier = Timestamp::from_unix_millis(1_000).unwrap();
        let later = Timestamp::from_unix_millis(4_500).unwrap();
        assert_eq!(
            later.saturating_duration_since(earlier),
            Duration::from_millis(3_500)
        );
        assert_eq!(earlier.saturating_duration_since(later), Duration::ZERO);
    }

    #[test]
    fn add_and_arithmetic() {
        let ts = Timestamp::from_unix_millis(1_000).unwrap();
        assert_eq!(
            ts.saturating_add(Duration::from_secs(2)).unix_millis(),
            3_000
        );
        assert_eq!(
            Duration::from_millis(5).saturating_sub(Duration::from_millis(9)),
            Duration::ZERO
        );
        assert_eq!(
            Duration::from(std::time::Duration::from_millis(1_500)),
            Duration::from_millis(1_500)
        );
    }

    #[test]
    fn rfc3339_round_trips() {
        let ts = Timestamp::from_unix_millis(1_793_059_200_042).unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-10-27T00:00:00.042Z");
        assert_eq!(Timestamp::parse_rfc3339(&ts.to_rfc3339()).unwrap(), ts);
        let offset = Timestamp::parse_rfc3339("2026-10-27T03:00:00+03:00").unwrap();
        assert_eq!(offset.to_rfc3339(), "2026-10-27T00:00:00Z");
        assert!(Timestamp::parse_rfc3339("yesterday").is_err());
    }

    #[test]
    fn rejects_out_of_range_millis() {
        assert!(Timestamp::from_unix_millis(i64::MAX).is_err());
    }
}
