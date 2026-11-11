//! Confirm/alert dialogs and toasts for the control panel. Any view can raise
//! one through the global `Overlays` entity; the control panel renders them
//! above everything else.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use gpui_kit::base::FocusTrapElement;

use super::components::{
    Button, ButtonSize, ButtonVariant, DismissMenu, KeyboardFocusable, backdrop, dialog_shadow,
    focus_ring, icon, keyed_focus_handle, text_tooltip,
};
use super::theme::{
    DUR_BASE, DUR_SHEET, DUR_SNAP, FONT_MONO, LINE_SMALL, LINE_TITLE, PaletteExt, RADIUS_2XL,
    RADIUS_LG, RADIUS_SM, TEXT_BODY, TEXT_MONO_SMALL, TEXT_SMALL, TEXT_TITLE, ease_out, ease_snap,
    motion,
};

pub type Callback = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct DialogRequest {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub confirm_label: SharedString,
    /// `None` makes an alert with a single OK button.
    pub cancel_label: Option<SharedString>,
    pub destructive: bool,
    /// An optional third choice, beside Cancel and Confirm.
    pub alternative: Option<(SharedString, Callback)>,
    pub on_confirm: Option<Callback>,
}

impl DialogRequest {
    pub fn alert(title: impl Into<SharedString>, description: Option<SharedString>) -> Self {
        Self {
            title: title.into(),
            description,
            confirm_label: "OK".into(),
            cancel_label: None,
            destructive: false,
            alternative: None,
            on_confirm: None,
        }
    }

    pub fn confirm(
        title: impl Into<SharedString>,
        description: Option<SharedString>,
        destructive: bool,
        on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            description,
            confirm_label: "Confirm".into(),
            cancel_label: Some(crate::i18n::t("common.cancel").into()),
            destructive,
            alternative: None,
            on_confirm: Some(Rc::new(on_confirm)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastVariant {
    Default,
    Success,
    Destructive,
}

#[derive(Clone)]
pub struct Toast {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub variant: ToastVariant,
    /// `None` uses the defaults: 6 s for destructive, else 3.5 s.
    pub duration: Option<Duration>,
    /// One inline action, such as Undo. Running it also closes the toast.
    pub action: Option<(SharedString, Callback)>,
    deadline: Option<Instant>,
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            variant: ToastVariant::Default,
            duration: None,
            action: None,
            deadline: None,
        }
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(f)));
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

struct ActiveToast {
    id: u64,
    toast: Toast,
    /// The toast's duration with the variant default applied.
    duration: Duration,
    /// Time shown before the current run of the countdown.
    banked: Duration,
    /// When the countdown last resumed; `None` while the pointer rests on
    /// the toast, so a message being read never disappears under it.
    running_since: Option<Instant>,
    /// Paused because its close button holds keyboard focus.
    held_by_focus: bool,
    closing: bool,
}

impl ActiveToast {
    fn elapsed(&self) -> Duration {
        if let Some(deadline) = self.toast.deadline {
            return self
                .duration
                .saturating_sub(deadline.saturating_duration_since(Instant::now()));
        }
        self.banked + self.running_since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn set_paused(&mut self, paused: bool) {
        if self.toast.deadline.is_some() {
            return;
        }
        match (paused, self.running_since) {
            (true, Some(since)) => {
                self.banked += since.elapsed();
                self.running_since = None;
            }
            (false, None) => self.running_since = Some(Instant::now()),
            _ => {}
        }
    }
}

pub struct Overlays {
    dialog: Option<DialogRequest>,
    /// Set when a dialog opens; its first render moves focus to the safe
    /// button so Return and Escape act on the dialog, not the page behind.
    dialog_needs_focus: bool,
    /// A view-specific modal (snippet editor, command search), drawn under dialogs.
    custom: Option<AnyView>,
    toasts: Vec<ActiveToast>,
    next_id: u64,
}

impl EventEmitter<DismissMenu> for Overlays {}

struct OverlaysGlobal(Entity<Overlays>);
impl Global for OverlaysGlobal {}

impl Overlays {
    pub fn init(cx: &mut App) {
        let entity = cx.new(|_| Overlays {
            dialog: None,
            dialog_needs_focus: false,
            custom: None,
            toasts: Vec::new(),
            next_id: 0,
        });
        cx.set_global(OverlaysGlobal(entity));
    }

    pub fn global(cx: &App) -> Entity<Overlays> {
        cx.global::<OverlaysGlobal>().0.clone()
    }

    pub fn has_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn has_custom(&self) -> bool {
        self.custom.is_some()
    }

    pub fn custom(&self) -> Option<AnyView> {
        self.custom.clone()
    }

    pub fn clear_panel(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        self.dialog_needs_focus = false;
        self.custom = None;
        self.toasts.clear();
        cx.notify();
    }

    pub fn close_custom(&mut self, cx: &mut Context<Self>) {
        if self.custom.take().is_some() {
            cx.notify();
        }
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        if self.dialog.take().is_some() {
            cx.notify();
        }
    }

    fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .toasts
            .iter()
            .any(|t| t.id == id && t.toast.deadline.is_some())
        {
            self.toasts.retain(|t| t.id != id);
            cx.notify();
            return;
        }
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
            if toast.closing {
                return;
            }
            toast.closing = true;
            cx.notify();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(DUR_SNAP))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.toasts.retain(|t| t.id != id);
                    cx.notify();
                });
            })
            .detach();
        }
    }
}

pub fn show_dialog(request: DialogRequest, cx: &mut App) {
    Overlays::global(cx).update(cx, |o, cx| {
        cx.emit(DismissMenu);
        o.dialog = Some(request);
        o.dialog_needs_focus = true;
        cx.notify();
    });
}

pub fn show_custom(view: AnyView, cx: &mut App) {
    Overlays::global(cx).update(cx, |o, cx| {
        cx.emit(DismissMenu);
        o.custom = Some(view);
        cx.notify();
    });
}

pub fn dismiss_menus(cx: &mut App) {
    Overlays::global(cx).update(cx, |_, cx| cx.emit(DismissMenu));
}

pub fn close_custom(cx: &mut App) {
    Overlays::global(cx).update(cx, |o, cx| o.close_custom(cx));
}

/// The backdrop + card frame shared by view-specific modals.
pub fn modal_frame(
    id: &'static str,
    width: f32,
    content: impl IntoElement,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    cx: &mut App,
) -> Stateful<Div> {
    let p = cx.palette();
    let trap = keyed_focus_handle(SharedString::from(format!("{id}-trap")).into(), cx);
    let close = close_button("modal-close", &p, cx);
    let on_dismiss = Rc::new(on_dismiss);
    let dismiss = on_dismiss.clone();
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(backdrop(&p))
        .occlude()
        .on_click(move |_, window, cx| dismiss(window, cx))
        .child(
            div()
                .id("modal-card")
                .occlude()
                .relative()
                .w(px(width))
                .p(px(24.))
                .rounded(px(RADIUS_2XL))
                .border_1()
                .border_color(p.border)
                .bg(p.popover)
                .shadow(dialog_shadow(&p))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(content)
                .child(close.on_click(move |_, window, cx| on_dismiss(window, cx)))
                .focus_trap(SharedString::from(format!("{id}-focus-trap")), &trap)
                .with_animation(
                    "modal-in",
                    motion(DUR_BASE, cx).with_easing(ease_out),
                    |d, t| d.opacity(t).mt(px(8. * (1. - t))),
                ),
        )
}

pub fn show_toast(toast: Toast, cx: &mut App) {
    show_tracked_toast(toast, cx);
}

pub fn remove_toast(id: u64, cx: &mut App) {
    Overlays::global(cx).update(cx, |o, cx| {
        o.toasts.retain(|t| t.id != id);
        cx.notify();
    });
}

pub fn show_tracked_toast(toast: Toast, cx: &mut App) -> u64 {
    let duration = toast.duration.unwrap_or(Duration::from_millis(
        if toast.variant == ToastVariant::Destructive {
            6000
        } else {
            3500
        },
    ));
    Overlays::global(cx).update(cx, |o, cx| {
        let id = o.next_id;
        o.next_id += 1;
        o.toasts.push(ActiveToast {
            id,
            toast,
            duration,
            banked: Duration::ZERO,
            running_since: Some(Instant::now()),
            held_by_focus: false,
            closing: false,
        });
        // Keep the toast stack short.
        if o.toasts.len() > 3 {
            o.toasts.remove(0);
        }
        cx.notify();
        id
    })
}

/// Renders the dialog backdrop and card, or nothing.
pub fn render_dialog(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let overlays = Overlays::global(cx);
    let request = overlays.read(cx).dialog.clone()?;
    let p = cx.palette();
    if overlays.read(cx).dialog_needs_focus {
        overlays.update(cx, |o, _| o.dialog_needs_focus = false);
        // Destructive dialogs start on Cancel, as macOS alerts do, so a stray
        // Return never deletes anything.
        let key = if (request.destructive || request.alternative.is_some())
            && request.cancel_label.is_some()
        {
            "dialog-cancel"
        } else {
            "dialog-confirm"
        };
        let handle = keyed_focus_handle(key.into(), cx);
        window.on_next_frame(move |window, cx| window.focus(&handle, cx));
    }
    let trap = keyed_focus_handle("dialog-trap".into(), cx);
    let close = {
        let overlays = overlays.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            overlays.update(cx, |o, cx| o.close_dialog(cx));
        }
    };
    let confirm = {
        let overlays = overlays.clone();
        let on_confirm = request.on_confirm.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            overlays.update(cx, |o, cx| o.close_dialog(cx));
            if let Some(f) = &on_confirm {
                f(window, cx);
            }
        }
    };
    let card = div()
        .id("dialog-card")
        .occlude()
        .relative()
        .w(px(448.))
        .flex()
        .flex_col()
        .rounded(px(RADIUS_2XL))
        .overflow_hidden()
        .border_1()
        .border_color(p.border)
        .bg(p.popover)
        .shadow(dialog_shadow(&p))
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(24.))
                .child(
                    div()
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(LINE_TITLE))
                        .text_color(p.foreground)
                        .pr(px(28.))
                        .child(request.title.clone()),
                )
                .when_some(request.description.clone(), |d, desc| {
                    d.child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .line_height(px(crate::ui::theme::LINE_BODY))
                            .text_color(p.muted_foreground)
                            .child(desc),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .px(px(24.))
                .py(px(16.))
                .border_t_1()
                .border_color(p.border)
                .bg(if p.dark {
                    p.white.opacity(0.02)
                } else {
                    p.muted.opacity(0.72)
                })
                .when_some(request.alternative.clone(), |d, (label, action)| {
                    let overlays = overlays.clone();
                    d.child(
                        Button::new("dialog-alternative")
                            .label(label)
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |_, window, cx| {
                                overlays.update(cx, |o, cx| o.close_dialog(cx));
                                action(window, cx);
                            }),
                    )
                })
                .when_some(request.cancel_label.clone(), |d, label| {
                    d.child(
                        Button::new("dialog-cancel")
                            .label(label)
                            .variant(ButtonVariant::Outline)
                            .on_click(close.clone()),
                    )
                })
                .child(
                    Button::new("dialog-confirm")
                        .label(request.confirm_label.clone())
                        .variant(if request.destructive {
                            ButtonVariant::Destructive
                        } else {
                            ButtonVariant::Primary
                        })
                        .on_click(confirm),
                ),
        )
        .child(close_button("dialog-close", &p, cx).on_click(close.clone()))
        .focus_trap("dialog-focus-trap", &trap)
        .with_animation(
            "dialog-in",
            motion(DUR_BASE, cx).with_easing(ease_out),
            |d, t| d.opacity(t).mt(px(6. * (1. - t))),
        );

    Some(
        div()
            .id("dialog-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop(&p))
            .occlude()
            .on_click(close)
            .child(card)
            .with_animation(
                "dialog-backdrop-in",
                motion(DUR_BASE, cx).with_easing(ease_out),
                |d, t| d.opacity(t),
            )
            .into_any_element(),
    )
}

/// A 28 px ghost close button pinned to a card's top-right corner.
fn close_button(id: &'static str, p: &super::theme::Palette, cx: &mut App) -> Stateful<Div> {
    let hover = p.hover_wash(if p.dark { 0.06 } else { 0.05 });
    div()
        .id(id)
        .keyboard_focusable(id, Role::Button, crate::i18n::t("common.close"), cx)
        .tooltip(text_tooltip(crate::i18n::t("common.close")))
        .absolute()
        .top(px(14.))
        .right(px(14.))
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(RADIUS_LG))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(icon("x", 16., p.muted_foreground))
}

/// Renders the toast stack in the bottom-right corner: 360 px popover cards
/// with a colored type icon, a hover close button and a 1 px countdown.
pub fn render_toasts(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let overlays = Overlays::global(cx);
    // Ordinary toasts pause on hover; undo toasts keep their operation's deadline.
    let expired: Vec<u64> = overlays
        .read(cx)
        .toasts
        .iter()
        .filter(|t| !t.closing && t.elapsed() >= t.duration)
        .map(|t| t.id)
        .collect();
    if !expired.is_empty() {
        let overlays = overlays.clone();
        cx.defer(move |cx| {
            overlays.update(cx, |o, cx| {
                for id in expired {
                    o.dismiss_toast(id, cx);
                }
            })
        });
    }
    // Close-button focus handles, fetched before the toasts are borrowed.
    let ids: Vec<u64> = overlays.read(cx).toasts.iter().map(|t| t.id).collect();
    let close_handles: Vec<FocusHandle> = ids
        .iter()
        .map(|id| keyed_focus_handle(("toast-close", *id as usize).into(), cx))
        .collect();
    let close_label = crate::i18n::t("common.close");
    // A focused close button holds its toast, as a resting pointer does.
    let focused: Vec<bool> = close_handles.iter().map(|h| h.is_focused(window)).collect();
    overlays.update(cx, |o, _| {
        for (t, focused) in o.toasts.iter_mut().zip(&focused) {
            if *focused && !t.held_by_focus {
                t.held_by_focus = true;
                t.set_paused(true);
            } else if !*focused && t.held_by_focus {
                t.held_by_focus = false;
                t.set_paused(false);
            }
        }
    });
    let o = overlays.read(cx);
    if o.toasts.is_empty() {
        return None;
    }
    let p = cx.palette();
    // A polite live region, so VoiceOver reads each toast as it arrives.
    let mut stack = div()
        .id("toast-stack")
        .role(Role::Status)
        .absolute()
        .bottom(px(24.))
        .right(px(24.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .w(px(360.));
    for (active, close_handle) in o.toasts.iter().zip(close_handles) {
        let id = active.id;
        let toast = &active.toast;
        let (type_icon, progress) = match toast.variant {
            ToastVariant::Default => (None, p.foreground.opacity(0.12)),
            ToastVariant::Success => (Some(("check", p.success)), p.success.opacity(0.35)),
            ToastVariant::Destructive => (
                Some(("alert-circle", p.destructive)),
                p.destructive.opacity(0.35),
            ),
        };
        let elapsed = active.elapsed().as_secs_f32();
        let total = active.duration.as_secs_f32();
        let remaining = (1. - elapsed / total).clamp(0., 1.);
        let (message, detail) = match &toast.description {
            Some(desc) if !toast.title.is_empty() => (toast.title.clone(), Some(desc.clone())),
            Some(desc) => (desc.clone(), None),
            None => (toast.title.clone(), None),
        };
        let destructive = toast.variant == ToastVariant::Destructive;
        let overlays_close = overlays.clone();
        let overlays_hover = overlays.clone();
        let group: SharedString = format!("toast-{id}").into();
        let close_hover = p.hover_wash(0.08);
        let close_focused = close_handle.is_focused(window);
        let card = div()
            .id(("toast", id as usize))
            .group(group.clone())
            .on_hover(move |hovered: &bool, _, cx| {
                let paused = *hovered;
                overlays_hover.update(cx, |o, cx| {
                    if let Some(t) = o.toasts.iter_mut().find(|t| t.id == id) {
                        t.set_paused(paused);
                        cx.notify();
                    }
                });
            })
            .occlude()
            .relative()
            .flex()
            .items_start()
            .gap(px(10.))
            .w_full()
            .px(px(14.))
            .py(px(12.))
            .rounded(px(RADIUS_LG))
            .overflow_hidden()
            .border_1()
            .border_color(p.foreground.opacity(0.1))
            .bg(p.popover)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., if p.dark { 0.5 } else { 0.14 }),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-12.),
                inset: false,
            }])
            .when_some(type_icon, |d, (name, color)| {
                d.child(div().pt(px(2.)).child(icon(name, 16., color)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(16.))
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .line_height(px(crate::ui::theme::LINE_BODY))
                            .text_color(p.foreground)
                            .child(message),
                    )
                    .when_some(detail, |d, detail| {
                        if destructive {
                            d.child(
                                div()
                                    .mt(px(6.))
                                    .px(px(8.))
                                    .py(px(6.))
                                    .rounded(px(RADIUS_SM))
                                    .bg(p.destructive.opacity(if p.dark { 0.12 } else { 0.06 }))
                                    .font_family(FONT_MONO)
                                    .font_features(crate::ui::theme::mono_features())
                                    .text_size(px(TEXT_MONO_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.destructive)
                                    .child(detail),
                            )
                        } else {
                            d.child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(TEXT_SMALL))
                                    .line_height(px(LINE_SMALL))
                                    .text_color(p.muted_foreground)
                                    .child(detail),
                            )
                        }
                    }),
            )
            .when_some(toast.action.clone(), |d, (label, f)| {
                let overlays = overlays.clone();
                d.child(
                    div().mr(px(20.)).flex_none().child(
                        Button::new(("toast-action", id as usize))
                            .label(label)
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Sm)
                            .on_click(move |_, window, cx| {
                                overlays.update(cx, |o, cx| o.dismiss_toast(id, cx));
                                f(window, cx);
                            }),
                    ),
                )
            })
            .child(
                div()
                    .id(("toast-close", id as usize))
                    .track_focus(&close_handle.tab_stop(true))
                    .role(Role::Button)
                    .aria_label(close_label.clone())
                    .focus_visible(move |s| s.shadow(focus_ring(&p)))
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    // Hidden until the toast is hovered or the button holds
                    // keyboard focus.
                    .opacity(if close_focused { 1. } else { 0. })
                    .group_hover(group, |s| s.opacity(1.))
                    .hover(move |s| s.bg(close_hover))
                    .cursor_pointer()
                    .child(icon("x", 14., p.muted_foreground))
                    .on_click(move |_, _, cx| {
                        overlays_close.update(cx, |o, cx| o.dismiss_toast(id, cx));
                    }),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .h(px(1.))
                    .w(relative(remaining))
                    .bg(progress),
            );
        let closing = active.closing;
        let card = card.with_animation(
            ("toast-anim", id as usize * 2 + closing as usize),
            motion(if closing { DUR_SNAP } else { DUR_SHEET }, cx).with_easing(if closing {
                ease_snap
            } else {
                ease_out
            }),
            // Slides in from the edge; leaves quickly with only a nudge.
            move |d, t| {
                if closing {
                    d.opacity(1. - t).ml(px(4. * t))
                } else {
                    d.opacity(t).mt(px(12. * (1. - t)))
                }
            },
        );
        stack = stack.child(card);
    }
    window.request_animation_frame();
    Some(stack.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::{ActiveToast, Duration, Instant, Toast};

    #[test]
    fn deadline_toasts_do_not_pause_for_hover_or_focus() {
        let now = Instant::now();
        let mut active = ActiveToast {
            id: 1,
            toast: Toast::new("Undo").deadline(now),
            duration: Duration::from_secs(6),
            banked: Duration::ZERO,
            running_since: Some(now),
            held_by_focus: false,
            closing: false,
        };
        active.set_paused(true);
        assert_eq!(active.running_since, Some(now));
        assert_eq!(active.elapsed(), active.duration);
        active.set_paused(false);
        assert_eq!(active.running_since, Some(now));
    }

    #[test]
    fn ordinary_toasts_still_pause_and_resume() {
        let mut active = ActiveToast {
            id: 1,
            toast: Toast::new("Copied"),
            duration: Duration::from_secs(4),
            banked: Duration::ZERO,
            running_since: Some(Instant::now()),
            held_by_focus: false,
            closing: false,
        };
        active.set_paused(true);
        assert!(active.running_since.is_none());
        assert_eq!(active.elapsed(), active.banked);
        active.set_paused(false);
        assert!(active.running_since.is_some());
    }
}
