//! Date and time labels shared by every view, so they read the same everywhere.

use std::time::Duration;

use chrono::{Datelike, NaiveDate};

use crate::i18n::{self, Language, t};

/// A calendar day relative to `today`: "Today", "Yesterday", then a short
/// weekday and date within the year ("Tue, Nov 3"), and the full date with
/// its year before that ("Nov 3, 2025"). A bare weekday is never shown,
/// since it is ambiguous past a week.
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    if date == today {
        return t("common.dates.today");
    }
    if Some(date) == today.pred_opt() {
        return t("common.dates.yesterday");
    }
    let same_year = date.year() == today.year();
    match (i18n::language(), same_year) {
        (Language::ZhCn, true) => format!("{}月{}日", date.month(), date.day()),
        (Language::ZhCn, false) => format!("{}年{}月{}日", date.year(), date.month(), date.day()),
        (Language::En, true) => date.format("%a, %b %-d").to_string(),
        (Language::En, false) => date.format("%b %-d, %Y").to_string(),
    }
}

/// Elapsed time as m:ss, or h:mm:ss from an hour up.
pub fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn labels_recent_days_by_name_and_older_ones_by_date() {
        // Tests never switch the UI language away from English.
        let today = d(2026, 11, 5);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(d(2026, 11, 4), today), "Yesterday");
        assert_eq!(day_label(d(2026, 11, 3), today), "Tue, Nov 3");
        assert_eq!(day_label(d(2025, 11, 3), today), "Nov 3, 2025");
    }

    #[test]
    fn elapsed_reads_as_a_clock() {
        assert_eq!(format_elapsed(Duration::from_millis(3_999)), "0:03");
        assert_eq!(format_elapsed(Duration::from_secs(102)), "1:42");
        assert_eq!(format_elapsed(Duration::from_secs(3_725)), "1:02:05");
    }
}
