//! The Cmd+K transcript palette. Keyboard-first, so it appears and
//! disappears without animation.

use chrono::{DateTime, Local, Utc};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::TranscriptionItem;
use crate::i18n::{self, Language, t, t_with};
use crate::text::format::{RelativeTime, relative_time};

use crate::app_state::AppState;

use gpui_kit::base::FocusTrapElement;

use super::components::{
    KeyboardFocusable, backdrop, dialog_shadow, icon, kbd, keyed_focus_handle, text_tooltip,
};
use super::overlays::close_custom;
use super::theme::{
    CONTROL_HEIGHT, FONT_VOICE, PaletteExt, RADIUS_2XL, RADIUS_LG, TEXT_BODY, TEXT_SMALL,
    WORKSPACE_TOP_BAR_HEIGHT, tabular_nums,
};

pub enum CommandSearchEvent {
    Selected(i64),
}

pub struct CommandSearch {
    input: Entity<InputState>,
    selected: usize,
    scroll: ScrollHandle,
}

/// Rows listed before anything is typed.
const RECENT_LIMIT: usize = 5;
/// Rows listed for a query; the list scrolls past about eight.
const RESULT_LIMIT: usize = 30;

/// Characters of a result shown before its first match.
const MATCH_LEAD: usize = 30;

/// The part of `text` a result row shows: all of it when the first match is
/// near the start, otherwise "…" and the text from just before the match,
/// so a one-line row always shows why it matched.
fn match_window(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let at = match (query.is_empty(), lower.len() == text.len()) {
        (false, true) => lower.find(query),
        _ => None,
    };
    let Some(at) = at else {
        return text.to_string();
    };
    let chars_before = text[..at].chars().count();
    if chars_before <= MATCH_LEAD + 10 {
        return text.to_string();
    }
    let skip = chars_before - MATCH_LEAD;
    let start = text.char_indices().nth(skip).map_or(at, |(i, _)| i);
    // Start on a word boundary so the window never opens mid-word.
    let start = text[start..at]
        .find(' ')
        .map_or(start, |space| start + space + 1);
    format!("…{}", &text[start..])
}

/// `text` with every case-insensitive occurrence of `query` emphasized. Text
/// whose lowercase form changes byte length (some non-ASCII) is left plain,
/// since match offsets would not map back onto it.
fn highlighted(text: &str, query: &str, color: Hsla) -> StyledText {
    let lower = text.to_lowercase();
    let mut highlights = Vec::new();
    if !query.is_empty() && lower.len() == text.len() {
        let mut from = 0;
        while let Some(at) = lower[from..].find(query) {
            let start = from + at;
            let end = start + query.len();
            highlights.push((
                start..end,
                HighlightStyle {
                    color: Some(color),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            ));
            from = end;
        }
    }
    StyledText::new(SharedString::from(text.to_string())).with_highlights(highlights)
}

impl EventEmitter<CommandSearchEvent> for CommandSearch {}

fn relative_label(item: &TranscriptionItem) -> String {
    // When it was spoken, as on Home; `created_at` is when the row was written.
    item.occurred_at()
        .map(relative_time_label)
        .unwrap_or_default()
}

/// A short age such as "5m", "3d" or "Mar 4".
pub(super) fn relative_time_label(ts: DateTime<Utc>) -> String {
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
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("commandSearch.placeholder")));
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
        Self {
            input,
            selected: 0,
            scroll: ScrollHandle::new(),
        }
    }

    fn results(&self, cx: &App) -> Vec<TranscriptionItem> {
        let query = self.input.read(cx).value().trim().to_lowercase();
        AppState::global(cx)
            .read(cx)
            .history
            .iter()
            .filter(|item| query.is_empty() || item.text.to_lowercase().contains(&query))
            .take(if query.is_empty() {
                RECENT_LIMIT
            } else {
                RESULT_LIMIT
            })
            .cloned()
            .collect()
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.results(cx).get(index) {
            cx.emit(CommandSearchEvent::Selected(item.id));
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
        match e.keystroke.key.as_str() {
            "down" if count > 0 => {
                self.selected = (self.selected + 1).min(count - 1);
                // Child 0 is the group header.
                self.scroll.scroll_to_item(self.selected + 1);
                cx.notify();
                cx.stop_propagation();
            }
            "up" => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll.scroll_to_item(self.selected + 1);
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
                row = row.child(kbd(*k, &p));
            }
            row.child(
                div()
                    .ml(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground)
                    .child(label),
            )
        };

        let needle = query.trim().to_lowercase();
        let mut list = div()
            .id("search-results")
            .max_h(px(340.))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(8.));
        if results.is_empty() {
            list = list.child(
                div()
                    .py(px(40.))
                    .flex()
                    .justify_center()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground)
                    .child(if !query.trim().is_empty() {
                        t("commandSearch.noResults")
                    } else if AppState::global(cx).read(cx).history.is_empty() {
                        t("controlPanel.history.empty")
                    } else {
                        t("commandSearch.emptyState")
                    }),
            );
        } else {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .pt(px(4.))
                    .pb(px(6.))
                    .child(
                        div()
                            .text_size(px(TEXT_SMALL))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.muted_foreground)
                            .child(if needle.is_empty() {
                                t("commandSearch.sections.recent")
                            } else {
                                t_with(
                                    "commandSearch.resultCount",
                                    &[("count", &results.len().to_string())],
                                )
                            }),
                    ),
            );
            for (idx, item) in results.iter().enumerate() {
                let selected = idx == self.selected;
                list = list.child(
                    div()
                        .id(("search-row", idx))
                        .role(Role::ListBoxOption)
                        .aria_selected(selected)
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .w_full()
                        .min_h(px(32.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(RADIUS_LG))
                        .cursor_pointer()
                        .when(selected, |d| d.bg(p.foreground.opacity(0.09)))
                        .when(!selected, |d| {
                            d.hover(move |s| s.bg(p.foreground.opacity(0.06)))
                        })
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.selected != idx {
                                this.selected = idx;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| this.choose(idx, cx)))
                        .child(icon(
                            "mic",
                            16.,
                            if selected {
                                p.foreground
                            } else {
                                p.subtle_foreground
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(FONT_VOICE)
                                .text_size(px(TEXT_BODY))
                                .text_color(if selected {
                                    p.foreground
                                } else {
                                    p.foreground.opacity(0.85)
                                })
                                .child(highlighted(
                                    &match_window(&item.text, &needle),
                                    &needle,
                                    p.foreground,
                                )),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(TEXT_SMALL))
                                .font_features(tabular_nums())
                                .text_color(p.muted_foreground)
                                .child(relative_label(item)),
                        ),
                );
            }
        }

        let trap = keyed_focus_handle("command-search-trap".into(), cx);
        let card = div()
            .id("command-search")
            .role(Role::Dialog)
            .aria_label(t("commandSearch.placeholder"))
            .occlude()
            .w(px(576.))
            .rounded(px(RADIUS_2XL))
            .border_1()
            .border_color(p.border)
            .bg(p.popover)
            .shadow(dialog_shadow(&p))
            .overflow_hidden()
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(Self::on_key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(48.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(icon("search", 16., p.muted_foreground))
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
                                .keyboard_focusable(
                                    "search-clear",
                                    Role::Button,
                                    t("commandSearch.clear"),
                                    cx,
                                )
                                .tooltip(text_tooltip(t("commandSearch.clear")))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(28.))
                                .rounded(px(RADIUS_LG))
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.foreground.opacity(0.06)))
                                .child(icon("x", 14., p.muted_foreground))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.input.update(cx, |i, cx| i.set_value("", window, cx));
                                })),
                        )
                    }),
            )
            .child(list.role(Role::ListBox))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(p.foreground.opacity(0.07))
                    .bg(p.foreground.opacity(0.025))
                    .child(hint(&["↑", "↓"], t("commandSearch.footer.navigate")))
                    .child(hint(&["↵"], t("commandSearch.footer.open")))
                    .child(hint(&["Esc"], t("commandSearch.footer.dismiss"))),
            )
            .focus_trap("command-search-focus-trap", &trap);

        let content_left = if AppState::settings(cx).sidebar_collapsed {
            0.
        } else {
            super::control_panel::SIDEBAR_WIDTH
        };
        div()
            .id("command-search-backdrop")
            .absolute()
            .inset_0()
            .bg(backdrop(&p))
            .occlude()
            .on_click(|_, _, cx| close_custom(cx))
            // Opens over the top-bar field it replaces: centered on the content
            // column, its top on the field's top.
            .child(
                div()
                    .absolute()
                    .top(px((WORKSPACE_TOP_BAR_HEIGHT - CONTROL_HEIGHT) / 2.))
                    .left(px(content_left))
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(card),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::match_window;

    #[test]
    fn keeps_short_leads_and_windows_late_matches() {
        assert_eq!(
            match_window("deploy the build", "build"),
            "deploy the build"
        );
        let long =
            "We should cache the dependency downloads in CI instead of the whole target directory";
        let shown = match_window(long, "target");
        assert!(shown.starts_with('…'));
        assert!(shown.contains("target directory"));
        assert!(!shown[3..].starts_with(' '));
        assert_eq!(match_window(long, ""), long);
    }
}
