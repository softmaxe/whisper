//! Styled wrappers around GPUI Kit's editable text inputs and textareas.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::color_tx;
use super::theme::{FONT_MONO, LINE_SMALL, PaletteExt, TEXT_BODY, TEXT_SMALL};

#[derive(Clone, Copy, PartialEq)]
pub enum FieldKind {
    /// `h-8 text-[13px]` single-line input.
    Line,
    /// Multi-line textarea (`rounded-xl px-3.5 py-3`).
    Area { min_height: f32 },
}

pub fn text_field(
    state: &Entity<InputState>,
    kind: FieldKind,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    text_field_sized(state, TEXT_BODY, kind, mono, window, cx)
}

pub fn text_field_sized(
    state: &Entity<InputState>,
    text_size: f32,
    kind: FieldKind,
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
        kind,
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
    let border = if focused {
        if p.dark {
            p.muted_foreground
        } else {
            p.primary
        }
    } else if p.dark {
        p.border_subtle.opacity(0.6)
    } else {
        p.border.opacity(0.7)
    };
    let border = color_tx(("field-border", id), border, 200, window, cx);
    let bg = if p.dark { p.surface_1 } else { p.input };
    let ring = if focused {
        Some(if p.dark {
            p.ring.opacity(0.1)
        } else {
            p.primary.opacity(0.1)
        })
    } else {
        None
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
        .when_some(ring, |d, ring| {
            d.shadow(vec![BoxShadow {
                color: ring,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(2.),
                inset: false,
            }])
        })
        .text_color(p.foreground)
        .when(mono, |d| d.font_family(FONT_MONO))
        .map(|d| match kind {
            FieldKind::Line => d
                .h(px(32.))
                .rounded(px(4.))
                .px(px(4.))
                .text_size(px(TEXT_BODY)),
            FieldKind::Area { min_height } => d
                .min_h(px(min_height))
                .items_start()
                .rounded(px(12.))
                .px(px(6.))
                .py(px(12.))
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL)),
        })
        .hover(move |s| {
            if focused {
                s
            } else {
                s.border_color(p.border_hover)
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
