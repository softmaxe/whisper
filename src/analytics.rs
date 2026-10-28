//! Insights math shared by the database and the Insights view.
//!
//! The database groups events per local day in SQL; everything derived from
//! those day totals (WPM, coverage, streaks, period totals, the trend chart)
//! lives here so there is a single owner of each figure.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Utc};
use icu_segmenter::WordSegmenter;
use icu_segmenter::options::WordBreakInvariantOptions;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// The trend chart switches from daily to weekly bars beyond this many days.
pub const TREND_DAILY_LIMIT_DAYS: i64 = 92;

static CJK_SCRIPT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}]")
        .expect("valid CJK regex")
});

/// Whitespace for trimming and word splitting: `char::is_whitespace` plus
/// the byte-order mark U+FEFF, minus NEL U+0085.
pub(crate) fn is_text_whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}'
}

pub(crate) fn trim_text(value: &str) -> &str {
    value.trim_matches(is_text_whitespace)
}

fn count_whitespace_delimited_words(text: &str) -> usize {
    trim_text(text)
        .split(is_text_whitespace)
        .filter(|part| !part.is_empty())
        .count()
}

/// Words in a dictation. Whitespace-delimited, except that text containing
/// CJK is segmented with ICU's dictionary word breaker, since those scripts
/// do not separate words with spaces.
pub fn count_spoken_words(text: &str) -> usize {
    let fallback = count_whitespace_delimited_words(text);
    if fallback == 0 || !CJK_SCRIPT.is_match(text) {
        return fallback;
    }
    let segmenter = WordSegmenter::new_dictionary(WordBreakInvariantOptions::default());
    let mut segments = segmenter.segment_str(text);
    let mut count = 0;
    // The first boundary is always 0 and closes no segment.
    let _ = segments.next();
    while segments.next().is_some() {
        if segments.is_word_like() {
            count += 1;
        }
    }
    if count == 0 { fallback } else { count }
}

/// `YYYY-MM-DD` of an instant in the user's local time zone.
pub fn local_date_key<Tz: TimeZone>(instant: &DateTime<Tz>) -> String {
    instant.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// Today's local date key.
pub fn today_key() -> String {
    local_date_key(&Utc::now())
}

/// One dictation's counter, as recorded right after it is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsEventInput {
    pub word_count: i64,
    /// When the dictation was spoken, in the database's timestamp shape
    /// (`2026-09-01 10:00:00.000Z`).
    pub occurred_at: String,
    /// Local `YYYY-MM-DD` of `occurred_at`.
    pub local_date: String,
    pub spoken_duration_ms: Option<i64>,
    /// The transcription source, e.g. "self-hosted".
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl AnalyticsEventInput {
    /// Builds the event recorded when a dictation is saved: words are counted
    /// from the raw transcript when there is one.
    pub fn for_dictation(
        text: &str,
        raw_text: Option<&str>,
        occurred_at: DateTime<Utc>,
        spoken_duration_ms: Option<i64>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Self {
        let source = raw_text.filter(|raw| !raw.is_empty()).unwrap_or(text);
        Self {
            word_count: count_spoken_words(source) as i64,
            occurred_at: crate::db::timestamp::format_db_timestamp(&occurred_at),
            local_date: local_date_key(&occurred_at),
            spoken_duration_ms,
            provider: provider.filter(|p| !p.is_empty()).map(str::to_string),
            model: model.filter(|m| !m.is_empty()).map(str::to_string),
        }
    }
}

/// Per-day totals as grouped in SQL.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnalyticsDayTotals {
    pub date: String,
    pub words: i64,
    pub dictations: i64,
    pub spoken_duration_ms: i64,
    /// Words from dictations whose spoken duration is known; the WPM basis.
    pub covered_words: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsDailyBucket {
    pub date: String,
    pub words: i64,
    pub dictations: i64,
    pub spoken_duration_ms: i64,
    /// Words from dictations whose spoken duration is known; the WPM basis.
    pub covered_words: i64,
}

/// Everything the Insights view shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSummary {
    pub total_words: i64,
    pub total_dictations: i64,
    pub total_spoken_duration_ms: i64,
    /// `None` until some dictation has a measured duration.
    pub average_wpm: Option<i64>,
    pub current_streak_days: i64,
    pub longest_streak_days: i64,
    /// Share of words the WPM figure is based on ("based on X% of words").
    pub wpm_coverage_percent: i64,
    /// Ascending, at most the last 366 active days.
    pub daily: Vec<AnalyticsDailyBucket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Streaks {
    pub current_streak_days: i64,
    pub longest_streak_days: i64,
}

/// Words per minute over `covered_words`, `None` without measured speech.
fn words_per_minute(covered_words: i64, spoken_duration_ms: i64) -> Option<i64> {
    (spoken_duration_ms > 0)
        .then(|| ((covered_words as f64 * 60_000.0) / spoken_duration_ms as f64).round() as i64)
}

fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Totals over an inclusive range of local dates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PeriodTotals {
    pub words: i64,
    pub dictations: i64,
    pub spoken_duration_ms: i64,
    pub covered_words: i64,
}

impl PeriodTotals {
    pub fn wpm(&self) -> Option<i64> {
        words_per_minute(self.covered_words, self.spoken_duration_ms)
    }
}

/// Sums the days from `first` through `last`, both inclusive.
pub fn period_totals(
    daily: &[AnalyticsDailyBucket],
    first: NaiveDate,
    last: NaiveDate,
) -> PeriodTotals {
    let (first, last) = (date_key(first), date_key(last));
    daily
        .iter()
        .filter(|b| b.date >= first && b.date <= last)
        .fold(PeriodTotals::default(), |acc, b| PeriodTotals {
            words: acc.words + b.words,
            dictations: acc.dictations + b.dictations,
            spoken_duration_ms: acc.spoken_duration_ms + b.spoken_duration_ms,
            covered_words: acc.covered_words + b.covered_words,
        })
}

/// One bar of the trend chart: a day, or a week ending on `end`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendBar {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub words: i64,
    pub dictations: i64,
    /// Trailing average in words per bar: 7 days for daily bars, 4 weeks for
    /// weekly ones. `None` while that window reaches back before first use.
    pub average: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trend {
    /// 1 for daily bars, 7 for weekly ones.
    pub bucket_days: i64,
    pub bars: Vec<TrendBar>,
}

/// Bars from `start` through `today`, aligned so the last one ends today.
/// Spans longer than [`TREND_DAILY_LIMIT_DAYS`] use weekly bars.
pub fn build_trend(daily: &[AnalyticsDailyBucket], start: NaiveDate, today: NaiveDate) -> Trend {
    let by_date: HashMap<&str, (i64, i64)> = daily
        .iter()
        .map(|b| (b.date.as_str(), (b.words, b.dictations)))
        .collect();
    let first_use = daily
        .iter()
        .filter(|b| b.words > 0)
        .map(|b| b.date.as_str())
        .min()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let span = (today - start).num_days().max(0) + 1;
    let bucket_days: i64 = if span > TREND_DAILY_LIMIT_DAYS { 7 } else { 1 };
    let average_days = if bucket_days == 1 { 7 } else { 28 };
    let count = (span + bucket_days - 1) / bucket_days;
    let back = |date: NaiveDate, days: i64| date - Days::new(days as u64);
    let sum = |first: NaiveDate, last: NaiveDate| {
        first
            .iter_days()
            .take_while(|d| *d <= last)
            .filter_map(|d| by_date.get(date_key(d).as_str()))
            .fold((0, 0), |(w, n), (dw, dn)| (w + dw, n + dn))
    };
    let bars = (0..count)
        .rev()
        .map(|i| {
            let end = back(today, i * bucket_days);
            let start = back(end, bucket_days - 1);
            let (words, dictations) = sum(start, end);
            let window_start = back(end, average_days - 1);
            let average = first_use.filter(|f| window_start >= *f).map(|_| {
                sum(window_start, end).0 as f64 * bucket_days as f64 / average_days as f64
            });
            TrendBar {
                start,
                end,
                words,
                dictations,
                average,
            }
        })
        .collect();
    Trend { bucket_days, bars }
}

fn day_number(value: &str) -> Option<i64> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .map(|d| i64::from(d.num_days_from_ce()))
}

fn consecutive(earlier: Option<i64>, later: Option<i64>) -> bool {
    matches!((earlier, later), (Some(a), Some(b)) if b - a == 1)
}

/// Current and longest runs of consecutive active days. The current streak
/// survives until a full day passes without dictation.
pub fn calculate_streaks<S: AsRef<str>>(days: &[S], today: &str) -> Streaks {
    let mut days: Vec<&str> = days.iter().map(AsRef::as_ref).collect();
    days.sort_unstable();
    days.dedup();
    if days.is_empty() {
        return Streaks {
            current_streak_days: 0,
            longest_streak_days: 0,
        };
    }
    let numbers: Vec<Option<i64>> = days.iter().map(|d| day_number(d)).collect();

    let mut run = 1;
    let mut longest = 1;
    for pair in numbers.windows(2) {
        run = if consecutive(pair[0], pair[1]) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
    }

    let last = numbers[numbers.len() - 1];
    if let (Some(today), Some(last)) = (day_number(today), last)
        && today - last > 1
    {
        return Streaks {
            current_streak_days: 0,
            longest_streak_days: longest,
        };
    }

    let mut current = 1;
    for pair in numbers.windows(2).rev() {
        if !consecutive(pair[0], pair[1]) {
            break;
        }
        current += 1;
    }
    Streaks {
        current_streak_days: current,
        longest_streak_days: longest,
    }
}

/// Derives the Insights summary from one row per local date.
pub fn summarize_analytics_days(
    mut days: Vec<AnalyticsDayTotals>,
    today: &str,
) -> AnalyticsSummary {
    days.sort_by(|a, b| a.date.cmp(&b.date));
    let mut total_words = 0i64;
    let mut total_dictations = 0i64;
    let mut total_spoken_duration_ms = 0i64;
    let mut covered_words = 0i64;
    for day in &days {
        total_words += day.words;
        total_dictations += day.dictations;
        total_spoken_duration_ms += day.spoken_duration_ms;
        covered_words += day.covered_words;
    }

    let average_wpm = words_per_minute(covered_words, total_spoken_duration_ms);
    let wpm_coverage_percent = if total_words > 0 {
        ((covered_words as f64 / total_words as f64) * 100.0).round() as i64
    } else {
        0
    };
    let dates: Vec<&str> = days.iter().map(|d| d.date.as_str()).collect();
    let streaks = calculate_streaks(&dates, today);
    let skip = days.len().saturating_sub(366);
    let daily = days
        .into_iter()
        .skip(skip)
        .map(|d| AnalyticsDailyBucket {
            date: d.date,
            words: d.words,
            dictations: d.dictations,
            spoken_duration_ms: d.spoken_duration_ms,
            covered_words: d.covered_words,
        })
        .collect();

    AnalyticsSummary {
        total_words,
        total_dictations,
        total_spoken_duration_ms,
        average_wpm,
        current_streak_days: streaks.current_streak_days,
        longest_streak_days: streaks.longest_streak_days,
        wpm_coverage_percent,
        daily,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, words: i64, dictations: i64, ms: i64, covered: i64) -> AnalyticsDayTotals {
        AnalyticsDayTotals {
            date: date.into(),
            words,
            dictations,
            spoken_duration_ms: ms,
            covered_words: covered,
        }
    }

    fn bucket(date: &str, words: i64) -> AnalyticsDailyBucket {
        AnalyticsDailyBucket {
            date: date.into(),
            words,
            dictations: 1,
            spoken_duration_ms: 0,
            covered_words: 0,
        }
    }

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn period_totals_sum_an_inclusive_range() {
        let mut daily = vec![
            bucket("2026-09-01", 10),
            bucket("2026-09-02", 20),
            bucket("2026-09-03", 40),
        ];
        daily[1].spoken_duration_ms = 6_000;
        daily[1].covered_words = 20;
        let totals = period_totals(&daily, date("2026-09-02"), date("2026-09-03"));
        assert_eq!(totals.words, 60);
        assert_eq!(totals.dictations, 2);
        assert_eq!(totals.wpm(), Some(200));
        let empty = period_totals(&daily, date("2026-08-01"), date("2026-08-31"));
        assert_eq!(empty, PeriodTotals::default());
        assert_eq!(empty.wpm(), None);
    }

    #[test]
    fn daily_trend_ends_today_with_a_trailing_average() {
        let daily: Vec<_> = (1..=10)
            .map(|d| bucket(&format!("2026-09-{d:02}"), 7))
            .collect();
        let trend = build_trend(&daily, date("2026-08-28"), date("2026-09-10"));
        assert_eq!(trend.bucket_days, 1);
        assert_eq!(trend.bars.len(), 14);
        assert_eq!(trend.bars[0].start, date("2026-08-28"));
        let last = trend.bars.last().unwrap();
        assert_eq!(
            (last.start, last.end, last.words),
            (date("2026-09-10"), date("2026-09-10"), 7)
        );
        // The 7-day window first fits after first use on Sep 7.
        let averages: Vec<Option<f64>> = trend.bars.iter().map(|b| b.average).collect();
        assert_eq!(averages[..10], [None; 10]);
        assert_eq!(averages[10..], [Some(7.); 4]);
    }

    #[test]
    fn long_trends_use_weekly_bars() {
        let trend = build_trend(
            &[bucket("2026-01-05", 3), bucket("2026-09-10", 5)],
            date("2026-01-01"),
            date("2026-09-10"),
        );
        assert_eq!(trend.bucket_days, 7);
        let last = trend.bars.last().unwrap();
        assert_eq!(
            (last.start, last.end, last.words),
            (date("2026-09-04"), date("2026-09-10"), 5)
        );
        assert!(trend.bars[0].start <= date("2026-01-01"));
        assert_eq!(trend.bars.iter().map(|b| b.words).sum::<i64>(), 8);
    }

    #[test]
    fn counts_whitespace_delimited_words() {
        assert_eq!(count_spoken_words("  one two\nthree  "), 3);
        assert_eq!(count_spoken_words("hello,world"), 1);
        assert_eq!(count_spoken_words(""), 0);
        assert_eq!(count_spoken_words("   "), 0);
    }

    #[test]
    fn segments_chinese_and_japanese_without_spaces() {
        assert_eq!(count_spoken_words("今天我们测试语音输入"), 5);
        assert_eq!(count_spoken_words("今日は良い天気です"), 5);
        assert_eq!(count_spoken_words("Whisperで音声入力を試します"), 7);
    }

    #[test]
    fn computes_weighted_wpm_coverage_and_streaks() {
        let summary = summarize_analytics_days(
            vec![
                day("2026-08-30", 100, 2, 30_000, 100),
                day("2026-08-28", 100, 1, 60_000, 100),
                day("2026-08-29", 50, 1, 0, 0),
            ],
            "2026-08-30",
        );
        assert_eq!(summary.total_words, 250);
        assert_eq!(summary.total_dictations, 4);
        assert_eq!(summary.average_wpm, Some(133));
        assert_eq!(summary.wpm_coverage_percent, 80);
        assert_eq!(summary.current_streak_days, 3);
        assert_eq!(summary.longest_streak_days, 3);
        let dates: Vec<&str> = summary.daily.iter().map(|b| b.date.as_str()).collect();
        assert_eq!(dates, ["2026-08-28", "2026-08-29", "2026-08-30"]);
        let json = serde_json::to_value(&summary.daily[0]).unwrap();
        let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "coveredWords",
                "date",
                "dictations",
                "spokenDurationMs",
                "words"
            ]
        );
    }

    #[test]
    fn empty_summary_has_no_wpm() {
        let summary = summarize_analytics_days(Vec::new(), "2026-08-30");
        assert_eq!(summary.total_dictations, 0);
        assert_eq!(summary.average_wpm, None);
        assert_eq!(summary.wpm_coverage_percent, 0);
        assert_eq!(summary.current_streak_days, 0);
    }

    #[test]
    fn expires_a_stale_current_streak() {
        assert_eq!(
            calculate_streaks(&["2026-08-27", "2026-08-28"], "2026-08-30"),
            Streaks {
                current_streak_days: 0,
                longest_streak_days: 2
            }
        );
        // Yesterday still counts as current.
        assert_eq!(
            calculate_streaks(&["2026-08-28", "2026-08-29"], "2026-08-30"),
            Streaks {
                current_streak_days: 2,
                longest_streak_days: 2
            }
        );
    }

    #[test]
    fn dictation_event_counts_raw_text_first() {
        let at = DateTime::parse_from_rfc3339("2026-09-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let event = AnalyticsEventInput::for_dictation(
            "Cleaned up text here",
            Some("cleaned text"),
            at,
            Some(1500),
            Some("self-hosted"),
            None,
        );
        assert_eq!(event.word_count, 2);
        assert_eq!(event.occurred_at, "2026-09-01 10:00:00.000Z");
        assert_eq!(event.provider.as_deref(), Some("self-hosted"));
        assert_eq!(event.local_date, local_date_key(&at));
        let fallback =
            AnalyticsEventInput::for_dictation("a b c", Some(""), at, None, Some(""), None);
        assert_eq!(fallback.word_count, 3);
        assert_eq!(fallback.provider, None);
    }
}
