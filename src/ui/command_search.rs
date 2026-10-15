//! The Cmd+K transcript palette. Keyboard-first, so it appears and
//! disappears without animation.

use chrono::Local;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::TranscriptionItem;
use crate::i18n::{self, Language, t, t_with};
use crate::text::format::{RelativeTime, normalize_db_date, relative_time};

use crate::app_state::AppState;

use super::components::icon;
use super::overlays::close_custom;
use super::theme::{FONT_VOICE, PaletteExt, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL};

pub enum CommandSearchEvent {
    Selected(i64),
}

pub struct CommandSearch {
    input: Entity<InputState>,
    selected: usize,
}

impl EventEmitter<CommandSearchEvent> for CommandSearch {}

fn relative_label(item: &TranscriptionItem) -> String {
    let source = if item.created_at.is_empty() {
        &item.timestamp
    } else {
        &item.created_at
    };
    let Some(ts) = normalize_db_date(source) else {
        return String::new();
    };
    match relative_time(ts, &Local::now()) {
        RelativeTime::Now => t("commandSearch.relativeTime.now"),
        RelativeTime::MinutesAgo(n) => t_with(
            "commandSearch.relativeTime.minutesAgo",
            &[("count", &n.to_string())],
        ),
        RelativeTime::HoursAgo(n) => t_with(
            "commandSearch.relativeTime.hoursAgo",
            &[("count", &n.to_string())],
        ),
        RelativeTime::DaysAgo(n) => t_with(
            "commandSearch.relativeTime.daysAgo",
            &[("count", &n.to_string())],
        ),
        RelativeTime::Date(date) => match i18n::language() {
            Language::ZhCn => date.format("%-m月%-d日").to_string(),
            Language::En => date.format("%b %-d").to_string(),
        },
    }
}

impl CommandSearch {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("commandSearch.sections.transcripts"))
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.subscribe(&input, |this, _, e: &InputEvent, cx| match e {
            InputEvent::Change => {
                this.selected = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.choose(this.selected, cx),
            _ => {}
        })
        .detach();
        Self { input, selected: 0 }
    }

    fn results(&self, cx: &App) -> Vec<TranscriptionItem> {
        let query = self.input.read(cx).value().trim().to_lowercase();
        AppState::global(cx)
            .read(cx)
            .history
            .iter()
            .filter(|item| query.is_empty() || item.text.to_lowercase().contains(&query))
            .take(5)
            .cloned()
            .collect()
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.results(cx).get(index) {
            cx.emit(CommandSearchEvent::Selected(item.id));
            close_custom(cx);
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        match e.keystroke.key.as_str() {
            "down" if count > 0 => {
                self.selected = (self.selected + 1).min(count - 1);
                cx.notify();
                cx.stop_propagation();
            }
            "up" => {
                self.selected = self.selected.saturating_sub(1);
                cx.notify();
                cx.stop_propagation();
            }
            "escape" => {
                close_custom(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for CommandSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let query = self.input.read(cx).value().to_string();
        let results = self.results(cx);
        let hint = |keys: &[&'static str], label: String| {
            let mut row = div().flex().items_center().gap(px(4.));
            for k in keys {
                row = row.child(
                    div()
                        .px(px(4.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.border.opacity(0.7))
                        .bg(p.muted.opacity(0.5))
                        .text_size(px(TEXT_CAPTION))
                        .text_color(p.muted_foreground.opacity(0.55))
                        .child(*k),
                );
            }
            row.child(
                div()
                    .ml(px(2.))
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(label),
            )
        };

        let mut list = div()
            .id("search-results")
            .max_h(px(340.))
            .overflow_y_scroll()
            .p(px(6.));
        if results.is_empty() {
            list = list.child(
                div()
                    .py(px(40.))
                    .flex()
                    .justify_center()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(if query.trim().is_empty() {
                        t("commandSearch.emptyState")
                    } else {
                        t("commandSearch.noResults")
                    }),
            );
        } else {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .pt(px(8.))
                    .pb(px(4.))
                    .child(icon("mic", 11., p.muted_foreground.opacity(0.7)))
                    .child(
                        div()
                            .text_size(px(TEXT_CAPTION))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.muted_foreground.opacity(0.7))
                            .child(t("commandSearch.sections.transcripts").to_uppercase()),
                    ),
            );
            for (idx, item) in results.iter().enumerate() {
                let selected = idx == self.selected;
                list = list.child(
                    div()
                        .id(("search-row", idx))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .w_full()
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .when(selected, |d| {
                            d.bg(p.primary.opacity(if p.dark { 0.1 } else { 0.08 }))
                        })
                        .when(!selected, |d| d.hover(move |s| s.bg(p.hover_wash(0.04))))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.selected != idx {
                                this.selected = idx;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(idx, cx)))
                        .child(icon(
                            "mic",
                            13.,
                            if selected {
                                p.primary
                            } else {
                                p.muted_foreground.opacity(0.7)
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(FONT_VOICE)
                                .text_size(px(TEXT_BODY))
                                .text_color(p.foreground.opacity(0.75))
                                .child(item.text.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(TEXT_CAPTION))
                                .text_color(p.muted_foreground.opacity(0.7))
                                .child(relative_label(item)),
                        ),
                );
            }
        }

        let card = div()
            .id("command-search")
            .occlude()
            .w(px(576.))
            .rounded(px(12.))
            .border_1()
            .border_color(if p.dark {
                p.border
            } else {
                p.border.opacity(0.7)
            })
            .bg(if p.dark { p.surface_2 } else { p.card })
            .shadow_2xl()
            .overflow_hidden()
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(12.))
                    .border_b_1()
                    .border_color(p.border.opacity(0.7))
                    .child(icon("search", 14., p.muted_foreground.opacity(0.7)))
                    .child(
                        div().flex_1().text_size(px(TEXT_BODY)).child(
                            Input::new(&self.input)
                                .appearance(false)
                                .with_size(Size::Size(px(14. / 0.875))),
                        ),
                    )
                    .when(!query.is_empty(), |d| {
                        d.child(
                            div()
                                .id("search-clear")
                                .text_size(px(TEXT_CAPTION))
                                .text_color(p.muted_foreground.opacity(0.7))
                                .cursor_pointer()
                                .hover(move |s| s.text_color(p.muted_foreground))
                                .child("✕")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.input.update(cx, |i, cx| i.set_value("", window, cx));
                                })),
                        )
                    }),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(14.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(p.border.opacity(0.7))
                    .bg(p.muted.opacity(0.15))
                    .child(hint(&["↑", "↓"], t("commandSearch.footer.navigate")))
                    .child(hint(&["↵"], t("commandSearch.footer.open")))
                    .child(hint(&["Esc"], t("commandSearch.footer.dismiss"))),
            );

        div()
            .id("command-search-backdrop")
            .absolute()
            .inset_0()
            .bg(hsla(0., 0., 0., 0.5))
            .occlude()
            .on_click(|_, _, cx| close_custom(cx))
            .child(
                div()
                    .absolute()
                    .top(relative(0.18))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(card),
            )
    }
}
