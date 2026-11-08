//! Words behavior and presentation for DictionaryView.

use super::*;

impl DictionaryView {
    pub(super) fn render_words(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let p = cx.palette();
        let query_raw = self.word_search.read(cx).value().trim().to_string();
        let key = WordRowsKey {
            query: query_raw.to_lowercase(),
            pending: self.pending_word().map(str::to_string),
            hidden_correction: self
                .pending_correction()
                .map(|(m, t)| (m.to_string(), t.to_string())),
            filter: self.filter,
            sort: self.sort,
            descending: self.descending,
        };
        if self
            .word_rows
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            let state = AppState::global(cx);
            let state = state.read(cx);
            let rows = Arc::new(WordRows::new(
                &state.dictionary_entries,
                &state.dictionary,
                &state.snippets,
                &state.known_corrections,
                &key,
            ));
            self.word_rows = Some((key, rows));
        }
        let rows = self.word_rows.as_ref().unwrap().1.clone();
        let visible = &rows.visible;
        let shown = visible.len();
        let max_uses = rows.max_uses;
        let prompt_chars = rows.prompt_chars;

        let panel = match self.panel {
            Panel::AddWords => Some(self.render_add_panel(window, cx)),
            Panel::Import => Some(self.render_import_panel(window, cx)),
            _ => None,
        };

        let col = div().flex().flex_col().gap(px(12.)).children(panel);
        if rows.total == 0 {
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
        self.row_keys.clone_from(&rows.row_keys);
        let mut focused_row = false;
        for (index, (entry, misheard)) in visible.iter().zip(&rows.misheard).enumerate() {
            let (row, focused) = self.render_word_row(index, entry, misheard, max_uses, window, cx);
            focused_row |= focused;
            table = table.child(row);
        }

        let count = if shown == rows.total {
            t_with(
                "dictionary.wordCount",
                &[("count", &rows.total.to_string())],
            )
        } else {
            t_with(
                "dictionary.wordCountFiltered",
                &[
                    ("shown", &shown.to_string()),
                    ("total", &rows.total.to_string()),
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

    pub(super) fn render_add_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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

    pub(super) fn render_import_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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

    /// All words and learned words, leaving out a word waiting on Undo.
    pub(super) fn source_counts(&self, cx: &App) -> (usize, usize) {
        let pending = self.pending_word();
        let entries = &AppState::global(cx).read(cx).dictionary_entries;
        let words = entries.iter().filter(|e| Some(e.word.as_str()) != pending);
        let learned = words
            .clone()
            .filter(|e| e.source == DictionarySource::Learned)
            .count();
        (words.count(), learned)
    }

    /// All / Manual / Learned, under the tab bar once auto-learn has added a
    /// word, so learned words can be reviewed without digging through a menu.
    /// The same segmented control as the Insights range, compact since it
    /// only narrows the list; underline tabs stay for switching views. Every
    /// option carries its count, so the three read as parts of one whole.
    pub(super) fn render_filter(
        &self,
        total: usize,
        learned: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = cx.palette();
        let mut track = segmented_track(&p);
        for (filter, id, key, count) in [
            (
                SourceFilter::All,
                "dict-filter-all",
                "dictionary.filterAll",
                total,
            ),
            (
                SourceFilter::Manual,
                "dict-filter-manual",
                "dictionary.filterManual",
                total - learned,
            ),
            (
                SourceFilter::Learned,
                "dict-filter-learned",
                "dictionary.filterLearned",
                learned,
            ),
        ] {
            let active = self.filter == filter;
            let name = t(key);
            let count = count.to_string();
            let label = t_with(
                "dictionary.filterOptionCount",
                &[("label", &name), ("count", &count)],
            );
            track = track.child(
                segment(id, label, active, cx)
                    .gap(px(5.))
                    .child(name)
                    // Quieter than its label, in figures that keep the
                    // segment's width steady as words are learned.
                    .child(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .font_features(tabular_nums())
                            .text_color(p.subtle_foreground)
                            .child(count),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter = filter;
                        cx.notify();
                    })),
            );
        }
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.subtle_foreground)
                    .child(t("dictionary.filterSource")),
            )
            .child(track)
    }

    /// A sortable column heading; a second click flips the direction. Idle
    /// headings always show a faint chevron, pointing the way a
    /// click would sort, so every sortable column reads as one.
    pub(super) fn column_heading(
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
        // Keep the chevron after every label and reserve its space so
        // headings do not shift when the sort indicator is highlighted.
        let chevron = div()
            .flex_none()
            .opacity(if active || focused { 1. } else { 0.7 })
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
            .child(t(key))
            .child(chevron)
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

    pub(super) fn render_no_matches(&self, query: &str, cx: &mut Context<Self>) -> Div {
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
    pub(super) fn misheard_chip(
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

    pub(super) fn chip_frame(p: &Palette) -> Div {
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
    pub(super) fn more_chip(word: &str, hidden: &[String], cx: &App) -> Stateful<Div> {
        let p = cx.palette();
        Self::chip_frame(&p)
            .id(SharedString::from(format!("dict-chip-more:{word}")))
            .flex_none()
            .pr(px(6.))
            .font_features(tabular_nums())
            .tooltip(text_tooltip(hidden.join(", ")))
            .child(format!("+{}", hidden.len()))
    }

    pub(super) fn learned_badge(p: &Palette) -> Div {
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
            // A category, not a state: the dot stays neutral so green keeps
            // meaning "healthy" elsewhere.
            .child(div().size(px(5.)).rounded_full().bg(p.subtle_foreground))
            .child(t("dictionary.filterLearned"))
    }

    /// One word row, and whether focus is inside it.
    pub(super) fn render_word_row(
        &mut self,
        index: usize,
        entry: &DictionaryEntry,
        misheard: &[String],
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
                .iter()
                .take(shown)
                .map(|m| Self::misheard_chip(m.clone(), word.clone(), max_chars, cx))
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
                            .bg(p.foreground.opacity(if p.dark { 0.45 } else { 0.5 })),
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

    pub(super) fn render_empty(p: &Palette, cx: &mut Context<Self>) -> Div {
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
}
