//! Styled wrappers around GPUI Kit's editable text inputs and textareas.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{color_tx, drop};
use super::theme::{
    CONTROL_HEIGHT, DUR_BASE, FONT_MONO, LINE_SMALL, PaletteExt, RADIUS_LG, RADIUS_XL, TEXT_BODY,
    TEXT_SMALL,
};

#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    /// `h-8 rounded-lg` single-line input.
    Line,
    /// Multi-line textarea (`rounded-xl px-3.5 py-3`).
    Area { min_height: f32 },
}

pub fn text_field(
    state: &Entity<InputState>,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    line_field(state, TEXT_BODY, mono, window, cx)
}

/// A single-line field with small (12 px) text.
pub fn text_field_small(
    state: &Entity<InputState>,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    line_field(state, TEXT_SMALL, mono, window, cx)
}

fn line_field(
    state: &Entity<InputState>,
    text_size: f32,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let id = state.entity_id().as_u64() as usize;
    // Size::Size(n) renders n × 0.875 text with 8px of horizontal padding.
    let input = Input::new(state)
        .appearance(false)
        .with_size(Size::Size(px(text_size / 0.875)));
    field_frame(
        id,
        focused,
        FieldKind::Line,
        mono,
        input.into_any_element(),
        window,
        cx,
    )
}

pub fn textarea_field(
    state: &Entity<TextareaState>,
    min_height: f32,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let id = state.entity_id().as_u64() as usize;
    field_frame(
        id,
        focused,
        FieldKind::Area { min_height },
        mono,
        Textarea::new(state)
            .appearance(false)
            .with_size(Size::Size(px(TEXT_SMALL / 0.875)))
            .h(px(min_height - 26.))
            .into_any_element(),
        window,
        cx,
    )
}

fn field_frame(
    id: usize,
    focused: bool,
    kind: FieldKind,
    mono: bool,
    editor: AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let p = cx.palette();
    let border = if focused { p.ring } else { p.input_border };
    let border = color_tx(("field-border", id), border, DUR_BASE, window, cx);
    let bg = if p.dark {
        p.white.opacity(0.035)
    } else {
        p.input
    };
    // Focus draws a 3 px ring outside the border; at rest a faint lift.
    let shadow = if focused {
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
    let is_area = matches!(kind, FieldKind::Area { .. });
    div()
        .id(("text-field", id))
        .w_full()
        .flex()
        .items_center()
        .bg(bg)
        .border_1()
        .border_color(border)
        .when(!shadow.is_empty(), |d| d.shadow(shadow))
        .text_color(p.foreground)
        .when(mono, |d| d.font_family(FONT_MONO))
        .map(|d| match kind {
            FieldKind::Line => d
                .h(px(CONTROL_HEIGHT))
                .rounded(px(RADIUS_LG))
                .px(px(4.))
                .text_size(px(TEXT_BODY)),
            FieldKind::Area { min_height } => d
                .min_h(px(min_height))
                .items_start()
                .rounded(px(RADIUS_XL))
                .px(px(6.))
                .py(px(12.))
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL)),
        })
        .hover(move |s| {
            if focused {
                s
            } else {
                s.border_color(if p.dark {
                    p.white.opacity(0.18)
                } else {
                    p.border_hover.blend(p.foreground.opacity(0.08))
                })
            }
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .when(is_area, |d| d.size_full())
                .child(editor),
        )
}
