//! UTC timestamps as `YYYY-MM-DDTHH:MM:SSZ`, without a date library (civil-calendar
//! conversion from Howard Hinnant's `days_from_civil` / `civil_from_days`).

use anyhow::{Context, Result, bail};

const SECS_PER_DAY: i64 = 86_400;

/// Days since 1970-01-01 → (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Both are in range by construction (1..=31, 1..=12).
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// (year, month, day) → days since 1970-01-01.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Formats Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn format(unix_secs: i64) -> String {
    let (year, month, day) = civil_from_days(unix_secs.div_euclid(SECS_PER_DAY));
    let secs = unix_secs.rem_euclid(SECS_PER_DAY);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` into Unix seconds.
///
/// # Errors
///
/// The text is not in that exact format or names an impossible date or time.
pub fn parse(text: &str) -> Result<i64> {
    let bytes = text.as_bytes();
    let shape_ok = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z';
    if !shape_ok {
        bail!("`{text}` is not YYYY-MM-DDTHH:MM:SSZ");
    }
    let field = |range: std::ops::Range<usize>| -> Result<u32> {
        let part = &text[range];
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            bail!("`{text}` has a non-digit field");
        }
        part.parse::<u32>()
            .with_context(|| format!("`{text}` has a bad field"))
    };
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let (hour, minute, second) = (field(11..13)?, field(14..16)?, field(17..19)?);
    let days = days_from_civil(i64::from(year), month, day);
    if !(1..=12).contains(&month) || civil_from_days(days) != (i64::from(year), month, day) {
        bail!("`{text}` is not a calendar date");
    }
    if hour > 23 || minute > 59 || second > 59 {
        bail!("`{text}` is not a time of day");
    }
    Ok(days * SECS_PER_DAY + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        assert_eq!(format(0), "1970-01-01T00:00:00Z");
        assert_eq!(format(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format(1_793_534_706), "2026-11-01T12:05:06Z");
        assert_eq!(format(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn round_trips() {
        for secs in [0, 59, 86_399, 951_782_400, 1_793_534_706, 4_102_444_800] {
            assert_eq!(parse(&format(secs)).unwrap(), secs);
        }
    }

    #[test]
    fn rejects_malformed_text() {
        for bad in [
            "",
            "2026-11-01 00:00:00Z",
            "2026-11-01T00:00:00",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-01T24:00:00Z",
            "2026-11-01T00:60:00Z",
            "2026-1a-01T00:00:00Z",
            "+026-11-01T00:00:00Z",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
