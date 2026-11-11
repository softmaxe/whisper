//! Styled wrappers around GPUI Kit's editable text inputs and textareas.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{color_tx, drop};
use super::theme::{
    DUR_BASE, FONT_MONO, LINE_SMALL, PaletteExt, RADIUS_LG, RADIUS_XL, ROW_CONTROL_HEIGHT,
    TEXT_BODY, TEXT_MONO_BODY, TEXT_MONO_SMALL, TEXT_SMALL,
};

/// JetBrains Mono reads a step larger than SF, so mono fields drop a step.
fn field_size(size: f32, mono: bool) -> f32 {
    match (mono, size == TEXT_BODY) {
        (false, _) => size,
        (true, true) => TEXT_MONO_BODY,
        (true, false) => TEXT_MONO_SMALL,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum FieldKind {
    /// `h-8 rounded-lg` single-line input.
    Line,
    /// Multi-line textarea (`rounded-xl px-3.5 py-3`).
    Area { min_height: f32 },
}

#[derive(Clone, Copy, PartialEq)]
enum FieldState {
    Idle,
    Focused,
    /// Takes no input and no hover; the caller dims the group around it.
    Disabled,
}

impl FieldState {
    fn from_focus(focused: bool) -> Self {
        if focused { Self::Focused } else { Self::Idle }
    }
}

pub fn text_field(
    state: &Entity<InputState>,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    line_field(state, TEXT_BODY, mono, false, window, cx)
}

/// A body-size field that can be switched off: it takes no focus or edits
/// and drops its hover, while the caller dims the group it belongs to.
pub fn text_field_disabled(
    state: &Entity<InputState>,
    mono: bool,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    line_field(state, TEXT_BODY, mono, disabled, window, cx)
}

/// A single-line field with small (12 px) text.
pub fn text_field_small(
    state: &Entity<InputState>,
    mono: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    line_field(state, TEXT_SMALL, mono, false, window, cx)
}

fn line_field(
    state: &Entity<InputState>,
    text_size: f32,
    mono: bool,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let id = state.entity_id().as_u64() as usize;
    // Size::Size(n) renders n × 0.875 text with 8px of horizontal padding.
    let input = Input::new(state)
        .appearance(false)
        .disabled(disabled)
        .with_size(Size::Size(px(field_size(text_size, mono) / 0.875)));
    field_frame(
        id,
        if disabled {
            FieldState::Disabled
        } else {
            FieldState::from_focus(focused)
        },
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
        FieldState::from_focus(focused),
        FieldKind::Area { min_height },
        mono,
        Textarea::new(state)
            .appearance(false)
            .with_size(Size::Size(px(field_size(TEXT_SMALL, mono) / 0.875)))
            .h(px(min_height - 26.))
            .into_any_element(),
        window,
        cx,
    )
}

fn field_frame(
    id: usize,
    state: FieldState,
    kind: FieldKind,
    mono: bool,
    editor: AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let p = cx.palette();
    let focused = state == FieldState::Focused;
    let disabled = state == FieldState::Disabled;
    let border = if focused { p.ring } else { p.input_border };
    let border = color_tx(("field-border", id), border, DUR_BASE, window, cx);
    let bg = p.field_background;
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
        .when(mono, |d| {
            d.font_family(FONT_MONO)
                .font_features(crate::ui::theme::mono_features())
        })
        .map(|d| match kind {
            FieldKind::Line => d
                .h(px(ROW_CONTROL_HEIGHT))
                .rounded(px(RADIUS_LG))
                .px(px(4.))
                .text_size(px(field_size(TEXT_BODY, mono))),
            FieldKind::Area { min_height } => d
                .min_h(px(min_height))
                .items_start()
                .rounded(px(RADIUS_XL))
                .px(px(6.))
                .py(px(12.))
                .text_size(px(field_size(TEXT_SMALL, mono)))
                .line_height(px(LINE_SMALL)),
        })
        .hover(move |s| {
            if focused || disabled {
                s
            } else {
                s.border_color(p.field_hover_border)
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
