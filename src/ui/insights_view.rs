//! Insights: period totals compared with the period before, and a trend chart
//! of words per day (or per week for long spans) with a trailing average.

use chrono::{Datelike, Days, Local, NaiveDate};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::analytics::{AnalyticsSummary, Trend, build_trend, period_totals};
use crate::i18n::{self, Language, t, t_with};

use crate::app_state::{AppEvent, AppState};

use super::components::{Button, ButtonSize, ButtonVariant, drop, icon, spinner};
use super::theme::{
    Palette, PaletteExt, RADIUS_LG, RADIUS_MD, RADIUS_XL, TEXT_BODY, TEXT_CAPTION, TEXT_FIGURE,
    TEXT_SMALL, TEXT_TITLE, tabular_nums,
};

const CHART_HEIGHT: f32 = 200.;
const AXIS_WIDTH: f32 = 40.;

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

    /// Days the chart spans, so the period is seen against what came before.
    fn chart_days(self) -> i64 {
        match self {
            Self::Week | Self::AllTime => 30,
            Self::Month => 90,
        }
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

fn day_label(date: NaiveDate) -> String {
    match i18n::language() {
        Language::ZhCn => {
            let weekday = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                [date.weekday().num_days_from_monday() as usize];
            format!("{} {weekday}", short_date(date))
        }
        Language::En => date.format("%a, %b %-d").to_string(),
    }
}

/// A 1-2-2.5-5 step giving about three gridlines up to `max`.
fn grid_step(max: f64) -> f64 {
    let raw = (max / 3.).max(1.);
    let magnitude = 10f64.powf(raw.log10().floor());
    [1., 2., 2.5, 5., 10.]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|step| *step >= raw)
        .unwrap_or(10. * magnitude)
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

    fn range_picker(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut picker = div()
            .flex()
            .p(px(2.))
            .rounded(px(RADIUS_LG))
            .bg(p.foreground.opacity(if p.dark { 0.06 } else { 0.045 }));
        for (index, range) in [Range::Week, Range::Month, Range::AllTime]
            .into_iter()
            .enumerate()
        {
            let active = self.range == range;
            picker = picker.child(
                div()
                    .id(("insights-range", index))
                    .flex()
                    .items_center()
                    .h(px(24.))
                    .px(px(10.))
                    .rounded(px(RADIUS_MD))
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| {
                        d.text_color(p.foreground).map(|d| {
                            if p.dark {
                                d.bg(p.white.opacity(0.1))
                            } else {
                                d.bg(p.card).shadow(vec![drop(0., 1., 2., 0.08)])
                            }
                        })
                    })
                    .when(!active, |d| {
                        d.text_color(p.muted_foreground)
                            .hover(|s| s.text_color(p.foreground))
                    })
                    .child(range.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.range = range;
                        cx.notify();
                    })),
            );
        }
        picker
    }

    /// A metric with its comparison line: an emphasized `change` (if any)
    /// followed by a muted `note`.
    fn metric(
        label: String,
        value: String,
        change: Option<String>,
        note: String,
        p: &Palette,
    ) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .border_t_1()
            .border_color(p.border)
            .pt(px(16.))
            .child(
                div()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(TEXT_FIGURE))
                    .font_weight(FontWeight::MEDIUM)
                    .font_features(tabular_nums())
                    .line_height(px(40.))
                    .text_color(p.foreground)
                    .child(value),
            )
            .child(
                div()
                    .mt(px(4.))
                    .flex()
                    .gap(px(4.))
                    .text_size(px(TEXT_SMALL))
                    .font_features(tabular_nums())
                    .text_color(p.muted_foreground)
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
            t_with(
                "insights.days",
                &[("count", &summary.current_streak_days.to_string())],
            ),
            None,
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
                    t("insights.allTime"),
                    p,
                ),
                Self::metric(
                    t("insights.dictations"),
                    compact_number(summary.total_dictations),
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
            (Some(now), Some(before)) => (Some(absolute_change(now, before)), versus.clone()),
            (Some(_), None) => (None, nothing_before.clone()),
        };
        [
            Self::metric(
                t("insights.wordsSpoken"),
                compact_number(current.words),
                words_change,
                words_note,
                p,
            ),
            Self::metric(
                t("insights.dictations"),
                compact_number(current.dictations),
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
                pace_change,
                pace_note,
                p,
            ),
            streak,
        ]
    }

    fn chart(&self, summary: &AnalyticsSummary, today: NaiveDate, p: &Palette) -> AnyElement {
        let mut start = today - Days::new(self.range.chart_days() as u64 - 1);
        if self.range == Range::AllTime
            && let Some(first) = summary
                .daily
                .first()
                .and_then(|b| NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").ok())
        {
            start = start.min(first);
        }
        let Trend { bucket_days, bars } = build_trend(&summary.daily, start, today);
        let count = bars.len().max(1);
        // Bars inside the selected period stand out against the context.
        let period_start = self.range.days().map(|d| today - Days::new(d as u64 - 1));

        let peak = bars
            .iter()
            .map(|b| (b.words as f64).max(b.average.unwrap_or(0.)))
            .fold(0., f64::max);
        let step = grid_step(peak);
        let top = ((peak / step).ceil().max(1.)) * step;
        let y = |value: f64| CHART_HEIGHT * (1. - (value / top) as f32);

        let mut chart = div().relative().h(px(CHART_HEIGHT + 22.));
        let mut tick = 0.;
        while tick <= top + 0.5 {
            let line_y = y(tick);
            chart = chart
                .child(
                    div()
                        .absolute()
                        .left(px(AXIS_WIDTH))
                        .right_0()
                        .top(px(line_y))
                        .h(px(1.))
                        .bg(p.border.opacity(if tick == 0. { 1. } else { 0.5 })),
                )
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
                        .text_color(p.muted_foreground.opacity(0.8))
                        .child(compact_number(tick as i64)),
                );
            tick += step;
        }

        let inset = if count > 60 { 1. } else { 3. };
        let last = bars.len().saturating_sub(1);
        let mut columns = div()
            .absolute()
            .left(px(AXIS_WIDTH))
            .right_0()
            .top_0()
            .h(px(CHART_HEIGHT))
            .flex();
        for (index, bar) in bars.iter().enumerate() {
            let in_period = period_start.is_none_or(|s| bar.end >= s);
            let color = if index == last {
                p.foreground.opacity(0.55)
            } else if in_period {
                p.foreground.opacity(0.3)
            } else {
                p.foreground.opacity(0.13)
            };
            let height = if bar.words > 0 {
                (CHART_HEIGHT - y(bar.words as f64)).max(2.)
            } else {
                0.
            };
            let when = if bucket_days == 1 {
                day_label(bar.end)
            } else {
                format!("{} – {}", short_date(bar.start), short_date(bar.end))
            };
            let tip = format!(
                "{} · {}",
                t_with(
                    "insights.dayTooltip",
                    &[("date", &when), ("count", &bar.words.to_string())],
                ),
                t_with(
                    "insights.dictationCount",
                    &[("count", &bar.dictations.to_string())],
                ),
            );
            columns = columns.child(
                div()
                    .id(("trend-bar", index))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .px(px(inset))
                    .rounded(px(3.))
                    .hover(|s| s.bg(p.foreground.opacity(0.04)))
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(tip.clone().into())).into())
                    .child(div().w_full().h(px(height)).rounded_t(px(3.)).bg(color)),
            );
        }

        // Anchor dates under the bars; "Today" closes the axis on the right.
        let min_gap = (count as f32 * 0.07).ceil() as usize;
        let mut labels = div()
            .absolute()
            .left(px(AXIS_WIDTH))
            .right_0()
            .top(px(CHART_HEIGHT + 6.))
            .flex();
        // Month starts (and mid-month on short spans) anchor the axis.
        let anchors: Vec<bool> = bars
            .iter()
            .map(|bar| {
                if bucket_days == 1 {
                    bar.end.day() == 1 || (count <= 31 && bar.end.day() == 15)
                } else {
                    bar.start.month() != bar.end.month() || bar.start.day() == 1
                }
            })
            .collect();
        let first_anchor = anchors.iter().position(|a| *a).unwrap_or(count);
        let mut previous: Option<usize> = None;
        for (index, bar) in bars.iter().enumerate() {
            // The first bar is labeled only when no anchor is close to it.
            let label = (index + min_gap + 2 < count
                && (anchors[index] || (index == 0 && first_anchor >= min_gap))
                && previous.is_none_or(|p| index >= p + min_gap))
            .then(|| {
                if bucket_days == 1 {
                    short_date(bar.end)
                } else {
                    month_label(bar.end)
                }
            });
            if label.is_some() {
                previous = Some(index);
            }
            let slot = div().flex_1().relative();
            labels = labels.child(if index == last {
                slot.child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .whitespace_nowrap()
                        .child(t("insights.today")),
                )
            } else {
                slot.children(label.map(|l| {
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .whitespace_nowrap()
                        .child(l)
                }))
            });
        }
        let labels = labels
            .text_size(px(TEXT_CAPTION))
            .font_features(tabular_nums())
            .text_color(p.muted_foreground.opacity(0.8));

        let points: Vec<Option<(f32, f32)>> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| b.average.map(|a| ((i as f32 + 0.5) / count as f32, y(a))))
            .collect();
        let line_color = p.foreground;
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
                let mut path = PathBuilder::stroke(px(2.));
                let mut drawing = false;
                for point in &points {
                    match point {
                        Some(p) if drawing => path.line_to(at(*p)),
                        Some(p) => {
                            path.move_to(at(*p));
                            drawing = true;
                        }
                        None => drawing = false,
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, line_color);
                }
                if let Some(Some(end)) = points.last() {
                    let center = at(*end);
                    for (radius, color) in [(px(5.), ring), (px(3.5), line_color)] {
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
        .right_0()
        .top_0()
        .h(px(CHART_HEIGHT));

        let (per_bar, average) = if bucket_days == 1 {
            ("insights.wordsPerDay", "insights.dailyAverage")
        } else {
            ("insights.wordsPerWeek", "insights.weeklyAverage")
        };
        let legend = div()
            .flex()
            .items_center()
            .gap(px(16.))
            .text_size(px(TEXT_CAPTION))
            .text_color(p.muted_foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(10.))
                            .rounded(px(2.))
                            .bg(p.foreground.opacity(0.3)),
                    )
                    .child(t(per_bar)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().w(px(16.)).h(px(2.)).rounded(px(1.)).bg(p.foreground))
                    .child(t(average)),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(legend)
            .child(chart.child(columns).child(line).child(labels))
            .into_any_element()
    }
}

/// A plain text tooltip.
pub struct TextTooltip(pub SharedString);

impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(RADIUS_MD))
            .bg(p.popover)
            .border_1()
            .border_color(p.border)
            .shadow(vec![drop(0., 4., 12., if p.dark { 0.4 } else { 0.08 })])
            .text_size(px(TEXT_SMALL))
            .font_features(tabular_nums())
            .text_color(p.foreground)
            .child(self.0.clone())
    }
}

impl Render for InsightsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let retention = AppState::settings(cx).data_retention_enabled;
        let has_data = !self.load_failed
            && self
                .summary
                .as_ref()
                .is_some_and(|s| s.total_dictations > 0);
        let card = || {
            div()
                .rounded(px(RADIUS_XL))
                .border_1()
                .border_color(p.border.opacity(if p.dark { 0.8 } else { 0.6 }))
                .bg(p.card.opacity(0.4))
        };

        let body: AnyElement = if self.load_failed {
            card()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.))
                        .px(px(16.))
                        .py(px(64.))
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .text_color(p.muted_foreground)
                                .child(t("insights.loadError")),
                        )
                        .child(
                            Button::new("insights-retry")
                                .label(t("common.retry"))
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm)
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        ),
                )
                .into_any_element()
        } else if let Some(summary) = self.summary.clone() {
            let mut col = div().flex().flex_col();
            if !retention {
                col = col.child(
                    div()
                        .mb(px(20.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(px(RADIUS_LG))
                        .border_1()
                        .border_color(p.amber_border)
                        .bg(p.amber_bg)
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .text_color(p.amber_icon)
                                .child("⊘"),
                        )
                        .child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.amber_text)
                                .child(t("insights.dataRetentionDisabled")),
                        ),
                );
            }
            if summary.total_dictations == 0 {
                col = col.child(
                    card().child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .px(px(16.))
                            .py(px(64.))
                            .child(div().mb(px(16.)).child(icon(
                                "bar-chart3",
                                40.,
                                p.foreground.opacity(0.45),
                            )))
                            .child(
                                div()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t("insights.emptyTitle")),
                            )
                            .child(
                                div()
                                    .mt(px(6.))
                                    .max_w(px(384.))
                                    .text_center()
                                    .text_size(px(TEXT_SMALL))
                                    .text_color(p.muted_foreground)
                                    .child(t("insights.emptyBody")),
                            ),
                    ),
                );
            } else {
                let wide = window.viewport_size().width > px(1024.);
                let today = Local::now().date_naive();
                let metrics = self.metrics(&summary, today, &p);
                let grid = if wide {
                    div().flex().gap(px(12.)).children(metrics)
                } else {
                    let [a, b, c, d] = metrics;
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(div().flex().gap(px(12.)).child(a).child(b))
                        .child(div().flex().gap(px(12.)).child(c).child(d))
                };
                col = col
                    .child(grid)
                    .child(div().mt(px(32.)).child(self.chart(&summary, today, &p)));
            }
            col.into_any_element()
        } else {
            card()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .py(px(32.))
                        .child(spinner("insights-loading", 14., p.primary))
                        .child(
                            div()
                                .text_size(px(TEXT_BODY))
                                .text_color(p.muted_foreground)
                                .child(t("controlPanel.loading")),
                        ),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .mx_auto()
            .max_w(px(1024.))
            .px(px(24.))
            .py(px(24.))
            .child(
                div()
                    .min_h(px(32.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .text_size(px(TEXT_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.foreground)
                            .child(t("insights.yourUsage")),
                    )
                    .when(has_data, |d| d.child(self.range_picker(&p, cx))),
            )
            .child(div().mt(px(24.)).flex_1().child(body))
            .child(
                div()
                    .pt(px(32.))
                    .text_center()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground.opacity(0.8))
                    .child(t("insights.onDevicePrivacy")),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{absolute_change, grid_step, relative_change};

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
        assert_eq!(grid_step(3_400.), 2_000.);
        assert_eq!(grid_step(1_520.), 1_000.);
        assert_eq!(grid_step(700.), 250.);
        assert_eq!(grid_step(0.), 1.);
    }
}
