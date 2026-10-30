//! Small presentational building blocks shared by the windows.

use std::time::{Duration, Instant};

use gpui_kit::base::{Transition, transition};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::theme::{
    CONTROL_HEIGHT, DUR_FAST, DUR_SLOW, DUR_SNAP, FONT_MONO, LINE_SMALL, LINE_TITLE,
    NAV_ROW_HEIGHT, Palette, PaletteExt, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XL, TEXT_BODY,
    TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, ease_css, ease_out, spring,
};

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A color transition keyed by `id`, 150 ms ease by default.
pub fn color_tx(
    id: impl Into<ElementId>,
    target: Hsla,
    ms: u64,
    window: &mut Window,
    cx: &mut App,
) -> Hsla {
    transition(
        id.into(),
        target,
        Transition::new(Duration::from_millis(ms)).ease(ease_css),
        window,
        cx,
    )
}

pub fn f32_tx(
    id: impl Into<ElementId>,
    target: f32,
    ms: u64,
    ease: fn(f32) -> f32,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    transition(
        id.into(),
        target,
        Transition::new(Duration::from_millis(ms)).ease(ease),
        window,
        cx,
    )
}

/// Pill switch: 30×18 track, 14 px knob, 120 ms slide.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    on_change: Option<Box<dyn Fn(bool, &mut Window, &mut App) + 'static>>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            on_change: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = cx.palette();
        let id = self.id.clone();
        let track = if self.disabled {
            if self.checked {
                p.primary.opacity(0.4)
            } else {
                p.muted
            }
        } else if self.checked {
            p.primary
        } else {
            p.input_border
        };
        let track = color_tx((id.clone(), "track"), track, DUR_SNAP, window, cx);
        let knob_x = f32_tx(
            (id.clone(), "knob"),
            if self.checked { 14.0 } else { 2.0 },
            DUR_SNAP,
            ease_css,
            window,
            cx,
        );
        let knob = if self.disabled {
            p.muted_foreground.opacity(0.5)
        } else {
            p.background
        };
        let checked = self.checked;
        let disabled = self.disabled;
        let on_change = self.on_change;
        div()
            .id(id)
            .relative()
            .flex_none()
            .w(px(30.))
            .h(px(18.))
            .rounded_full()
            .bg(track)
            .when(disabled, |d| d.opacity(0.5))
            .when(!disabled, |d| d.cursor_pointer())
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(knob_x))
                    .size(px(14.))
                    .rounded_full()
                    .bg(knob)
                    .when(!disabled, |d| d.shadow(vec![drop(0., 1., 2., 0.1)])),
            )
            .when_some(on_change.filter(|_| !disabled), |d, f| {
                d.on_click(move |_, window, cx| f(!checked, window, cx))
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Brand blue with a top highlight.
    Primary,
    Outline,
    OutlineFlat,
    Ghost,
    Destructive,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Default,
    Sm,
}

/// Rounded buttons in several variants, 32 px tall by default.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<(&'static str, f32)>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    full_width: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            variant: ButtonVariant::Primary,
            size: ButtonSize::Default,
            disabled: false,
            full_width: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, name: &'static str, size: f32) -> Self {
        self.icon = Some((name, size));
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }
}

/// How far a pressed button has sunk: in over 60 ms, back out on the
/// 450 ms spring, which overshoots a little past rest.
fn press_depth(id: &ElementId, window: &mut Window, cx: &mut App) -> (Entity<bool>, f32) {
    let pressed = window.use_keyed_state((id.clone(), "pressed"), cx, |_, _| false);
    let down = *pressed.read(cx);
    let depth = f32_tx(
        (id.clone(), "press"),
        if down { 1. } else { 0. },
        if down { DUR_FAST } else { DUR_SLOW },
        if down { ease_out } else { spring },
        window,
        cx,
    );
    (pressed, depth)
}

fn set_pressed(pressed: &Entity<bool>, down: bool, cx: &mut App) {
    pressed.update(cx, |p, cx| {
        if *p != down {
            *p = down;
            cx.notify();
        }
    });
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = cx.palette();
        let (height, pad_x, text) = match self.size {
            ButtonSize::Default => (CONTROL_HEIGHT, 12., TEXT_BODY),
            ButtonSize::Sm => (28., 10., TEXT_SMALL),
        };
        let style = variant_style(&p, self.variant);
        let disabled = self.disabled;
        let (pressed, depth) = press_depth(&self.id, window, cx);
        // A press reads as scale(.97). GPUI cannot scale a div, so the face
        // insets inside its own footprint: margins give back what the size
        // loses, and the layout around it does not move.
        let inset_y = height * 0.015 * depth;
        // A full-width face only sinks vertically; side margins would overflow.
        let inset_x = if self.full_width { 0. } else { inset_y * 2.5 };
        // Pressed faces lose their lift and their highlight.
        let lift = 1. - depth;
        let mut shadows: Vec<BoxShadow> = style
            .shadows
            .iter()
            .map(|s| BoxShadow {
                color: s.color.opacity(s.color.a * lift),
                ..s.clone()
            })
            .collect();
        if depth > 0.01 && style.bg.is_some() {
            shadows.push(inset(0., 1., 0., hsla(0., 0., 0., 0.08 * depth)));
        }
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(if self.size == ButtonSize::Sm { 6. } else { 8. }))
            .h(px(height - 2. * inset_y))
            .my(px(inset_y))
            .mx(px(inset_x))
            .px(px(pad_x - inset_x))
            .rounded(px(RADIUS_LG))
            .when(self.full_width, |d| d.w_full().flex_1())
            .text_size(px(text))
            .font_weight(FontWeight::MEDIUM)
            .text_color(style.fg)
            .when_some(style.border, |d, b| d.border_1().border_color(b))
            .when(!shadows.is_empty() && !disabled, |d| d.shadow(shadows))
            .whitespace_nowrap();
        if let Some(bg) = style.bg {
            el = el.bg(bg);
        }
        if disabled {
            el = el.opacity(0.64);
        } else {
            el = el.cursor_pointer();
            if let Some(hover_bg) = style.hover_bg {
                el = el.hover(move |s| s.bg(hover_bg));
            }
            if let Some(f) = self.on_click {
                el = el.on_click(f);
            }
            let (down, up, up_out) = (pressed.clone(), pressed.clone(), pressed);
            el = el
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    set_pressed(&down, true, cx)
                })
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    set_pressed(&up, false, cx)
                })
                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                    set_pressed(&up_out, false, cx)
                });
        }
        el.when_some(self.icon, |d, (name, size)| {
            d.child(icon(name, size, style.icon))
        })
        .when_some(self.label, |d, label| d.child(label))
    }
}

/// An outer shadow.
pub fn drop(x: f32, y: f32, blur: f32, alpha: f32) -> BoxShadow {
    BoxShadow {
        color: hsla(0., 0., 0., alpha),
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: false,
    }
}

/// An inner shadow, used as the 1 px highlight along a control's edge.
pub fn inset(x: f32, y: f32, blur: f32, color: Hsla) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: true,
    }
}

/// Elevation of dialogs, the settings sheet and the command palette.
pub fn dialog_shadow(p: &Palette) -> Vec<BoxShadow> {
    let mut shadows = vec![BoxShadow {
        color: hsla(0., 0., 0., if p.dark { 0.65 } else { 0.3 }),
        offset: point(px(0.), px(24.)),
        blur_radius: px(64.),
        spread_radius: px(-24.),
        inset: false,
    }];
    if p.dark {
        shadows.push(inset(0., 1., 0., p.white.opacity(0.04)));
    }
    shadows
}

/// The scrim behind dialogs and sheets.
pub fn backdrop(p: &Palette) -> Hsla {
    if p.dark {
        hsla(0., 0., 0., 0.72)
    } else {
        hsla(240. / 360., 0.06, 0.1, 0.32)
    }
}

struct ButtonStyle {
    bg: Option<Hsla>,
    hover_bg: Option<Hsla>,
    fg: Hsla,
    icon: Hsla,
    border: Option<Hsla>,
    shadows: Vec<BoxShadow>,
}

fn variant_style(p: &Palette, variant: ButtonVariant) -> ButtonStyle {
    match variant {
        ButtonVariant::Primary => ButtonStyle {
            bg: Some(p.primary),
            hover_bg: Some(p.primary.opacity(0.9)),
            fg: p.primary_foreground,
            icon: p.primary_foreground,
            border: Some(p.primary),
            shadows: vec![
                BoxShadow {
                    color: p.primary.opacity(0.24),
                    ..drop(0., 1., 2., 0.)
                },
                inset(0., 1., 0., p.white.opacity(0.16)),
            ],
        },
        ButtonVariant::Outline => ButtonStyle {
            bg: Some(if p.dark {
                p.white.opacity(0.035)
            } else {
                p.popover
            }),
            hover_bg: Some(if p.dark {
                p.white.opacity(0.06)
            } else {
                p.muted.opacity(0.5)
            }),
            fg: p.foreground,
            icon: p.muted_foreground,
            border: Some(p.input_border),
            shadows: if p.dark {
                vec![inset(0., 1., 0., p.white.opacity(0.06))]
            } else {
                vec![
                    drop(0., 1., 2., 0.05),
                    inset(0., -1., 0., hsla(0., 0., 0., 0.04)),
                ]
            },
        },
        ButtonVariant::OutlineFlat => ButtonStyle {
            bg: None,
            hover_bg: Some(p.hover_wash(0.05)),
            fg: p.muted_foreground,
            icon: p.muted_foreground,
            border: Some(p.border),
            shadows: Vec::new(),
        },
        ButtonVariant::Ghost => ButtonStyle {
            bg: None,
            hover_bg: Some(p.hover_wash(if p.dark { 0.06 } else { 0.05 })),
            fg: p.foreground,
            icon: p.muted_foreground,
            border: None,
            shadows: Vec::new(),
        },
        ButtonVariant::Destructive => ButtonStyle {
            bg: Some(p.destructive),
            hover_bg: Some(p.destructive.opacity(0.9)),
            fg: p.white,
            icon: p.white,
            border: Some(p.destructive),
            shadows: vec![
                BoxShadow {
                    color: p.destructive.opacity(0.24),
                    ..drop(0., 1., 2., 0.)
                },
                inset(0., 1., 0., p.white.opacity(0.16)),
            ],
        },
    }
}

/// `SectionHeader`: a semibold title with an optional description.
pub fn section_header(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    p: &Palette,
) -> Div {
    div()
        .mb(px(10.))
        .px(px(4.))
        .child(
            div()
                .text_size(px(TEXT_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(px(LINE_TITLE))
                .text_color(p.foreground)
                .child(title.into()),
        )
        .when_some(description, |d, desc| {
            d.child(
                div()
                    .mt(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground.opacity(0.8))
                    .child(desc),
            )
        })
}

/// A rounded card with a soft border on a faint fill: the frame shared by
/// settings panels, the dictionary list and the Insights cards.
pub fn surface_card(p: &Palette) -> Div {
    div()
        .rounded(px(RADIUS_XL))
        .border_1()
        .border_color(p.border.opacity(if p.dark { 0.8 } else { 0.6 }))
        .bg(p.card.opacity(0.4))
}

/// `SettingsPanel`: a bordered group whose rows are separated by hairlines.
pub fn settings_panel(rows: Vec<AnyElement>, p: &Palette) -> Div {
    let divider = p.border.opacity(0.5);
    let mut panel = surface_card(p)
        .flex()
        .flex_col()
        .shadow(vec![drop(0., 1., 2., 0.05)]);
    for (index, row) in rows.into_iter().enumerate() {
        panel = panel.child(
            div()
                .when(index > 0, |d| d.border_t_1().border_color(divider))
                .child(row),
        );
    }
    panel
}

/// A titled settings group: a `section_header` over a `settings_panel`.
pub fn settings_section(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    rows: Vec<AnyElement>,
    p: &Palette,
) -> Div {
    div()
        .child(section_header(title, description, p))
        .child(settings_panel(rows, p))
}

/// `SettingsPanelRow` padding.
pub fn panel_row() -> Div {
    div().px(px(16.)).py(px(12.))
}

/// `SettingsRow`: label + description on the left, a control on the right.
pub fn settings_row(
    label: impl Into<SharedString>,
    description: Option<SharedString>,
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    panel_row().child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(32.))
            .child(label_block(label, description, p))
            .child(div().flex_none().child(control)),
    )
}

pub fn label_block(
    label: impl Into<SharedString>,
    description: Option<SharedString>,
    p: &Palette,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.foreground)
                .child(label.into()),
        )
        .when_some(description, |d, desc| {
            d.child(
                div()
                    .mt(px(2.))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground.opacity(0.8))
                    .child(desc),
            )
        })
}

/// A healthy state: a small success dot beside a muted label. Filled banners
/// stay reserved for states that need the user's attention.
pub fn status_label(label: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(TEXT_SMALL))
        .line_height(px(LINE_SMALL))
        .text_color(p.muted_foreground)
        .child(div().size(px(6.)).flex_none().rounded_full().bg(p.success))
        .child(label.into())
}

/// `SettingsField`: a label above a full-width control.
pub fn settings_field(
    label: impl Into<SharedString>,
    description: Option<AnyElement>,
    action: Option<AnyElement>,
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    let label = div()
        .text_size(px(TEXT_BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground)
        .child(label.into());
    field_with_label(label.into_any_element(), description, action, control, p)
}

/// A settings field whose label carries a protocol tag such as
/// `OpenAI · /chat/completions`, so users know what the server must speak.
pub fn settings_field_tagged(
    label: impl Into<SharedString>,
    tag: impl Into<SharedString>,
    description: Option<AnyElement>,
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    let label = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.foreground)
                .child(label.into()),
        )
        .child(
            div()
                .px(px(6.))
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(if p.dark { p.border_hover } else { p.border })
                .bg(if p.dark { p.surface_3 } else { p.muted })
                .font_family(FONT_MONO)
                .text_size(px(TEXT_CAPTION))
                .line_height(px(LINE_SMALL))
                .text_color(p.muted_foreground)
                .child(tag.into()),
        );
    field_with_label(label.into_any_element(), description, None, control, p)
}

fn field_with_label(
    label: AnyElement,
    description: Option<AnyElement>,
    action: Option<AnyElement>,
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    panel_row().child(
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.))
                    .child(div().flex_1().min_w_0().child(label).when_some(
                        description,
                        |d, desc| {
                            d.child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.muted_foreground.opacity(0.8))
                                    .child(desc),
                            )
                        },
                    ))
                    .when_some(action, |d, a| d.child(div().flex_none().child(a))),
            )
            .child(control),
    )
}

/// Which sidebar a [`nav_row`] sits on; the two follow different T3 Code row tokens.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavSurface {
    /// The true-black app sidebar (`[data-app-sidebar]`).
    App,
    /// The settings navigation on the card-toned `settings_sidebar`.
    Settings,
}

/// A sidebar row: muted until hovered, foreground and a raised fill when active.
pub fn nav_row(
    id: SharedString,
    icon_name: &'static str,
    label: impl Into<SharedString>,
    active: bool,
    surface: NavSurface,
    p: &Palette,
) -> Stateful<Div> {
    let (fg, icon_color) = if active {
        (p.foreground, p.foreground)
    } else {
        (p.muted_foreground, p.muted_foreground.opacity(0.85))
    };
    // Dark washes: the app sidebar uses 11% / 8% white over black; settings rows
    // use T3 Code's muted (3%) / accent (4%) white over the card tone.
    let (active_wash, hover_wash) = match surface {
        NavSurface::App => (0.09, 0.08),
        NavSurface::Settings => (0.03, 0.04),
    };
    let hover_bg = p.hover_wash(if p.dark { hover_wash } else { 0.04 });
    let hover_fg = p.foreground;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .w_full()
        .h(px(NAV_ROW_HEIGHT))
        .px(px(10.))
        .rounded(px(RADIUS_LG))
        .cursor_pointer()
        .text_size(px(TEXT_BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .when(active, |d| {
            if p.dark {
                d.bg(p.white.opacity(active_wash))
            } else {
                d.bg(p.card)
                    .border_1()
                    .border_color(p.border.opacity(0.7))
                    .shadow(vec![drop(0., 1., 2., 0.04)])
            }
        })
        .when(!active, |d| {
            d.hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        })
        .child(icon(icon_name, 16., icon_color))
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
}

/// The track of a segmented control: a 2 px inset on a faint wash. Callers
/// add their own height, gap and alignment.
pub fn segmented_track(p: &Palette) -> Div {
    div()
        .flex()
        .p(px(2.))
        .rounded(px(RADIUS_LG))
        .bg(p.foreground.opacity(if p.dark { 0.06 } else { 0.045 }))
}

/// One option in a `segmented_track`: raised with foreground text when
/// active, muted until hovered otherwise. Callers add height, padding on the
/// vertical axis, gap, children and the click handler.
pub fn segment(id: impl Into<ElementId>, active: bool, p: &Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .px(px(10.))
        .rounded(px(RADIUS_MD))
        .text_size(px(TEXT_SMALL))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(active, |d| {
            d.text_color(p.foreground).map(|d| {
                if p.dark {
                    d.bg(p.white.opacity(0.1))
                } else {
                    d.bg(p.card).shadow(vec![drop(0., 1., 2., 0.08)])
                }
            })
        })
        .when(!active, |d| {
            d.text_color(p.muted_foreground)
                .hover(|s| s.text_color(p.foreground))
        })
}

/// The amber notice shown while data retention is off. `line_height` pins
/// the message's line height; `None` keeps the inherited one.
pub fn retention_banner(
    message: impl Into<SharedString>,
    line_height: Option<f32>,
    p: &Palette,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(p.amber_border)
        .bg(p.amber_bg)
        .child(
            div()
                .text_size(px(TEXT_BODY))
                .text_color(p.amber_icon)
                .child("⊘"),
        )
        .child(
            div()
                .text_size(px(TEXT_SMALL))
                .when_some(line_height, |d, h| d.line_height(px(h)))
                .text_color(p.amber_text)
                .child(message.into()),
        )
}

/// The 36 px raised tile that holds an empty state's icon.
pub fn empty_state_icon(name: &str, p: &Palette) -> Div {
    div()
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(p.border)
        .bg(p.card)
        .shadow(vec![drop(0., 1., 2., 0.05)])
        .child(icon(name, 16., p.muted_foreground))
}

/// A `<kbd>` chip: a flat 20 px tint, no border or shadow.
pub fn kbd(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(20.))
        .min_w(px(20.))
        .px(px(4.))
        .rounded(px(RADIUS_SM))
        .bg(p.foreground.opacity(0.07))
        .text_size(px(TEXT_SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.muted_foreground)
        .child(text.into())
}

/// A spinning loader icon.
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    icon("loader2", size, color).with_animation(
        id.into(),
        Animation::new(Duration::from_millis(800)).repeat(),
        |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
    )
}

/// Brightness of the shimmer band at `x` (0..1 across the text), `ms` into
/// its loop: the band crosses in the middle 70% of each 1.6 s loop and
/// rests off the edges for the rest.
pub fn shimmer_level(x: f32, ms: f32) -> f32 {
    let cycle = (ms % 1600.) / 1600.;
    let u = ((cycle - 0.15) / 0.7).clamp(0., 1.);
    let center = -1. / 6. + u * (4. / 3.);
    (1. - (x - center).abs() / 0.5).max(0.)
}

/// Muted text with a brighter band sweeping across it, for work in flight.
pub fn shimmer_text(
    text: impl Into<SharedString>,
    since: Instant,
    p: &Palette,
    window: &mut Window,
    cx: &App,
) -> StyledText {
    let text: SharedString = text.into();
    if cx.reduce_motion() {
        return StyledText::new(text);
    }
    window.request_animation_frame();
    let ms = since.elapsed().as_secs_f32() * 1000.;
    let count = text.chars().count().max(1) as f32;
    let highlights: Vec<_> = text
        .char_indices()
        .enumerate()
        .map(|(i, (at, ch))| {
            let level = shimmer_level((i as f32 + 0.5) / count, ms);
            let color = p.muted_foreground.blend(p.foreground.opacity(level));
            (
                at..at + ch.len_utf8(),
                HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                },
            )
        })
        .collect();
    StyledText::new(text).with_highlights(highlights)
}

/// A placeholder text line with a soft highlight sweeping across it every
/// 2 s; `index` staggers stacked lines by 90 ms.
pub fn skeleton_line(width: f32, index: usize, since: Instant, p: &Palette, cx: &App) -> Div {
    let wash = p.hover_wash(0.06);
    let line = div()
        .relative()
        .h(px(10.))
        .w(relative(width))
        .rounded(px(RADIUS_SM))
        .bg(wash)
        .overflow_hidden();
    if cx.reduce_motion() {
        return line;
    }
    let delay = 150. + index as f32 * 90.;
    let ms = since.elapsed().as_secs_f32() * 1000. - delay;
    if ms < 0. {
        return line;
    }
    let u = (ms % 2000.) / 2000.;
    let glow = p.hover_wash(0.07);
    let half = |left: f32, from: Hsla, to: Hsla| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(left))
            .w(relative(0.15))
            .bg(linear_gradient(
                90.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ))
    };
    line.child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w_full()
            .left(relative(-1. + 2. * u))
            .child(half(0.35, glow.opacity(0.), glow))
            .child(half(0.5, glow, glow.opacity(0.))),
    )
}

#[cfg(test)]
mod tests {
    use super::shimmer_level;

    #[test]
    fn shimmer_rests_off_the_edges_and_crosses_the_text() {
        // Resting before the sweep: the band sits left of the text.
        assert_eq!(shimmer_level(0.5, 100.), 0.);
        // Mid-sweep the middle of the text is brightest.
        assert!((shimmer_level(0.5, 800.) - 1.).abs() < 1e-3);
        assert!(shimmer_level(0.1, 800.) < shimmer_level(0.5, 800.));
        // Resting after the sweep: right of the text.
        assert_eq!(shimmer_level(0.5, 1500.), 0.);
        assert!(shimmer_level(1., 1500.) > 0.);
    }
}
