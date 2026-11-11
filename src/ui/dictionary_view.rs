//! Dictionary words and Snippets.

mod chrome;
mod snippets;
mod words;
use snippets::{SnippetRows, SnippetRowsKey};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::{DictionaryEntry, DictionarySource, MAX_SNIPPET_TRIGGER_LENGTH, Snippet};
use crate::i18n::{self, Language, t, t_with};
use crate::text::correction_learner::KnownCorrection;
use crate::text::dictionary_import::{format_dictionary_export, parse_dictionary_import_text};
use crate::text::prompt_cap::WHISPER_DECODER_PROMPT_CHARS;
use crate::text::snippets::get_dictionary_hint_words;

use crate::app_state::{AppEvent, AppState};

use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, MenuTriggerBounds, drop, empty_state,
    icon, kbd, keyed_focus_handle, page_container, segment, segmented_track, surface_card,
    text_tooltip,
};
use super::control_panel::{OpenSettings, SIDEBAR_WIDTH};
use super::format::day_label;
use super::menu::{MenuAppearance, MenuFocus, MenuPlacement, PopupMenu};
use super::overlays::{
    DialogRequest, Toast, ToastVariant, close_custom, modal_frame, show_custom, show_dialog,
    show_toast,
};
use super::text_input::{text_field, text_field_small, textarea_field};
use super::theme::{
    DUR_BASE, LINE_SMALL, PAGE_GUTTER, PAGE_MAX_WIDTH, Palette, PaletteExt, RADIUS_LG, RADIUS_MD,
    RADIUS_SM, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, delayed, ease_out, motion,
    tabular_nums,
};

/// Rows ease in one after another as a list opens: 50 ms, then 30 ms more
/// per row, capped at the ninth so long lists do not drag.
/// Keyed by the row's content so a row animates once, when it first appears.
/// With Reduce motion on, rows appear together and do not slide.
fn stagger_in(
    row: Stateful<Div>,
    key: impl Into<SharedString>,
    index: usize,
    cx: &App,
) -> AnimationElement<Stateful<Div>> {
    let reduced = cx.reduce_motion();
    let delay = if reduced {
        0
    } else {
        50 + index.min(8) as u64 * 30
    };
    row.relative().with_animation(
        ElementId::Name(key.into()),
        motion(delay + DUR_BASE, cx).with_easing(delayed(delay, DUR_BASE, ease_out)),
        move |d, t| {
            let d = d.opacity(t);
            if reduced { d } else { d.top(px(3. * (1. - t))) }
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Words,
    Snippets,
}

/// The inline editor shown above a list, at most one at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    None,
    AddWords,
    Import,
    NewSnippet,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceFilter {
    All,
    Manual,
    Learned,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortKey {
    Word,
    Uses,
    LastUsed,
}

/// Dictionary table column widths, in px. The word column takes the rest.
/// The use count is right-aligned in its own column so digits line up under
/// the heading; the bar gets a separate column so it reads as a scale, not as
/// part of the number.
const COL_USES: f32 = 64.;
const COL_USE_BAR: f32 = 96.;
const COL_LAST_USED: f32 = 112.;
/// Every use bar sits on a track this wide; the most used word fills it, so
/// bars compare against one scale.
const USE_TRACK: f32 = 64.;
/// A word used even once shows a sliver, so it never reads as unused.
const USE_BAR_MIN: f32 = 3.;
/// Table rows bleed their hover fill this far past the page edge and pad it
/// back, so cell text starts on the page edge, under the tabs, as on History.
const ROW_BLEED: f32 = 12.;
/// Snippet table trigger column width, in px.
const COL_TRIGGER: f32 = 200.;
/// Height of the header controls: search, buttons.
const HEADER_CONTROL: f32 = 28.;
/// Height of the Words / Snippets tab bar; its underline sits on the bottom edge.
const TAB_BAR: f32 = 40.;
/// Side padding of a tab. The tab row is pulled out by the same amount, so
/// the first label starts on the page edge, over the table.
const TAB_PAD: f32 = 8.;
/// Content width, in px, below which the search no longer fits in the tab
/// bar and drops to the row under it.
const HEADER_ONE_ROW: f32 = 440.;
/// Below this content width the table drops Last used, so the word column
/// keeps room for its chips and row actions.
const TABLE_FULL: f32 = 600.;
/// How long a removed row can be brought back before it is deleted for good.
const UNDO_WINDOW: Duration = Duration::from_secs(6);
/// Mishearings shown on a Dictionary row; past this, the last chip becomes
/// "+N" so rows keep one height.
const MAX_ROW_CORRECTIONS: usize = 3;

/// Sorts dictionary rows in place. Ties list the newest word first, so a word
/// just added tops the rows it ties with.
fn sort_entries(entries: &mut [DictionaryEntry], key: SortKey, descending: bool) {
    entries.reverse();
    if key == SortKey::Word {
        if descending {
            entries.sort_by_cached_key(|e| std::cmp::Reverse(e.word.to_lowercase()));
        } else {
            entries.sort_by_cached_key(|e| e.word.to_lowercase());
        }
        return;
    }
    entries.sort_by(|a, b| {
        let order = match key {
            SortKey::Word => unreachable!(),
            SortKey::Uses => a.use_count.cmp(&b.use_count),
            SortKey::LastUsed => a.last_used_at.cmp(&b.last_used_at),
        };
        if descending { order.reverse() } else { order }
    });
}

/// Words read A to Z; counts and dates read most first.
fn default_descending(key: SortKey) -> bool {
    key != SortKey::Word
}

/// The local day a word was last spoken, if ever. The table only needs to
/// tell words in daily use from ones that went quiet, so it shows the day
/// (labelled by `day_label`, as every view does) and keeps the time for the
/// tooltip.
fn last_used_day(use_count: i64, at: Option<DateTime<Utc>>) -> Option<NaiveDate> {
    // A word added but never spoken may still carry a timestamp; it was not used.
    at.filter(|_| use_count > 0)
        .map(|at| at.with_timezone(&Local).date_naive())
}

/// The exact time, for the Last used tooltip.
fn full_timestamp(at: DateTime<Utc>) -> String {
    let format = if i18n::language() == Language::ZhCn {
        "%Y年%-m月%-d日 %H:%M"
    } else {
        "%A, %B %-d, %Y at %-I:%M %p"
    };
    at.with_timezone(&Local).format(format).to_string()
}

/// How much of the use track a word fills, scaled to the most used word.
fn use_bar_width(uses: i64, max_uses: i64) -> f32 {
    if uses <= 0 || max_uses <= 0 {
        return 0.;
    }
    (USE_TRACK * uses as f32 / max_uses as f32).clamp(USE_BAR_MIN, USE_TRACK)
}

/// Caps a chip label at `max` characters with an ellipsis. Measured
/// truncation collapses a chip inside a flex row to its padding, so the cap
/// is applied to the string instead.
fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// A replacement on one line: line breaks become a return glyph.
fn one_line(text: &str) -> String {
    text.split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ↵ ")
}

/// A tab label whose width does not change with its weight: a hidden
/// semibold copy holds the width, so switching tabs never nudges its
/// neighbours.
fn steady_label(label: String, active: bool) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .child(label.clone()),
        )
        .child(
            div()
                .h_0()
                .overflow_hidden()
                .text_color(transparent_black())
                .font_weight(FontWeight::SEMIBOLD)
                .child(label),
        )
}

fn word_key(word: &str) -> ElementId {
    SharedString::from(format!("dict-word:{word}")).into()
}

fn snippet_key(trigger: &str) -> ElementId {
    SharedString::from(format!("dict-snippet:{trigger}")).into()
}

fn menu_item_key(index: usize) -> ElementId {
    ("dict-menu-item", index).into()
}

fn focus_key(key: ElementId, window: &mut Window, cx: &mut App) {
    keyed_focus_handle(key, cx).focus(window, cx);
}

/// The overflow menu's trigger; focus returns here when the menu closes.
const MORE_BUTTON: &str = "dict-more";

#[derive(Clone)]
enum MenuAction {
    Import,
    Export,
    Clear,
    OpenSettings,
    EditWord(String),
    RemoveWord(String),
    EditSnippet(Snippet),
    RemoveSnippet(String),
}

#[derive(Clone)]
struct MenuEntry {
    label: String,
    icon: &'static str,
    shortcut: Option<&'static str>,
    destructive: bool,
    action: MenuAction,
}

impl MenuEntry {
    fn new(label: String, icon: &'static str, action: MenuAction) -> Self {
        Self {
            label,
            icon,
            shortcut: None,
            destructive: false,
            action,
        }
    }

    fn shortcut(mut self, keys: &'static str) -> Self {
        self.shortcut = Some(keys);
        self
    }

    fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
}

#[derive(Clone)]
struct OpenMenu {
    entries: Vec<MenuEntry>,
    /// Where a context menu opened; `None` hangs the menu under the overflow button.
    position: Option<Point<Pixels>>,
    /// What takes focus back when the menu closes.
    return_focus: ElementId,
}

#[derive(Clone)]
enum Removal {
    Word(DictionaryEntry),
    Snippet(Snippet),
    /// A learned mishearing of `term`, shown as a "heard as" chip.
    Correction {
        misheard: String,
        term: String,
    },
}

/// A row or chip taken off screen but not yet deleted, so Undo loses nothing:
/// deleting a word also forgets its use count and mishearings, and a learned
/// mishearing takes many corrections to learn again.
struct PendingRemoval {
    id: u64,
    removal: Removal,
}

#[derive(PartialEq, Eq)]
struct WordRowsKey {
    query: String,
    pending: Option<String>,
    hidden_correction: Option<(String, String)>,
    filter: SourceFilter,
    sort: SortKey,
    descending: bool,
}

/// Content-derived work is shared across animation and pointer redraws.
struct WordRows {
    visible: Vec<DictionaryEntry>,
    row_keys: Vec<ElementId>,
    misheard: Vec<Vec<String>>,
    total: usize,
    max_uses: i64,
    prompt_chars: usize,
}

impl WordRows {
    fn new(
        entries: &[DictionaryEntry],
        words: &[String],
        snippets: &[Snippet],
        known_corrections: &[KnownCorrection],
        key: &WordRowsKey,
    ) -> Self {
        let entries: Vec<&DictionaryEntry> = entries
            .iter()
            .filter(|e| Some(&e.word) != key.pending.as_ref())
            .collect();
        let learned = entries
            .iter()
            .any(|e| e.source == DictionarySource::Learned);
        let filter = if learned {
            key.filter
        } else {
            SourceFilter::All
        };
        let mut visible: Vec<DictionaryEntry> = entries
            .iter()
            .copied()
            .filter(|e| match filter {
                SourceFilter::All => true,
                SourceFilter::Manual => e.source == DictionarySource::Manual,
                SourceFilter::Learned => e.source == DictionarySource::Learned,
            })
            .filter(|e| key.query.is_empty() || e.word.to_lowercase().contains(&key.query))
            .cloned()
            .collect();
        sort_entries(&mut visible, key.sort, key.descending);
        let mut corrections: HashMap<&str, Vec<&str>> = HashMap::new();
        for correction in known_corrections {
            if key
                .hidden_correction
                .as_ref()
                .is_some_and(|(m, t)| *m == correction.misheard && *t == correction.term)
            {
                continue;
            }
            corrections
                .entry(&correction.term)
                .or_default()
                .push(&correction.misheard);
        }
        let misheard = visible
            .iter()
            .map(|e| {
                corrections
                    .remove(e.word.as_str())
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        Self {
            total: entries.len(),
            max_uses: entries.iter().map(|e| e.use_count).max().unwrap_or(0),
            prompt_chars: get_dictionary_hint_words(words, snippets)
                .join(", ")
                .chars()
                .count(),
            row_keys: visible.iter().map(|e| word_key(&e.word)).collect(),
            visible,
            misheard,
        }
    }
}

pub struct DictionaryView {
    word_rows: Option<(WordRowsKey, Arc<WordRows>)>,
    snippet_rows: Option<(SnippetRowsKey, Arc<SnippetRows>)>,
    snippet_revision: u64,
    tab: Tab,
    panel: Panel,
    // Words
    word_search: Entity<InputState>,
    add_input: Entity<InputState>,
    /// Feedback under the add field after the last Enter.
    add_note: Option<(String, bool)>,
    bulk: Entity<TextareaState>,
    filter: SourceFilter,
    sort: SortKey,
    descending: bool,
    editing: Option<String>,
    edit_input: Entity<InputState>,
    /// Why the inline edit was not saved; the editor stays open under it.
    edit_error: Option<String>,
    // Snippets
    snippet_search: Entity<InputState>,
    trigger_input: Entity<InputState>,
    expansion: Entity<TextareaState>,
    // Shared
    menu: Option<OpenMenu>,
    pending: Option<PendingRemoval>,
    next_removal: u64,
    /// Focus keys of the rows on screen, top to bottom, for arrow-key moves.
    row_keys: Vec<ElementId>,
    /// The content width on the last frame, which picks the header and
    /// table layouts.
    width: Option<f32>,
    /// ⌘F to the Filter field while the page is on screen.
    _find: Subscription,
}

fn update_dictionary(add: Vec<String>, remove: Vec<String>, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        s.change_dictionary(&add, &remove, DictionarySource::Manual, cx);
    });
}

impl DictionaryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let notify_on_change = |cx: &mut Context<Self>, state: &Entity<InputState>| {
            cx.subscribe(state, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        };
        let word_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("dictionary.searchWordsPlaceholder"))
        });
        notify_on_change(cx, &word_search);
        let snippet_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("dictionary.snippets.searchPlaceholder"))
        });
        notify_on_change(cx, &snippet_search);

        let add_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(t("dictionary.addWordsPlaceholder")));
        cx.subscribe_in(
            &add_input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.add_from_input(window, cx),
                InputEvent::Change => {
                    this.add_note = None;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        let bulk = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(t("dictionary.importPlaceholder"))
        });
        cx.subscribe(&bulk, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(
            &edit_input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => {
                    if let Some(word) = this.commit_edit(true, cx) {
                        focus_key(word_key(&word), window, cx);
                    }
                }
                InputEvent::Blur => {
                    this.commit_edit(false, cx);
                }
                InputEvent::Change if this.edit_error.take().is_some() => cx.notify(),
                _ => {}
            },
        )
        .detach();

        let trigger_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("dictionary.snippets.triggerPlaceholder"))
        });
        cx.subscribe_in(
            &trigger_input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => {
                    this.expansion.update(cx, |i, cx| i.focus(window, cx));
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )
        .detach();
        let expansion = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(t("dictionary.snippets.replacementPlaceholder"))
        });
        cx.subscribe(&expansion, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();

        let state = AppState::global(cx);
        cx.subscribe(&state, |this, _, e: &AppEvent, cx| {
            if matches!(e, AppEvent::SnippetsChanged) {
                this.snippet_revision += 1;
                this.snippet_rows = None;
            }
            if matches!(e, AppEvent::DictionaryChanged | AppEvent::SnippetsChanged) {
                this.word_rows = None;
                cx.notify();
            }
        })
        .detach();
        Self {
            word_rows: None,
            snippet_rows: None,
            snippet_revision: 0,
            tab: Tab::Words,
            panel: Panel::None,
            word_search,
            add_input,
            add_note: None,
            bulk,
            filter: SourceFilter::All,
            sort: SortKey::Uses,
            descending: true,
            editing: None,
            edit_input,
            edit_error: None,
            snippet_search,
            trigger_input,
            expansion,
            menu: None,
            pending: None,
            next_removal: 0,
            row_keys: Vec::new(),
            width: None,
            _find: Self::intercept_find(cx),
        }
    }

    /// ⌘F focuses the Filter field, as in any Mac list. Key events only
    /// reach the page when focus is inside it, and the window usually holds
    /// focus at its root, so the shortcut is caught before dispatch. It acts
    /// only while the page is rendered and focus is at or above it (the
    /// window root), never while a dialog or palette holds focus.
    fn intercept_find(cx: &mut Context<Self>) -> Subscription {
        let this = cx.entity().downgrade();
        cx.intercept_keystrokes(move |e, window, cx| {
            if !is_find(&e.keystroke) {
                return;
            }
            let page = keyed_focus_handle("dict-tab-words".into(), cx);
            if !page.within_focused(window, cx) {
                return;
            }
            let _ = this.update(cx, |this, cx| this.focus_filter(window, cx));
            cx.stop_propagation();
        })
    }

    /// Focuses the current tab's Filter field, which shows once there is a
    /// row to filter.
    fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = AppState::global(cx).read(cx);
        let has_rows = match self.tab {
            Tab::Words => !state.dictionary.is_empty(),
            Tab::Snippets => !state.snippets.is_empty(),
        };
        if has_rows {
            self.menu = None;
            self.search().update(cx, |i, cx| i.focus(window, cx));
            cx.notify();
        }
    }

    fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.panel = Panel::None;
            self.editing = None;
            self.menu = None;
            cx.notify();
        }
    }

    fn search(&self) -> &Entity<InputState> {
        match self.tab {
            Tab::Words => &self.word_search,
            Tab::Snippets => &self.snippet_search,
        }
    }

    fn any_text_field_focused(&self, window: &Window, cx: &App) -> bool {
        [
            &self.word_search,
            &self.add_input,
            &self.edit_input,
            &self.snippet_search,
            &self.trigger_input,
        ]
        .iter()
        .any(|s| s.read(cx).focus_handle(cx).is_focused(window))
            || [&self.bulk, &self.expansion]
                .iter()
                .any(|s| s.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Moves keyboard focus `delta` rows from `from`; above the first row is
    /// the search field.
    fn focus_neighbor(
        &mut self,
        from: &ElementId,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.row_keys.iter().position(|k| k == from) else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 {
            self.search().update(cx, |i, cx| i.focus(window, cx));
        } else if let Some(key) = self.row_keys.get(target as usize) {
            focus_key(key.clone(), window, cx);
        }
    }

    /// The row that should take focus once `key`'s row is gone.
    fn row_after_removal(&self, key: &ElementId) -> Option<ElementId> {
        let index = self.row_keys.iter().position(|k| k == key)?;
        self.row_keys
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|i| self.row_keys.get(i)))
            .cloned()
    }

    // ---- Removal with undo -------------------------------------------------------------------

    fn remove(&mut self, removal: Removal, cx: &mut Context<Self>) {
        self.commit_pending(cx);
        self.next_removal += 1;
        let id = self.next_removal;
        self.pending = Some(PendingRemoval { id, removal });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_WINDOW).await;
            let _ = this.update(cx, |this, cx| {
                if this.pending.as_ref().is_some_and(|p| p.id == id) {
                    this.commit_pending(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Removes a row and, when the keyboard removed it, focuses its neighbor.
    fn remove_row(
        &mut self,
        key: &ElementId,
        removal: Removal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self.row_after_removal(key);
        self.remove(removal, cx);
        match next {
            Some(next) => focus_key(next, window, cx),
            None => self.search().update(cx, |i, cx| i.focus(window, cx)),
        }
    }

    fn commit_pending(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending.removal {
            Removal::Word(entry) => update_dictionary(Vec::new(), vec![entry.word], cx),
            Removal::Snippet(snippet) => Self::remove_snippet(snippet.trigger, cx),
            Removal::Correction { misheard, term } => {
                AppState::global(cx).update(cx, |s, cx| s.remove_correction(&misheard, &term, cx))
            }
        }
        cx.notify();
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let key = match &pending.removal {
            Removal::Word(entry) => word_key(&entry.word),
            Removal::Snippet(snippet) => snippet_key(&snippet.trigger),
            Removal::Correction { term, .. } => word_key(term),
        };
        focus_key(key, window, cx);
        cx.notify();
    }

    fn pending_word(&self) -> Option<&str> {
        match &self.pending.as_ref()?.removal {
            Removal::Word(entry) => Some(&entry.word),
            _ => None,
        }
    }

    fn pending_trigger(&self) -> Option<&str> {
        match &self.pending.as_ref()?.removal {
            Removal::Snippet(snippet) => Some(&snippet.trigger),
            _ => None,
        }
    }

    /// The (misheard, term) pair waiting on Undo, if any.
    fn pending_correction(&self) -> Option<(&str, &str)> {
        match &self.pending.as_ref()?.removal {
            Removal::Correction { misheard, term } => Some((misheard, term)),
            _ => None,
        }
    }

    /// Forgets a mishearing, with Undo. The chip's × goes with it, so focus
    /// moves to its word's row.
    fn forget_correction(
        &mut self,
        misheard: String,
        term: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        focus_key(word_key(&term), window, cx);
        self.remove(Removal::Correction { misheard, term }, cx);
    }

    // ---- Menus -------------------------------------------------------------------------------

    fn open_menu(&mut self, menu: OpenMenu, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = Some(menu);
        // Focus lands in the menu so arrows, Return and Escape work at once;
        // the ring only shows when the keyboard opened it.
        focus_key(menu_item_key(0), window, cx);
        cx.notify();
    }

    fn close_menu(&mut self, restore_focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.menu.take() {
            if restore_focus {
                MenuFocus::new(
                    (0..menu.entries.len()).map(menu_item_key),
                    menu.return_focus,
                    cx,
                )
                .restore(true, window, cx);
            }
            cx.notify();
        }
    }

    fn toggle_overflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close_menu(true, window, cx);
            return;
        }
        let entries = vec![
            MenuEntry::new(t("dictionary.importList"), "upload", MenuAction::Import),
            MenuEntry::new(t("dictionary.exportList"), "save", MenuAction::Export),
            // Auto-learn, which fills the Learned filter, lives in Settings ›
            // General; this is the way there from the page it affects.
            MenuEntry::new(
                t("dictionary.openSettings"),
                "settings",
                MenuAction::OpenSettings,
            ),
            MenuEntry::new(t("dictionary.clearDictionary"), "trash2", MenuAction::Clear)
                .destructive(),
        ];
        self.open_menu(
            OpenMenu {
                entries,
                position: None,
                return_focus: MORE_BUTTON.into(),
            },
            window,
            cx,
        );
    }

    fn row_menu_entries(&self, word: Option<&str>, snippet: Option<&Snippet>) -> Vec<MenuEntry> {
        let (edit, remove) = match (word, snippet) {
            (Some(word), _) => (
                MenuAction::EditWord(word.to_string()),
                MenuAction::RemoveWord(word.to_string()),
            ),
            (None, Some(snippet)) => (
                MenuAction::EditSnippet(snippet.clone()),
                MenuAction::RemoveSnippet(snippet.trigger.clone()),
            ),
            (None, None) => return Vec::new(),
        };
        vec![
            MenuEntry::new(t("dictionary.edit"), "pencil", edit).shortcut("↩"),
            MenuEntry::new(t("dictionary.remove"), "trash2", remove)
                .shortcut("⌫")
                .destructive(),
        ]
    }

    fn run(&mut self, action: MenuAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            MenuAction::Import => self.open_panel(Panel::Import, window, cx),
            MenuAction::Export => {
                self.commit_pending(cx);
                Self::export(cx);
            }
            MenuAction::Clear => self.confirm_clear(cx),
            MenuAction::OpenSettings => window.dispatch_action(Box::new(OpenSettings), cx),
            MenuAction::EditWord(word) => self.start_edit(word, window, cx),
            MenuAction::RemoveWord(word) => self.remove_word(&word, window, cx),
            MenuAction::EditSnippet(snippet) => Self::edit_snippet(snippet, window, cx),
            MenuAction::RemoveSnippet(trigger) => self.remove_snippet_row(&trigger, window, cx),
        }
    }

    // ---- Words -------------------------------------------------------------------------------

    fn open_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = panel;
        self.add_note = None;
        match panel {
            Panel::AddWords => self.add_input.update(cx, |i, cx| i.focus(window, cx)),
            Panel::Import => self.bulk.update(cx, |i, cx| i.focus(window, cx)),
            Panel::NewSnippet => self.trigger_input.update(cx, |i, cx| i.focus(window, cx)),
            Panel::None => {}
        }
        cx.notify();
    }

    /// Closes the inline editor and hands focus back to the button that opened it.
    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opener = match self.panel {
            Panel::AddWords => Some("dict-add-word"),
            Panel::Import => Some(MORE_BUTTON),
            Panel::NewSnippet => Some("dict-new-snippet"),
            Panel::None => None,
        };
        self.panel = Panel::None;
        self.add_note = None;
        self.add_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.bulk.update(cx, |i, cx| i.set_value("", window, cx));
        self.trigger_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.expansion
            .update(cx, |i, cx| i.set_value("", window, cx));
        if let Some(opener) = opener {
            focus_key(opener.into(), window, cx);
        }
        cx.notify();
    }

    /// Adds the words in `text` that are not in the Dictionary yet. Returns
    /// how many words `text` held and how many were new.
    fn add_words(&mut self, text: &str, cx: &mut Context<Self>) -> (usize, usize) {
        // A word waiting to be deleted is still stored; settle it first so
        // adding it back works.
        self.commit_pending(cx);
        let dictionary = AppState::global(cx).read(cx).dictionary.clone();
        let mut existing: std::collections::HashSet<String> =
            dictionary.iter().map(|w| w.to_lowercase()).collect();
        let parsed = parse_dictionary_import_text(text);
        let total = parsed.len();
        let words: Vec<String> = parsed
            .into_iter()
            .filter(|w| existing.insert(w.to_lowercase()))
            .collect();
        let added = words.len();
        if added > 0 {
            update_dictionary(words, Vec::new(), cx);
        }
        (total, added)
    }

    fn add_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.add_input.read(cx).value().to_string();
        let (total, added) = self.add_words(&text, cx);
        if total == 0 {
            return;
        }
        if added > 0 {
            self.add_input
                .update(cx, |i, cx| i.set_value("", window, cx));
            self.add_note = Some((
                t_with("dictionary.addedCount", &[("count", &added.to_string())]),
                true,
            ));
        } else {
            self.add_note = Some((t("dictionary.alreadyInDictionary"), false));
        }
        cx.notify();
    }

    fn import_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.bulk.read(cx).value().to_string();
        let (_, added) = self.add_words(&text, cx);
        self.close_panel(window, cx);
        show_toast(
            Toast::new(t_with(
                "dictionary.importedCount",
                &[("count", &added.to_string())],
            ))
            .variant(ToastVariant::Success),
            cx,
        );
    }

    fn start_edit(&mut self, word: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(word.clone());
        self.edit_error = None;
        self.edit_input.update(cx, |i, cx| {
            i.set_value(word, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_error = None;
        if let Some(word) = self.editing.take() {
            focus_key(word_key(&word), window, cx);
            cx.notify();
        }
    }

    /// Saves the word being edited and returns the word the row now shows.
    /// A name taken by another word, or an empty one confirmed with Return,
    /// keeps the editor open with the reason under it and returns `None`.
    /// Leaving an empty field just keeps the old word.
    fn commit_edit(&mut self, confirmed: bool, cx: &mut Context<Self>) -> Option<String> {
        let original = self.editing.clone()?;
        let value = self.edit_input.read(cx).value().trim().to_string();
        let duplicate = AppState::global(cx)
            .read(cx)
            .dictionary
            .iter()
            .any(|w| *w != original && w.to_lowercase() == value.to_lowercase());
        let error = if duplicate {
            Some(t_with("dictionary.editDuplicate", &[("word", &value)]))
        } else if value.is_empty() && confirmed {
            Some(t("dictionary.editEmpty"))
        } else {
            None
        };
        cx.notify();
        if error.is_some() {
            self.edit_error = error;
            return None;
        }
        self.editing = None;
        self.edit_error = None;
        if !value.is_empty() && value != original {
            update_dictionary(vec![value.clone()], vec![original], cx);
            Some(value)
        } else {
            Some(original)
        }
    }

    fn remove_word(&mut self, word: &str, window: &mut Window, cx: &mut Context<Self>) {
        let entry = AppState::global(cx)
            .read(cx)
            .dictionary_entries
            .iter()
            .find(|e| e.word == word)
            .cloned();
        if let Some(entry) = entry {
            self.remove_row(&word_key(word), Removal::Word(entry), window, cx);
        }
    }

    fn confirm_clear(&mut self, cx: &mut Context<Self>) {
        let count = AppState::global(cx).read(cx).dictionary.len();
        let view = cx.entity().downgrade();
        let request = DialogRequest::confirm(
            t("dictionary.clearTitle"),
            Some(
                t_with(
                    "dictionary.clearDescription",
                    &[("count", &count.to_string())],
                )
                .into(),
            ),
            true,
            move |_, cx| {
                // Clearing deletes everything, the row waiting on Undo included.
                let _ = view.update(cx, |this, cx| {
                    this.pending = None;
                    cx.notify();
                });
                AppState::global(cx).update(cx, |s, cx| s.clear_dictionary(cx))
            },
        );
        show_dialog(
            DialogRequest {
                confirm_label: t("dictionary.clearConfirm").into(),
                ..request
            },
            cx,
        );
    }

    fn export(cx: &mut App) {
        let words = AppState::global(cx).read(cx).dictionary.clone();
        let count = words.len();
        let dialog = rfd::AsyncFileDialog::new()
            .set_file_name(crate::text::dictionary_import::DICTIONARY_EXPORT_FILE_NAME)
            .add_filter("Text", &["txt"]);
        cx.spawn(async move |cx| {
            let Some(file) = dialog.save_file().await else {
                return;
            };
            let toast = match std::fs::write(file.path(), format_dictionary_export(&words)) {
                Ok(()) => Toast::new(t_with(
                    "dictionary.exportedCount",
                    &[("count", &count.to_string())],
                ))
                .variant(ToastVariant::Success),
                Err(err) => Toast::new(t("dictionary.exportFailed"))
                    .description(err.to_string())
                    .variant(ToastVariant::Destructive),
            };
            cx.update(|cx| show_toast(toast, cx));
        })
        .detach();
    }
}

impl Render for DictionaryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::components::prune_focus_handles(cx);
        let header = self.render_header(window, cx);
        let body = match self.tab {
            Tab::Words => self.render_words(window, cx),
            Tab::Snippets => self.render_snippets(window, cx),
        };
        let is_empty = {
            let state = AppState::global(cx).read(cx);
            match self.tab {
                Tab::Words => state.dictionary.is_empty(),
                Tab::Snippets => state.snippets.is_empty(),
            }
        };
        let body = if is_empty && self.panel == Panel::None {
            super::components::empty_state_placement(
                window.viewport_size().height
                    - px(super::theme::WORKSPACE_TOP_BAR_HEIGHT
                        + super::theme::PAGE_TOP
                        + 40.
                        + TAB_BAR
                        + 16.),
                body,
            )
        } else {
            body
        };
        let context_menu = self
            .menu
            .clone()
            .and_then(|m| m.position.map(|at| (m, at)))
            .map(|(m, _)| self.render_menu(m, None, cx));
        let undo = self.render_undo(window, cx);
        page_container()
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
                // Focus already inside the page never reaches the window-level
                // interceptor's root check, so ⌘F is handled here too.
                if is_find(k) {
                    this.focus_filter(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if k.key == "z"
                    && k.modifiers.platform
                    && !k.modifiers.shift
                    && this.pending.is_some()
                    && !this.any_text_field_focused(window, cx)
                {
                    this.undo(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .child(body)
            .children(context_menu)
            .children(undo)
    }
}

/// ⌘F with no other modifier.
fn is_find(k: &Keystroke) -> bool {
    let m = &k.modifiers;
    k.key == "f" && m.platform && !m.shift && !m.alt && !m.control && !m.function
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};

    use super::{
        DictionaryEntry, DictionarySource, SortKey, USE_BAR_MIN, USE_TRACK, clip_chars,
        last_used_day, one_line, sort_entries, use_bar_width,
    };

    fn entry(word: &str, use_count: i64, last_used_at: Option<i64>) -> DictionaryEntry {
        DictionaryEntry {
            word: word.to_string(),
            source: DictionarySource::Manual,
            use_count,
            last_used_at: last_used_at.and_then(DateTime::<Utc>::from_timestamp_millis),
        }
    }

    fn sorted(key: SortKey, descending: bool) -> Vec<String> {
        // Insertion order, oldest first.
        let mut entries = vec![
            entry("codex", 3, Some(2_000)),
            entry("Alpha", 0, None),
            entry("tailscale", 9, Some(1_000)),
            entry("beta", 0, None),
        ];
        sort_entries(&mut entries, key, descending);
        entries.into_iter().map(|e| e.word).collect()
    }

    #[test]
    fn sorts_by_uses_with_the_newest_word_first_on_ties() {
        assert_eq!(
            sorted(SortKey::Uses, true),
            ["tailscale", "codex", "beta", "Alpha"]
        );
    }

    #[test]
    fn sorts_words_case_insensitively() {
        assert_eq!(
            sorted(SortKey::Word, false),
            ["Alpha", "beta", "codex", "tailscale"]
        );
    }

    #[test]
    fn never_used_words_sort_after_used_ones_by_last_use() {
        assert_eq!(
            sorted(SortKey::LastUsed, true),
            ["codex", "tailscale", "beta", "Alpha"]
        );
    }

    #[test]
    fn last_used_is_the_local_day_of_the_last_use() {
        let at = Local.with_ymd_and_hms(2026, 11, 4, 23, 30, 0).unwrap();
        assert_eq!(
            last_used_day(3, Some(at.with_timezone(&Utc))),
            NaiveDate::from_ymd_opt(2026, 11, 4)
        );
    }

    #[test]
    fn a_word_never_spoken_was_never_used_whatever_its_timestamp() {
        let now = Local.with_ymd_and_hms(2026, 11, 4, 15, 0, 0).unwrap();
        assert_eq!(last_used_day(0, Some(now.with_timezone(&Utc))), None);
        assert_eq!(last_used_day(4, None), None);
    }

    #[test]
    fn use_bars_share_one_scale_and_never_vanish_for_a_used_word() {
        assert_eq!(use_bar_width(0, 14), 0.);
        assert_eq!(use_bar_width(14, 14), USE_TRACK);
        assert_eq!(use_bar_width(7, 14), USE_TRACK / 2.);
        assert_eq!(use_bar_width(1, 1_000), USE_BAR_MIN);
        assert_eq!(use_bar_width(3, 0), 0.);
    }

    #[test]
    fn long_chip_labels_end_in_an_ellipsis() {
        assert_eq!(clip_chars("post gress", 12), "post gress");
        assert_eq!(clip_chars("kubernetes cluster", 12), "kubernetes…");
        assert_eq!(clip_chars("库伯内蒂斯集群管理工具", 6), "库伯内蒂斯…");
    }

    #[test]
    fn replacements_show_on_one_line() {
        assert_eq!(one_line("Best,\nRui\n"), "Best, ↵ Rui");
    }

    #[test]
    fn presentation_hides_pending_words_and_mishearing_without_changing_prompt_budget() {
        let entries = vec![entry("Alpha", 3, None), entry("Beta", 8, None)];
        let words = vec!["Alpha".to_string(), "Beta".to_string()];
        let corrections = vec![
            crate::text::correction_learner::KnownCorrection {
                misheard: "alfa".into(),
                term: "Alpha".into(),
                count: 2,
            },
            crate::text::correction_learner::KnownCorrection {
                misheard: "alpha word".into(),
                term: "Alpha".into(),
                count: 3,
            },
        ];
        let key = super::WordRowsKey {
            query: "ALPHA".to_lowercase(),
            pending: Some("Beta".into()),
            hidden_correction: Some(("alfa".into(), "Alpha".into())),
            filter: super::SourceFilter::All,
            sort: SortKey::Word,
            descending: false,
        };
        let rows = super::WordRows::new(&entries, &words, &[], &corrections, &key);
        assert_eq!(rows.total, 1);
        assert_eq!(rows.max_uses, 3);
        assert_eq!(rows.visible[0].word, "Alpha");
        assert_eq!(rows.misheard, [vec!["alpha word".to_string()]]);
        assert_eq!(rows.prompt_chars, "Alpha, Beta".chars().count());
    }

    #[test]
    fn presentation_ignores_learned_filter_when_pending_removal_hides_last_learned_word() {
        let mut learned = entry("Learned", 8, None);
        learned.source = DictionarySource::Learned;
        let entries = vec![entry("Manual", 3, None), learned];
        let key = super::WordRowsKey {
            query: String::new(),
            pending: Some("Learned".into()),
            hidden_correction: None,
            filter: super::SourceFilter::Learned,
            sort: SortKey::Uses,
            descending: true,
        };
        let rows = super::WordRows::new(&entries, &[], &[], &[], &key);
        assert_eq!(rows.visible.len(), 1);
        assert_eq!(rows.visible[0].word, "Manual");
    }

    #[test]
    fn cached_word_sort_keeps_the_newest_spelling_first_for_case_insensitive_ties() {
        for descending in [false, true] {
            let mut entries = vec![entry("Alpha", 0, None), entry("ALPHA", 0, None)];
            sort_entries(&mut entries, SortKey::Word, descending);
            assert_eq!(entries[0].word, "ALPHA");
        }
    }
}
