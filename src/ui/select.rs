//! A compact dropdown select, used inline in settings rows. Long lists, such
//! as the transcription languages, get a search field.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{MenuTriggerBounds, drop, f32_tx, focus_ring, icon, text_tooltip};
use super::control_panel::CloseOverlay;
use super::theme::{
    DUR_BASE, DUR_SNAP, FONT_MONO, PaletteExt, RADIUS_LG, RADIUS_SM, ROW_CONTROL_HEIGHT,
    TEXT_MONO_SMALL, TEXT_SMALL, ease_out, ease_snap, motion,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
    /// A Lucide icon drawn before the label.
    pub icon: Option<&'static str>,
    /// Extra text the search field matches, such as a language's English
    /// name beside its native one.
    pub keywords: Option<SharedString>,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
            keywords: None,
            disabled: false,
        }
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn keywords(mut self, keywords: impl Into<SharedString>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Model IDs are case-sensitive; whitespace around a typed ID is not part
/// of the value sent to the server.
fn unlisted_value<'a>(options: &[SelectOption], query: &'a str) -> Option<&'a str> {
    let value = query.trim();
    (!value.is_empty() && !options.iter().any(|option| option.value.as_ref() == value))
        .then_some(value)
}

pub enum SelectEvent {
    Change(SharedString),
}

/// List length above which the menu offers a search field.
pub const LIST_SEARCH_THRESHOLD: usize = 12;

pub struct Select {
    options: Vec<SelectOption>,
    selected: Option<SharedString>,
    placeholder: SharedString,
    /// The name VoiceOver reads for the trigger, usually its row's label.
    label: SharedString,
    open: bool,
    width: Option<Pixels>,
    /// When set, `width` is only the floor and the trigger grows with its
    /// value up to this cap, so short values keep a shared column edge while
    /// long ones (device names) get more room before truncating.
    max_width: Option<Pixels>,
    menu_min_width: Pixels,
    disabled: bool,
    /// Model selectors accept IDs the server does not list.
    custom_value: bool,
    search: Option<Entity<InputState>>,
    query: String,
    highlighted: usize,
    focus: FocusHandle,
    /// The trigger's painted bounds, used for menu sizing and dismissal.
    trigger_bounds: MenuTriggerBounds,
}

impl EventEmitter<SelectEvent> for Select {}

impl Select {
    pub fn new(
        options: Vec<SelectOption>,
        selected: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = if options.len() > LIST_SEARCH_THRESHOLD {
            Some(Self::search_input(
                "languageSelector.searchPlaceholder",
                window,
                cx,
            ))
        } else {
            None
        };
        Self {
            options,
            selected,
            placeholder: "".into(),
            label: "".into(),
            open: false,
            width: None,
            max_width: None,
            menu_min_width: px(160.),
            disabled: false,
            custom_value: false,
            search,
            query: String::new(),
            highlighted: 0,
            focus: cx.focus_handle(),
            trigger_bounds: MenuTriggerBounds::default(),
        }
    }

    pub fn with_width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Sizes the trigger to its value, between `min` and `max`.
    pub fn with_width_range(mut self, min: Pixels, max: Pixels) -> Self {
        self.width = Some(min);
        self.max_width = Some(max);
        self
    }

    pub fn with_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn with_menu_min_width(mut self, width: Pixels) -> Self {
        self.menu_min_width = width;
        self
    }

    /// Combines server choices and manual model IDs in the same search menu.
    pub fn with_custom_value(mut self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        self.custom_value = true;
        self.search = Some(Self::search_input(
            "reasoning.custom.modelSearchPlaceholder",
            window,
            cx,
        ));
        self
    }

    fn search_input(
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(crate::i18n::t(placeholder)));
        cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.query = input.read(cx).value().to_string();
                    this.highlighted = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.choose_highlighted(window, cx),
                _ => {}
            },
        )
        .detach();
        input
    }

    pub fn set_options(&mut self, options: Vec<SelectOption>, cx: &mut Context<Self>) {
        if self.options != options {
            self.options = options;
            self.highlighted = self
                .highlighted
                .min(self.filtered().len().saturating_sub(1));
            cx.notify();
        }
    }

    pub fn set_selected(&mut self, selected: Option<SharedString>, cx: &mut Context<Self>) {
        if self.selected != selected {
            self.selected = selected;
            cx.notify();
        }
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.open = false;
            }
            cx.notify();
        }
    }

    fn filtered(&self) -> Vec<SelectOption> {
        if self.search.is_none() || self.query.trim().is_empty() {
            return self.options.clone();
        }
        let q = self.query.trim().to_lowercase();
        let mut options: Vec<_> = self
            .options
            .iter()
            .filter(|o| {
                o.label.to_lowercase().contains(&q)
                    || o.value.to_lowercase().contains(&q)
                    || o.keywords
                        .as_ref()
                        .is_some_and(|k| k.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        if self.custom_value
            && let Some(value) = unlisted_value(&self.options, &self.query)
        {
            options.push(SelectOption::new(
                value.to_string(),
                crate::i18n::t_with("reasoning.custom.useModelId", &[("model", value)]),
            ));
        }
        options
    }

    /// Opens the real menu after its trigger has laid out in a screenshot scene.
    #[cfg(debug_assertions)]
    pub(super) fn preview_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.toggle(window, cx);
        }
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.open = !self.open;
        if self.open {
            self.query.clear();
            self.highlighted = self
                .options
                .iter()
                .position(|o| Some(&o.value) == self.selected.as_ref())
                .unwrap_or(0);
            if let Some(search) = &self.search {
                search.update(cx, |s, cx| s.set_value("", window, cx));
                let handle = search.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            } else {
                window.focus(&self.focus, cx);
            }
        }
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    /// Closes the menu and hands focus back to the trigger, so keyboard
    /// users continue from where they opened it.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close(cx);
        window.focus(&self.focus, cx);
    }

    fn choose(&mut self, value: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(window, cx);
        self.selected = Some(value.clone());
        cx.emit(SelectEvent::Change(value));
        cx.notify();
    }

    fn choose_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(option) = self.filtered().get(self.highlighted).cloned()
            && !option.disabled
        {
            self.choose(option.value, window, cx);
        }
    }

    /// Arrow keys move through the open menu and open a closed one, like a
    /// macOS pop-up button. Return and Space arrive as keyboard clicks on the
    /// trigger instead (see `on_trigger_click`).
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        // Tab leaves an open menu the way Escape does, back on the trigger,
        // instead of moving on with the menu still floating behind.
        if key == "tab" && self.open {
            cx.stop_propagation();
            self.dismiss(window, cx);
            return;
        }
        // Return and Space act on the root, which holds focus; in the
        // search field they keep typing.
        if matches!(key, "enter" | "space")
            && !event.keystroke.modifiers.modified()
            && self.focus.is_focused(window)
        {
            cx.stop_propagation();
            if self.open {
                self.choose_highlighted(window, cx);
            } else {
                self.toggle(window, cx);
            }
            return;
        }
        if !matches!(key, "down" | "up") || event.keystroke.modifiers.modified() {
            return;
        }
        cx.stop_propagation();
        if !self.open {
            self.toggle(window, cx);
            return;
        }
        let count = self.filtered().len();
        if count == 0 {
            return;
        }
        self.highlighted = if key == "down" {
            (self.highlighted + 1) % count
        } else {
            (self.highlighted + count - 1) % count
        };
        cx.notify();
    }

    fn on_trigger_click(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Keyboard activation is handled in `on_key`, on the focused root.
        if !event.is_keyboard() {
            self.toggle(window, cx);
        }
    }

    /// Escape closes an open menu before it can close the sheet around it.
    fn on_close_overlay(&mut self, _: &CloseOverlay, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.dismiss(window, cx);
        } else {
            cx.propagate();
        }
    }
}

impl Render for Select {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        let selected_option = self
            .options
            .iter()
            .find(|o| Some(&o.value) == self.selected.as_ref())
            .cloned();
        let label: SharedString = selected_option
            .as_ref()
            .map(|o| o.label.clone())
            .or_else(|| self.custom_value.then(|| self.selected.clone()).flatten())
            .unwrap_or_else(|| self.placeholder.clone());
        let lead_icon = selected_option.as_ref().and_then(|o| o.icon);
        let open = self.open;
        let chevron_turn = f32_tx(
            ("select-chevron", cx.entity_id().as_u64() as usize),
            if open { 0.5 } else { 0.0 },
            DUR_BASE,
            ease_out,
            window,
            cx,
        );

        // Opening the menu is distinct from keyboard focus on the root.
        let trigger_border = if open {
            p.control_border
        } else {
            p.input_border
        };
        let trigger_shadow = if p.dark {
            Vec::new()
        } else {
            vec![drop(0., 1., 2., 0.05)]
        };
        let aria_name = self.label.clone();
        let value = selected_option
            .as_ref()
            .map(|o| o.label.clone())
            .or_else(|| self.custom_value.then(|| self.selected.clone()).flatten())
            .unwrap_or_default();
        // The button radius, so a select sits beside buttons as their kin.
        let radius = px(RADIUS_LG);
        let trigger = div()
            .id("select-trigger")
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .map(|d| self.size_trigger(d))
            .h(px(ROW_CONTROL_HEIGHT))
            .px(px(10.))
            .rounded(radius)
            .border_1()
            .border_color(trigger_border)
            .bg(if p.dark {
                p.white.opacity(0.035)
            } else {
                p.popover
            })
            .when(!trigger_shadow.is_empty(), |d| d.shadow(trigger_shadow))
            .text_size(px(TEXT_SMALL))
            .font_weight(FontWeight::MEDIUM)
            .when(self.custom_value, |d| {
                d.font_family(FONT_MONO)
                    .text_size(px(TEXT_MONO_SMALL))
                    .tooltip(text_tooltip(label.to_string()))
            })
            .text_color(
                if selected_option.is_some() || (self.custom_value && self.selected.is_some()) {
                    p.foreground
                } else {
                    p.muted_foreground
                },
            )
            .when(self.disabled, |d| d.opacity(0.5))
            .when(!self.disabled, |d| {
                d.cursor_pointer().hover(|s| {
                    if open {
                        s
                    } else {
                        s.bg(if p.dark {
                            p.white.opacity(0.06)
                        } else {
                            p.muted.opacity(0.6)
                        })
                    }
                })
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .overflow_hidden()
                    .when_some(lead_icon, |d, name| {
                        d.child(icon(name, 14., p.muted_foreground))
                    })
                    .child(div().truncate().child(label)),
            )
            .child(
                icon("chevron-down", 14., p.muted_foreground).with_transformation(
                    Transformation::rotate(radians(chevron_turn * std::f32::consts::PI)),
                ),
            )
            .on_click(cx.listener(Self::on_trigger_click));

        let menu = open.then(|| self.render_menu(cx));

        // Focus sits on the root, which holds both the trigger and the menu,
        // so the highlighted option can report itself as the active
        // descendant while the arrow keys move it.
        div()
            .id("select")
            .when(!self.disabled, |d| {
                d.track_focus(&self.focus.clone().tab_stop(true))
                    .focus_visible(move |s| s.shadow(focus_ring(&p)))
            })
            .role(Role::ComboBox)
            .aria_label(aria_name)
            .aria_value(value)
            .aria_expanded(open)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::on_close_overlay))
            .relative()
            .rounded(radius)
            .map(|d| self.size_trigger(d))
            .child(trigger)
            .child(self.trigger_bounds.measure())
            .when_some(menu, |d, menu| {
                d.child(
                    deferred(anchored().snap_to_window_with_margin(px(8.)).child(menu))
                        .with_priority(2),
                )
            })
    }
}

impl Select {
    fn size_trigger<E: Styled>(&self, el: E) -> E {
        match (self.width, self.max_width) {
            (Some(min), Some(max)) => el.min_w(min).max_w(max),
            (Some(w), None) => el.w(w),
            (None, _) => el.w_full(),
        }
    }

    fn render_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let filtered = self.filtered();
        let highlighted = self.highlighted;
        let selected = self.selected.clone();
        let mut list = div()
            .id("select-options")
            .role(Role::ListBox)
            .aria_label(self.label.clone())
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(4.));
        if filtered.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground)
                    .child(crate::i18n::t(if self.custom_value {
                        "reasoning.custom.modelEmpty"
                    } else {
                        "languageSelector.noLanguagesFound"
                    })),
            );
        }
        for (index, option) in filtered.into_iter().enumerate() {
            let is_selected = Some(&option.value) == selected.as_ref();
            let is_highlighted = index == highlighted;
            let value = option.value.clone();
            let row_bg = is_highlighted.then_some(p.list_highlight);
            list = list.child(
                div()
                    .id(("option", index))
                    .role(Role::ListBoxOption)
                    .aria_label(option.label.clone())
                    .aria_selected(is_selected)
                    .when(is_highlighted, |d| d.aria_active_descendant())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(RADIUS_SM))
                    .text_size(px(TEXT_SMALL))
                    .when(self.custom_value, |d| {
                        d.font_family(FONT_MONO)
                            .text_size(px(TEXT_MONO_SMALL))
                            .tooltip(text_tooltip(option.value.to_string()))
                    })
                    .text_color(if is_highlighted {
                        p.selection_foreground
                    } else {
                        p.foreground
                    })
                    .when_some(row_bg, |d, bg| d.bg(bg))
                    .when(option.disabled, |d| d.opacity(0.5))
                    .when(!option.disabled, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(p.list_highlight).text_color(p.selection_foreground))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose(value.clone(), window, cx)
                            }))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .when_some(option.icon, |d, name| {
                                d.child(icon(name, 14., p.muted_foreground))
                            })
                            .child(div().truncate().child(option.label.clone())),
                    )
                    .when(is_selected, |d| d.child(icon("check", 14., p.foreground))),
            );
        }

        let search = self.search.clone().map(|search| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .h(px(34.))
                .border_b_1()
                .border_color(p.border)
                .child(icon("search", 14., p.muted_foreground))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(TEXT_SMALL))
                        .child(Input::new(&search).appearance(false)),
                )
        });

        div()
            .id("select-menu")
            .occlude()
            .mt(px(4.))
            .min_w(self.menu_min_width.max(self.trigger_bounds.width()))
            .when(self.custom_value, |d| {
                d.w(self.menu_min_width.max(self.trigger_bounds.width()))
            })
            .when_some(self.width, |d, w| d.w(w.max(self.menu_min_width)))
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
            .overflow_hidden()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !this.trigger_bounds.contains(event.position) {
                    this.close(cx);
                }
            }))
            .when_some(search, |d, s| d.child(s))
            .child(
                div()
                    .id("select-list")
                    .max_h(px(192.))
                    .overflow_y_scroll()
                    .child(list),
            )
            .with_animation(
                "select-menu-in",
                motion(DUR_SNAP, cx).with_easing(ease_snap),
                |d, t| d.opacity(t),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{SelectOption, unlisted_value};

    #[test]
    fn manual_ids_preserve_case_and_trim_only_surrounding_whitespace() {
        let options = vec![SelectOption::new("Qwen:9b", "Qwen")];
        assert_eq!(unlisted_value(&options, "  Qwen:9b \n"), None);
        assert_eq!(unlisted_value(&options, " qwen:9b "), Some("qwen:9b"));
        assert_eq!(unlisted_value(&options, "Qwen"), Some("Qwen"));
    }

    #[test]
    fn manual_ids_work_before_any_server_models_are_loaded() {
        assert_eq!(
            unlisted_value(&[], " hf.co/team/model:tag "),
            Some("hf.co/team/model:tag")
        );
        assert_eq!(unlisted_value(&[], " \n\t "), None);
    }
}
