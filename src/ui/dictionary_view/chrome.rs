//! Chrome behavior and presentation for DictionaryView.

use super::*;

impl DictionaryView {
    pub(super) fn render_header(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
        let (total, learned) = self.source_counts(cx);
        let filter =
            (self.tab == Tab::Words && learned > 0).then(|| self.render_filter(total, learned, cx));
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
        // the add action; the page's ⋮ menu lives in the window's top bar.
        // The source filter only narrows the Words view, so it sits under
        // the bar as a compact segmented control.
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
    pub fn render_page_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_words = !AppState::global(cx).read(cx).dictionary.is_empty();
        if self.tab != Tab::Words || !has_words {
            return Empty.into_any_element();
        }
        let trigger = MenuTriggerBounds::default();
        let menu = self
            .menu
            .clone()
            .filter(|m| m.position.is_none())
            .map(|m| self.render_menu(m, Some(trigger.clone()), window, cx));
        div()
            .relative()
            .flex()
            // The top bar drags the window on mouse-down; the menu must not.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div().relative().child(trigger.measure()).child(
                    Button::new(MORE_BUTTON)
                        .icon("more-vertical", 15.)
                        .aria_label(t("dictionary.pageActions"))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_overflow(window, cx)),
                        ),
                ),
            )
            .children(menu)
            .into_any_element()
    }

    /// The page's content width, measured from the header on the last frame;
    /// before the first, estimated from the window with the sidebar open.
    pub(super) fn content_width(&self, window: &Window) -> f32 {
        self.width.unwrap_or_else(|| {
            let window = f32::from(window.viewport_size().width);
            (window - SIDEBAR_WIDTH).min(PAGE_MAX_WIDTH) - 2. * PAGE_GUTTER
        })
    }

    /// Words / Snippets: the page-level view switch, in body text with a
    /// semibold active label and an underline on the bar's bottom edge.
    pub(super) fn render_tabs(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
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
    pub(super) fn render_search(
        &self,
        full_width: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
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

    pub(super) fn render_menu(
        &self,
        menu: OpenMenu,
        trigger: Option<MenuTriggerBounds>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let popup = PopupMenu {
            id: "dict-menu".into(),
            min_width: 196.,
            appearance: MenuAppearance::Dictionary,
            placement: menu.position.map_or(
                MenuPlacement::ToolbarRight {
                    top: px(HEADER_CONTROL + 4.),
                },
                MenuPlacement::Pointer,
            ),
            trigger,
            focus: MenuFocus::new(
                (0..menu.entries.len()).map(menu_item_key),
                menu.return_focus,
                cx,
            ),
        };
        let mut items = Vec::new();
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
                items.push(
                    div()
                        .my(px(4.))
                        .mx(px(4.))
                        .h(px(1.))
                        .bg(p.border)
                        .into_any_element(),
                );
            }
            let action = entry.action.clone();
            items.push(
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
                    }))
                    .into_any_element(),
            );
        }
        popup.render(
            items,
            |this, _, cx| {
                this.menu = None;
                cx.notify();
            },
            window,
            cx,
        )
    }

    /// The floating "Removed … Undo" bar at the bottom of the window.
    pub(super) fn render_undo(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
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
    pub(super) fn row_action(
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
    pub(super) fn row_rule(color: Hsla) -> Div {
        div()
            .absolute()
            .bottom_0()
            .left(px(ROW_BLEED))
            .right(px(ROW_BLEED))
            .h(px(1.))
            .bg(color)
    }

    pub(super) fn table_header(cells: Vec<AnyElement>, p: &Palette) -> Div {
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

    pub(super) fn row_frame(
        key: &ElementId,
        label: String,
        focused: bool,
        cx: &mut App,
    ) -> Stateful<Div> {
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

    pub(super) fn quiet_message(text: String, p: &Palette) -> Div {
        div()
            .py(px(28.))
            .text_center()
            .text_size(px(TEXT_SMALL))
            .text_color(p.muted_foreground)
            .child(text)
    }

    /// A key hint for the footer: `↩ Edit`.
    pub(super) fn key_hint(keys: &'static str, label: String, p: &Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(kbd(keys, p))
            .child(label)
    }

    pub(super) fn footer(left: String, focused_row: bool, p: &Palette) -> Div {
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
    pub(super) fn panel_frame(p: &Palette) -> Div {
        surface_card(p)
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .shadow(vec![drop(0., 1., 2., 0.04)])
    }
}
