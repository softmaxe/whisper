//! Dictionary and Snippets.

use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use whisper::db::{DictionarySource, Snippet};
use whisper::i18n::{t, t_with};
use whisper::text::dictionary_import::{format_dictionary_export, parse_dictionary_import_text};
use whisper::text::prompt_cap::WHISPER_DECODER_PROMPT_CHARS;
use whisper::text::snippets::get_dictionary_hint_words;

use crate::app_state::{AppEvent, AppState};

use super::components::{Button, ButtonSize, ButtonVariant, icon};
use super::overlays::{
    DialogRequest, Toast, ToastVariant, close_custom, modal_frame, show_custom, show_dialog,
    show_toast,
};
use super::text_input::{FieldKind, text_field, text_field_sized, textarea_field};
use super::theme::{
    FONT_MONO, LINE_SMALL, Palette, PaletteExt, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dictionary,
    Snippets,
}

pub struct DictionaryView {
    tab: Tab,
    // Dictionary
    add_input: Entity<InputState>,
    bulk: Entity<TextareaState>,
    show_bulk: bool,
    editing: Option<String>,
    edit_input: Entity<InputState>,
    // Snippets
    trigger_input: Entity<InputState>,
    expansion: Entity<TextareaState>,
    panel_open: bool,
}

fn update_dictionary(add: Vec<String>, remove: Vec<String>, cx: &mut App) {
    let state = AppState::global(cx);
    let db = state.read(cx).db.clone();
    if let Err(err) = db.apply_dictionary_changes(&add, &remove, DictionarySource::Manual) {
        log::warn!("failed to update the dictionary: {err}");
    }
    state.update(cx, |s, cx| s.reload_dictionary(cx));
}

fn agent_name() -> String {
    whisper::text::DEFAULT_AGENT_NAME.to_string()
}

impl DictionaryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let add_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("dictionary.addPlaceholder")));
        cx.subscribe_in(
            &add_input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.add_from_input(window, cx),
                InputEvent::Change => cx.notify(),
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
            |this, _, e: &InputEvent, _, cx| match e {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_edit(cx),
                _ => {}
            },
        )
        .detach();
        let trigger_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("dictionary.snippets.addPlaceholder"))
        });
        cx.subscribe_in(
            &trigger_input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.open_panel(window, cx),
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
            tab: Tab::Dictionary,
            add_input,
            bulk,
            show_bulk: false,
            editing: None,
            edit_input,
            trigger_input,
            expansion,
            panel_open: false,
        }
    }

    // ---- Dictionary --------------------------------------------------------------------------

    fn add_words(text: &str, cx: &mut App) -> usize {
        let dictionary = AppState::global(cx).read(cx).dictionary.clone();
        let mut existing: std::collections::HashSet<String> =
            dictionary.iter().map(|w| w.to_lowercase()).collect();
        let words: Vec<String> = parse_dictionary_import_text(text)
            .into_iter()
            .filter(|w| existing.insert(w.to_lowercase()))
            .collect();
        let count = words.len();
        if count > 0 {
            update_dictionary(words, Vec::new(), cx);
        }
        count
    }

    fn add_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.add_input.read(cx).value().to_string();
        if Self::add_words(&text, cx) > 0 {
            self.add_input
                .update(cx, |i, cx| i.set_value("", window, cx));
        }
        cx.notify();
    }

    fn import_bulk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.bulk.read(cx).value().to_string();
        Self::add_words(&text, cx);
        self.bulk.update(cx, |i, cx| i.set_value("", window, cx));
        self.show_bulk = false;
        cx.notify();
    }

    fn start_edit(&mut self, word: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(word.clone());
        self.edit_input.update(cx, |i, cx| {
            i.set_value(word, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(original) = self.editing.take() else {
            return;
        };
        let value = self.edit_input.read(cx).value().trim().to_string();
        let dictionary = AppState::global(cx).read(cx).dictionary.clone();
        let duplicate = dictionary
            .iter()
            .any(|w| *w != original && w.to_lowercase() == value.to_lowercase());
        if !value.is_empty() && value != original && !duplicate {
            update_dictionary(vec![value], vec![original], cx);
        }
        cx.notify();
    }

    fn export(cx: &mut App) {
        let words = AppState::global(cx).read(cx).dictionary.clone();
        let dialog = rfd::AsyncFileDialog::new()
            .set_file_name(whisper::text::dictionary_import::DICTIONARY_EXPORT_FILE_NAME)
            .add_filter("Text", &["txt"]);
        cx.spawn(async move |cx| {
            let Some(file) = dialog.save_file().await else {
                return;
            };
            if let Err(err) = std::fs::write(file.path(), format_dictionary_export(&words)) {
                cx.update(|cx| {
                    show_toast(
                        Toast::new(t("dictionary.exportFailed"))
                            .description(err.to_string())
                            .variant(ToastVariant::Destructive),
                        cx,
                    )
                });
            }
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

    fn open_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        if trigger.is_empty() || Self::trigger_exists(&snippets, &trigger, None) {
            return;
        }
        self.panel_open = true;
        self.expansion.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel_open = false;
        self.expansion
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.trigger_input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    fn create_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let replacement = self.expansion.read(cx).value().trim().to_string();
        let state = AppState::global(cx);
        let mut snippets = state.read(cx).snippets.clone();
        if trigger.is_empty()
            || replacement.is_empty()
            || Self::trigger_exists(&snippets, &trigger, None)
        {
            return;
        }
        snippets.push(Snippet {
            trigger,
            replacement,
        });
        state.update(cx, |s, cx| s.set_snippets(snippets, cx));
        self.trigger_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.close_panel(window, cx);
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

    fn edit_snippet(snippet: Snippet, window: &mut Window, cx: &mut App) {
        let editor = cx.new(|cx| SnippetEditor::new(snippet, window, cx));
        show_custom(editor.into(), cx);
    }

    // ---- Rendering ---------------------------------------------------------------------------

    fn render_tabs(&self, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let mut tabs = div().flex().items_center().h(px(28.)).gap(px(2.));
        for (tab, key) in [
            (Tab::Dictionary, "dictionary.tabDictionary"),
            (Tab::Snippets, "dictionary.tabSnippets"),
        ] {
            let active = self.tab == tab;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(key))
                    .h(px(24.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(5.))
                    .text_size(px(TEXT_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when(active, |d| {
                        d.bg(if p.dark { p.surface_raised } else { p.muted })
                            .text_color(p.foreground)
                    })
                    .when(!active, |d| {
                        d.text_color(p.muted_foreground)
                            .hover(|s| s.text_color(p.foreground))
                    })
                    .child(t(key))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    })),
            );
        }
        tabs
    }

    fn list_frame(p: &Palette) -> Div {
        div()
            .rounded(px(6.))
            .border_1()
            .border_color(if p.dark {
                p.white.opacity(0.1)
            } else {
                p.foreground.opacity(0.08)
            })
            .bg(if p.dark {
                p.white.opacity(0.03)
            } else {
                p.foreground.opacity(0.02)
            })
            .px(px(16.))
            .py(px(12.))
    }

    fn list_heading(title: String, p: &Palette) -> Div {
        div()
            .text_size(px(TEXT_SMALL))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(p.foreground.opacity(0.45))
            .child(title)
    }

    fn dashed_rule(p: &Palette) -> Div {
        div()
            .mt(px(10.))
            .border_t_1()
            .border_dashed()
            .border_color(if p.dark {
                p.white.opacity(0.1)
            } else {
                p.foreground.opacity(0.1)
            })
    }

    fn row(id: impl Into<ElementId>, group: SharedString, p: &Palette) -> Stateful<Div> {
        div()
            .id(id)
            .group(group)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(36.))
            .border_b_1()
            .border_color(if p.dark {
                p.white.opacity(0.1)
            } else {
                p.foreground.opacity(0.04)
            })
    }

    fn row_action(
        id: impl Into<ElementId>,
        name: &'static str,
        destructive: bool,
        p: &Palette,
    ) -> Stateful<Div> {
        let hover = if destructive {
            p.destructive.opacity(0.7)
        } else {
            p.foreground.opacity(0.6)
        };
        div()
            .id(id)
            .p(px(4.))
            .cursor_pointer()
            .child(icon(name, 11., p.foreground.opacity(0.45)))
            .hover(move |s| s.text_color(hover))
    }

    fn render_dictionary(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let (words, snippets) = {
            let s = AppState::global(cx).read(cx);
            (s.dictionary.clone(), s.snippets.clone())
        };
        let query_raw = self.add_input.read(cx).value().trim().to_string();
        let query = query_raw.to_lowercase();
        let visible: Vec<String> = if query.is_empty() {
            words.clone()
        } else {
            words
                .iter()
                .filter(|w| w.to_lowercase().contains(&query))
                .cloned()
                .collect()
        };
        let prompt_chars = get_dictionary_hint_words(&words, &snippets)
            .join(", ")
            .chars()
            .count();
        let pending = parse_dictionary_import_text(&self.bulk.read(cx).value()).len();
        let can_add = !query_raw.is_empty();

        let add_row = div()
            .relative()
            .child(
                text_field_sized(&self.add_input, 12., FieldKind::Line, false, window, cx)
                    .pr(px(96.)),
            )
            .child(
                div()
                    .absolute()
                    .right(px(10.))
                    .top(px(0.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("dict-add")
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.foreground.opacity(0.45))
                            .when(can_add, |d| {
                                d.cursor_pointer().hover(|s| s.text_color(p.primary))
                            })
                            .child(t("dictionary.add"))
                            .child(icon("corner-down-left", 10., p.foreground.opacity(0.45)))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_from_input(window, cx)),
                            ),
                    )
                    .child(div().w(px(1.)).h(px(14.)).bg(if p.dark {
                        p.white.opacity(0.08)
                    } else {
                        p.foreground.opacity(0.1)
                    }))
                    .child(
                        div()
                            .id("dict-import")
                            .cursor_pointer()
                            .child(icon("upload", 11., p.foreground.opacity(0.45)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_bulk = true;
                                this.bulk.update(cx, |i, cx| i.focus(window, cx));
                                cx.notify();
                            })),
                    ),
            );

        let bulk = self.show_bulk.then(|| {
            div()
                .rounded(px(6.))
                .border_1()
                .border_color(p.primary.opacity(if p.dark { 0.4 } else { 0.3 }))
                .px(px(12.))
                .pt(px(10.))
                .pb(px(8.))
                .child(
                    div()
                        .min_h(px(72.))
                        .text_size(px(TEXT_SMALL))
                        .child(Textarea::new(&self.bulk).appearance(false).h(px(72.))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pt(px(6.))
                        .child(
                            div()
                                .flex()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.foreground.opacity(0.45))
                                .child(t("dictionary.separateWithCommas"))
                                .when(pending > 0, |d| {
                                    d.child(div().text_color(p.success).child(format!(
                                        " • {}",
                                        t_with(
                                            "dictionary.wordsReady",
                                            &[("count", &pending.to_string())]
                                        )
                                    )))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    Button::new("bulk-cancel")
                                        .label(t("common.cancel"))
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Sm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.bulk
                                                .update(cx, |i, cx| i.set_value("", window, cx));
                                            this.show_bulk = false;
                                            cx.notify();
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
        });

        let mut list = Self::list_frame(&p);
        if !words.is_empty() {
            list = list
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(Self::list_heading(t("dictionary.yourDictionary"), &p))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .id("dict-clear")
                                        .text_size(px(TEXT_SMALL))
                                        .text_color(p.foreground.opacity(0.45))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(p.destructive.opacity(0.7)))
                                        .child(t("dictionary.clearAll"))
                                        .on_click(move |_, _, cx| {
                                            let words =
                                                AppState::global(cx).read(cx).dictionary.clone();
                                            show_dialog(
                                                DialogRequest::confirm(
                                                    t("dictionary.clearTitle"),
                                                    Some(t("dictionary.clearDescription").into()),
                                                    true,
                                                    move |_, cx| {
                                                        update_dictionary(
                                                            Vec::new(),
                                                            words.clone(),
                                                            cx,
                                                        )
                                                    },
                                                ),
                                                cx,
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .id("dict-export")
                                        .cursor_pointer()
                                        .child(icon("download", 12., p.foreground.opacity(0.45)))
                                        .on_click(|_, _, cx| Self::export(cx)),
                                ),
                        ),
                )
                .child(Self::dashed_rule(&p));
        }
        if words.is_empty() {
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .py(px(32.))
                    .child(
                        div()
                            .size(px(40.))
                            .mb(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(p.primary.opacity(if p.dark { 0.15 } else { 0.1 }))
                            .bg(p.primary.opacity(if p.dark { 0.09 } else { 0.06 }))
                            .child(icon(
                                "book-open",
                                17.,
                                p.primary.opacity(if p.dark { 0.6 } else { 0.5 }),
                            )),
                    )
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t("dictionary.emptyTitle")),
                    )
                    .child(
                        div()
                            .mb(px(16.))
                            .max_w(px(240.))
                            .text_center()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.foreground.opacity(0.45))
                            .child(t_with(
                                "dictionary.emptyDescription",
                                &[("agentName", &agent_name())],
                            )),
                    )
                    .child(
                        Button::new("dict-first")
                            .icon("plus", 12.)
                            .label(t("dictionary.addFirstWord"))
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_input.update(cx, |i, cx| i.focus(window, cx));
                            })),
                    )
                    .child(
                        div()
                            .id("dict-import-list")
                            .mt(px(12.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.foreground.opacity(0.45))
                            .cursor_pointer()
                            .hover(|s| s.text_color(p.foreground.opacity(0.6)))
                            .child(icon("upload", 11., p.foreground.opacity(0.45)))
                            .child(t("dictionary.importList"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_bulk = true;
                                cx.notify();
                            })),
                    ),
            );
        } else if visible.is_empty() {
            list = list.child(
                div()
                    .py(px(24.))
                    .text_center()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.foreground.opacity(0.45))
                    .child(t_with("dictionary.noMatches", &[("word", &query_raw)])),
            );
        } else {
            for (index, word) in visible.into_iter().enumerate() {
                let group: SharedString = format!("dict-row-{index}").into();
                let editing = self.editing.as_deref() == Some(word.as_str());
                let mut row = Self::row(("dict-row", index), group.clone(), &p);
                if editing {
                    row = row.child(
                        div().flex_1().child(
                            text_field_sized(
                                &self.edit_input,
                                12.,
                                FieldKind::Line,
                                false,
                                window,
                                cx,
                            )
                            .h(px(28.)),
                        ),
                    );
                } else {
                    let edit_word = word.clone();
                    let remove_word = word.clone();
                    row = row
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.foreground.opacity(0.6))
                                .child(word.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .opacity(0.)
                                .group_hover(group, |s| s.opacity(1.))
                                .child(
                                    Self::row_action(("dict-edit", index), "pencil", false, &p)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.start_edit(edit_word.clone(), window, cx)
                                        })),
                                )
                                .child(
                                    Self::row_action(("dict-remove", index), "x", true, &p)
                                        .on_click(move |_, _, cx| {
                                            update_dictionary(
                                                Vec::new(),
                                                vec![remove_word.clone()],
                                                cx,
                                            )
                                        }),
                                ),
                        );
                }
                list = list.child(row);
            }
        }

        div()
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(add_row)
            .children(bulk)
            .child(list)
            .when(prompt_chars > WHISPER_DECODER_PROMPT_CHARS, |d| {
                d.child(
                    div()
                        .text_size(px(TEXT_SMALL))
                        .line_height(px(LINE_SMALL))
                        .text_color(p.foreground.opacity(0.45))
                        .child(t_with(
                            "dictionary.promptLimitNotice",
                            &[("chars", &prompt_chars.to_string())],
                        )),
                )
            })
            .into_any_element()
    }

    fn render_snippets(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let duplicate = !trigger.is_empty() && Self::trigger_exists(&snippets, &trigger, None);
        let query = trigger.to_lowercase();
        let visible: Vec<Snippet> = if !query.is_empty() && !self.panel_open {
            snippets
                .iter()
                .filter(|s| {
                    s.trigger.to_lowercase().contains(&query)
                        || s.replacement.to_lowercase().contains(&query)
                })
                .cloned()
                .collect()
        } else {
            snippets.clone()
        };
        let can_create =
            !trigger.is_empty() && !self.expansion.read(cx).value().trim().is_empty() && !duplicate;

        let add_row = div()
            .child(
                div()
                    .relative()
                    .child(
                        text_field_sized(
                            &self.trigger_input,
                            12.,
                            FieldKind::Line,
                            false,
                            window,
                            cx,
                        )
                        .pr(px(64.)),
                    )
                    .child(
                        div()
                            .id("snippet-add")
                            .absolute()
                            .right(px(10.))
                            .top(px(0.))
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.foreground.opacity(0.45))
                            .when(!trigger.is_empty() && !duplicate, |d| {
                                d.cursor_pointer().hover(|s| s.text_color(p.primary))
                            })
                            .child(t("dictionary.add"))
                            .child(icon("corner-down-left", 10., p.foreground.opacity(0.45)))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_panel(window, cx)),
                            ),
                    ),
            )
            .when(duplicate, |d| {
                d.child(
                    div()
                        .mt(px(6.))
                        .text_size(px(TEXT_SMALL))
                        .text_color(p.destructive)
                        .child(t("dictionary.snippets.duplicate")),
                )
            });

        let panel = self.panel_open.then(|| {
            let kbd = |s: &'static str| {
                div()
                    .px(px(4.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if p.dark {
                        p.white.opacity(0.1)
                    } else {
                        p.border.opacity(0.7)
                    })
                    .bg(p.muted.opacity(0.4))
                    .font_family(FONT_MONO)
                    .text_size(px(TEXT_CAPTION))
                    .text_color(p.muted_foreground.opacity(0.7))
                    .child(s)
            };
            div()
                .rounded(px(6.))
                .border_1()
                .border_color(p.primary.opacity(if p.dark { 0.4 } else { 0.3 }))
                .px(px(12.))
                .pt(px(10.))
                .pb(px(8.))
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    let key = e.keystroke.key.as_str();
                    if key == "escape" {
                        this.close_panel(window, cx);
                    } else if key == "enter"
                        && (e.keystroke.modifiers.platform || e.keystroke.modifiers.control)
                    {
                        this.create_snippet(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .min_h(px(72.))
                        .text_size(px(TEXT_SMALL))
                        .child(Textarea::new(&self.expansion).appearance(false).h(px(72.))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .pt(px(6.))
                        .child(div().flex().gap(px(2.)).child(kbd("⌘")).child(kbd("⏎")))
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
        });

        let mut list = Self::list_frame(&p);
        if !snippets.is_empty() {
            list = list
                .child(Self::list_heading(t("dictionary.snippets.title"), &p))
                .child(Self::dashed_rule(&p));
        }
        if snippets.is_empty() {
            let mut examples = Self::list_frame(&p)
                .flex_1()
                .min_w(px(260.))
                .px(px(14.))
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
                                .rounded(px(5.))
                                .border_1()
                                .border_color(p.primary.opacity(if p.dark { 0.2 } else { 0.15 }))
                                .bg(p.primary.opacity(if p.dark { 0.15 } else { 0.1 }))
                                .px(px(6.))
                                .py(px(2.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.primary)
                                .child(icon("mic", 9., p.primary))
                                .child(t(&format!("dictionary.snippets.examples.{key}Trigger"))),
                        )
                        .child(
                            div()
                                .flex_none()
                                .mt(px(2.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.foreground.opacity(0.45))
                                .child("→"),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(TEXT_SMALL))
                                .line_height(px(LINE_SMALL))
                                .text_color(p.foreground.opacity(0.45))
                                .child(t(&format!("dictionary.snippets.examples.{key}Text"))),
                        ),
                );
            }
            list = list.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(32.))
                    .gap_y(px(20.))
                    .px(px(8.))
                    .py(px(24.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(220.))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(4.))
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
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
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.foreground.opacity(0.45))
                                    .child(t("dictionary.snippets.emptyDescription")),
                            )
                            .child(
                                div().mt(px(16.)).child(
                                    Button::new("snippet-new")
                                        .icon("plus", 12.)
                                        .label(t("dictionary.snippets.new"))
                                        .size(ButtonSize::Sm)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.trigger_input
                                                .update(cx, |i, cx| i.focus(window, cx));
                                        })),
                                ),
                            ),
                    )
                    .child(examples),
            );
        } else if visible.is_empty() {
            list = list.child(
                div()
                    .py(px(24.))
                    .text_center()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.foreground.opacity(0.45))
                    .child(t_with("dictionary.noMatches", &[("word", &trigger)])),
            );
        } else {
            for (index, snippet) in visible.into_iter().enumerate() {
                let group: SharedString = format!("snippet-row-{index}").into();
                let for_edit = snippet.clone();
                let remove = snippet.trigger.clone();
                list = list.child(
                    Self::row(("snippet-row", index), group.clone(), &p)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .text_size(px(TEXT_SMALL))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(p.foreground.opacity(0.6))
                                        .child(snippet.trigger.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(p.foreground.opacity(0.45))
                                        .child("→"),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_color(p.foreground.opacity(0.45))
                                        .child(snippet.replacement.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .opacity(0.)
                                .group_hover(group, |s| s.opacity(1.))
                                .child(
                                    Self::row_action(("snippet-edit", index), "pencil", false, &p)
                                        .on_click(move |_, window, cx| {
                                            Self::edit_snippet(for_edit.clone(), window, cx)
                                        }),
                                )
                                .child(
                                    Self::row_action(("snippet-remove", index), "x", true, &p)
                                        .on_click(move |_, _, cx| {
                                            Self::remove_snippet(remove.clone(), cx)
                                        }),
                                ),
                        ),
                );
            }
        }

        div()
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(add_row)
            .children(panel)
            .child(list)
            .into_any_element()
    }
}

impl Render for DictionaryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.render_tabs(cx);
        let body = match self.tab {
            Tab::Dictionary => self.render_dictionary(window, cx),
            Tab::Snippets => self.render_snippets(window, cx),
        };
        div()
            .flex()
            .flex_col()
            .child(div().px(px(20.)).pt(px(16.)).child(tabs))
            .child(body)
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

    fn save(&mut self, cx: &mut Context<Self>) {
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
                    .child(text_field(
                        &self.trigger,
                        FieldKind::Line,
                        false,
                        window,
                        cx,
                    ))
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
                    .justify_end()
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
                            .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
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
