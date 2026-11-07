//! Small presentational building blocks shared by the windows.

use std::time::{Duration, Instant};
use std::{cell::Cell, rc::Rc};

use gpui_kit::base::{Transition, transition};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::theme::{
    CONTROL_HEIGHT, DUR_FAST, DUR_SLOW, DUR_SNAP, LINE_SMALL, LINE_TITLE, NAV_ROW_HEIGHT,
    PAGE_GUTTER, PAGE_MAX_WIDTH, PAGE_TOP, Palette, PaletteExt, RADIUS_LG, RADIUS_MD, RADIUS_SM,
    RADIUS_XL, ROW_CONTROL_HEIGHT, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, ease_css,
    ease_out, spring,
};

pub fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// Keeps a menu's trigger out of its outside-dismiss region. Outside mouse
/// downs run during capture, before the trigger's click toggles the menu.
#[derive(Clone, Default)]
pub struct MenuTriggerBounds(Rc<Cell<Bounds<Pixels>>>);

impl MenuTriggerBounds {
    /// Place inside a relatively positioned trigger or its tight wrapper.
    pub fn measure(&self) -> impl IntoElement + use<> {
        let bounds = self.0.clone();
        canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
    }

    pub fn contains(&self, position: Point<Pixels>) -> bool {
        self.0.get().contains(&position)
    }

    pub fn width(&self) -> Pixels {
        self.0.get().size.width
    }
}

/// The keyboard focus ring: a 2 px ring color band just inside the
/// element's edge, following its corners, shown only when focus arrived
/// from the keyboard. It is inset because GPUI paints an outer shadow as a
/// filled shape under the element, which would flood a transparent control.
pub fn focus_ring(p: &Palette) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: p.ring.opacity(if p.dark { 0.9 } else { 0.75 }),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: true,
    }]
}

/// The focus ring for opaque filled controls (accent buttons, an on
/// switch): a ring color halo just outside the edge, parted from the fill
/// by a 1 px gap in the canvas color, as macOS draws it. An outer shadow is
/// safe here because the opaque fill hides the part GPUI paints beneath.
pub fn focus_ring_on_fill(p: &Palette) -> Vec<BoxShadow> {
    let ring = |color: Hsla, spread: f32| BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(spread),
        inset: false,
    };
    vec![
        ring(p.ring.opacity(if p.dark { 0.9 } else { 0.8 }), 3.),
        ring(p.background, 1.),
    ]
}

/// Focus handles of [`KeyboardFocusable`] elements, by key. Kept app-wide so
/// render helpers that only hold an `App` can make elements focusable.
#[derive(Default)]
struct FocusHandles(std::collections::HashMap<ElementId, FocusHandle>);

impl Global for FocusHandles {}

/// The focus handle stored under `key`, created on first use.
pub fn keyed_focus_handle(key: ElementId, cx: &mut App) -> FocusHandle {
    if let Some(handle) = cx.default_global::<FocusHandles>().0.get(&key) {
        return handle.clone();
    }
    let handle = cx.focus_handle();
    cx.global_mut::<FocusHandles>()
        .0
        .insert(key, handle.clone());
    handle
}

/// Makes a clickable element reachable with Tab, activatable with Return or
/// Space (GPUI turns those into clicks on the focused element), named for
/// VoiceOver, and ringed while keyboard-focused. `key` must be unique in the
/// app.
pub trait KeyboardFocusable: StatefulInteractiveElement + Sized {
    fn keyboard_focusable(
        self,
        key: impl Into<ElementId>,
        role: Role,
        label: impl Into<SharedString>,
        cx: &mut App,
    ) -> Self {
        let ring = focus_ring(&cx.palette());
        self.keyboard_focusable_with_ring(key, role, label, ring, cx)
    }

    /// As [`keyboard_focusable`](Self::keyboard_focusable), with a caller's
    /// ring: filled accent controls use [`focus_ring_on_fill`], since an
    /// accent ring disappears on an accent fill.
    fn keyboard_focusable_with_ring(
        self,
        key: impl Into<ElementId>,
        role: Role,
        label: impl Into<SharedString>,
        ring: Vec<BoxShadow>,
        cx: &mut App,
    ) -> Self {
        let handle = keyed_focus_handle(key.into(), cx);
        self.track_focus(&handle.tab_stop(true))
            .role(role)
            .aria_label(label)
            .focus_visible(move |s| s.shadow(ring))
    }
}

impl<T: StatefulInteractiveElement + Sized> KeyboardFocusable for T {}

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

/// Pill switch: 36×20 track (close to the macOS regular switch), 16 px
/// knob, 120 ms slide.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    label: Option<SharedString>,
    on_change: Option<Box<dyn Fn(bool, &mut Window, &mut App) + 'static>>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            label: None,
            on_change: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The name VoiceOver reads; usually the settings row's label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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
        } else if p.dark {
            p.white.opacity(0.22)
        } else {
            p.foreground.opacity(0.2)
        };
        let track = color_tx((id.clone(), "track"), track, DUR_SNAP, window, cx);
        let knob_x = f32_tx(
            (id.clone(), "knob"),
            if self.checked { 18.0 } else { 2.0 },
            DUR_SNAP,
            ease_css,
            window,
            cx,
        );
        // The knob stays light in both themes, like the macOS switch, so the
        // off state reads as a control and not an empty slot.
        let knob = if self.disabled {
            p.muted_foreground.opacity(0.5)
        } else if p.dark {
            p.foreground
        } else {
            p.white
        };
        let checked = self.checked;
        let disabled = self.disabled;
        let on_change = self.on_change;
        let label = self.label.unwrap_or_default();
        div()
            .id(id.clone())
            .when(!disabled, |d| {
                let ring = if checked {
                    focus_ring_on_fill(&p)
                } else {
                    focus_ring(&p)
                };
                d.keyboard_focusable_with_ring(id.clone(), Role::Switch, label, ring, cx)
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
            })
            .relative()
            .flex_none()
            .w(px(36.))
            .h(px(20.))
            .rounded_full()
            .bg(track)
            // An off track is pale against its card; an inner edge keeps
            // its outline at about 3:1, as WCAG 1.4.11 asks.
            .when(!checked && !disabled, |d| {
                d.shadow(vec![BoxShadow {
                    color: if p.dark {
                        p.white.opacity(0.32)
                    } else {
                        p.foreground.opacity(0.4)
                    },
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: true,
                }])
            })
            .when(disabled, |d| d.opacity(0.5))
            .when(!disabled, |d| d.cursor_pointer())
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .left(px(knob_x))
                    .size(px(16.))
                    .rounded_full()
                    .bg(knob)
                    .when(!disabled, |d| {
                        d.shadow(vec![
                            drop(0., 1., 3., if p.dark { 0.3 } else { 0.25 }),
                            BoxShadow {
                                color: hsla(0., 0., 0., 0.12),
                                offset: point(px(0.), px(0.)),
                                blur_radius: px(0.),
                                spread_radius: px(0.5),
                                inset: false,
                            },
                        ])
                    }),
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
    icon_rotation: f32,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    full_width: bool,
    aria_label: Option<SharedString>,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            icon_rotation: 0.,
            variant: ButtonVariant::Primary,
            size: ButtonSize::Default,
            disabled: false,
            full_width: false,
            aria_label: None,
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

    /// Rotates only the icon, measured in turns.
    pub fn icon_rotation(mut self, turns: f32) -> Self {
        self.icon_rotation = turns;
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

    /// The VoiceOver name of an icon-only button.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
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
            ButtonSize::Sm => (ROW_CONTROL_HEIGHT, 10., TEXT_SMALL),
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
        let name = self
            .aria_label
            .clone()
            .or_else(|| self.label.clone())
            .unwrap_or_default();
        let mut el = div()
            .id(self.id.clone())
            .when(!disabled, |d| {
                let filled = matches!(
                    self.variant,
                    ButtonVariant::Primary | ButtonVariant::Destructive
                );
                let ring = if filled {
                    focus_ring_on_fill(&p)
                } else {
                    focus_ring(&p)
                };
                d.keyboard_focusable_with_ring(self.id.clone(), Role::Button, name, ring, cx)
            })
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
            el = el.opacity(0.45);
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
        // An icon-only button shows its name as a tooltip as well.
        let tip = self
            .label
            .is_none()
            .then(|| self.aria_label.clone())
            .flatten();
        el.when_some(tip, |d, tip| d.tooltip(text_tooltip(tip)))
            .when_some(self.icon, |d, (name, size)| {
                d.child(icon(name, size, style.icon).when(
                    self.icon_rotation != 0. && !cx.reduce_motion(),
                    |svg| {
                        svg.with_transformation(Transformation::rotate(percentage(
                            self.icon_rotation.rem_euclid(1.),
                        )))
                    },
                ))
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
        color: hsla(0., 0., 0., if p.dark { 0.65 } else { 0.34 }),
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
        hsla(240. / 360., 0.06, 0.1, 0.11)
    }
}

/// A destructive button's fill. Dark mode's destructive text color is bright
/// for legibility on black, too bright under white text, so dark fills use
/// a deeper red that keeps white labels at 4.5:1.
fn destructive_fill(p: &Palette) -> Hsla {
    if p.dark {
        Hsla::from(rgb(0xdc2626))
    } else {
        p.destructive
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
            // Light outline buttons sit a step off white so they keep their
            // shape inside white cards.
            bg: Some(if p.dark {
                p.white.opacity(0.035)
            } else {
                p.surface_1
            }),
            hover_bg: Some(if p.dark {
                p.white.opacity(0.06)
            } else {
                p.muted
            }),
            fg: p.foreground,
            icon: p.muted_foreground,
            // Buttons carry their own fill and lift; the stronger input
            // edge is for fields, where the outline is the only cue.
            border: Some(p.control_border),
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
            bg: Some(destructive_fill(p)),
            hover_bg: Some(destructive_fill(p).opacity(0.9)),
            fg: p.white,
            icon: p.white,
            border: Some(destructive_fill(p)),
            shadows: vec![
                BoxShadow {
                    color: destructive_fill(p).opacity(0.24),
                    ..drop(0., 1., 2., 0.)
                },
                inset(0., 1., 0., p.white.opacity(0.16)),
            ],
        },
    }
}

/// Helper text wraps at about 90 characters instead of spanning a wide card.
pub const DESCRIPTION_MAX_WIDTH: f32 = 560.;

/// `SectionHeader`: a semibold title with an optional description.
pub fn section_header(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    p: &Palette,
) -> Div {
    div()
        .mb(px(10.))
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
                    .max_w(px(DESCRIPTION_MAX_WIDTH))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.subtle_foreground)
                    .child(desc),
            )
        })
}

/// A one-line tooltip on the popover surface.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(RADIUS_MD))
            .bg(p.popover)
            .border_1()
            .border_color(p.border)
            .shadow(vec![drop(0., 4., 12., if p.dark { 0.4 } else { 0.08 })])
            .text_size(px(TEXT_SMALL))
            .text_color(p.foreground)
            .child(self.0.clone())
    }
}

/// A tooltip builder for `.tooltip(...)`: names icon-only controls for
/// pointer users, as `aria_label` does for VoiceOver.
pub fn text_tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

/// Where a scroll area's thumb sits: its top and height inside the area, or
/// `None` when nothing overflows. Long pages show it at rest so a clipped
/// last row reads as "more below".
pub fn scroll_thumb(scroll: &ScrollHandle) -> Option<(Pixels, Pixels)> {
    const INSET: f32 = 3.;
    const MIN_THUMB: f32 = 32.;
    let viewport = f32::from(scroll.bounds().size.height);
    let max = f32::from(scroll.max_offset().y);
    if max < 1. || viewport <= 2. * INSET + MIN_THUMB {
        return None;
    }
    let track = viewport - 2. * INSET;
    let height = (track * viewport / (viewport + max)).max(MIN_THUMB);
    let progress = (-f32::from(scroll.offset().y) / max).clamp(0., 1.);
    Some((px(INSET + (track - height) * progress), px(height)))
}

/// The shared page column: centered, [`PAGE_MAX_WIDTH`] wide, with the
/// standard gutter and top inset. Pages add their own bottom padding.
pub fn page_container() -> Div {
    div()
        .w_full()
        .max_w(px(PAGE_MAX_WIDTH))
        .mx_auto()
        .px(px(PAGE_GUTTER))
        .pt(px(PAGE_TOP))
}

/// A rounded card with a soft border on a faint fill: the frame shared by
/// settings panels, the dictionary list and the Insights cards.
pub fn surface_card(p: &Palette) -> Div {
    div().rounded(px(RADIUS_XL)).border_1().map(|d| {
        if p.dark {
            // The full card tone, so groups read as surfaces on black and
            // not only as hairlines.
            d.border_color(p.border).bg(p.card)
        } else {
            // Light cards are white on the off-white canvas with a full
            // hairline, so groups separate without relying on the tint.
            d.border_color(p.border).bg(p.card)
        }
    })
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
                    .max_w(px(DESCRIPTION_MAX_WIDTH))
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.subtle_foreground)
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
    control: impl IntoElement,
    p: &Palette,
) -> Div {
    let label = div()
        .text_size(px(TEXT_BODY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground)
        .child(label.into());
    field_with_label(label.into_any_element(), description, control, p)
}

/// A settings field whose label carries a protocol note such as
/// "OpenAI-compatible · no auth", so users know what the server must speak.
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
        // Plain metadata in the UI face: a bordered chip would read as one
        // more clickable preset, and the tag is prose, not a code value.
        .child(
            div()
                .text_size(px(TEXT_CAPTION))
                .line_height(px(LINE_SMALL))
                .text_color(p.muted_foreground)
                .child(tag.into()),
        );
    field_with_label(label.into_any_element(), description, control, p)
}

fn field_with_label(
    label: AnyElement,
    description: Option<AnyElement>,
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
                    .min_w_0()
                    .child(label)
                    .when_some(description, |d, desc| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .max_w(px(DESCRIPTION_MAX_WIDTH))
                                .text_size(px(TEXT_SMALL))
                                .line_height(px(LINE_SMALL))
                                .text_color(p.subtle_foreground)
                                .child(desc),
                        )
                    }),
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
    cx: &mut App,
) -> Stateful<Div> {
    let p = &cx.palette();
    let label: SharedString = label.into();
    let (fg, icon_color) = if active {
        (p.foreground, p.foreground)
    } else {
        (p.muted_foreground, p.muted_foreground.opacity(0.85))
    };
    // Dark washes (active / hover): the selected row always reads stronger
    // than a hovered one.
    let (active_wash, hover_wash) = match surface {
        NavSurface::App => (0.1, 0.07),
        NavSurface::Settings => (0.08, 0.05),
    };
    let hover_bg = p.hover_wash(if p.dark { hover_wash } else { 0.04 });
    let hover_fg = p.foreground;
    div()
        .id(id.clone())
        .keyboard_focusable(id.clone(), Role::Tab, label.clone(), cx)
        .aria_selected(active)
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
        // Every row carries a 1 px border, transparent unless active, so the
        // active row's outline never shifts its label.
        .border_1()
        .border_color(transparent_black())
        .when(active, |d| {
            if p.dark {
                d.bg(p.white.opacity(active_wash))
            } else {
                d.bg(p.card)
                    .border_color(p.border_hover)
                    .shadow(vec![drop(0., 1., 2., 0.05)])
            }
        })
        .group(id.clone())
        .when(!active, |d| {
            d.hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        })
        // The icon brightens with the label on hover.
        .child(
            icon(icon_name, 16., icon_color)
                .group_hover(id.clone(), move |s| s.text_color(hover_fg)),
        )
        .child(div().flex_1().min_w_0().truncate().child(label))
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
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &mut App,
) -> Stateful<Div> {
    let p = &cx.palette();
    let id: ElementId = id.into();
    div()
        .id(id.clone())
        .keyboard_focusable(id, Role::RadioButton, label, cx)
        .aria_toggled(if active {
            Toggled::True
        } else {
            Toggled::False
        })
        // One height for every segmented control: with the track's 2 pt
        // inset it stands 28 pt, level with small buttons.
        .h(px(24.))
        .flex()
        .items_center()
        .px(px(10.))
        .rounded(px(RADIUS_MD))
        .text_size(px(TEXT_SMALL))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        // A transparent border on every segment, so the active one's edge
        // never shifts its label.
        .border_1()
        .border_color(transparent_black())
        .when(active, |d| {
            d.text_color(p.foreground).map(|d| {
                if p.dark {
                    d.bg(p.white.opacity(0.15))
                        .border_color(p.white.opacity(0.08))
                } else {
                    d.bg(p.card)
                        .border_color(p.border_hover)
                        .shadow(vec![drop(0., 1., 2., 0.08)])
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

/// The one empty-state layout every page uses: the icon tile, a semibold
/// title and a short muted body, centered. Callers append actions or hints.
pub fn empty_state(
    icon_name: &str,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    p: &Palette,
) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .child(empty_state_icon(icon_name, p))
        .child(
            div()
                .mt(px(16.))
                .text_size(px(TEXT_TITLE))
                .line_height(px(LINE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.foreground)
                .child(title.into()),
        )
        .child(
            div()
                .mt(px(4.))
                .max_w(px(340.))
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL))
                .text_color(p.muted_foreground)
                .child(body.into()),
        )
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

/// A spinning loader icon. Under Reduce motion it holds still, since
/// `with_animation` would otherwise keep it turning.
pub fn spinner(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    Spinner {
        id: id.into(),
        size,
        color,
    }
}

#[derive(IntoElement)]
struct Spinner {
    id: ElementId,
    size: f32,
    color: Hsla,
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let loader = icon("loader2", self.size, self.color);
        if cx.reduce_motion() {
            return loader.into_any_element();
        }
        loader
            .with_animation(
                self.id,
                Animation::new(Duration::from_millis(800)).repeat(),
                |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
            )
            .into_any_element()
    }
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
