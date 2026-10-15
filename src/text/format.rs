//! Byte sizes, database timestamps and relative ages.
//!
//! Labels such as "5 minutes ago" stay with the UI; these helpers return which
//! label applies.

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};

const BYTE_UNITS: [&str; 9] = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];

/// "1.5 KB"-style size with up to `decimals` fraction digits, trailing zeros
/// dropped.
pub fn format_bytes(bytes: f64, decimals: i32) -> String {
    if !bytes.is_finite() || bytes <= 0.0 {
        return "0 Bytes".into();
    }
    let k = 1024f64;
    let dm = decimals.max(0) as usize;
    let i = ((bytes.ln() / k.ln()).floor().max(0.0) as usize).min(BYTE_UNITS.len() - 1);
    let fixed = format!("{:.*}", dm, bytes / k.powi(i as i32));
    let value: f64 = fixed.parse().unwrap_or(0.0);
    format!("{value} {}", BYTE_UNITS[i])
}

/// Parse a database timestamp. SQLite `CURRENT_TIMESTAMP` values carry no
/// zone and are UTC; explicit `Z` or `±HH:MM`/`±HHMM` offsets are honored.
pub fn normalize_db_date(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_t = |s: &str| s.replacen(' ', "T", 1);
    if let Ok(dt) = DateTime::parse_from_rfc3339(&with_t(trimmed)) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f%z", "%Y-%m-%dT%H:%M%z"] {
        if let Ok(dt) = DateTime::<FixedOffset>::parse_from_str(&with_t(trimmed), fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    let zoneless = trimmed
        .strip_suffix('Z')
        .or_else(|| trimmed.strip_suffix('z'))
        .unwrap_or(trimmed);
    let zoneless = with_t(zoneless);
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&zoneless, fmt) {
            return Some(naive.and_utc());
        }
    }
    NaiveDate::parse_from_str(&zoneless, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|n| n.and_utc())
}

/// Relative age of a timestamp; the UI maps each case to its i18n string
/// (`commandSearch.relativeTime.now`, `minutesAgo`, `hoursAgo`, `daysAgo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTime {
    Now,
    MinutesAgo(i64),
    HoursAgo(i64),
    DaysAgo(i64),
    /// A week or older, or in the future: show the short local date.
    Date(NaiveDate),
}

pub fn relative_time<Tz: TimeZone>(ts: DateTime<Utc>, now: &DateTime<Tz>) -> RelativeTime {
    let diff_ms = now.timestamp_millis() - ts.timestamp_millis();
    let local_date = ts.with_timezone(&now.timezone()).date_naive();
    if diff_ms < 0 {
        return RelativeTime::Date(local_date);
    }
    let (minutes, hours, days) = (diff_ms / 60_000, diff_ms / 3_600_000, diff_ms / 86_400_000);
    if minutes < 1 {
        RelativeTime::Now
    } else if minutes < 60 {
        RelativeTime::MinutesAgo(minutes)
    } else if hours < 24 {
        RelativeTime::HoursAgo(hours)
    } else if days < 7 {
        RelativeTime::DaysAgo(days)
    } else {
        RelativeTime::Date(local_date)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(0.0, 2), "0 Bytes");
        assert_eq!(format_bytes(-1.0, 2), "0 Bytes");
        assert_eq!(format_bytes(f64::NAN, 2), "0 Bytes");
        assert_eq!(format_bytes(500.0, 2), "500 Bytes");
        assert_eq!(format_bytes(1024.0, 2), "1 KB");
        assert_eq!(format_bytes(1536.0, 2), "1.5 KB");
        assert_eq!(format_bytes(1234567.0, 2), "1.18 MB");
        assert_eq!(format_bytes(1234567.0, 0), "1 MB");
        assert_eq!(format_bytes(1234567.0, -1), "1 MB");
        assert_eq!(format_bytes(0.5, 2), "0.5 Bytes");
        assert_eq!(format_bytes(1024f64.powi(9), 2), "1024 YB");
    }

    #[test]
    fn db_dates_without_zone_are_utc() {
        let d = normalize_db_date("2024-01-15 10:30:00").unwrap();
        assert_eq!(d.to_rfc3339(), "2024-01-15T10:30:00+00:00");
        let z = normalize_db_date("2024-01-15T10:30:00Z").unwrap();
        assert_eq!(z, d);
        let off = normalize_db_date("2024-01-15T10:30:00+02:00").unwrap();
        assert_eq!(off.to_rfc3339(), "2024-01-15T08:30:00+00:00");
        let compact = normalize_db_date("2024-01-15T10:30:00+0200").unwrap();
        assert_eq!(compact, off);
        let frac = normalize_db_date("2024-01-15T10:30:00.123Z").unwrap();
        assert_eq!(frac.timestamp_subsec_millis(), 123);
        assert!(normalize_db_date("2024-01-15").is_some());
    }

    #[test]
    fn invalid_db_dates() {
        for s in ["", "   ", "not-a-date", "123"] {
            assert_eq!(normalize_db_date(s), None, "{s:?}");
        }
    }

    #[test]
    fn relative_times() {
        let now = Utc.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).unwrap();
        let ago = |secs: i64| now - chrono::Duration::seconds(secs);
        assert_eq!(relative_time(ago(30), &now), RelativeTime::Now);
        assert_eq!(
            relative_time(ago(5 * 60), &now),
            RelativeTime::MinutesAgo(5)
        );
        assert_eq!(
            relative_time(ago(3 * 3600), &now),
            RelativeTime::HoursAgo(3)
        );
        assert_eq!(
            relative_time(ago(2 * 86400), &now),
            RelativeTime::DaysAgo(2)
        );
        // Future timestamps show a calendar date instead of "now".
        let future = Utc.with_ymd_and_hms(2026, 8, 21, 14, 0, 0).unwrap();
        assert_eq!(
            relative_time(future, &now),
            RelativeTime::Date(NaiveDate::from_ymd_opt(2026, 8, 21).unwrap())
        );
    }
}
