//! Small presentational building blocks shared by the windows.

use std::time::{Duration, Instant};

use gpui_kit::base::{Transition, transition};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::theme::{
    DUR_FAST, DUR_SLOW, DUR_SNAP, FONT_MONO, LINE_SMALL, LINE_TITLE, Palette, PaletteExt,
    TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, ease_css, ease_out, spring,
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

/// Pill switch: 34×20 track, 14 px knob, 120 ms slide.
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
        } else if p.dark {
            p.surface_raised
        } else {
            p.muted_foreground.opacity(0.3)
        };
        let track = color_tx((id.clone(), "track"), track, DUR_SNAP, window, cx);
        let knob_x = f32_tx(
            (id.clone(), "knob"),
            if self.checked { 17.0 } else { 3.0 },
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
            .w(px(34.))
            .h(px(20.))
            .rounded_full()
            .bg(track)
            .when(disabled, |d| d.opacity(0.5))
            .when(!disabled, |d| d.cursor_pointer())
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .left(px(knob_x))
                    .size(px(14.))
                    .rounded_full()
                    .bg(knob)
                    .when(!disabled, |d| d.shadow_sm()),
            )
            .when_some(on_change.filter(|_| !disabled), |d, f| {
                d.on_click(move |_, window, cx| f(!checked, window, cx))
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Brand-blue glass in light mode; near-white in dark mode.
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

/// Rounded-full buttons in several variants.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<(&'static str, f32)>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    full_width: bool,
    height: Option<f32>,
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
            height: None,
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

    pub fn h(mut self, height: f32) -> Self {
        self.height = Some(height);
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
            ButtonSize::Default => (36., 16., TEXT_BODY),
            ButtonSize::Sm => (30., 12., TEXT_SMALL),
        };
        let height = self.height.unwrap_or(height);
        let (bg, hover_bg, fg, border, weight) = variant_colors(&p, self.variant);
        let disabled = self.disabled;
        let (pressed, depth) = press_depth(&self.id, window, cx);
        // A press reads as scale(.975). GPUI cannot scale a div, so the face
        // insets inside its own footprint: margins give back what the size
        // loses, and the layout around it does not move.
        let inset_y = height * 0.0125 * depth;
        // A full-width face only sinks vertically; side margins would overflow.
        let inset_x = if self.full_width { 0. } else { inset_y * 2.5 };
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
            .rounded_full()
            .when(self.full_width, |d| d.w_full().flex_1())
            .text_size(px(text))
            .font_weight(weight)
            .text_color(fg)
            .when_some(border, |d, b| d.border_1().border_color(b))
            .whitespace_nowrap();
        if let Some(bg) = bg {
            el = el.bg(bg);
        }
        if disabled {
            el = el.opacity(0.5);
        } else {
            el = el.cursor_pointer();
            if let Some(hover_bg) = hover_bg {
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
        el.when_some(self.icon, |d, (name, size)| d.child(icon(name, size, fg)))
            .when_some(self.label, |d, label| d.child(label))
    }
}

fn variant_colors(
    p: &Palette,
    variant: ButtonVariant,
) -> (Option<Hsla>, Option<Hsla>, Hsla, Option<Hsla>, FontWeight) {
    match variant {
        ButtonVariant::Primary => (
            Some(p.primary),
            Some(p.primary.opacity(0.9)),
            p.primary_foreground,
            None,
            FontWeight::SEMIBOLD,
        ),
        ButtonVariant::Outline => {
            if p.dark {
                (
                    Some(p.white.opacity(0.035)),
                    Some(p.white.opacity(0.07)),
                    p.foreground,
                    Some(p.border_hover),
                    FontWeight::MEDIUM,
                )
            } else {
                (
                    Some(p.popover),
                    Some(p.muted.opacity(0.5)),
                    p.foreground,
                    Some(p.border_hover),
                    FontWeight::MEDIUM,
                )
            }
        }
        ButtonVariant::OutlineFlat => (
            None,
            Some(p.hover_wash(0.05)),
            p.muted_foreground.opacity(0.7),
            Some(if p.dark {
                p.white.opacity(0.1)
            } else {
                p.border.opacity(0.7)
            }),
            FontWeight::MEDIUM,
        ),
        ButtonVariant::Ghost => (
            None,
            Some(if p.dark {
                p.white.opacity(0.08)
            } else {
                p.muted
            }),
            if p.dark {
                p.foreground.opacity(0.9)
            } else {
                p.foreground
            },
            None,
            FontWeight::MEDIUM,
        ),
        ButtonVariant::Destructive => (
            Some(p.destructive),
            Some(p.destructive.opacity(0.9)),
            p.white,
            Some(p.destructive.opacity(0.7)),
            FontWeight::SEMIBOLD,
        ),
    }
}

/// `SectionHeader`: a semibold title with an optional description.
pub fn section_header(
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    p: &Palette,
) -> Div {
    div()
        .mb(px(12.))
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

/// `SettingsPanel`: a bordered group whose rows are separated by hairlines.
pub fn settings_panel(rows: Vec<AnyElement>, p: &Palette) -> Div {
    let divider = if p.dark {
        p.border_subtle.opacity(0.5)
    } else {
        p.border.opacity(0.6)
    };
    let mut panel = div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(if p.dark {
            p.border_subtle.opacity(0.7)
        } else {
            p.border.opacity(0.7)
        })
        .bg(if p.dark {
            p.surface_2.opacity(0.5)
        } else {
            p.card.opacity(0.5)
        });
    for (index, row) in rows.into_iter().enumerate() {
        panel = panel.child(
            div()
                .when(index > 0, |d| d.border_t_1().border_color(divider))
                .child(row),
        );
    }
    panel
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
            .gap(px(16.))
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
                .rounded(px(4.))
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

/// A `<kbd>` chip.
pub fn kbd(text: impl Into<SharedString>, p: &Palette) -> Div {
    div()
        .rounded(px(6.))
        .bg(if p.dark { p.surface_2 } else { p.background })
        .px(px(6.))
        .py(px(1.))
        .font_family(FONT_MONO)
        .text_size(px(TEXT_CAPTION))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground.opacity(0.8))
        .shadow_sm()
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
        .rounded(px(4.))
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
