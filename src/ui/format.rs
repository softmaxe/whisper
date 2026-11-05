//! Date labels shared by every view, so one day reads the same everywhere.

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

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn labels_recent_days_by_name_and_older_ones_by_date() {
        let today = d(2026, 11, 5);
        assert_eq!(day_label(today, today), t("common.dates.today"));
        assert_eq!(
            day_label(d(2026, 11, 4), today),
            t("common.dates.yesterday")
        );
        if i18n::language() == Language::En {
            assert_eq!(day_label(d(2026, 11, 3), today), "Tue, Nov 3");
            assert_eq!(day_label(d(2025, 11, 3), today), "Nov 3, 2025");
        }
    }
}
