//! The Cmd+K transcript palette. Keyboard-first, so it appears and
//! disappears without animation.

use std::collections::HashSet;
use std::sync::Arc;

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
    results: Option<SearchResults>,
}

struct SearchResults {
    query: String,
    /// Arc identity is the data revision: History publishes immutable snapshots
    /// and uses copy-on-write for immediate deletions.
    history: Arc<Vec<TranscriptionItem>>,
    /// Undo changes visibility without replacing the history snapshot.
    pending_deletes: Arc<HashSet<i64>>,
    matches: Arc<Vec<TranscriptionItem>>,
}

impl SearchResults {
    fn new(
        query: String,
        history: Arc<Vec<TranscriptionItem>>,
        pending_deletes: Arc<HashSet<i64>>,
    ) -> Self {
        let matches = Arc::new(
            history
                .iter()
                .filter(|item| !pending_deletes.contains(&item.id))
                .filter(|item| query.is_empty() || item.text.to_lowercase().contains(&query))
                .take(if query.is_empty() {
                    RECENT_LIMIT
                } else {
                    RESULT_LIMIT
                })
                .cloned()
                .collect(),
        );
        Self {
            query,
            history,
            pending_deletes,
            matches,
        }
    }

    fn is_current(
        &self,
        query: &str,
        history: &Arc<Vec<TranscriptionItem>>,
        pending_deletes: &Arc<HashSet<i64>>,
    ) -> bool {
        self.query == query
            && Arc::ptr_eq(&self.history, history)
            && Arc::ptr_eq(&self.pending_deletes, pending_deletes)
    }
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
fn highlighted(text: &str, query: &str, color: Hsla, mark: Hsla) -> StyledText {
    let lower = text.to_lowercase();
    let mut highlights = Vec::new();
    if !query.is_empty() && lower.len() == text.len() {
        let mut from = 0;
        while let Some(at) = lower[from..].find(query) {
            let start = from + at;
            let end = start + query.len();
            highlights.push((
                start..end,
                // A tinted mark, not only weight, so matches stand out on dark.
                HighlightStyle {
                    color: Some(color),
                    background_color: Some(mark),
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
        cx.observe(&AppState::global(cx), |_, _, cx| cx.notify())
            .detach();
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
            results: None,
        }
    }

    fn results(&mut self, cx: &App) -> Arc<Vec<TranscriptionItem>> {
        let query = self.input.read(cx).value().trim().to_lowercase();
        let state = AppState::global(cx).read(cx);
        let history = state.history.clone();
        let pending_deletes = state.pending_history_deletes.clone();
        if self
            .results
            .as_ref()
            .is_none_or(|cached| !cached.is_current(&query, &history, &pending_deletes))
        {
            self.results = Some(SearchResults::new(query, history, pending_deletes));
        }
        self.results.as_ref().unwrap().matches.clone()
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
        super::components::prune_focus_handles(cx);
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
                    .child(if query.trim().is_empty() {
                        t("controlPanel.history.empty")
                    } else {
                        t("commandSearch.noResults")
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
                        .when(selected, |d| d.bg(p.list_highlight))
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
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(FONT_VOICE)
                                .text_size(px(TEXT_BODY))
                                .text_color(if selected {
                                    p.selection_foreground
                                } else {
                                    p.foreground.opacity(0.85)
                                })
                                .child(highlighted(
                                    &match_window(&item.text, &needle),
                                    &needle,
                                    if selected {
                                        p.selection_foreground
                                    } else {
                                        p.foreground
                                    },
                                    p.primary.opacity(if p.dark { 0.32 } else { 0.16 }),
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
            // Narrow windows shrink the palette instead of clipping it.
            .w_full()
            .max_w(px(576.))
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
                                    // `set_value` emits no change event.
                                    this.input.update(cx, |i, cx| {
                                        i.set_value("", window, cx);
                                        i.focus(window, cx);
                                    });
                                    this.selected = 0;
                                    this.scroll.scroll_to_item(0);
                                    cx.notify();
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
                    .child(hint(&["↩"], t("commandSearch.footer.open")))
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
    use super::{RECENT_LIMIT, RESULT_LIMIT, SearchResults, TranscriptionItem, match_window};
    use std::collections::HashSet;
    use std::sync::Arc;

    fn transcript(id: i64) -> TranscriptionItem {
        TranscriptionItem {
            id,
            text: "Deployment finished".into(),
            raw_text: None,
            timestamp: String::new(),
            created_at: String::new(),
            has_audio: false,
            audio_duration_ms: None,
        }
    }

    #[test]
    fn cached_results_invalidate_after_a_query_change_or_history_edit() {
        let mut history = Arc::new(vec![transcript(1)]);
        let pending = Arc::default();
        let cache = SearchResults::new("deployment".into(), history.clone(), Arc::clone(&pending));
        assert!(cache.is_current("deployment", &history, &pending));
        assert_eq!(cache.matches.len(), 1);
        assert!(!cache.is_current("finished", &history, &pending));
        Arc::make_mut(&mut history).clear();
        assert!(!cache.is_current("deployment", &history, &pending));
        assert!(
            SearchResults::new("deployment".into(), history, pending)
                .matches
                .is_empty()
        );
    }

    #[test]
    fn pending_delete_and_undo_invalidate_cached_recent_and_query_results() {
        let history = Arc::new(vec![transcript(1), transcript(2)]);
        for query in ["", "deployment"] {
            let mut pending = Arc::new(HashSet::new());
            let before = SearchResults::new(query.into(), history.clone(), pending.clone());
            Arc::make_mut(&mut pending).insert(1);
            assert!(!before.is_current(query, &history, &pending));
            let deleted = SearchResults::new(query.into(), history.clone(), pending.clone());
            assert_eq!(
                deleted.matches.iter().map(|r| r.id).collect::<Vec<_>>(),
                [2]
            );
            assert!(deleted.is_current(query, &history, &pending));
            Arc::make_mut(&mut pending).remove(&1);
            assert!(!deleted.is_current(query, &history, &pending));
            let undone = SearchResults::new(query.into(), history.clone(), pending);
            assert_eq!(
                undone.matches.iter().map(|r| r.id).collect::<Vec<_>>(),
                [1, 2]
            );
            assert!(Arc::ptr_eq(&before.history, &undone.history));
        }
    }

    #[test]
    fn pending_deletes_do_not_use_up_recent_or_query_result_limits() {
        let history = Arc::new(
            (1..=(RESULT_LIMIT + 2) as i64)
                .map(transcript)
                .collect::<Vec<_>>(),
        );
        let pending = Arc::new(HashSet::from([1, 2]));
        for (query, limit) in [("", RECENT_LIMIT), ("deployment", RESULT_LIMIT)] {
            let results = SearchResults::new(query.into(), history.clone(), pending.clone());
            assert_eq!(results.matches.len(), limit);
            assert_eq!(results.matches.first().unwrap().id, 3);
            assert_eq!(results.matches.last().unwrap().id, (limit + 2) as i64);
        }
    }

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
