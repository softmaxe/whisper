//! Dictionary words and Snippets.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::db::{DictionaryEntry, DictionarySource, Snippet};
use crate::i18n::{self, Language, t, t_with};
use crate::text::dictionary_import::{format_dictionary_export, parse_dictionary_import_text};
use crate::text::prompt_cap::WHISPER_DECODER_PROMPT_CHARS;
use crate::text::snippets::get_dictionary_hint_words;

use crate::app_state::{AppEvent, AppState};

use super::components::{
    Button, ButtonSize, ButtonVariant, KeyboardFocusable, drop, empty_state, icon, kbd,
    keyed_focus_handle, page_container, surface_card, text_tooltip,
};
use super::control_panel::{OpenSettings, SIDEBAR_WIDTH};
use super::format::day_label;
use super::overlays::{
    DialogRequest, Toast, ToastVariant, close_custom, modal_frame, show_custom, show_dialog,
    show_toast,
};
use super::text_input::{text_field, text_field_small, textarea_field};
use super::theme::{
    DUR_BASE, DUR_SNAP, LINE_SMALL, PAGE_GUTTER, PAGE_MAX_WIDTH, Palette, PaletteExt, RADIUS_LG,
    RADIUS_MD, RADIUS_SM, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, delayed, ease_out,
    ease_snap, motion, tabular_nums,
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
/// Side padding of a tab and of a filter chip. Each row is pulled out by the
/// same amount, so the first label starts on the page edge, over the table.
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
    entries.sort_by(|a, b| {
        let order = match key {
            SortKey::Word => a.word.to_lowercase().cmp(&b.word.to_lowercase()),
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

pub struct DictionaryView {
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
    /// When a menu last closed, so the click that closed it does not reopen it.
    menu_closed_at: Option<Instant>,
    pending: Option<PendingRemoval>,
    next_removal: u64,
    /// Focus keys of the rows on screen, top to bottom, for arrow-key moves.
    row_keys: Vec<ElementId>,
    /// The content width on the last frame, which picks the header and
    /// table layouts.
    width: Option<f32>,
}

fn update_dictionary(add: Vec<String>, remove: Vec<String>, cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Err(err) = s.change_dictionary(&add, &remove, DictionarySource::Manual, cx) {
            log::warn!("failed to update the dictionary: {err}");
        }
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
        cx.subscribe(&state, |_, _, e: &AppEvent, cx| {
            if matches!(e, AppEvent::DictionaryChanged | AppEvent::SnippetsChanged) {
                cx.notify();
            }
        })
        .detach();
        Self {
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
            menu_closed_at: None,
            pending: None,
            next_removal: 0,
            row_keys: Vec::new(),
            width: None,
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
            self.menu_closed_at = Some(Instant::now());
            if restore_focus {
                focus_key(menu.return_focus, window, cx);
            }
            cx.notify();
        }
    }

    fn toggle_overflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close_menu(true, window, cx);
            return;
        }
        // The mouse-down that dismissed the menu also lands here as a click.
        if self
            .menu_closed_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(DUR_BASE))
        {
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

    // ---- Snippets ----------------------------------------------------------------------------

    fn trigger_exists(snippets: &[Snippet], value: &str, except: Option<&str>) -> bool {
        let lower = value.to_lowercase();
        let except = except.map(str::to_lowercase);
        snippets.iter().any(|s| {
            let existing = s.trigger.to_lowercase();
            existing == lower && Some(&existing) != except.as_ref()
        })
    }

    fn create_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let replacement = self.expansion.read(cx).value().trim().to_string();
        self.commit_pending(cx);
        let state = AppState::global(cx);
        let mut snippets = state.read(cx).snippets.clone();
        if trigger.is_empty()
            || replacement.is_empty()
            || Self::trigger_exists(&snippets, &trigger, None)
        {
            return;
        }
        snippets.push(Snippet {
            trigger: trigger.clone(),
            replacement,
        });
        state.update(cx, |s, cx| s.set_snippets(snippets, cx));
        self.close_panel(window, cx);
        focus_key(snippet_key(&trigger), window, cx);
    }

    fn remove_snippet(trigger: String, cx: &mut App) {
        let state = AppState::global(cx);
        let snippets: Vec<Snippet> = state
            .read(cx)
            .snippets
            .iter()
            .filter(|s| s.trigger != trigger)
            .cloned()
            .collect();
        state.update(cx, |s, cx| s.set_snippets(snippets, cx));
    }

    fn remove_snippet_row(&mut self, trigger: &str, window: &mut Window, cx: &mut Context<Self>) {
        let snippet = AppState::global(cx)
            .read(cx)
            .snippets
            .iter()
            .find(|s| s.trigger == trigger)
            .cloned();
        if let Some(snippet) = snippet {
            self.remove_row(&snippet_key(trigger), Removal::Snippet(snippet), window, cx);
        }
    }

    fn edit_snippet(snippet: Snippet, window: &mut Window, cx: &mut App) {
        let editor = cx.new(|cx| SnippetEditor::new(snippet, window, cx));
        show_custom(editor.into(), cx);
    }

    // ---- Rendering: shared -------------------------------------------------------------------

    fn render_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let (has_rows, add): (bool, Button) = match self.tab {
            Tab::Words => (
                !AppState::global(cx).read(cx).dictionary.is_empty(),
                Button::new("dict-add-word")
                    .icon("plus", 12.)
                    .label(t("dictionary.addWord"))
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_panel(Panel::AddWords, window, cx)
                    })),
            ),
            Tab::Snippets => (
                !AppState::global(cx).read(cx).snippets.is_empty(),
                Button::new("dict-new-snippet")
                    .icon("plus", 12.)
                    .label(t("dictionary.snippets.new"))
                    .variant(ButtonVariant::Outline)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_panel(Panel::NewSnippet, window, cx)
                    })),
            ),
        };
        // Only auto-learn makes words of two sources, so until it adds one
        // the filter has nothing to tell apart. Snippets have no source.
        let learned = self.learned_count(cx);
        let filter =
            (self.tab == Tab::Words && learned > 0).then(|| self.render_filter(learned, cx));
        let compact = self.content_width(window) < HEADER_ONE_ROW;
        let search = has_rows.then(|| self.render_search(compact, window, cx));
        let (bar_search, row_search) = if compact {
            (None, search)
        } else {
            (search, None)
        };
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(add);
        // The view switch owns the page: an underlined tab bar that also holds
        // the add action; the page's ⋮ menu lives in the window's top bar. The source filter only narrows the Words view,
        // so it sits under the bar, smaller and without a track.
        let bar = div()
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(TAB_BAR))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(p.border),
            )
            .child(self.render_tabs(&p, cx))
            .child(
                // The search field gives up width first as the window narrows.
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .children(bar_search)
                    .child(actions),
            );
        let below = (filter.is_some() || row_search.is_some()).then(|| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .children(filter)
                .children(row_search)
        });
        let header = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(bar)
            .children(below);
        let this = cx.entity().downgrade();
        header.relative().child(
            canvas(
                move |bounds, _, cx| {
                    let width = f32::from(bounds.size.width);
                    let _ = this.update(cx, |this, cx| {
                        if this.width != Some(width) {
                            this.width = Some(width);
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }

    /// The page's ⋮ menu (Import, Export, Dictionary Settings…, Clear
    /// Dictionary…), drawn in the window's top bar so every page keeps its
    /// page-level menu in the same place. Its items act on words, so it shows
    /// only on the Words view once there is a word to act on.
    // Until control_panel.rs's top bar calls this, nothing in the crate does.
    #[allow(dead_code)]
    pub fn render_page_menu(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_words = !AppState::global(cx).read(cx).dictionary.is_empty();
        if self.tab != Tab::Words || !has_words {
            return Empty.into_any_element();
        }
        let menu = self
            .menu
            .clone()
            .filter(|m| m.position.is_none())
            .map(|m| self.render_menu(m, cx));
        div()
            .relative()
            .flex()
            // The top bar drags the window on mouse-down; the menu must not.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new(MORE_BUTTON)
                    .icon("more-vertical", 15.)
                    .aria_label(t("dictionary.pageActions"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_overflow(window, cx))),
            )
            .when_some(menu, |d, menu| {
                // Hangs from the button's right edge; deferred so it paints
                // over the page below the top bar.
                d.child(
                    deferred(
                        div()
                            .absolute()
                            .top(px(HEADER_CONTROL + 4.))
                            .right_0()
                            .child(menu),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    /// The page's content width, measured from the header on the last frame;
    /// before the first, estimated from the window with the sidebar open.
    fn content_width(&self, window: &Window) -> f32 {
        self.width.unwrap_or_else(|| {
            let window = f32::from(window.viewport_size().width);
            (window - SIDEBAR_WIDTH).min(PAGE_MAX_WIDTH) - 2. * PAGE_GUTTER
        })
    }

    /// Words / Snippets: the page-level view switch, in body text with a
    /// semibold active label and an underline on the bar's bottom edge.
    fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut row = div()
            .flex_none()
            .flex()
            .h_full()
            .gap(px(8.))
            .ml(px(-TAB_PAD));
        for (tab, id, key) in [
            (Tab::Words, "dict-tab-words", "dictionary.tabWords"),
            (Tab::Snippets, "dict-tab-snippets", "dictionary.tabSnippets"),
        ] {
            let active = self.tab == tab;
            let label = t(key);
            let hover_fg = p.foreground;
            row = row.child(
                div()
                    .id(id)
                    .keyboard_focusable(id, Role::Tab, label.clone(), cx)
                    .aria_selected(active)
                    .relative()
                    .h_full()
                    .flex()
                    .items_center()
                    .px(px(TAB_PAD))
                    .rounded(px(RADIUS_MD))
                    .text_size(px(TEXT_BODY))
                    .cursor_pointer()
                    .text_color(if active {
                        p.foreground
                    } else {
                        p.muted_foreground
                    })
                    .when(!active, |d| d.hover(move |s| s.text_color(hover_fg)))
                    .child(steady_label(label, active))
                    .when(active, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left(px(TAB_PAD))
                                .right(px(TAB_PAD))
                                .h(px(2.))
                                .rounded(px(1.))
                                .bg(p.foreground),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx))),
            );
        }
        row
    }

    /// The header search field: Escape clears it, ↓ moves into the list.
    fn render_search(&self, full_width: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let state = self.search().clone();
        div()
            .relative()
            .flex_1()
            .min_w(px(120.))
            .when(!full_width, |d| d.max_w(px(200.)))
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                match e.keystroke.key.as_str() {
                    "escape" if !state.read(cx).value().is_empty() => {
                        state.update(cx, |i, cx| i.set_value("", window, cx));
                        cx.stop_propagation();
                    }
                    "down" => {
                        if let Some(first) = this.row_keys.first() {
                            focus_key(first.clone(), window, cx);
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            .child(
                text_field_small(self.search(), false, window, cx)
                    .h(px(HEADER_CONTROL))
                    .pl(px(22.)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(9.))
                    .top_0()
                    .h(px(HEADER_CONTROL))
                    .flex()
                    .items_center()
                    .child(icon("search", 13., p.subtle_foreground)),
            )
    }

    fn render_menu(&self, menu: OpenMenu, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let count = menu.entries.len();
        let mut list = div()
            .id("dict-menu")
            .occlude()
            .min_w(px(196.))
            .p(px(4.))
            .flex()
            .flex_col()
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(p.foreground.opacity(0.1))
            .bg(p.popover)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., if p.dark { 0.55 } else { 0.22 }),
                offset: point(px(0.), px(16.)),
                blur_radius: px(40.),
                spread_radius: px(-18.),
                inset: false,
            }])
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_menu(false, window, cx)),
            )
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                let current = (0..count)
                    .find(|&i| keyed_focus_handle(menu_item_key(i), cx).is_focused(window));
                let step = |delta: usize| current.map_or(0, |i| (i + delta) % count);
                match e.keystroke.key.as_str() {
                    "down" => focus_key(menu_item_key(step(1)), window, cx),
                    "up" => focus_key(menu_item_key(step(count - 1)), window, cx),
                    "escape" | "tab" => this.close_menu(true, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }));
        for (index, entry) in menu.entries.into_iter().enumerate() {
            let fg = if entry.destructive {
                p.destructive
            } else {
                p.foreground
            };
            let wash = if entry.destructive {
                p.destructive.opacity(0.1)
            } else {
                p.hover_wash(0.08)
            };
            // Destructive items sit apart from routine ones.
            if entry.destructive && index > 0 {
                list = list.child(div().my(px(4.)).mx(px(4.)).h(px(1.)).bg(p.border));
            }
            let action = entry.action.clone();
            list = list.child(
                div()
                    .id(menu_item_key(index))
                    .keyboard_focusable(
                        menu_item_key(index),
                        Role::MenuItem,
                        entry.label.clone(),
                        cx,
                    )
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(RADIUS_SM))
                    .text_size(px(TEXT_BODY))
                    .text_color(fg)
                    .cursor_pointer()
                    .hover(move |s| s.bg(wash))
                    .focus(move |s| s.bg(wash))
                    .child(icon(
                        entry.icon,
                        14.,
                        if entry.destructive {
                            p.destructive
                        } else {
                            p.muted_foreground
                        },
                    ))
                    .child(div().flex_1().whitespace_nowrap().child(entry.label))
                    .when_some(entry.shortcut, |d, keys| {
                        d.child(
                            div()
                                .pl(px(16.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.subtle_foreground)
                                .child(keys),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_menu(false, window, cx);
                        this.run(action.clone(), window, cx);
                    })),
            );
        }
        list.with_animation(
            "dict-menu-in",
            motion(DUR_SNAP, cx).with_easing(ease_snap),
            |d, t| d.opacity(t),
        )
        .into_any_element()
    }

    /// The floating "Removed … Undo" bar at the bottom of the window.
    fn render_undo(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending.as_ref()?;
        let p = cx.palette();
        let message = match &pending.removal {
            Removal::Word(entry) => t_with("dictionary.removedWord", &[("word", &entry.word)]),
            Removal::Snippet(snippet) => t_with(
                "dictionary.snippets.removed",
                &[("trigger", &snippet.trigger)],
            ),
            Removal::Correction { misheard, .. } => {
                t_with("dictionary.forgotMishearing", &[("word", misheard)])
            }
        };
        let viewport = window.viewport_size();
        let reduced = cx.reduce_motion();
        let bar = div()
            .id("dict-undo")
            .occlude()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(40.))
            .pl(px(14.))
            .pr(px(6.))
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(p.foreground.opacity(0.1))
            .bg(p.popover)
            .shadow(vec![drop(0., 8., 24., if p.dark { 0.5 } else { 0.14 })])
            .child(
                div()
                    .max_w(px(280.))
                    .truncate()
                    .text_size(px(TEXT_BODY))
                    .text_color(p.foreground)
                    .child(message),
            )
            .child(kbd("⌘Z", &p))
            .child(
                Button::new("dict-undo-button")
                    .label(t("dictionary.undo"))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
            )
            .with_animation(
                ("dict-undo-in", pending.id as usize),
                motion(DUR_BASE, cx).with_easing(ease_out),
                move |d, t| {
                    let d = d.opacity(t);
                    if reduced { d } else { d.mt(px(6. * (1. - t))) }
                },
            );
        Some(
            deferred(
                anchored()
                    .position(point(viewport.width / 2., viewport.height - px(24.)))
                    .anchor(Anchor::BottomCenter)
                    .child(bar),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// An icon button that appears on a row while it is hovered or focused.
    fn row_action(
        key: ElementId,
        name: &'static str,
        label: String,
        destructive: bool,
        cx: &mut App,
    ) -> Stateful<Div> {
        let p = cx.palette();
        let (fg, wash) = if destructive {
            (p.destructive, p.destructive.opacity(0.1))
        } else {
            (p.foreground, p.hover_wash(0.08))
        };
        let group: SharedString = format!("{key}-action").into();
        div()
            .id(key.clone())
            .keyboard_focusable(key, Role::Button, label.clone(), cx)
            .group(group.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded(px(RADIUS_MD))
            .cursor_pointer()
            .hover(move |s| s.bg(wash))
            .tooltip(text_tooltip(label.clone()))
            .child(
                icon(name, 14., p.muted_foreground).group_hover(group, move |s| s.text_color(fg)),
            )
    }

    /// A hairline under a bleeding row, inset back to the page edges so it
    /// lines up with the header controls while the hover fill runs wider.
    fn row_rule(color: Hsla) -> Div {
        div()
            .absolute()
            .bottom_0()
            .left(px(ROW_BLEED))
            .right(px(ROW_BLEED))
            .h(px(1.))
            .bg(color)
    }

    fn table_header(cells: Vec<AnyElement>, p: &Palette) -> Div {
        div()
            .relative()
            .flex()
            .items_center()
            .h(px(30.))
            .mx(px(-ROW_BLEED))
            .px(px(ROW_BLEED))
            .children(cells)
            .child(Self::row_rule(p.border.opacity(0.7)))
    }

    fn row_frame(key: &ElementId, label: String, focused: bool, cx: &mut App) -> Stateful<Div> {
        let p = cx.palette();
        let wash = p.hover_wash(if p.dark { 0.04 } else { 0.045 });
        div()
            .id(key.clone())
            .keyboard_focusable(key.clone(), Role::ListItem, label, cx)
            .aria_description(t("dictionary.rowKeysHint"))
            .relative()
            .flex()
            .items_center()
            .h(px(40.))
            .mx(px(-ROW_BLEED))
            .px(px(ROW_BLEED))
            .rounded(px(RADIUS_MD))
            .hover(move |s| s.bg(wash))
            .when(focused, |d| d.bg(wash))
            .child(Self::row_rule(p.border.opacity(0.5)))
    }

    fn quiet_message(text: String, p: &Palette) -> Div {
        div()
            .py(px(28.))
            .text_center()
            .text_size(px(TEXT_SMALL))
            .text_color(p.muted_foreground)
            .child(text)
    }

    /// A key hint for the footer: `↩ Edit`.
    fn key_hint(keys: &'static str, label: String, p: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(kbd(keys, p))
            .child(label)
    }

    fn footer(left: String, focused_row: bool, p: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(20.))
            .text_size(px(TEXT_SMALL))
            .text_color(p.subtle_foreground)
            .child(left)
            .when(focused_row, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .child(Self::key_hint("↩", t("dictionary.edit"), p))
                        .child(Self::key_hint("⌫", t("dictionary.remove"), p)),
                )
            })
    }

    /// The frame of an inline editor above a list.
    fn panel_frame(p: &Palette) -> Div {
        surface_card(p)
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .shadow(vec![drop(0., 1., 2., 0.04)])
    }

    // ---- Rendering: Words --------------------------------------------------------------------

    fn render_words(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let (entries, words, snippets) = {
            let s = AppState::global(cx).read(cx);
            (
                s.dictionary_entries.clone(),
                s.dictionary.clone(),
                s.snippets.clone(),
            )
        };
        let pending = self.pending_word().map(str::to_string);
        let entries: Vec<DictionaryEntry> = entries
            .into_iter()
            .filter(|e| Some(&e.word) != pending.as_ref())
            .collect();
        let query_raw = self.word_search.read(cx).value().trim().to_string();
        let query = query_raw.to_lowercase();
        let learned = entries
            .iter()
            .filter(|e| e.source == DictionarySource::Learned)
            .count();
        // With no learned words every row is manual, so the filter says nothing.
        let filter = if learned > 0 {
            self.filter
        } else {
            SourceFilter::All
        };
        let mut visible: Vec<DictionaryEntry> = entries
            .iter()
            .filter(|e| match filter {
                SourceFilter::All => true,
                SourceFilter::Manual => e.source == DictionarySource::Manual,
                SourceFilter::Learned => e.source == DictionarySource::Learned,
            })
            .filter(|e| query.is_empty() || e.word.to_lowercase().contains(&query))
            .cloned()
            .collect();
        sort_entries(&mut visible, self.sort, self.descending);
        let shown = visible.len();
        let max_uses = entries.iter().map(|e| e.use_count).max().unwrap_or(0);
        let prompt_chars = get_dictionary_hint_words(&words, &snippets)
            .join(", ")
            .chars()
            .count();

        let panel = match self.panel {
            Panel::AddWords => Some(self.render_add_panel(window, cx)),
            Panel::Import => Some(self.render_import_panel(window, cx)),
            _ => None,
        };

        let col = div().flex().flex_col().gap(px(12.)).children(panel);
        if entries.is_empty() {
            self.row_keys.clear();
            return col.child(Self::render_empty(&p, cx));
        }

        let show_last_used = self.content_width(window) >= TABLE_FULL;
        let mut table = div().flex().flex_col().child(Self::table_header(
            vec![
                div()
                    .flex_1()
                    .child(self.column_heading(
                        0,
                        "dictionary.columnWord",
                        SortKey::Word,
                        window,
                        cx,
                    ))
                    .into_any_element(),
                div()
                    .w(px(COL_USES))
                    .flex()
                    .justify_end()
                    .child(self.column_heading(
                        1,
                        "dictionary.columnUses",
                        SortKey::Uses,
                        window,
                        cx,
                    ))
                    .into_any_element(),
                div().w(px(COL_USE_BAR)).into_any_element(),
            ]
            .into_iter()
            .chain(show_last_used.then(|| {
                div()
                    .w(px(COL_LAST_USED))
                    .flex()
                    .justify_end()
                    .child(self.column_heading(
                        2,
                        "dictionary.columnLastUsed",
                        SortKey::LastUsed,
                        window,
                        cx,
                    ))
                    .into_any_element()
            }))
            .collect(),
            &p,
        ));
        if visible.is_empty() {
            table = table.child(self.render_no_matches(&query_raw, cx));
        }
        self.row_keys = visible.iter().map(|e| word_key(&e.word)).collect();
        let corrections = AppState::global(cx).read(cx).known_corrections.clone();
        let hidden = self
            .pending_correction()
            .map(|(m, t)| (m.to_string(), t.to_string()));
        let mut focused_row = false;
        for (index, entry) in visible.into_iter().enumerate() {
            let misheard: Vec<String> = corrections
                .iter()
                .filter(|c| c.term == entry.word)
                .filter(|c| {
                    hidden
                        .as_ref()
                        .is_none_or(|(m, t)| *m != c.misheard || *t != c.term)
                })
                .map(|c| c.misheard.clone())
                .collect();
            let (row, focused) = self.render_word_row(index, entry, misheard, max_uses, window, cx);
            focused_row |= focused;
            table = table.child(row);
        }

        let count = if shown == entries.len() {
            t_with(
                "dictionary.wordCount",
                &[("count", &entries.len().to_string())],
            )
        } else {
            t_with(
                "dictionary.wordCountFiltered",
                &[
                    ("shown", &shown.to_string()),
                    ("total", &entries.len().to_string()),
                ],
            )
        };
        col.child(table)
            .child(Self::footer(count, focused_row, &p))
            .when(prompt_chars > WHISPER_DECODER_PROMPT_CHARS, |d| {
                d.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        // Kept to a readable measure, not the full table width.
                        .max_w(px(560.))
                        .child(div().pt(px(2.)).child(icon(
                            "alert-circle",
                            13.,
                            p.subtle_foreground,
                        )))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(TEXT_SMALL))
                                .line_height(px(LINE_SMALL))
                                .text_color(p.subtle_foreground)
                                .child(t_with(
                                    "dictionary.promptLimitNotice",
                                    &[("chars", &prompt_chars.to_string())],
                                )),
                        ),
                )
            })
    }

    fn render_add_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let can_add = !self.add_input.read(cx).value().trim().is_empty();
        let (note, positive) = self
            .add_note
            .clone()
            .unwrap_or_else(|| (t("dictionary.addHint"), false));
        let note_color = if positive {
            p.success
        } else {
            p.subtle_foreground
        };
        Self::panel_frame(&p)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    this.close_panel(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(text_field_small(
                        &self.add_input,
                        false,
                        window,
                        cx,
                    )))
                    .child(
                        Button::new("dict-add-cancel")
                            .label(t("common.cancel"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close_panel(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("dict-add-confirm")
                            .label(t("dictionary.add"))
                            .size(ButtonSize::Sm)
                            .disabled(!can_add)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_from_input(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .text_color(note_color)
                    .child(note),
            )
            .into_any_element()
    }

    fn render_import_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let pending = parse_dictionary_import_text(&self.bulk.read(cx).value()).len();
        let hint = if pending > 0 {
            t_with("dictionary.wordsReady", &[("count", &pending.to_string())])
        } else {
            t("dictionary.separateWithCommas")
        };
        Self::panel_frame(&p)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.as_str();
                if key == "escape" {
                    this.close_panel(window, cx);
                    cx.stop_propagation();
                } else if key == "enter" && e.keystroke.modifiers.platform {
                    this.import_bulk(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(textarea_field(&self.bulk, 96., false, window, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .px(px(2.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(if pending > 0 {
                                p.muted_foreground
                            } else {
                                p.subtle_foreground
                            })
                            .child(hint),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Button::new("bulk-cancel")
                                    .label(t("common.cancel"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_panel(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("bulk-import")
                                    .label(t("dictionary.import"))
                                    .size(ButtonSize::Sm)
                                    .disabled(pending == 0)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.import_bulk(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Learned words, leaving out a word waiting on Undo.
    fn learned_count(&self, cx: &App) -> usize {
        let pending = self.pending_word();
        AppState::global(cx)
            .read(cx)
            .dictionary_entries
            .iter()
            .filter(|e| Some(e.word.as_str()) != pending)
            .filter(|e| e.source == DictionarySource::Learned)
            .count()
    }

    /// All / Manual / Learned, under the tab bar once auto-learn has added a
    /// word, so learned words can be reviewed without digging through a menu.
    /// Quiet ghost chips; only Learned carries a count, since how many words
    /// auto-learn has added is the one number the footer does not give.
    fn render_filter(&self, learned: usize, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let active_bg = p.hover_wash(if p.dark { 0.1 } else { 0.06 });
        let hover_bg = p.hover_wash(if p.dark { 0.05 } else { 0.035 });
        let hover_fg = p.foreground;
        let mut row = div().flex_none().flex().items_center().gap(px(2.)).child(
            div()
                .mr(px(6.))
                .text_size(px(TEXT_SMALL))
                .text_color(p.subtle_foreground)
                .child(t("dictionary.filterSource")),
        );
        for (filter, id, key) in [
            (SourceFilter::All, "dict-filter-all", "dictionary.filterAll"),
            (
                SourceFilter::Manual,
                "dict-filter-manual",
                "dictionary.filterManual",
            ),
            (
                SourceFilter::Learned,
                "dict-filter-learned",
                "dictionary.filterLearned",
            ),
        ] {
            let active = self.filter == filter;
            let count = (filter == SourceFilter::Learned).then(|| learned.to_string());
            let label = match &count {
                Some(n) => t_with("dictionary.filterLearnedCount", &[("count", n)]),
                None => t(key),
            };
            row = row.child(
                div()
                    .id(id)
                    .keyboard_focusable(id, Role::RadioButton, label.clone(), cx)
                    .aria_toggled(if active {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .h(px(24.))
                    .px(px(TAB_PAD))
                    .rounded(px(RADIUS_MD))
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| d.bg(active_bg).text_color(p.foreground))
                    .when(!active, |d| {
                        d.text_color(p.muted_foreground)
                            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                    })
                    .child(t(key))
                    // The count is quieter than its label, in figures that
                    // keep the chip's width steady as words are learned.
                    .children(count.map(|n| {
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .font_features(tabular_nums())
                            .text_color(p.subtle_foreground)
                            .child(n)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter = filter;
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// A sortable column heading; a second click flips the direction. Idle
    /// headings show a faint chevron on hover or focus, pointing the way a
    /// click would sort, so every sortable column reads as one.
    fn column_heading(
        &self,
        index: usize,
        key: &'static str,
        sort: SortKey,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = cx.palette();
        let active = sort == self.sort;
        let focus: ElementId = ("dict-column", index).into();
        let focused = keyed_focus_handle(focus.clone(), cx).is_focused(window);
        let label = t_with("dictionary.sortBy", &[("column", &t(key))]);
        let group: SharedString = format!("dict-column-{index}").into();
        let (descending, chevron_color) = if active {
            (self.descending, p.foreground)
        } else {
            (default_descending(sort), p.subtle_foreground)
        };
        // The chevron always takes its space so headings do not shift as it
        // appears. In the right-aligned number and date columns it leads, so
        // the label's edge lines up with the values below.
        let chevron = div()
            .flex_none()
            .opacity(if active || focused { 1. } else { 0. })
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(icon(
                if descending {
                    "chevron-down"
                } else {
                    "chevron-up"
                },
                12.,
                chevron_color,
            ));
        let (before, after) = if sort != SortKey::Word {
            (Some(chevron), None)
        } else {
            (None, Some(chevron))
        };
        let state = match (active, self.descending) {
            (false, _) => t("dictionary.notSorted"),
            (true, true) => t("dictionary.sortedDescending"),
            (true, false) => t("dictionary.sortedAscending"),
        };
        div()
            .id(focus.clone())
            .keyboard_focusable(focus, Role::ColumnHeader, label, cx)
            // AccessKit has no sort-direction property here, so VoiceOver
            // hears the state after the heading's name.
            .aria_description(state)
            .group(group)
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(4.))
            .mx(px(-4.))
            .rounded(px(RADIUS_SM))
            .text_size(px(TEXT_SMALL))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if active {
                p.foreground
            } else {
                p.muted_foreground
            })
            .cursor_pointer()
            .hover(|s| s.text_color(p.foreground))
            .children(before)
            .child(t(key))
            .children(after)
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.sort == sort {
                    this.descending = !this.descending;
                } else {
                    this.sort = sort;
                    this.descending = default_descending(sort);
                }
                cx.notify();
            }))
    }

    fn render_no_matches(&self, query: &str, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        if query.is_empty() {
            return Self::quiet_message(t("dictionary.filterEmpty"), &p);
        }
        let exists = AppState::global(cx)
            .read(cx)
            .dictionary
            .iter()
            .any(|w| w.to_lowercase() == query.to_lowercase());
        let word = query.to_string();
        Self::quiet_message(t_with("dictionary.noMatches", &[("word", query)]), &p)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .when(!exists, |d| {
                d.child(
                    Button::new("dict-add-query")
                        .icon("plus", 12.)
                        .label(t_with("dictionary.addQuery", &[("word", query)]))
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.add_words(&word, cx);
                            this.word_search
                                .update(cx, |i, cx| i.set_value("", window, cx));
                            focus_key(word_key(&word), window, cx);
                            cx.notify();
                        })),
                )
            })
    }

    /// A way the word was misheard. Only its × forgets it, so a stray click
    /// while reading the row cannot, and forgetting goes through Undo.
    fn misheard_chip(
        misheard: String,
        word: String,
        max_chars: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = cx.palette();
        let key: ElementId =
            SharedString::from(format!("dict-chip-forget:{word}:{misheard}")).into();
        let group: SharedString = format!("{key}-group").into();
        let label = t_with("dictionary.forgetMishearing", &[("word", &misheard)]);
        let label_text = misheard.clone();
        let wash = p.hover_wash(0.1);
        let forget = div()
            .id(key.clone())
            .keyboard_focusable(key, Role::Button, label.clone(), cx)
            .group(group.clone())
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            // A 24 pt target around the 10 pt glyph. It overhangs the 20 pt
            // chip vertically (the chip's height is fixed) and its hover
            // fill stays a 20 pt square inside the chip. Negative margins
            // here would collapse the chip's measured width.
            .size(px(24.))
            .rounded(px(RADIUS_SM))
            .cursor_pointer()
            .tooltip(text_tooltip(label))
            .child(
                div()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .group_hover(group.clone(), move |s| s.bg(wash))
                    // The × stays visible so the chip reads as removable
                    // before it is hovered.
                    .child(
                        icon("x", 10., p.subtle_foreground)
                            .group_hover(group, move |s| s.text_color(p.foreground)),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.forget_correction(misheard.clone(), word.clone(), window, cx);
            }));
        Self::chip_frame(&p)
            .flex_none()
            .child(
                div()
                    .whitespace_nowrap()
                    .child(clip_chars(&label_text, max_chars)),
            )
            .child(forget)
    }

    fn chip_frame(p: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .h(px(20.))
            .pl(px(6.))
            .rounded(px(RADIUS_SM))
            .bg(p.foreground.opacity(if p.dark { 0.06 } else { 0.045 }))
            .text_size(px(TEXT_SMALL))
            .text_color(p.muted_foreground)
    }

    /// Stands in for the mishearings that do not fit; its tooltip lists them.
    fn more_chip(word: &str, hidden: &[String], cx: &App) -> Stateful<Div> {
        let p = cx.palette();
        Self::chip_frame(&p)
            .id(SharedString::from(format!("dict-chip-more:{word}")))
            .flex_none()
            .pr(px(6.))
            .font_features(tabular_nums())
            .tooltip(text_tooltip(hidden.join(", ")))
            .child(format!("+{}", hidden.len()))
    }

    fn learned_badge(p: &Palette) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(18.))
            .px(px(6.))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(p.border)
            .text_size(px(TEXT_CAPTION))
            .font_weight(FontWeight::MEDIUM)
            .text_color(p.muted_foreground)
            .child(div().size(px(5.)).rounded_full().bg(p.success))
            .child(t("dictionary.filterLearned"))
    }

    /// One word row, and whether focus is inside it.
    fn render_word_row(
        &mut self,
        index: usize,
        entry: DictionaryEntry,
        misheard: Vec<String>,
        max_uses: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnimationElement<Stateful<Div>>, bool) {
        let p = cx.palette();
        let key = word_key(&entry.word);
        let handle = keyed_focus_handle(key.clone(), cx);
        let focused = handle.contains_focused(window, cx);
        let group: SharedString = format!("dict-row-{index}").into();
        let editing = self.editing.as_deref() == Some(entry.word.as_str());
        let word = entry.word.clone();

        let edit_error = self.edit_error.clone().filter(|_| editing);
        let word_cell = if editing {
            div()
                .flex_1()
                .min_w_0()
                .pr(px(12.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        this.cancel_edit(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(text_field_small(&self.edit_input, false, window, cx).h(px(28.)))
                .when_some(edit_error.clone(), |d, error| {
                    d.child(
                        div()
                            .px(px(2.))
                            .truncate()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.destructive)
                            .child(error),
                    )
                })
        } else {
            let has_misheard = !misheard.is_empty();
            // A narrow table keeps one chip and folds the rest into "+N".
            let narrow = self.content_width(window) < TABLE_FULL;
            let (max_chips, max_chars) = if narrow {
                (1, 12)
            } else {
                (MAX_ROW_CORRECTIONS, 24)
            };
            let shown = if misheard.len() > max_chips {
                max_chips.saturating_sub(1).max(1)
            } else {
                misheard.len()
            };
            let more =
                (misheard.len() > shown).then(|| Self::more_chip(&word, &misheard[shown..], cx));
            let chips: Vec<Div> = misheard
                .into_iter()
                .take(shown)
                .map(|m| Self::misheard_chip(m, word.clone(), max_chars, cx))
                .collect();
            let (edit_word, remove_word) = (word.clone(), word.clone());
            let actions = div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.))
                .pl(px(8.))
                .opacity(if focused { 1. } else { 0. })
                .group_hover(group.clone(), |s| s.opacity(1.))
                .child(
                    Self::row_action(
                        SharedString::from(format!("dict-edit:{word}")).into(),
                        "pencil",
                        t_with("dictionary.editWord", &[("word", &word)]),
                        false,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.start_edit(edit_word.clone(), window, cx)
                    })),
                )
                .child(
                    Self::row_action(
                        SharedString::from(format!("dict-remove:{word}")).into(),
                        "trash2",
                        t_with("dictionary.removeWord", &[("word", &word)]),
                        true,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.remove_word(&remove_word, window, cx)
                    })),
                );
            div()
                .flex_1()
                .min_w_0()
                .pr(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(280.))
                        .truncate()
                        .text_size(px(TEXT_BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.foreground)
                        .child(word.clone()),
                )
                .when(entry.source == DictionarySource::Learned, |d| {
                    d.child(Self::learned_badge(&p))
                })
                .when(has_misheard, |d| {
                    // The chip budget above keeps this strip to one line, so
                    // every row stays 40 pt tall. (A clipping strip would
                    // break GPUI's sizing of the chips inside it.)
                    d.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            // On a narrow table the chips speak for themselves.
                            .when(!narrow, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .pl(px(4.))
                                        .pr(px(2.))
                                        .text_size(px(TEXT_SMALL))
                                        .text_color(p.subtle_foreground)
                                        .child(t("dictionary.heardAs")),
                                )
                            })
                            .children(chips)
                            .children(more),
                    )
                })
                .child(div().flex_1())
                .child(actions)
        };

        let show_last_used = self.content_width(window) >= TABLE_FULL;
        let bar = use_bar_width(entry.use_count, max_uses);
        let used = last_used_day(entry.use_count, entry.last_used_at);
        let tooltip = entry
            .last_used_at
            .filter(|_| used.is_some())
            .map(full_timestamp);
        let count_color = if entry.use_count > 0 {
            p.muted_foreground
        } else {
            p.subtle_foreground
        };
        let uses_cell = div()
            .w(px(COL_USES))
            .flex()
            .justify_end()
            .text_size(px(TEXT_SMALL))
            .font_features(tabular_nums())
            .text_color(count_color)
            .child(entry.use_count.to_string());
        // The bar's tooltip names the scale, so a full bar reads as "the most
        // used word", not as a quota being reached.
        let bar_tip = t_with(
            "dictionary.usesBar",
            &[
                ("count", &entry.use_count.to_string()),
                ("max", &max_uses.to_string()),
            ],
        );
        let bar_cell = div()
            .id(("dict-use-bar", index))
            .w(px(COL_USE_BAR))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .when(max_uses > 0, |d| {
                d.role(Role::Meter)
                    .aria_label(bar_tip.clone())
                    .tooltip(text_tooltip(bar_tip.clone()))
            })
            .child(
                div()
                    .w(px(USE_TRACK))
                    .h(px(4.))
                    .flex()
                    .rounded(px(2.))
                    .bg(p.foreground.opacity(if p.dark { 0.08 } else { 0.07 }))
                    .child(
                        div()
                            .h_full()
                            .w(px(bar))
                            .rounded(px(2.))
                            .bg(p.foreground.opacity(if p.dark { 0.45 } else { 0.4 })),
                    ),
            );
        let last_used_cell = div()
            .id(("dict-last-used", index))
            .w(px(COL_LAST_USED))
            .flex()
            .justify_end()
            .text_size(px(TEXT_SMALL))
            .font_features(tabular_nums())
            .text_color(if used.is_some() {
                p.muted_foreground
            } else {
                p.subtle_foreground
            })
            .child(used.map_or_else(
                || "—".to_string(),
                |day| day_label(day, Local::now().date_naive()),
            ))
            .when_some(tooltip, |d, tip| d.tooltip(text_tooltip(tip.clone())));

        let row_handle = handle.clone();
        let row_key = key.clone();
        let menu_word = word.clone();
        let edit_word = word.clone();
        let key_word = word.clone();
        let click_handle = handle;
        let row = Self::row_frame(&key, word.clone(), focused && !editing, cx)
            // Only an editor with a message under it grows past one line.
            .when(edit_error.is_some(), |d| d.h_auto().py(px(6.)))
            .group(group)
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                let keyboard = e.is_keyboard() && click_handle.is_focused(window);
                if keyboard || e.click_count() >= 2 {
                    this.start_edit(edit_word.clone(), window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    let entries = this.row_menu_entries(Some(&menu_word), None);
                    let key = word_key(&menu_word);
                    this.open_menu(
                        OpenMenu {
                            entries,
                            position: Some(e.position),
                            return_focus: key,
                        },
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                // Keys typed into the inline editor or a chip bubble up here.
                if !row_handle.is_focused(window) {
                    return;
                }
                match e.keystroke.key.as_str() {
                    "up" => this.focus_neighbor(&row_key, -1, window, cx),
                    "down" => this.focus_neighbor(&row_key, 1, window, cx),
                    "backspace" | "delete" => this.remove_word(&key_word, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(word_cell)
            .child(uses_cell)
            .child(bar_cell)
            .when(show_last_used, |d| d.child(last_used_cell));
        (
            stagger_in(row, format!("dict-in-{word}"), index, cx),
            focused,
        )
    }

    fn render_empty(p: &Palette, cx: &mut Context<Self>) -> Div {
        // The shared empty state, bare like History's, with the two ways in.
        div().flex().justify_center().py(px(48.)).px(px(24.)).child(
            empty_state(
                "book-open",
                t("dictionary.emptyTitle"),
                t_with(
                    "dictionary.emptyDescription",
                    &[("agentName", crate::text::DEFAULT_AGENT_NAME)],
                ),
                p,
            )
            .child(
                div()
                    .mt(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Button::new("dict-first")
                            .icon("plus", 12.)
                            .label(t("dictionary.addFirstWord"))
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_panel(Panel::AddWords, window, cx)
                            })),
                    )
                    .child(
                        Button::new("dict-import-first")
                            .icon("upload", 12.)
                            .label(t("dictionary.importFirstList"))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_panel(Panel::Import, window, cx)
                            })),
                    ),
            ),
        )
    }

    // ---- Rendering: Snippets -----------------------------------------------------------------

    fn render_snippets(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let pending = self.pending_trigger().map(str::to_string);
        let snippets: Vec<Snippet> = AppState::global(cx)
            .read(cx)
            .snippets
            .iter()
            .filter(|s| Some(&s.trigger) != pending.as_ref())
            .cloned()
            .collect();
        let query_raw = self.snippet_search.read(cx).value().trim().to_string();
        let query = query_raw.to_lowercase();
        let visible: Vec<Snippet> = snippets
            .iter()
            .filter(|s| {
                query.is_empty()
                    || s.trigger.to_lowercase().contains(&query)
                    || s.replacement.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();

        let panel =
            (self.panel == Panel::NewSnippet).then(|| self.render_snippet_panel(window, cx));
        let col = div().flex().flex_col().gap(px(12.)).children(panel);
        if snippets.is_empty() {
            self.row_keys.clear();
            return col.child(self.render_snippets_empty(&p, cx));
        }

        let heading = |key: &str| {
            div()
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(t(key))
        };
        let mut table = div().flex().flex_col().child(Self::table_header(
            vec![
                div()
                    .w(px(COL_TRIGGER))
                    .child(heading("dictionary.snippets.triggerLabel"))
                    .into_any_element(),
                div()
                    .flex_1()
                    .child(heading("dictionary.snippets.replacementLabel"))
                    .into_any_element(),
            ],
            &p,
        ));
        if visible.is_empty() {
            let trigger = query_raw.clone();
            table = table.child(
                Self::quiet_message(
                    t_with("dictionary.snippets.noMatches", &[("query", &query_raw)]),
                    &p,
                )
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(
                    Button::new("dict-snippet-from-query")
                        .icon("plus", 12.)
                        .label(t("dictionary.snippets.new"))
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.trigger_input
                                .update(cx, |i, cx| i.set_value(trigger.clone(), window, cx));
                            this.snippet_search
                                .update(cx, |i, cx| i.set_value("", window, cx));
                            this.open_panel(Panel::NewSnippet, window, cx);
                            this.expansion.update(cx, |i, cx| i.focus(window, cx));
                        })),
                ),
            );
        }
        self.row_keys = visible.iter().map(|s| snippet_key(&s.trigger)).collect();
        let mut focused_row = false;
        for (index, snippet) in visible.iter().cloned().enumerate() {
            let (row, focused) = self.render_snippet_row(index, snippet, window, cx);
            focused_row |= focused;
            table = table.child(row);
        }
        let count = if visible.len() == snippets.len() {
            t_with(
                "dictionary.snippets.count",
                &[("count", &snippets.len().to_string())],
            )
        } else {
            t_with(
                "dictionary.snippets.countFiltered",
                &[
                    ("shown", &visible.len().to_string()),
                    ("total", &snippets.len().to_string()),
                ],
            )
        };
        col.child(table).child(Self::footer(count, focused_row, &p))
    }

    fn render_snippet_row(
        &mut self,
        index: usize,
        snippet: Snippet,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnimationElement<Stateful<Div>>, bool) {
        let p = cx.palette();
        let key = snippet_key(&snippet.trigger);
        let handle = keyed_focus_handle(key.clone(), cx);
        let focused = handle.contains_focused(window, cx);
        let group: SharedString = format!("snippet-row-{index}").into();
        let trigger = snippet.trigger.clone();

        let (for_edit, remove) = (snippet.clone(), trigger.clone());
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .pl(px(8.))
            .opacity(if focused { 1. } else { 0. })
            .group_hover(group.clone(), |s| s.opacity(1.))
            .child(
                Self::row_action(
                    SharedString::from(format!("dict-snippet-edit:{trigger}")).into(),
                    "pencil",
                    t_with("dictionary.snippets.editLabel", &[("trigger", &trigger)]),
                    false,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    Self::edit_snippet(for_edit.clone(), window, cx)
                }),
            )
            .child(
                Self::row_action(
                    SharedString::from(format!("dict-snippet-remove:{trigger}")).into(),
                    "trash2",
                    t_with("dictionary.snippets.removeLabel", &[("trigger", &trigger)]),
                    true,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.remove_snippet_row(&remove, window, cx)
                })),
            );

        let row_handle = handle.clone();
        let click_handle = handle;
        let row_key = key.clone();
        let (menu_snippet, open_snippet, key_trigger) =
            (snippet.clone(), snippet.clone(), trigger.clone());
        let row = Self::row_frame(&key, trigger.clone(), focused, cx)
            .group(group)
            .on_click(move |e: &ClickEvent, window, cx| {
                let keyboard = e.is_keyboard() && click_handle.is_focused(window);
                if keyboard || e.click_count() >= 2 {
                    Self::edit_snippet(open_snippet.clone(), window, cx);
                }
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    let entries = this.row_menu_entries(None, Some(&menu_snippet));
                    this.open_menu(
                        OpenMenu {
                            entries,
                            position: Some(e.position),
                            return_focus: snippet_key(&menu_snippet.trigger),
                        },
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                if !row_handle.is_focused(window) {
                    return;
                }
                match e.keystroke.key.as_str() {
                    "up" => this.focus_neighbor(&row_key, -1, window, cx),
                    "down" => this.focus_neighbor(&row_key, 1, window, cx),
                    "backspace" | "delete" => this.remove_snippet_row(&key_trigger, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .w(px(COL_TRIGGER))
                    .flex_none()
                    .pr(px(16.))
                    .min_w_0()
                    .truncate()
                    .text_size(px(TEXT_BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.foreground)
                    .child(trigger.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.muted_foreground)
                            .child(one_line(&snippet.replacement)),
                    )
                    .child(actions),
            );
        (
            stagger_in(row, format!("snippet-in-{trigger}"), index, cx),
            focused,
        )
    }

    fn render_snippet_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let duplicate = !trigger.is_empty() && Self::trigger_exists(&snippets, &trigger, None);
        let can_create =
            !trigger.is_empty() && !self.expansion.read(cx).value().trim().is_empty() && !duplicate;
        let label = |key: &str| {
            div()
                .px(px(2.))
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(t(key))
        };
        Self::panel_frame(&p)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.as_str();
                if key == "escape" {
                    this.close_panel(window, cx);
                    cx.stop_propagation();
                } else if key == "enter" && e.keystroke.modifiers.platform {
                    this.create_snippet(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("dictionary.snippets.triggerLabel"))
                    .child(text_field_small(&self.trigger_input, false, window, cx))
                    .when(duplicate, |d| {
                        d.child(
                            div()
                                .px(px(2.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.destructive)
                                .child(t("dictionary.snippets.duplicate")),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("dictionary.snippets.replacementLabel"))
                    .child(textarea_field(&self.expansion, 72., false, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.subtle_foreground)
                            .child(kbd("⌘↩", &p))
                            .child(t("dictionary.snippets.createHint")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("snippet-cancel")
                                    .label(t("common.cancel"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_panel(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("snippet-create")
                                    .label(t("dictionary.snippets.create"))
                                    .size(ButtonSize::Sm)
                                    .disabled(!can_create)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_snippet(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_snippets_empty(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut examples = div()
            .flex_1()
            .min_w(px(260.))
            .flex()
            .flex_col()
            .gap(px(10.));
        for key in ["linkedin", "rewrite", "intro", "signoff"] {
            examples = examples.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(RADIUS_MD))
                            .border_1()
                            .border_color(p.primary.opacity(if p.dark { 0.2 } else { 0.15 }))
                            .bg(p.primary.opacity(if p.dark { 0.15 } else { 0.08 }))
                            .px(px(6.))
                            .py(px(2.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.primary)
                            .child(icon("mic", 10., p.primary))
                            .child(t(&format!("dictionary.snippets.examples.{key}Trigger"))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .mt(px(2.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.subtle_foreground)
                            .child("→"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .mt(px(2.))
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.muted_foreground)
                            .child(t(&format!("dictionary.snippets.examples.{key}Text"))),
                    ),
            );
        }
        surface_card(p).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(40.))
                .gap_y(px(24.))
                .px(px(28.))
                .py(px(32.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(220.))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(4.))
                                .text_size(px(TEXT_TITLE))
                                .line_height(px(24.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.foreground)
                                .child(t("dictionary.snippets.emptyTitle"))
                                .child(
                                    div()
                                        .text_color(p.primary)
                                        .child(t("dictionary.snippets.emptyTitleAccent")),
                                ),
                        )
                        .child(
                            div()
                                .mt(px(6.))
                                .text_size(px(TEXT_BODY))
                                .line_height(px(20.))
                                .text_color(p.muted_foreground)
                                .child(t("dictionary.snippets.emptyDescription")),
                        )
                        .when(self.panel != Panel::NewSnippet, |d| {
                            d.child(
                                div().mt(px(20.)).child(
                                    Button::new("snippet-new")
                                        .icon("plus", 12.)
                                        .label(t("dictionary.snippets.new"))
                                        .size(ButtonSize::Sm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_panel(Panel::NewSnippet, window, cx)
                                        })),
                                ),
                            )
                        }),
                )
                .child(examples),
        )
    }
}

impl Render for DictionaryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(window, cx);
        let body = match self.tab {
            Tab::Words => self.render_words(window, cx),
            Tab::Snippets => self.render_snippets(window, cx),
        };
        let context_menu = self
            .menu
            .clone()
            .and_then(|m| m.position.map(|at| (m, at)))
            .map(|(m, at)| {
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(self.render_menu(m, cx)),
                )
                .with_priority(2)
            });
        let undo = self.render_undo(window, cx);
        page_container()
            .pb(px(40.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
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

/// The "Edit snippet" dialog.
struct SnippetEditor {
    original: Snippet,
    trigger: Entity<InputState>,
    replacement: Entity<TextareaState>,
}

impl SnippetEditor {
    fn new(snippet: Snippet, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let trigger = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("dictionary.snippets.triggerPlaceholder"))
                .default_value(snippet.trigger.clone())
        });
        let replacement = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(t("dictionary.snippets.replacementPlaceholder"))
                .default_value(snippet.replacement.clone())
        });
        replacement.update(cx, |i, cx| i.focus(window, cx));
        cx.subscribe(&trigger, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        cx.subscribe(&replacement, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        Self {
            original: snippet,
            trigger,
            replacement,
        }
    }

    fn state(&self, cx: &App) -> (String, String, bool) {
        let trigger = self.trigger.read(cx).value().trim().to_string();
        let replacement = self.replacement.read(cx).value().trim().to_string();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        let duplicate = !trigger.is_empty()
            && DictionaryView::trigger_exists(&snippets, &trigger, Some(&self.original.trigger));
        (trigger, replacement, duplicate)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (trigger, replacement, duplicate) = self.state(cx);
        if trigger.is_empty() || replacement.is_empty() || duplicate {
            return;
        }
        let state = AppState::global(cx);
        let original = self.original.trigger.clone();
        let snippets: Vec<Snippet> = state
            .read(cx)
            .snippets
            .iter()
            .map(|s| {
                if s.trigger == original {
                    Snippet {
                        trigger: trigger.clone(),
                        replacement: replacement.clone(),
                    }
                } else {
                    s.clone()
                }
            })
            .collect();
        state.update(cx, |s, cx| s.set_snippets(snippets, cx));
        close_custom(cx);
        // Back to the edited row, so keyboard editing can continue.
        focus_key(snippet_key(&trigger), window, cx);
    }
}

impl Render for SnippetEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let (trigger, replacement, duplicate) = self.state(cx);
        let can_save = !trigger.is_empty() && !replacement.is_empty() && !duplicate;
        let label = |text: String| {
            div()
                .text_size(px(TEXT_SMALL))
                .font_weight(FontWeight::MEDIUM)
                .child(text)
        };
        let content = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "enter" && e.keystroke.modifiers.platform {
                    this.save(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(TEXT_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t("dictionary.snippets.editTitle")),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .text_color(p.muted_foreground)
                            .child(t("dictionary.snippets.dialogDescription")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label(t("dictionary.snippets.triggerLabel")))
                    .child(text_field(&self.trigger, false, window, cx))
                    .when(duplicate, |d| {
                        d.child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.destructive)
                                .child(t("dictionary.snippets.duplicate")),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label(t("dictionary.snippets.replacementLabel")))
                    .child(textarea_field(&self.replacement, 96., false, window, cx)),
            )
            .child(
                div()
                    .pt(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.subtle_foreground)
                            .child(kbd("⌘↩", &p))
                            .child(t("dictionary.snippets.saveHint")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("snippet-edit-cancel")
                                    .label(t("common.cancel"))
                                    .variant(ButtonVariant::Ghost)
                                    .on_click(|_, _, cx| close_custom(cx)),
                            )
                            .child(
                                Button::new("snippet-edit-save")
                                    .label(t("common.save"))
                                    .disabled(!can_save)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            ),
                    ),
            );
        modal_frame(
            "snippet-editor",
            448.,
            content,
            |_, cx| close_custom(cx),
            cx,
        )
    }
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
}
