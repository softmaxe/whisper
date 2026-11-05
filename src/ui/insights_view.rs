//! Insights: period totals compared with the period before, a trend chart of
//! words per day (or per week for long spans) with a trailing average, and
//! day-level highlights.

use chrono::{Datelike, Days, Local, NaiveDate};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::analytics::{
    AnalyticsSummary, DayHighlights, Trend, TrendBar, build_trend, day_highlights, period_totals,
    weekday_averages,
};
use crate::i18n::{self, Language, t, t_with};

use crate::app_state::{AppEvent, AppState};

use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, drop, empty_state, icon,
    keyed_focus_handle, page_container, retention_banner, segment, segmented_track, surface_card,
};
use super::format::day_label;
use super::theme::{
    PAGE_TOP, Palette, PaletteExt, RADIUS_LG, RADIUS_MD, RADIUS_SM, TEXT_BODY, TEXT_CAPTION,
    TEXT_FIGURE, TEXT_SMALL, TEXT_TITLE, WORKSPACE_TOP_BAR_HEIGHT, tabular_nums,
};

const CHART_HEIGHT: f32 = 220.;
const AXIS_WIDTH: f32 = 40.;
/// Room right of the plot for the average's end label.
const END_GUTTER: f32 = 72.;
/// Height of the inspected bar's value label.
const VALUE_LABEL_HEIGHT: f32 = 24.;
/// A strip above the plot that holds the value label, so it never covers a
/// bar or the average line; a leader drops from it to the inspected bar.
const LABEL_STRIP: f32 = VALUE_LABEL_HEIGHT + 12.;
/// Weeks the weekly rhythm averages over, whatever the selected range: a
/// shorter span leaves one sample per weekday and repeats the trend chart.
const RHYTHM_WEEKS: u64 = 8;
/// How far the selected period's band rises above the plot.
const BAND_LIFT: f32 = 8.;
/// Room below the plot for the date axis.
const AXIS_STRIP: f32 = 24.;
const CHART_KEY: &str = "insights-chart";

/// The period the metrics cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Week,
    Month,
    AllTime,
}

impl Range {
    /// Days in the period, `None` for all time.
    fn days(self) -> Option<i64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::AllTime => None,
        }
    }

    /// Days the chart spans: the period and the one the metrics compare it
    /// with, or at least a month of all-time use.
    fn chart_days(self) -> i64 {
        self.days().map_or(30, |days| 2 * days)
    }

    fn label(self) -> String {
        match self.days() {
            Some(days) => t_with("insights.days", &[("count", &days.to_string())]),
            None => t("insights.allTime"),
        }
    }
}

pub struct InsightsView {
    summary: Option<AnalyticsSummary>,
    load_failed: bool,
    range: Range,
    /// The bar under the pointer.
    hovered: Option<usize>,
    /// The bar the arrow keys last moved to while the chart had focus.
    cursor: Option<usize>,
}

/// Locale-aware compact notation with at most one fraction digit.
pub fn compact_number(value: i64) -> String {
    let trim = |v: f64| {
        let s = format!("{:.1}", (v * 10.).round() / 10.);
        s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
    };
    let v = value as f64;
    match i18n::language() {
        Language::ZhCn => {
            if v.abs() >= 1e8 {
                format!("{}亿", trim(v / 1e8))
            } else if v.abs() >= 1e4 {
                format!("{}万", trim(v / 1e4))
            } else {
                value.to_string()
            }
        }
        Language::En => {
            if v.abs() >= 1e9 {
                format!("{}B", trim(v / 1e9))
            } else if v.abs() >= 1e6 {
                format!("{}M", trim(v / 1e6))
            } else if v.abs() >= 1e3 {
                format!("{}K", trim(v / 1e3))
            } else {
                value.to_string()
            }
        }
    }
}

/// Change from `previous` to `current`: a multiple from double up, else a
/// percentage. `None` when there is nothing to compare against.
fn relative_change(current: i64, previous: i64) -> Option<String> {
    if previous <= 0 {
        return None;
    }
    let ratio = current as f64 / previous as f64;
    let percent = ((ratio - 1.) * 100.).round() as i64;
    Some(if ratio >= 2. {
        format!("▲ {:.1}×", ratio)
    } else if percent > 0 {
        format!("▲ {percent}%")
    } else if percent < 0 {
        format!("▼ {}%", -percent)
    } else {
        "±0%".into()
    })
}

fn absolute_change(current: i64, previous: i64) -> String {
    let delta = current - previous;
    match delta.signum() {
        1 => format!("▲ {delta}"),
        -1 => format!("▼ {}", -delta),
        _ => "±0".into(),
    }
}

fn month_label(date: NaiveDate) -> String {
    match i18n::language() {
        Language::ZhCn => format!("{}月", date.month()),
        Language::En => date.format("%b").to_string(),
    }
}

fn short_date(date: NaiveDate) -> String {
    match i18n::language() {
        Language::ZhCn => format!("{}月{}日", date.month(), date.day()),
        Language::En => date.format("%b %-d").to_string(),
    }
}

/// A closed span of days, naming the month once when both ends share it:
/// "Oct 22 – 28", "Sep 28 – Oct 4". An end of today reads "Today".
fn date_span(start: NaiveDate, end: NaiveDate, today: NaiveDate) -> String {
    let same_month = start.month() == end.month() && start.year() == end.year();
    let end = if end == today {
        t("insights.today")
    } else if same_month {
        match i18n::language() {
            Language::ZhCn => format!("{}日", end.day()),
            Language::En => end.day().to_string(),
        }
    } else {
        short_date(end)
    };
    format!("{} – {end}", short_date(start))
}

fn weekday_name(index: usize) -> String {
    match i18n::language() {
        Language::ZhCn => ["周一", "周二", "周三", "周四", "周五", "周六", "周日"][index].into(),
        Language::En => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][index].into(),
    }
}

/// Spoken time as minutes, hours and minutes, or seconds under a minute.
fn spoken_time(ms: i64) -> String {
    let seconds = (ms as f64 / 1000.).round() as i64;
    if seconds < 60 {
        return t_with("insights.seconds", &[("count", &seconds.to_string())]);
    }
    let minutes = (seconds as f64 / 60.).round() as i64;
    if minutes < 60 {
        return t_with("insights.minutes", &[("count", &minutes.to_string())]);
    }
    t_with(
        "insights.hoursMinutes",
        &[
            ("hours", &(minutes / 60).to_string()),
            ("minutes", &(minutes % 60).to_string()),
        ],
    )
}

/// A 1-2-2.5-5 step giving about four gridlines up to `max`. Steps stay
/// whole numbers, so a 2.5 step only appears from 25 up and every tick
/// reads exactly.
fn grid_step(max: f64) -> f64 {
    let raw = (max / 4.).max(1.);
    let magnitude = 10f64.powf(raw.log10().floor());
    [1., 2., 2.5, 5., 10.]
        .into_iter()
        .map(|m| m * magnitude)
        .filter(|step| step.fract() == 0.)
        .find(|step| *step >= raw)
        .unwrap_or(10. * magnitude)
}

/// A bar's drawn height. Idle days keep a stub on the baseline so they read
/// as zero, not as missing data.
fn bar_height(bar: &TrendBar, y: &impl Fn(f64) -> f32) -> f32 {
    if bar.words > 0 {
        (CHART_HEIGHT - y(bar.words as f64)).max(3.)
    } else {
        2.
    }
}

/// Splits the average line into unbroken runs, each wholly in the current
/// period or the comparison one. A segment takes the period of its later
/// end, so the line turns to the accent as it enters the current period.
type Run = (bool, Vec<(f32, f32)>);
fn average_runs(points: &[Option<(f32, f32)>], period_from: usize) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut current: Option<Run> = None;
    let mut previous = None;
    for (index, point) in points.iter().enumerate() {
        let Some(point) = *point else {
            runs.extend(current.take());
            previous = None;
            continue;
        };
        let in_period = index >= period_from;
        match current.as_mut() {
            Some((kind, run)) if *kind == in_period => run.push(point),
            _ => {
                runs.extend(current.take());
                current = previous.map(|from| (in_period, vec![from, point]));
            }
        }
        previous = Some(point);
    }
    runs.extend(current);
    runs
}

/// Date labels under the bars. Short daily spans label every few days back
/// from today; longer spans label month starts (and mid-month on daily bars)
/// with enough room between them. The last bar is always "Today".
fn axis_labels(bars: &[TrendBar], bucket_days: i64) -> Vec<Option<String>> {
    let count = bars.len();
    let mut labels = vec![None; count];
    let Some(last) = count.checked_sub(1) else {
        return labels;
    };
    if bucket_days == 1 && count <= 31 {
        let every = count.div_ceil(8);
        for (index, bar) in bars.iter().enumerate().take(last) {
            if (last - index) % every == 0 {
                labels[index] = Some(short_date(bar.end));
            }
        }
    } else {
        let min_gap = (count as f32 * 0.1).ceil() as usize;
        // "Today" sits flush right, so an anchor needs less room before it
        // than between two centered labels.
        let today_gap = (count as f32 * 0.05).ceil() as usize;
        let mut previous: Option<usize> = None;
        for (index, bar) in bars.iter().enumerate().take(last) {
            let anchor = if bucket_days == 1 {
                bar.end.day() == 1 || bar.end.day() == 15
            } else {
                bar.start.month() != bar.end.month() || bar.start.day() == 1
            };
            if anchor && index + today_gap <= last && previous.is_none_or(|p| index >= p + min_gap)
            {
                labels[index] = Some(if bucket_days == 1 {
                    short_date(bar.end)
                } else {
                    month_label(bar.end)
                });
                previous = Some(index);
            }
        }
    }
    labels[last] = Some(t("insights.today"));
    labels
}

/// A bar in the selected period, today included: the accent hue at a lower
/// lightness and chroma that still clears 3:1 against the canvas, so the
/// full accent stays free to mark the bar under the pointer or cursor. Its
/// 1 px edge is a step stronger in the same hue, matching the comparison
/// bars' edge so neither period looks more defined than the other.
fn period_tone(p: &Palette) -> (Hsla, Hsla) {
    if p.dark {
        (
            hsla(p.primary.h, 0.45, 0.42, 1.),
            hsla(p.primary.h, 0.5, 0.56, 1.),
        )
    } else {
        (
            hsla(p.primary.h, 0.55, 0.60, 1.),
            hsla(p.primary.h, 0.5, 0.48, 1.),
        )
    }
}

/// A comparison-period bar: a neutral fill inside a stronger neutral 1 px
/// edge. The edge clears 3:1 against the canvas (4:1 dark, 3.4:1 light) so
/// the bar's shape reads on its own, while the grey keeps the period a step
/// behind the tinted bars it is compared with. Dark mode needs a lighter
/// fill than light mode for the same separation from the canvas.
fn comparison_tone(p: &Palette) -> (Hsla, Hsla) {
    if p.dark {
        (p.foreground.opacity(0.3), p.foreground.opacity(0.5))
    } else {
        (p.foreground.opacity(0.14), p.foreground.opacity(0.5))
    }
}

/// A weekday that is not the busiest: clearly recessive, so the one bar in
/// the full accent is the only thing that stands out.
fn rhythm_bar(p: &Palette) -> Hsla {
    p.foreground.opacity(if p.dark { 0.28 } else { 0.22 })
}

/// When a bar covers: its day, or the week it closes.
fn bar_when(bar: &TrendBar, bucket_days: i64, today: NaiveDate) -> String {
    if bucket_days == 1 {
        day_label(bar.end, today)
    } else {
        format!("{} – {}", short_date(bar.start), short_date(bar.end))
    }
}

/// A bar's figures, "52 words · 3 dictations".
fn bar_figures(bar: &TrendBar) -> String {
    format!(
        "{} · {}",
        t_with("insights.wordCount", &[("count", &bar.words.to_string())]),
        t_with(
            "insights.dictationCount",
            &[("count", &bar.dictations.to_string())],
        ),
    )
}

/// A card section with a title and a one-line description.
fn panel(title: String, description: String, p: &Palette) -> Div {
    surface_card(p)
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .px(px(16.))
        .pt(px(14.))
        .pb(px(8.))
        .child(
            div()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.foreground)
                .child(title),
        )
        .child(
            div()
                .mt(px(2.))
                .text_size(px(TEXT_SMALL))
                .text_color(p.subtle_foreground)
                .truncate()
                .child(description),
        )
}

impl InsightsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        cx.subscribe(&state, |this, _, event: &AppEvent, cx| {
            if matches!(
                event,
                AppEvent::AnalyticsChanged | AppEvent::HistoryChanged | AppEvent::SettingsChanged
            ) {
                this.load(cx);
            }
        })
        .detach();
        let mut this = Self {
            summary: None,
            load_failed: false,
            range: Range::Week,
            hovered: None,
            cursor: None,
        };
        this.load(cx);
        this
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        let db = AppState::global(cx).read(cx).db.clone();
        match db.get_analytics_summary() {
            Ok(summary) => {
                self.summary = Some(summary);
                self.load_failed = false;
            }
            Err(err) => {
                log::error!("reading local Insights failed: {err}");
                self.load_failed = true;
            }
        }
        cx.notify();
    }

    fn set_range(&mut self, range: Range, cx: &mut Context<Self>) {
        self.range = range;
        self.hovered = None;
        self.cursor = None;
        cx.notify();
    }

    /// First day of the selected period; for all time, the first day of use.
    fn period_start(&self, summary: &AnalyticsSummary, today: NaiveDate) -> NaiveDate {
        match self.range.days() {
            Some(days) => today - Days::new(days as u64 - 1),
            None => first_use(summary).unwrap_or(today),
        }
    }

    fn range_picker(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut picker = segmented_track(p);
        for (index, range) in [Range::Week, Range::Month, Range::AllTime]
            .into_iter()
            .enumerate()
        {
            let active = self.range == range;
            picker = picker.child(
                segment(("insights-range", index), range.label(), active, cx)
                    .h(px(28.))
                    .child(range.label())
                    .on_click(cx.listener(move |this, _, _, cx| this.set_range(range, cx))),
            );
        }
        picker
    }

    /// What the period-scoped cards cover: "Last 7 days" or "All time".
    fn scope(&self) -> String {
        match self.range.days() {
            Some(days) => t_with("insights.lastDays", &[("count", &days.to_string())]),
            None => t("insights.allTime"),
        }
    }

    /// The dates the selected period covers, e.g. "Oct 29 – Today".
    fn period_caption(&self, summary: &AnalyticsSummary, today: NaiveDate) -> String {
        let start = self.period_start(summary, today);
        match self.range.days() {
            Some(_) => date_span(start, today, today),
            None => t_with("insights.since", &[("date", &short_date(start))]),
        }
    }

    /// A metric with its comparison line: an emphasized `change` (if any)
    /// followed by a muted `note`. A `unit` sits after the figure in a
    /// smaller, quieter face so the number leads.
    fn metric(
        label: String,
        value: String,
        unit: Option<String>,
        change: Option<String>,
        note: String,
        p: &Palette,
    ) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .border_t_1()
            .border_color(p.border)
            .pt(px(14.))
            .child(
                div()
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.muted_foreground)
                    .truncate()
                    .child(label),
            )
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .line_height(px(40.))
                    .font_features(tabular_nums())
                    .child(
                        div()
                            .text_size(px(TEXT_FIGURE))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(value),
                    )
                    .children(unit.map(|unit| {
                        div()
                            .text_size(px(TEXT_TITLE))
                            .text_color(p.muted_foreground)
                            .child(unit)
                    })),
            )
            .child(
                div()
                    .mt(px(2.))
                    .flex()
                    .gap(px(4.))
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .text_color(p.subtle_foreground)
                    .children(change.map(|c| {
                        div()
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(c)
                    }))
                    .child(div().min_w_0().truncate().child(note)),
            )
    }

    fn metrics(&self, summary: &AnalyticsSummary, today: NaiveDate, p: &Palette) -> [Div; 4] {
        let streak = Self::metric(
            t("insights.currentStreak"),
            summary.current_streak_days.to_string(),
            Some(t_with(
                "insights.dayUnit",
                &[("count", &summary.current_streak_days.to_string())],
            )),
            None,
            // The streak runs up to today whatever the range, so its note
            // names its own, all-time scope.
            t_with(
                "insights.longestStreak",
                &[("count", &summary.longest_streak_days.to_string())],
            ),
            p,
        );
        let Some(days) = self.range.days() else {
            return [
                Self::metric(
                    t("insights.wordsSpoken"),
                    compact_number(summary.total_words),
                    None,
                    None,
                    t("insights.allTime"),
                    p,
                ),
                Self::metric(
                    t("insights.dictations"),
                    compact_number(summary.total_dictations),
                    None,
                    None,
                    t("insights.allTime"),
                    p,
                ),
                Self::metric(
                    t("insights.wordsPerMinute"),
                    summary
                        .average_wpm
                        .map(compact_number)
                        .unwrap_or_else(|| "—".into()),
                    None,
                    None,
                    t_with(
                        "insights.wpmCoverage",
                        &[("count", &summary.wpm_coverage_percent.to_string())],
                    ),
                    p,
                ),
                streak,
            ];
        };

        let back = |n: i64| today - Days::new(n as u64);
        let current = period_totals(&summary.daily, back(days - 1), today);
        let previous = period_totals(&summary.daily, back(2 * days - 1), back(days));
        let count = days.to_string();
        let versus = t_with("insights.vsPrevious", &[("count", &count)]);
        let nothing_before = t_with("insights.noPrevious", &[("count", &count)]);
        let compared = |now: i64, before: i64| match relative_change(now, before) {
            Some(change) => (Some(change), versus.clone()),
            None => (None, nothing_before.clone()),
        };

        let (words_change, words_note) = compared(current.words, previous.words);
        let (dictations_change, dictations_note) =
            compared(current.dictations, previous.dictations);
        let (pace_change, pace_note) = match (current.wpm(), previous.wpm()) {
            (None, _) => (None, t("insights.noMeasured")),
            (Some(now), Some(before)) => (
                Some(t_with(
                    "insights.wpmChange",
                    &[("change", &absolute_change(now, before))],
                )),
                versus.clone(),
            ),
            (Some(_), None) => (None, nothing_before.clone()),
        };
        [
            Self::metric(
                t("insights.wordsSpoken"),
                compact_number(current.words),
                None,
                words_change,
                words_note,
                p,
            ),
            Self::metric(
                t("insights.dictations"),
                compact_number(current.dictations),
                None,
                dictations_change,
                dictations_note,
                p,
            ),
            Self::metric(
                t("insights.wordsPerMinute"),
                current
                    .wpm()
                    .map(compact_number)
                    .unwrap_or_else(|| "—".into()),
                None,
                pace_change,
                pace_note,
                p,
            ),
            streak,
        ]
    }

    /// Moves the inspected bar with the arrow keys while the chart has focus.
    fn inspect_key(&mut self, event: &KeyDownEvent, count: usize, cx: &mut Context<Self>) {
        let Some(last) = count.checked_sub(1) else {
            return;
        };
        let current = self.cursor.unwrap_or(last).min(last);
        let next = match event.keystroke.key.as_str() {
            "left" => current.saturating_sub(1),
            "right" => (current + 1).min(last),
            "home" => 0,
            "end" => last,
            _ => return,
        };
        self.cursor = Some(next);
        // The keyboard takes over from a pointer resting on another bar.
        self.hovered = None;
        cx.stop_propagation();
        cx.notify();
    }

    fn chart(
        &self,
        summary: &AnalyticsSummary,
        today: NaiveDate,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut start = today - Days::new(self.range.chart_days() as u64 - 1);
        if self.range == Range::AllTime
            && let Some(first) = first_use(summary)
        {
            start = start.min(first);
        }
        let Trend { bucket_days, bars } = build_trend(&summary.daily, start, today);
        let count = bars.len().max(1);
        let last = bars.len().saturating_sub(1);
        // Bars before this index belong to the comparison period.
        let period_from = match self.range.days() {
            Some(days) => bars.len().saturating_sub(days as usize),
            None => 0,
        };
        let has_regions = period_from > 0;
        let plot_top = LABEL_STRIP;

        // The inspected bar: under the pointer, else the keyboard cursor
        // while the chart has focus, else today. It carries the value label.
        // Only a bar the user points at or moves to wears the full accent;
        // at rest today keeps its period's tone, so the chart shows just
        // the two series the legend names.
        let focused = keyed_focus_handle(CHART_KEY.into(), cx).is_focused(window);
        let pointed = self
            .hovered
            .or(if focused { self.cursor } else { None })
            .filter(|i| *i <= last);
        let selected = pointed.unwrap_or(last);
        let accented = pointed.is_some() || focused;

        let peak = bars
            .iter()
            .map(|b| (b.words as f64).max(b.average.unwrap_or(0.)))
            .fold(0., f64::max);
        let step = grid_step(peak);
        let top = ((peak / step).ceil().max(1.)) * step;
        let y = |value: f64| CHART_HEIGHT * (1. - (value / top) as f32);
        // Every layer spans the same box, between the value axis on the left
        // and the average's end label on the right.
        let layer = || div().absolute().left(px(AXIS_WIDTH)).right(px(END_GUTTER));

        let mut plot = div().relative().h(px(plot_top + CHART_HEIGHT + AXIS_STRIP));

        // The selected period sits on a tinted band so it reads as one unit
        // against the period it is compared with; the legend names both.
        if has_regions {
            plot = plot.child(
                layer()
                    .top(px(plot_top - BAND_LIFT))
                    .h(px(CHART_HEIGHT + BAND_LIFT))
                    .flex()
                    .child(div().w(relative(period_from as f32 / count as f32)))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .rounded_t(px(RADIUS_MD))
                            .bg(p.foreground.opacity(if p.dark { 0.05 } else { 0.035 })),
                    ),
            );
        }

        let mut tick = 0.;
        while tick <= top + step / 2. {
            let line_y = plot_top + y(tick);
            plot = plot
                .child(layer().top(px(line_y)).h(px(1.)).bg(if tick == 0. {
                    p.border_hover
                } else {
                    p.border.opacity(0.6)
                }))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .w(px(AXIS_WIDTH - 8.))
                        .top(px(line_y - 7.))
                        .flex()
                        .justify_end()
                        .text_size(px(TEXT_CAPTION))
                        .font_features(tabular_nums())
                        .text_color(p.subtle_foreground)
                        .child(compact_number(tick.round() as i64)),
                );
            tick += step;
        }

        let inset = if count > 40 { 1.5 } else { 3. };
        let mut columns = layer().top(px(plot_top)).h(px(CHART_HEIGHT)).flex();
        for (index, bar) in bars.iter().enumerate() {
            let in_period = index >= period_from;
            let is_selected = selected == index;
            let (fill, edge) = if is_selected && accented {
                (p.primary, None)
            } else if in_period {
                let (fill, edge) = period_tone(p);
                (fill, Some(edge))
            } else {
                let (fill, edge) = comparison_tone(p);
                (fill, Some(edge))
            };
            let height = bar_height(bar, &y);
            let name = format!(
                "{}: {}",
                bar_when(bar, bucket_days, today),
                bar_figures(bar)
            );
            columns = columns.child(
                div()
                    .id(("insights-bar", index))
                    .role(Role::ListBoxOption)
                    .aria_label(name)
                    .aria_selected(is_selected)
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(bars.len())
                    .when(is_selected, |d| d.aria_active_descendant())
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .px(px(inset))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hovered = Some(index);
                        } else if this.hovered == Some(index) {
                            this.hovered = None;
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.cursor = Some(index);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(height))
                            .rounded_t(px(RADIUS_SM.min(height / 2.)))
                            .bg(fill)
                            .when_some(edge, |d, edge| d.border_1().border_color(edge)),
                    ),
            );
        }

        // Short spans center each date under its bar; dense spans pin
        // "Today" to the right edge so it never runs past the plot.
        let centered = count <= 31;
        let mut labels = layer()
            .top(px(plot_top + CHART_HEIGHT + 7.))
            .flex()
            .text_size(px(TEXT_CAPTION))
            .font_features(tabular_nums())
            .text_color(p.subtle_foreground);
        for (index, label) in axis_labels(&bars, bucket_days).into_iter().enumerate() {
            let strong = selected == index;
            let slot = div().flex_1().relative().flex().justify_center();
            labels = labels.child(slot.children(label.map(|text| {
                let text = div()
                    .flex_none()
                    .whitespace_nowrap()
                    .when(strong, |d| d.text_color(p.foreground))
                    .child(text);
                if centered || index != last {
                    text
                } else {
                    div().absolute().right_0().top_0().child(text)
                }
            })));
        }

        let points: Vec<Option<(f32, f32)>> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| b.average.map(|a| ((i as f32 + 0.5) / count as f32, y(a))))
            .collect();
        let end = points.last().copied().flatten();
        let runs = average_runs(&points, period_from);
        // The current period's average is the line people follow, so it
        // takes the accent; the comparison period's is a muted dashed trace.
        let period_line = p.primary;
        let comparison_line = p.foreground.opacity(0.5);
        let ring = p.background;
        let line = canvas(
            move |_, _, _| {},
            move |bounds, _, window, _| {
                let at = |(fx, fy): (f32, f32)| {
                    point(
                        bounds.origin.x + bounds.size.width * fx,
                        bounds.origin.y + px(fy),
                    )
                };
                let dashes = [px(4.), px(3.)];
                // A canvas-coloured halo under the line keeps it legible
                // where it crosses bars of similar lightness.
                for (in_period, run) in &runs {
                    let (width, color) = if *in_period {
                        (px(2.), period_line)
                    } else {
                        (px(1.5), comparison_line)
                    };
                    for (width, color) in [(width + px(2.), ring), (width, color)] {
                        let mut path = PathBuilder::stroke(width);
                        if !*in_period {
                            path = path.dash_array(&dashes);
                        }
                        path.move_to(at(run[0]));
                        for p in &run[1..] {
                            path.line_to(at(*p));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, color);
                        }
                    }
                }
                if let Some(end) = end {
                    let center = at(end);
                    // A hairline leader carries the latest average to its
                    // label in the gutter.
                    let mut leader = PathBuilder::stroke(px(1.));
                    leader.move_to(center);
                    leader.line_to(point(bounds.origin.x + bounds.size.width, center.y));
                    if let Ok(leader) = leader.build() {
                        window.paint_path(leader, period_line.opacity(0.45));
                    }
                    for (radius, color) in [(px(4.), ring), (px(2.5), period_line)] {
                        let bounds = Bounds::new(
                            point(center.x - radius, center.y - radius),
                            size(radius * 2., radius * 2.),
                        );
                        window.paint_quad(fill(bounds, color).corner_radii(radius));
                    }
                }
            },
        )
        .absolute()
        .left(px(AXIS_WIDTH))
        .right(px(END_GUTTER))
        .top(px(plot_top))
        .h(px(CHART_HEIGHT));

        let (per_bar, average) = if bucket_days == 1 {
            ("insights.wordsPerDay", "insights.dailyAverage")
        } else {
            ("insights.wordsPerWeek", "insights.weeklyAverage")
        };

        // The average is labeled once, at its latest value, instead of in
        // the legend: the label sits where the eye already is.
        let end_label = end
            .zip(bars.last().and_then(|b| b.average))
            .map(|((_, fy), value)| {
                div()
                    .absolute()
                    .right_0()
                    .w(px(END_GUTTER))
                    .top(px(plot_top + fy - 16.))
                    .pl(px(8.))
                    .flex()
                    .flex_col()
                    .font_features(tabular_nums())
                    .child(
                        div()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(compact_number(value.round() as i64)),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_CAPTION))
                            .line_height(px(14.))
                            .text_color(p.subtle_foreground)
                            .whitespace_nowrap()
                            .child(t(average)),
                    )
            });

        // The inspected bar's figures sit in the strip above the plot,
        // centered over the bar and held inside the plot at either end, with
        // a hairline down to the bar. Pinned there, they never cover a bar.
        let value_label = bars.get(selected).map(|bar| {
            let card = div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(VALUE_LABEL_HEIGHT))
                .px(px(8.))
                .rounded(px(RADIUS_MD))
                .border_1()
                .border_color(p.border_hover)
                .bg(p.popover)
                .shadow(vec![drop(0., 1., 3., if p.dark { 0.4 } else { 0.08 })])
                .text_size(px(TEXT_SMALL))
                .font_features(tabular_nums())
                .whitespace_nowrap()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.foreground)
                        .child(bar_when(bar, bucket_days, today)),
                )
                .child(div().text_color(p.muted_foreground).child(bar_figures(bar)));
            // The card's box spans the widest stretch centered on the bar
            // that fits the plot. A wider card grows the box past it, and the
            // spacer on the open side shrinks, which clamps it to the edge.
            let center = (selected as f32 + 0.5) / count as f32;
            let reach = center.min(1. - center);
            let strip = layer()
                .top_0()
                .h(px(VALUE_LABEL_HEIGHT))
                .flex()
                .child(div().min_w_0().flex_basis(relative(center - reach)))
                .child(
                    div()
                        .flex_shrink(0.)
                        .flex_basis(relative(2. * reach))
                        .flex()
                        .justify_center()
                        .child(card),
                )
                .child(div().min_w_0().flex_basis(relative(1. - center - reach)));
            let bar_top = plot_top + CHART_HEIGHT - bar_height(bar, &y);
            let mut leader = layer()
                .top(px(VALUE_LABEL_HEIGHT))
                .h(px(bar_top - VALUE_LABEL_HEIGHT - 2.))
                .flex();
            for index in 0..count {
                leader = leader.child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .when(index == selected, |d| {
                            d.child(div().w(px(1.)).h_full().bg(p.border_hover))
                        }),
                );
            }
            (leader, strip)
        });
        let (leader, value_label) = value_label.unzip();

        let swatch = |fill: Hsla, edge: Option<Hsla>, label: String| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .size(px(10.))
                        .rounded(px(2.))
                        .bg(fill)
                        .when_some(edge, |d, edge| d.border_1().border_color(edge)),
                )
                .child(label)
        };
        let legend = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .text_size(px(TEXT_SMALL))
            .text_color(p.muted_foreground)
            .map(|d| match self.range.days().filter(|_| has_regions) {
                Some(days) => {
                    let days = days.to_string();
                    let (fill, edge) = comparison_tone(p);
                    let (period_fill, period_edge) = period_tone(p);
                    d.child(swatch(
                        period_fill,
                        Some(period_edge),
                        t_with("insights.lastDays", &[("count", &days)]),
                    ))
                    .child(swatch(
                        fill,
                        Some(edge),
                        t_with("insights.previousDays", &[("count", &days)]),
                    ))
                }
                None => {
                    let (fill, edge) = period_tone(p);
                    d.child(swatch(fill, Some(edge), t(per_bar)))
                }
            });

        let bar_count = bars.len();
        div()
            .id(CHART_KEY)
            .keyboard_focusable(CHART_KEY, Role::ListBox, t(per_bar), cx)
            .aria_description(t("insights.chartHint"))
            .on_key_down(
                cx.listener(move |this, event, _, cx| this.inspect_key(event, bar_count, cx)),
            )
            .m(px(-8.))
            .p(px(8.))
            .rounded(px(RADIUS_LG))
            // GPUI paints a shadow under the whole box, so the focus ring
            // needs an opaque fill to stay a ring.
            .bg(p.background)
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_x(px(16.))
                    .gap_y(px(6.))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(t(per_bar)),
                    )
                    .child(legend),
            )
            .child(
                plot.child(columns)
                    .children(leader)
                    .child(line)
                    .child(labels)
                    .children(end_label)
                    .children(value_label),
            )
            .into_any_element()
    }

    /// Day-level figures for the selected period.
    fn highlights(&self, summary: &AnalyticsSummary, today: NaiveDate, p: &Palette) -> Div {
        let highlights = day_highlights(&summary.daily, self.period_start(summary, today), today);
        let per_active_day = highlights.words_per_active_day();
        let DayHighlights {
            days,
            active_days,
            spoken_duration_ms,
            busiest,
            ..
        } = highlights;
        let dash = || "—".to_string();
        let rows: [(String, String, Option<String>); 4] = [
            (
                t("insights.activeDays"),
                t_with(
                    "insights.activeDaysValue",
                    &[
                        ("active", &active_days.to_string()),
                        ("count", &days.to_string()),
                    ],
                ),
                None,
            ),
            (
                t("insights.busiestDay"),
                busiest.map_or_else(dash, |(date, _)| day_label(date, today)),
                busiest.map(|(_, words)| {
                    t_with("insights.wordCount", &[("count", &words.to_string())])
                }),
            ),
            (
                t("insights.timeSpoken"),
                if spoken_duration_ms > 0 {
                    spoken_time(spoken_duration_ms)
                } else {
                    dash()
                },
                None,
            ),
            (
                t("insights.wordsPerActiveDay"),
                per_active_day.map_or_else(dash, compact_number),
                None,
            ),
        ];
        let mut card = panel(t("insights.highlights"), self.scope(), p).child(div().h(px(6.)));
        for (index, (label, value, detail)) in rows.into_iter().enumerate() {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .h(px(40.))
                    .when(index > 0, |d| d.border_t_1().border_color(p.border))
                    .text_size(px(TEXT_BODY))
                    .font_features(tabular_nums())
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(p.muted_foreground)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap(px(6.))
                            .text_color(p.foreground)
                            .child(value)
                            .children(
                                detail.map(|d| div().text_color(p.subtle_foreground).child(d)),
                            ),
                    ),
            );
        }
        card
    }

    /// Average words per weekday over the last eight weeks, or since the
    /// first dictation if that is more recent, with the busiest weekday
    /// called out. The span is fixed so each weekday averages several days.
    fn weekly_rhythm(&self, summary: &AnalyticsSummary, today: NaiveDate, p: &Palette) -> Div {
        let window_start = today - Days::new(RHYTHM_WEEKS * 7 - 1);
        let (start, scope) = match first_use(summary).filter(|first| *first > window_start) {
            Some(first) => (
                first,
                t_with("insights.since", &[("date", &short_date(first))]),
            ),
            None => (
                window_start,
                t_with(
                    "insights.lastWeeks",
                    &[("count", &RHYTHM_WEEKS.to_string())],
                ),
            ),
        };
        let averages = weekday_averages(&summary.daily, start, today);
        let max = averages.iter().copied().fold(0., f64::max);
        let busiest = averages
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .filter(|_| max > 0.);
        let description = match busiest {
            Some(day) => format!(
                "{scope} · {}",
                t_with("insights.busiestWeekday", &[("day", &weekday_name(day))])
            ),
            None => scope,
        };
        // The figures are averages, so their column says so once.
        let unit = div()
            .mt(px(6.))
            .h(px(16.))
            .flex()
            .justify_end()
            .text_size(px(TEXT_CAPTION))
            .text_color(p.subtle_foreground)
            .child(t("insights.averageWords"));
        let mut card = panel(t("insights.weeklyRhythm"), description, p).child(unit);
        for (index, average) in averages.into_iter().enumerate() {
            let strong = busiest == Some(index);
            let fraction = if max > 0. { (average / max) as f32 } else { 0. };
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(24.))
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .child(
                        div()
                            .w(px(32.))
                            .flex_none()
                            .text_color(if strong {
                                p.foreground
                            } else {
                                p.muted_foreground
                            })
                            .child(weekday_name(index)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.))
                            .rounded(px(RADIUS_SM))
                            .bg(p.foreground.opacity(if p.dark { 0.07 } else { 0.05 }))
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded(px(RADIUS_SM))
                                    .bg(if strong { p.primary } else { rhythm_bar(p) }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(44.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_color(p.foreground)
                            .when(strong, |d| d.font_weight(FontWeight::MEDIUM))
                            .child(compact_number(average.round() as i64)),
                    ),
            );
        }
        card.child(div().h(px(6.)))
    }

    /// A centered message with an icon tile, for the states without data.
    fn notice(icon_name: &str, title: String, body: Option<String>, p: &Palette) -> Div {
        empty_state(icon_name, title, body.unwrap_or_default(), p)
    }

    /// With History off nothing new is counted, so the message says how to
    /// start rather than promising data that will never arrive.
    fn empty_state(retention: bool, p: &Palette) -> Div {
        if retention {
            Self::notice(
                "bar-chart3",
                t("insights.emptyTitle"),
                Some(t("insights.emptyBody")),
                p,
            )
        } else {
            Self::notice(
                "bar-chart3",
                t("insights.historyOffTitle"),
                Some(t("insights.historyOffBody")),
                p,
            )
        }
    }

    fn load_error(p: &Palette, cx: &mut Context<Self>) -> Div {
        Self::notice("alert-circle", t("insights.loadError"), None, p).child(
            div().mt(px(16.)).child(
                Button::new("insights-retry")
                    .label(t("common.retry"))
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            ),
        )
    }

    fn dashboard(
        &self,
        summary: &AnalyticsSummary,
        window: &Window,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let width = window.viewport_size().width;
        let today = Local::now().date_naive();
        let metrics = self.metrics(summary, today, p);
        let grid = if width > px(1024.) {
            div().flex().gap(px(16.)).children(metrics)
        } else {
            let [a, b, c, d] = metrics;
            div()
                .flex()
                .flex_col()
                .gap(px(20.))
                .child(div().flex().gap(px(16.)).child(a).child(b))
                .child(div().flex().gap(px(16.)).child(c).child(d))
        };
        let highlights = self.highlights(summary, today, p);
        let rhythm = self.weekly_rhythm(summary, today, p);
        let panels = if width > px(900.) {
            div().flex().gap(px(16.)).child(highlights).child(rhythm)
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(highlights)
                .child(rhythm)
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(32.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .font_features(tabular_nums())
                            .text_color(p.foreground)
                            .child(self.period_caption(summary, today)),
                    )
                    .child(self.range_picker(p, cx)),
            )
            .child(div().mt(px(20.)).child(grid))
            .child(
                div()
                    .mt(px(36.))
                    .child(self.chart(summary, today, p, window, cx)),
            )
            .child(div().mt(px(32.)).child(panels))
    }
}

/// The first day with any words.
fn first_use(summary: &AnalyticsSummary) -> Option<NaiveDate> {
    summary
        .daily
        .iter()
        .find(|b| b.words > 0)
        .and_then(|b| NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").ok())
}

impl Render for InsightsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let retention = AppState::settings(cx).data_retention_enabled;

        let privacy = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .text_size(px(TEXT_SMALL))
            .text_color(p.subtle_foreground)
            .child(icon("shield", 12., p.subtle_foreground))
            .child(t("insights.onDevicePrivacy"));
        let page = page_container().pb(px(32.)).flex().flex_col();

        // `load` runs in `new`, so a summary exists unless loading failed.
        let notice = match self.summary.clone().filter(|_| !self.load_failed) {
            Some(summary) if summary.total_dictations > 0 => {
                return page
                    .when(!retention, |d| {
                        d.child(
                            retention_banner(t("insights.dataRetentionDisabled"), None, &p)
                                .mb(px(20.)),
                        )
                    })
                    .child(self.dashboard(&summary, window, &p, cx))
                    .child(privacy.mt(px(28.)));
            }
            // The empty state itself explains History being off.
            Some(_) => Self::empty_state(retention, &p),
            None => Self::load_error(&p, cx),
        };
        // Without data the message sits in the middle of the visible area
        // rather than atop an empty canvas.
        let visible = window.viewport_size().height - px(WORKSPACE_TOP_BAR_HEIGHT);
        page.min_h(visible).child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .pb(px(PAGE_TOP))
                .child(notice)
                .child(privacy.mt(px(32.))),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{TrendBar, absolute_change, axis_labels, grid_step, relative_change};

    #[test]
    fn changes_read_as_multiples_from_double_up() {
        assert_eq!(relative_change(110, 100).as_deref(), Some("▲ 10%"));
        assert_eq!(relative_change(82, 100).as_deref(), Some("▼ 18%"));
        assert_eq!(relative_change(100, 100).as_deref(), Some("±0%"));
        assert_eq!(relative_change(283, 10).as_deref(), Some("▲ 28.3×"));
        assert_eq!(relative_change(5, 0), None);
        assert_eq!(absolute_change(153, 151), "▲ 2");
        assert_eq!(absolute_change(140, 151), "▼ 11");
    }

    #[test]
    fn grid_steps_round_to_readable_values() {
        assert_eq!(grid_step(3_400.), 1_000.);
        assert_eq!(grid_step(1_520.), 500.);
        assert_eq!(grid_step(700.), 200.);
        assert_eq!(grid_step(96.), 25.);
        // A 2.5 step would label a 7.5 top as 7.
        assert_eq!(grid_step(7.), 2.);
        assert_eq!(grid_step(0.), 1.);
    }

    fn days(from: &str, count: u64) -> Vec<TrendBar> {
        let first = NaiveDate::parse_from_str(from, "%Y-%m-%d").unwrap();
        (0..count)
            .map(|i| {
                let date = first + chrono::Days::new(i);
                TrendBar {
                    start: date,
                    end: date,
                    words: 0,
                    dictations: 0,
                    average: None,
                }
            })
            .collect()
    }

    #[test]
    fn short_spans_label_every_other_day_back_from_today() {
        let labels = axis_labels(&days("2026-10-22", 14), 1);
        let shown: Vec<usize> = (0..14).filter(|i| labels[*i].is_some()).collect();
        assert_eq!(shown, [1, 3, 5, 7, 9, 11, 13]);
        assert_eq!(labels[13].as_deref(), Some("Today"));
    }

    #[test]
    fn long_spans_keep_a_month_start_near_today() {
        // Sep 6 through Nov 4: Nov 1 sits three bars before today.
        let labels = axis_labels(&days("2026-09-06", 60), 1);
        assert_eq!(labels[56].as_deref(), Some("Nov 1"));
        assert_eq!(labels[9].as_deref(), Some("Sep 15"));
        assert_eq!(labels[59].as_deref(), Some("Today"));
    }
}
