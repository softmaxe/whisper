//! A dropdown select. One component covers three flavors — the compact
//! select used for retention and pill position, the searchable language
//! picker, and the full-height field select — through `SelectStyle`.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{drop, f32_tx, icon};
use super::theme::{
    CONTROL_HEIGHT, DUR_BASE, DUR_SNAP, PaletteExt, RADIUS_LG, RADIUS_MD, RADIUS_SM, TEXT_BODY,
    TEXT_SMALL, ease_out, ease_snap,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: SharedString,
    pub label: SharedString,
    pub prefix: Option<SharedString>,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            prefix: None,
            disabled: false,
        }
    }

    pub fn prefix(mut self, prefix: impl Into<SharedString>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectStyle {
    /// h-7 compact trigger (plain select / language picker).
    Compact,
    /// h-8 rounded-lg trigger (field select), styled like a text input.
    Field,
}

pub enum SelectEvent {
    Change(SharedString),
}

/// List length above which the menu offers a search field.
pub const LIST_SEARCH_THRESHOLD: usize = 12;

pub struct Select {
    options: Vec<SelectOption>,
    selected: Option<SharedString>,
    /// Shown on the trigger instead of the selected option's label.
    display_override: Option<SharedString>,
    placeholder: SharedString,
    open: bool,
    style: SelectStyle,
    width: Option<Pixels>,
    menu_min_width: Pixels,
    disabled: bool,
    search: Option<Entity<InputState>>,
    query: String,
    highlighted: usize,
    focus: FocusHandle,
}

impl EventEmitter<SelectEvent> for Select {}

impl Select {
    pub fn new(
        options: Vec<SelectOption>,
        selected: Option<SharedString>,
        style: SelectStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = if options.len() > LIST_SEARCH_THRESHOLD {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(crate::i18n::t("languageSelector.searchPlaceholder"))
            });
            cx.subscribe(
                &input,
                |this: &mut Self, input, event: &InputEvent, cx| match event {
                    InputEvent::Change => {
                        this.query = input.read(cx).value().to_string();
                        this.highlighted = 0;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(option) = this.filtered().get(this.highlighted).cloned() {
                            this.choose(option.value, cx);
                        }
                    }
                    _ => {}
                },
            )
            .detach();
            Some(input)
        } else {
            None
        };
        Self {
            options,
            selected,
            display_override: None,
            placeholder: "".into(),
            open: false,
            style,
            width: None,
            menu_min_width: px(160.),
            disabled: false,
            search,
            query: String::new(),
            highlighted: 0,
            focus: cx.focus_handle(),
        }
    }

    pub fn with_width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn with_menu_min_width(mut self, width: Pixels) -> Self {
        self.menu_min_width = width;
        self
    }

    pub fn set_options(&mut self, options: Vec<SelectOption>, cx: &mut Context<Self>) {
        if self.options != options {
            self.options = options;
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
        let q = self.query.to_lowercase();
        self.options
            .iter()
            .filter(|o| o.label.to_lowercase().contains(&q) || o.value.to_lowercase().contains(&q))
            .cloned()
            .collect()
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

    fn choose(&mut self, value: SharedString, cx: &mut Context<Self>) {
        self.open = false;
        self.selected = Some(value.clone());
        cx.emit(SelectEvent::Change(value));
        cx.notify();
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let count = self.filtered().len();
        match event.keystroke.key.as_str() {
            "down" if count > 0 => {
                self.highlighted = (self.highlighted + 1) % count;
                cx.notify();
            }
            "up" if count > 0 => {
                self.highlighted = (self.highlighted + count - 1) % count;
                cx.notify();
            }
            "enter" => {
                if let Some(option) = self.filtered().get(self.highlighted).cloned()
                    && !option.disabled
                {
                    self.choose(option.value, cx);
                }
            }
            "escape" => {
                self.close(cx);
                cx.stop_propagation();
            }
            _ => {}
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
        let label: SharedString = self
            .display_override
            .clone()
            .or_else(|| selected_option.as_ref().map(|o| o.label.clone()))
            .unwrap_or_else(|| self.placeholder.clone());
        let prefix = if self.display_override.is_none() {
            selected_option.as_ref().and_then(|o| o.prefix.clone())
        } else {
            None
        };
        let open = self.open;
        let compact = self.style == SelectStyle::Compact;
        let chevron_turn = f32_tx(
            ("select-chevron", cx.entity_id().as_u64() as usize),
            if open { 0.5 } else { 0.0 },
            DUR_BASE,
            ease_out,
            window,
            cx,
        );

        let trigger_border = if open { p.ring } else { p.input_border };
        let trigger_shadow = if open {
            vec![BoxShadow {
                color: p.ring.opacity(0.24),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(3.),
                inset: false,
            }]
        } else if p.dark {
            Vec::new()
        } else {
            vec![drop(0., 1., 2., 0.05)]
        };
        let trigger = div()
            .id("select-trigger")
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .when_some(self.width, |d, w| d.w(w))
            .when(self.width.is_none(), |d| d.w_full())
            .h(px(if compact { 28. } else { CONTROL_HEIGHT }))
            .px(px(if compact { 10. } else { 11. }))
            .rounded(px(if compact { RADIUS_MD } else { RADIUS_LG }))
            .border_1()
            .border_color(trigger_border)
            .bg(if p.dark {
                p.white.opacity(0.035)
            } else {
                p.popover
            })
            .when(!trigger_shadow.is_empty(), |d| d.shadow(trigger_shadow))
            .text_size(px(if compact { TEXT_SMALL } else { TEXT_BODY }))
            .when(compact, |d| d.font_weight(FontWeight::MEDIUM))
            .text_color(
                if selected_option.is_some() || self.display_override.is_some() {
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
                    .when_some(prefix, |d, prefix| d.child(prefix))
                    .child(div().truncate().child(label)),
            )
            .child(
                icon(
                    "chevron-down",
                    if compact { 14. } else { 16. },
                    p.muted_foreground,
                )
                .with_transformation(Transformation::rotate(radians(
                    chevron_turn * std::f32::consts::PI,
                ))),
            )
            .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)));

        let menu = open.then(|| self.render_menu(cx));

        div()
            .id("select")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .relative()
            .when_some(self.width, |d, w| d.w(w))
            .child(trigger)
            .when_some(menu, |d, menu| {
                d.child(
                    deferred(anchored().snap_to_window_with_margin(px(8.)).child(menu))
                        .with_priority(2),
                )
            })
    }
}

impl Select {
    fn render_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let compact = self.style == SelectStyle::Compact;
        let filtered = self.filtered();
        let highlighted = self.highlighted;
        let selected = self.selected.clone();
        let mut list = div().flex().flex_col().gap(px(2.)).p(px(4.));
        if filtered.is_empty() {
            list = list.child(
                div()
                    .px(px(10.))
                    .py(px(8.))
                    .text_size(px(TEXT_SMALL))
                    .text_color(p.muted_foreground)
                    .child(crate::i18n::t("languageSelector.noLanguagesFound")),
            );
        }
        for (index, option) in filtered.into_iter().enumerate() {
            let is_selected = Some(&option.value) == selected.as_ref();
            let is_highlighted = index == highlighted;
            let value = option.value.clone();
            let row_bg = is_highlighted.then(|| p.foreground.opacity(0.08));
            list = list.child(
                div()
                    .id(("option", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .h(px(28.))
                    .px(px(8.))
                    .rounded(px(RADIUS_SM))
                    .text_size(px(if compact { TEXT_SMALL } else { TEXT_BODY }))
                    .text_color(p.foreground)
                    .when_some(row_bg, |d, bg| d.bg(bg))
                    .when(option.disabled, |d| d.opacity(0.5))
                    .when(!option.disabled, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(p.foreground.opacity(0.08)))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.choose(value.clone(), cx)),
                            )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .when_some(option.prefix.clone(), |d, prefix| d.child(prefix))
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
            .min_w(self.menu_min_width)
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
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close(cx)))
            .when_some(search, |d, s| d.child(s))
            .child(
                div()
                    .id("select-list")
                    .max_h(px(if compact { 192. } else { 384. }))
                    .overflow_y_scroll()
                    .child(list),
            )
            .with_animation(
                "select-menu-in",
                Animation::new(std::time::Duration::from_millis(DUR_SNAP)).with_easing(ease_snap),
                |d, t| d.opacity(t),
            )
            .into_any_element()
    }
}
