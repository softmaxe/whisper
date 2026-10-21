//! The Dictation hotkey editor: a stack of capturable hotkey rows plus "Add
//! another hotkey".

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::{t, t_with};
use crate::settings::DEFAULT_HOTKEY;

use crate::app_state::AppState;
use crate::backend::{self, RawKeyEvent};
use crate::text::hotkeys::{
    Hotkey, ListenerKey, Modifier, Side, format_hotkey_list_label, parse_hotkey_list,
    serialize_hotkey_list,
};

use super::components::{Button, ButtonSize, ButtonVariant, icon};
use super::overlays::{Toast, ToastVariant, show_toast};
use super::theme::{FONT_MONO, PaletteExt, TEXT_BODY, TEXT_SMALL, TEXT_TITLE};

const MODIFIER_ONLY_HOLD_THRESHOLD: Duration = Duration::from_millis(200);
const WARNING_TIMEOUT: Duration = Duration::from_secs(4);

/// Which row is capturing: an existing entry or the trailing "new" row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Existing(usize),
    New,
}

#[derive(Default)]
struct Capture {
    /// Modifier kinds held, in display order (ctrl, meta, alt, shift).
    held: [bool; 4],
    /// Right-side modifiers reported by the native listener.
    right_held: Vec<Modifier>,
    fn_held: bool,
    fn_used: bool,
    pressed_at: Option<Instant>,
}

pub struct HotkeyListInput {
    items: Vec<Hotkey>,
    adding: bool,
    capturing: Option<Slot>,
    capture: Capture,
    warning: Option<String>,
    warning_task: Option<Task<()>>,
    registering: bool,
    focus: FocusHandle,
    _raw_keys: Option<Task<()>>,
}

/// Modifier kinds in display order (ctrl, meta, alt, shift).
const MODIFIERS: [Modifier; 4] = [
    Modifier::Control,
    Modifier::Command,
    Modifier::Option,
    Modifier::Shift,
];

impl HotkeyListInput {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let items = parse_hotkey_list(&AppState::settings(cx).effective_dictation_key());
        let state = AppState::global(cx);
        cx.observe(&state, |this: &mut Self, state, cx| {
            if this.capturing.is_none() && !this.registering {
                let incoming =
                    parse_hotkey_list(&state.read(cx).settings.effective_dictation_key());
                if incoming != this.items {
                    this.items = incoming;
                    cx.notify();
                }
            }
        })
        .detach();
        let raw_keys = backend::raw_key_events().map(|rx| {
            cx.spawn(async move |this, cx| {
                while let Ok(key) = rx.recv().await {
                    if this
                        .update(cx, |this, cx| this.on_raw_key(key, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        Self {
            items,
            adding: false,
            capturing: None,
            capture: Capture::default(),
            warning: None,
            warning_task: None,
            registering: false,
            focus: cx.focus_handle(),
            _raw_keys: raw_keys,
        }
    }

    fn start_capture(&mut self, slot: Slot, window: &mut Window, cx: &mut Context<Self>) {
        if self.registering {
            return;
        }
        self.capturing = Some(slot);
        self.capture = Capture::default();
        self.warning = None;
        window.focus(&self.focus, cx);
        backend::set_hotkey_listening_mode(true);
        cx.notify();
    }

    fn stop_capture(&mut self, cx: &mut Context<Self>) {
        if self.capturing.take().is_some() {
            backend::set_hotkey_listening_mode(false);
        }
        self.adding = false;
        self.capture = Capture::default();
        self.warning = None;
        cx.notify();
    }

    fn reject(&mut self, message: String, cx: &mut Context<Self>) {
        self.warning = Some(message);
        self.capture = Capture::default();
        self.warning_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WARNING_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                this.warning = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn finalize(&mut self, hotkey: Hotkey, cx: &mut Context<Self>) {
        let Some(slot) = self.capturing else { return };
        let exclude = match slot {
            Slot::Existing(i) => Some(i),
            Slot::New => None,
        };
        if self
            .items
            .iter()
            .enumerate()
            .any(|(i, h)| Some(i) != exclude && *h == hotkey)
        {
            self.reject(t("hotkeyInput.duplicate"), cx);
            return;
        }
        if let Some(message) = backend::validate_hotkey(&hotkey) {
            self.reject(message, cx);
            return;
        }
        let mut next = self.items.clone();
        match slot {
            Slot::Existing(i) if i < next.len() => next[i] = hotkey,
            _ => next.push(hotkey),
        }
        self.stop_capture(cx);
        self.commit(next, cx);
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.items.len() <= 1 {
            return;
        }
        let mut next = self.items.clone();
        next.remove(index);
        self.commit(next, cx);
    }

    fn reset_to_default(&mut self, cx: &mut Context<Self>) {
        self.commit(parse_hotkey_list(DEFAULT_HOTKEY), cx);
    }

    /// Applies the list optimistically and rolls back when registration fails.
    fn commit(&mut self, next: Vec<Hotkey>, cx: &mut Context<Self>) {
        let previous = std::mem::replace(&mut self.items, next.clone());
        let list = serialize_hotkey_list(&next);
        self.registering = true;
        cx.notify();
        match backend::register_hotkeys(&list, cx) {
            Ok(active) => {
                AppState::global(cx).update(cx, |s, cx| {
                    s.update_settings(cx, |s| s.dictation_key = active)
                });
            }
            Err(message) => {
                self.items = previous;
                show_toast(
                    Toast::new(t_with(
                        "hotkey.errors.registrationFailed",
                        &[("hotkey", &format_hotkey_list_label(&list))],
                    ))
                    .description(message)
                    .variant(ToastVariant::Destructive),
                    cx,
                );
            }
        }
        self.registering = false;
        cx.notify();
    }

    fn on_raw_key(&mut self, event: RawKeyEvent, cx: &mut Context<Self>) {
        if self.capturing.is_none() {
            return;
        }
        match event {
            RawKeyEvent::KeyDown(ListenerKey::Globe) => {
                self.warning = None;
                self.capture.fn_held = true;
                self.capture.fn_used = false;
            }
            RawKeyEvent::KeyUp(ListenerKey::Globe) => {
                if self.capture.fn_held && !self.capture.fn_used {
                    self.capture.fn_held = false;
                    self.finalize(ListenerKey::Globe.into(), cx);
                    return;
                }
                self.capture.fn_held = false;
            }
            RawKeyEvent::KeyDown(ListenerKey::RightModifier(modifier)) => {
                if !self.capture.right_held.contains(&modifier) {
                    self.capture.right_held.push(modifier);
                }
            }
            RawKeyEvent::KeyUp(ListenerKey::RightModifier(_)) => {}
            RawKeyEvent::KeyDown(key @ ListenerKey::MouseButton(_)) => {
                self.finalize(key.into(), cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.capturing.is_none() || self.registering || event.is_held {
            return;
        }
        cx.stop_propagation();
        let ks = &event.keystroke;
        let m = ks.modifiers;
        let held = [m.control, m.platform, m.alt, m.shift];
        let modifiers: Vec<Modifier> = MODIFIERS
            .into_iter()
            .zip(held)
            .filter_map(|(modifier, held)| held.then_some(modifier))
            .collect();
        let Some(mut hotkey) = Hotkey::capture(&modifiers, Some(&ks.key)) else {
            return;
        };
        if self.capture.fn_held || m.function && !(ks.key.starts_with('f') && ks.key.len() > 1) {
            self.capture.fn_used = true;
            hotkey = hotkey.with_fn();
        }
        self.capture.pressed_at = None;
        self.finalize(hotkey, cx);
    }

    fn on_modifiers(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.capturing.is_none() {
            return;
        }
        let m = event.modifiers;
        let now = [m.control, m.platform, m.alt, m.shift];
        let any_now = now.iter().any(|v| *v);
        let any_before = self.capture.held.iter().any(|v| *v);
        if any_now {
            if !any_before {
                self.capture.pressed_at = Some(Instant::now());
                self.warning = None;
            }
            // Keep the widest chord seen during this press.
            for (held, now) in self.capture.held.iter_mut().zip(now) {
                *held |= now;
            }
            cx.notify();
            return;
        }
        if !any_before {
            return;
        }
        // All modifiers released: a modifier-only chord may be complete.
        let held = std::mem::take(&mut self.capture.held);
        let right = std::mem::take(&mut self.capture.right_held);
        let duration = self
            .capture
            .pressed_at
            .take()
            .map(|t| t.elapsed())
            .unwrap_or_default();
        let kinds: Vec<usize> = (0..4).filter(|i| held[*i]).collect();
        let with_fn = |hotkey: Hotkey, this: &mut Self| {
            if this.capture.fn_held {
                this.capture.fn_used = true;
                hotkey.with_fn()
            } else {
                hotkey
            }
        };
        if kinds.len() == 1 {
            let modifier = MODIFIERS[kinds[0]];
            if right.contains(&modifier) {
                let hotkey = with_fn(ListenerKey::RightModifier(modifier).into(), self);
                self.finalize(hotkey, cx);
            } else if duration >= MODIFIER_ONLY_HOLD_THRESHOLD && !self.capture.fn_held {
                let held_label = modifier.label(Some(Side::Left));
                let right_label = modifier.label(Some(Side::Right));
                self.reject(
                    t_with(
                        "hotkeyInput.singleModifierNeedsRightSide",
                        &[("key", &held_label), ("alternative", &right_label)],
                    ),
                    cx,
                );
            }
        } else if kinds.len() >= 2
            && duration >= MODIFIER_ONLY_HOLD_THRESHOLD
            && let Some(hotkey) = Hotkey::capture(
                &kinds.iter().map(|k| MODIFIERS[*k]).collect::<Vec<_>>(),
                None,
            )
        {
            let hotkey = with_fn(hotkey, self);
            self.finalize(hotkey, cx);
        }
        cx.notify();
    }

    fn held_labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (modifier, held) in MODIFIERS.into_iter().zip(self.capture.held) {
            if held {
                let side = self
                    .capture
                    .right_held
                    .contains(&modifier)
                    .then_some(Side::Right);
                out.push(modifier.label(side));
            }
        }
        if self.capture.fn_held {
            out.push("Fn".into());
        }
        out
    }

    fn render_row(
        &self,
        slot: Slot,
        value: Option<&Hotkey>,
        can_remove: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = cx.palette();
        let capturing = self.capturing == Some(slot);
        let disabled = self.registering;
        let id: SharedString = match slot {
            Slot::Existing(i) => format!("hotkey-row-{i}").into(),
            Slot::New => "hotkey-row-new".into(),
        };
        let mut row = div()
            .id(id)
            .group("hotkey-row")
            .relative()
            .overflow_hidden()
            .rounded(px(6.))
            .border_1()
            .cursor_pointer()
            .map(|d| {
                if disabled {
                    d.bg(p.muted.opacity(0.3))
                        .border_color(p.border)
                        .opacity(0.5)
                } else if capturing {
                    d.bg(p.primary.opacity(0.05))
                        .border_color(p.primary.opacity(0.3))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.6, 0.7, 0.49, 0.1),
                            offset: point(px(0.), px(0.)),
                            blur_radius: px(0.),
                            spread_radius: px(2.),
                            inset: false,
                        }])
                } else {
                    d.bg(p.surface_1)
                        .border_color(p.border)
                        .hover(|s| s.border_color(p.border_hover).bg(p.surface_2))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.capturing != Some(slot) {
                    this.start_capture(slot, window, cx);
                }
            }));
        if capturing {
            row = row.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(p.primary)
                    .with_animation(
                        "capture-pulse",
                        Animation::new(Duration::from_millis(2000)).repeat(),
                        |d, t| d.opacity(0.5 + 0.5 * (t * std::f32::consts::TAU).cos().abs()),
                    ),
            );
        }
        let content = if capturing {
            let held = self.held_labels();
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(p.primary)
                                        .with_animation(
                                            "rec-dot",
                                            Animation::new(Duration::from_millis(2000)).repeat(),
                                            |d, t| {
                                                d.opacity(
                                                    0.5 + 0.5
                                                        * (t * std::f32::consts::TAU).cos().abs(),
                                                )
                                            },
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(TEXT_SMALL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.muted_foreground)
                                        .child(t("hotkeyInput.recording")),
                                ),
                        )
                        .child(if held.is_empty() {
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground)
                                .child(t("hotkeyInput.tryShortcut"))
                        } else {
                            let mut chips = div().flex().items_center().gap(px(4.));
                            for label in held {
                                chips = chips.child(
                                    div()
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(p.primary.opacity(0.2))
                                        .bg(p.primary.opacity(0.1))
                                        .font_family(FONT_MONO)
                                        .text_size(px(TEXT_SMALL))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.primary)
                                        .child(label),
                                );
                            }
                            chips.child(
                                div()
                                    .text_size(px(TEXT_SMALL))
                                    .text_color(p.primary.opacity(0.4))
                                    .child(if self.capture.fn_held {
                                        t("hotkeyInput.fnCaptureHint")
                                    } else {
                                        t("hotkeyInput.keyHint")
                                    }),
                            )
                        }),
                )
                .when_some(self.warning.clone(), |d, warning| {
                    d.child(
                        div()
                            .mt(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(p.warning.opacity(if p.dark { 0.25 } else { 0.2 }))
                            .bg(p.warning.opacity(if p.dark { 0.12 } else { 0.08 }))
                            .child(icon("alert-triangle", 12., p.warning))
                            .child(
                                div()
                                    .text_size(px(TEXT_SMALL))
                                    .text_color(p.warning)
                                    .child(warning),
                            ),
                    )
                })
                .into_any_element()
        } else if let Some(value) = value {
            let parts = value.label_parts();
            let keys = if parts.len() > 1 {
                let mut keys = div().flex().items_center().gap(px(4.));
                for (i, part) in parts.into_iter().enumerate() {
                    if i > 0 {
                        keys = keys.child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground.opacity(0.7))
                                .child("+"),
                        );
                    }
                    keys = keys.child(key_chip(part, &p));
                }
                keys.into_any_element()
            } else if value.is_globe() {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.surface_raised)
                            .text_size(px(TEXT_TITLE))
                            .child("🌐"),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_SMALL))
                            .text_color(p.muted_foreground)
                            .child(t("hotkeyInput.globe")),
                    )
                    .into_any_element()
            } else {
                key_chip(value.label(), &p)
                    .px(px(10.))
                    .py(px(4.))
                    .into_any_element()
            };
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(TEXT_BODY))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.foreground)
                        .child(t("hotkeyInput.hotkeyLabel")),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(keys)
                        .child(
                            div()
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground.opacity(0.7))
                                .child(t("hotkeyInput.clickToChangeLower")),
                        )
                        .when(can_remove && !disabled, |d| {
                            let index = match slot {
                                Slot::Existing(i) => i,
                                Slot::New => 0,
                            };
                            d.child(
                                div()
                                    .id(("remove-hotkey", index))
                                    .opacity(0.)
                                    .group_hover("hotkey-row", |s| s.opacity(1.))
                                    .cursor_pointer()
                                    .child(icon("trash2", 14., p.muted_foreground.opacity(0.7)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.remove(index, cx);
                                    })),
                            )
                        }),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(t("hotkeyInput.clickToSet"))
                .into_any_element()
        };
        row.child(div().px(px(16.)).py(px(12.)).child(content))
            .into_any_element()
    }
}

fn key_chip(text: String, p: &super::theme::Palette) -> Div {
    div()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(4.))
        .border_1()
        .border_color(p.border)
        .bg(p.surface_raised)
        .font_family(FONT_MONO)
        .text_size(px(TEXT_SMALL))
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.foreground)
        .child(text)
}

impl Render for HotkeyListInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = cx.palette();
        // Losing focus ends a capture.
        if self.capturing.is_some() && !self.focus.is_focused(window) {
            self.capturing = None;
            self.adding = false;
            self.capture = Capture::default();
            backend::set_hotkey_listening_mode(false);
        }
        let can_remove = self.items.len() > 1;
        let mut col = div().flex().flex_col().gap(px(8.));
        let items = self.items.clone();
        for (i, item) in items.iter().enumerate() {
            col = col.child(self.render_row(Slot::Existing(i), Some(item), can_remove, cx));
        }
        if items.is_empty() || self.adding {
            col = col.child(self.render_row(Slot::New, None, false, cx));
        }
        let show_reset = !items.is_empty() && items != parse_hotkey_list(DEFAULT_HOTKEY);
        let show_add = !self.adding && !items.is_empty();
        if (show_add || show_reset) && !self.adding {
            col = col.child(
                div()
                    .mt(px(2.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(if show_add {
                        Button::new("add-hotkey")
                            .icon("plus", 14.)
                            .label(t("hotkeyInput.addAnother"))
                            .variant(ButtonVariant::OutlineFlat)
                            .size(ButtonSize::Sm)
                            .disabled(self.registering)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.adding = true;
                                this.start_capture(Slot::New, window, cx);
                            }))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    })
                    .when(show_reset, |d| {
                        d.child(
                            div()
                                .id("reset-hotkey")
                                .text_size(px(TEXT_SMALL))
                                .text_color(p.muted_foreground.opacity(0.7))
                                .cursor_pointer()
                                .hover(|s| s.text_color(p.foreground))
                                .child(t_with(
                                    "settings.general.hotkey.resetToDefault",
                                    &[("hotkey", &format_hotkey_list_label(DEFAULT_HOTKEY))],
                                ))
                                .on_click(cx.listener(|this, _, _, cx| this.reset_to_default(cx))),
                        )
                    }),
            );
        }
        div()
            .id("hotkey-list")
            .track_focus(&self.focus)
            .key_context("HotkeyCapture")
            .on_key_down(cx.listener(Self::on_key_down))
            .on_modifiers_changed(cx.listener(Self::on_modifiers))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.capturing.is_some() {
                    this.stop_capture(cx);
                }
            }))
            .child(col)
    }
}

impl Drop for HotkeyListInput {
    fn drop(&mut self) {
        if self.capturing.is_some() {
            backend::set_hotkey_listening_mode(false);
        }
    }
}
