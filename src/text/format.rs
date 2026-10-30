//! Byte sizes and relative ages.
//!
//! Labels such as "5 minutes ago" stay with the UI; these helpers return which
//! label applies.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

const BYTE_UNITS: [&str; 9] = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];

/// "1.5 KB"-style size with up to two fraction digits, trailing zeros
/// dropped.
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 Bytes".into();
    }
    let bytes = bytes as f64;
    let k = 1024f64;
    let i = ((bytes.ln() / k.ln()).floor().max(0.0) as usize).min(BYTE_UNITS.len() - 1);
    let fixed = format!("{:.2}", bytes / k.powi(i as i32));
    let value: f64 = fixed.parse().unwrap_or(0.0);
    format!("{value} {}", BYTE_UNITS[i])
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
        assert_eq!(format_bytes(0), "0 Bytes");
        assert_eq!(format_bytes(500), "500 Bytes");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1234567), "1.18 MB");
        assert_eq!(format_bytes(u64::MAX), "16 EB");
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
