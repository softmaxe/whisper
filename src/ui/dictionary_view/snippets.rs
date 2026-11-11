//! Snippets behavior and presentation for DictionaryView.

use super::*;

impl DictionaryView {
    pub(super) fn trigger_exists(snippets: &[Snippet], value: &str, except: Option<&str>) -> bool {
        let lower = value.to_lowercase();
        let except = except.map(str::to_lowercase);
        snippets.iter().any(|s| {
            let existing = s.trigger.to_lowercase();
            existing == lower && Some(&existing) != except.as_ref()
        })
    }

    /// Why `trigger` cannot be saved, as the message shown under the field.
    /// Storage drops overlong triggers, so they must never reach it.
    pub(super) fn trigger_problem(
        snippets: &[Snippet],
        trigger: &str,
        except: Option<&str>,
    ) -> Option<String> {
        if trigger.chars().count() > MAX_SNIPPET_TRIGGER_LENGTH {
            Some(t_with(
                "dictionary.snippets.tooLong",
                &[("max", &MAX_SNIPPET_TRIGGER_LENGTH.to_string())],
            ))
        } else if !trigger.is_empty() && Self::trigger_exists(snippets, trigger, except) {
            Some(t("dictionary.snippets.duplicate"))
        } else {
            None
        }
    }

    pub(super) fn create_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let replacement = self.expansion.read(cx).value().trim().to_string();
        self.commit_pending(cx);
        let state = AppState::global(cx);
        let mut snippets = state.read(cx).snippets.clone();
        if trigger.is_empty()
            || replacement.is_empty()
            || Self::trigger_problem(&snippets, &trigger, None).is_some()
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

    pub(super) fn remove_snippet(trigger: String, cx: &mut App) {
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

    pub(super) fn remove_snippet_row(
        &mut self,
        trigger: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(super) fn edit_snippet(snippet: Snippet, window: &mut Window, cx: &mut App) {
        let editor = cx.new(|cx| SnippetEditor::new(snippet, window, cx));
        show_custom(editor.into(), cx);
    }

    pub(super) fn render_snippets(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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

    pub(super) fn render_snippet_row(
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

    pub(super) fn render_snippet_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        let trigger = self.trigger_input.read(cx).value().trim().to_string();
        let problem = Self::trigger_problem(&snippets, &trigger, None);
        let can_create = !trigger.is_empty()
            && !self.expansion.read(cx).value().trim().is_empty()
            && problem.is_none();
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
                    .when_some(problem, |d, problem| {
                        d.child(
                            div()
                                .px(px(2.))
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.destructive)
                                .child(problem),
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

    pub(super) fn render_snippets_empty(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
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
                            .text_color(p.foreground)
                            .child(icon("mic", 10., p.muted_foreground))
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
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(24.))
            .child(
                empty_state(
                    "scroll-text",
                    format!(
                        "{} {}",
                        t("dictionary.snippets.emptyTitle"),
                        t("dictionary.snippets.emptyTitleAccent")
                    ),
                    t("dictionary.snippets.emptyDescription"),
                    p,
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
            .child(
                examples
                    .flex_none()
                    .max_w(px(super::super::theme::UPLOAD_COPY_MAX_WIDTH)),
            )
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

    fn state(&self, cx: &App) -> (String, String, Option<String>) {
        let trigger = self.trigger.read(cx).value().trim().to_string();
        let replacement = self.replacement.read(cx).value().trim().to_string();
        let snippets = AppState::global(cx).read(cx).snippets.clone();
        let problem =
            DictionaryView::trigger_problem(&snippets, &trigger, Some(&self.original.trigger));
        (trigger, replacement, problem)
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (trigger, replacement, problem) = self.state(cx);
        if trigger.is_empty() || replacement.is_empty() || problem.is_some() {
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
        let (trigger, replacement, problem) = self.state(cx);
        let can_save = !trigger.is_empty() && !replacement.is_empty() && problem.is_none();
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
                    .when_some(problem, |d, problem| {
                        d.child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.destructive)
                                .child(problem),
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
