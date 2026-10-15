//! Insights: local dictation statistics and the six-month activity heatmap.

use chrono::{Datelike, Local, NaiveDate};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::analytics::{AnalyticsSummary, build_activity_days};
use crate::i18n::{self, Language, t, t_with};

use crate::app_state::{AppEvent, AppState};

use super::components::{icon, spinner};
use super::theme::{
    Palette, PaletteExt, TEXT_BODY, TEXT_CAPTION, TEXT_FIGURE, TEXT_SMALL, TEXT_TITLE,
};

pub struct InsightsView {
    summary: Option<AnalyticsSummary>,
    load_failed: bool,
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

fn month_label(date: NaiveDate) -> String {
    match i18n::language() {
        Language::ZhCn => format!("{}月", date.month()),
        Language::En => date.format("%b").to_string(),
    }
}

fn weekday_labels() -> [&'static str; 7] {
    match i18n::language() {
        Language::ZhCn => ["周一", "周二", "周三", "周四", "周五", "周六", "周日"],
        Language::En => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    }
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

    fn metric(
        icon_name: &'static str,
        label: String,
        value: String,
        detail: String,
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
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(p.muted_foreground)
                    .child(icon(icon_name, 14., p.muted_foreground))
                    .child(div().text_size(px(TEXT_SMALL)).child(label)),
            )
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(TEXT_FIGURE))
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(px(40.))
                    .text_color(p.foreground)
                    .child(value),
            )
            .child(
                div()
                    .mt(px(4.))
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(detail),
            )
    }

    fn heatmap(summary: &AnalyticsSummary, p: &Palette) -> AnyElement {
        let today = Local::now().date_naive();
        let days = build_activity_days(&summary.daily, today);
        if days.is_empty() {
            return div().into_any_element();
        }
        let first = NaiveDate::parse_from_str(&days[0].date, "%Y-%m-%d").unwrap_or(today);
        let lead = first.weekday().num_days_from_monday() as usize;
        let mut cells: Vec<Option<(String, i64)>> = vec![None; lead];
        cells.extend(days.iter().map(|d| Some((d.date.clone(), d.words))));
        while !cells.len().is_multiple_of(7) {
            cells.push(None);
        }
        let weeks: Vec<Vec<Option<(String, i64)>>> = cells.chunks(7).map(|c| c.to_vec()).collect();
        let max_words = days.iter().map(|d| d.words).max().unwrap_or(1).max(1);
        let today_key = days.last().map(|d| d.date.clone()).unwrap_or_default();

        let intensity_color = |level: usize| -> Hsla {
            match level {
                0 => {
                    if p.dark {
                        p.white.opacity(0.06)
                    } else {
                        p.foreground.opacity(0.06)
                    }
                }
                1 => p.primary.opacity(0.25),
                2 => p.primary.opacity(0.45),
                3 => p.primary.opacity(0.7),
                _ => p.primary,
            }
        };

        // Month labels sit above the week that holds the 1st (or the first week).
        let mut previous_month = String::new();
        let mut month_row = div().flex().gap(px(8.)).h(px(16.));
        for (index, week) in weeks.iter().enumerate() {
            let label_day = week
                .iter()
                .flatten()
                .find(|(d, _)| d.ends_with("-01"))
                .or_else(|| {
                    if index == 0 {
                        week.iter().flatten().next()
                    } else {
                        None
                    }
                });
            let label = label_day.and_then(|(d, _)| {
                let month = d[..7].to_string();
                if month == previous_month {
                    return None;
                }
                previous_month = month;
                NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .ok()
                    .map(month_label)
            });
            month_row = month_row.child(
                div()
                    .w(px(24.))
                    .flex_none()
                    .whitespace_nowrap()
                    .text_size(px(TEXT_CAPTION))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.muted_foreground.opacity(0.7))
                    .children(label),
            );
        }

        let mut weekday_col = div().flex().flex_col().gap(px(2.));
        for label in weekday_labels() {
            weekday_col = weekday_col.child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(label),
            );
        }

        let mut grid = div().flex().gap(px(8.));
        for (wi, week) in weeks.iter().enumerate() {
            let mut col = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .w(px(24.))
                .items_center();
            for (di, cell) in week.iter().enumerate() {
                col = col.child(match cell {
                    None => div().h(px(24.)).into_any_element(),
                    Some((date, words)) => {
                        let level = if *words == 0 {
                            0
                        } else {
                            ((*words as f64 / max_words as f64) * 4.).ceil().max(1.) as usize
                        };
                        let tip = t_with(
                            "insights.dayTooltip",
                            &[("date", date), ("count", &words.to_string())],
                        );
                        let is_today = *date == today_key;
                        let cell = div()
                            .id(("heat", wi * 7 + di))
                            .size(px(16.))
                            .rounded(px(3.))
                            .bg(intensity_color(level))
                            .when(is_today, |d| {
                                d.border_1().border_color(p.primary).shadow(vec![BoxShadow {
                                    color: p.background,
                                    offset: point(px(0.), px(0.)),
                                    blur_radius: px(0.),
                                    spread_radius: px(1.),
                                    inset: true,
                                }])
                            })
                            .tooltip(move |_, cx| {
                                cx.new(|_| TextTooltip(tip.clone().into())).into()
                            });
                        div()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .child(cell)
                            .into_any_element()
                    }
                });
            }
            grid = grid.child(col);
        }

        let mut legend = div().flex().items_center().gap(px(4.));
        for level in 1..=4 {
            legend = legend.child(
                div()
                    .size(px(12.))
                    .rounded(px(2.))
                    .bg(intensity_color(level)),
            );
        }

        div()
            .id("heatmap")
            .overflow_x_scroll()
            .pb(px(4.))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().h(px(16.)))
                            .child(weekday_col),
                    )
                    .child(div().flex().flex_col().child(month_row).child(grid)),
            )
            .child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground)
                    .child(t("insights.activityLess"))
                    .child(legend)
                    .child(t("insights.activityMore")),
            )
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
            .rounded(px(6.))
            .bg(p.toast_surface)
            .border_1()
            .border_color(p.toast_border)
            .text_size(px(TEXT_CAPTION))
            .text_color(hsla(0., 0., 0.96, 1.))
            .child(self.0.clone())
    }
}

impl Render for InsightsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let retention = AppState::settings(cx).data_retention_enabled;
        let card = || {
            div()
                .rounded(px(8.))
                .border_1()
                .border_color(p.border)
                .bg(p.card.opacity(if p.dark { 0.6 } else { 0.5 }))
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
                            div()
                                .id("insights-retry")
                                .px(px(12.))
                                .py(px(6.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.border)
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .cursor_pointer()
                                .hover(|s| s.bg(p.muted))
                                .child(t("common.retry"))
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
                        .rounded(px(8.))
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
                let metrics = [
                    Self::metric(
                        "mic2",
                        t("insights.wordsSpoken"),
                        compact_number(summary.total_words),
                        t("insights.allTime"),
                        &p,
                    ),
                    Self::metric(
                        "gauge",
                        t("insights.wordsPerMinute"),
                        summary
                            .average_wpm
                            .map(compact_number)
                            .unwrap_or_else(|| "—".into()),
                        t_with(
                            "insights.wpmCoverage",
                            &[("count", &summary.wpm_coverage_percent.to_string())],
                        ),
                        &p,
                    ),
                    Self::metric(
                        "bar-chart3",
                        t("insights.dictations"),
                        compact_number(summary.total_dictations),
                        t("insights.allTime"),
                        &p,
                    ),
                    Self::metric(
                        "flame",
                        t("insights.currentStreak"),
                        t_with(
                            "insights.days",
                            &[("count", &summary.current_streak_days.to_string())],
                        ),
                        t_with(
                            "insights.longestStreak",
                            &[("count", &summary.longest_streak_days.to_string())],
                        ),
                        &p,
                    ),
                ];
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
                col = col.child(grid).child(
                    div()
                        .mt(px(32.))
                        .border_t_1()
                        .border_color(p.border)
                        .pt(px(16.))
                        .child(
                            div()
                                .text_size(px(TEXT_TITLE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.foreground)
                                .child(t("insights.activity")),
                        )
                        .child(div().mt(px(8.)).child(Self::heatmap(&summary, &p))),
                );
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
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.foreground)
                            .child(t("insights.yourUsage")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(TEXT_CAPTION))
                            .text_color(p.muted_foreground)
                            .child(icon("cloud", 13., p.muted_foreground))
                            .child(t("insights.onDevice")),
                    ),
            )
            .child(div().mt(px(24.)).flex_1().child(body))
            .child(
                div()
                    .pt(px(32.))
                    .text_center()
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(t("insights.onDevicePrivacy")),
            )
    }
}
