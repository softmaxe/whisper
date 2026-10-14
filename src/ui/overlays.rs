//! Confirm/alert dialogs and toasts for the control panel. Any view can raise
//! one through the global `Overlays` entity; the control panel renders them
//! above everything else.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{Button, ButtonVariant, icon};
use super::theme::{
    DUR_BASE, DUR_SHEET, DUR_SNAP, FONT_MONO, LINE_TITLE, PaletteExt, TEXT_BODY, TEXT_SMALL,
    TEXT_TITLE, ease_out, ease_snap,
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
            cancel_label: Some(whisper::i18n::t("common.cancel").into()),
            destructive,
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
pub struct ToastAction {
    pub label: SharedString,
    pub on_click: Callback,
}

#[derive(Clone)]
pub struct Toast {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub variant: ToastVariant,
    /// `None` uses the defaults: 6 s for destructive, else 3.5 s.
    pub duration: Option<Duration>,
    pub action: Option<ToastAction>,
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            variant: ToastVariant::Default,
            duration: None,
            action: None,
        }
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
}

struct ActiveToast {
    id: u64,
    toast: Toast,
    shown_at: Instant,
    closing: bool,
}

pub struct Overlays {
    dialog: Option<DialogRequest>,
    /// A view-specific modal (snippet editor, command search), drawn under dialogs.
    custom: Option<AnyView>,
    toasts: Vec<ActiveToast>,
    next_id: u64,
}

struct OverlaysGlobal(Entity<Overlays>);
impl Global for OverlaysGlobal {}

impl Overlays {
    pub fn init(cx: &mut App) {
        let entity = cx.new(|_| Overlays {
            dialog: None,
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
        o.dialog = Some(request);
        cx.notify();
    });
}

pub fn show_custom(view: AnyView, cx: &mut App) {
    Overlays::global(cx).update(cx, |o, cx| {
        o.custom = Some(view);
        cx.notify();
    });
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
    cx: &App,
) -> Stateful<Div> {
    let p = cx.palette();
    let on_dismiss = Rc::new(on_dismiss);
    let dismiss = on_dismiss.clone();
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(hsla(0., 0., 0., 0.6))
        .occlude()
        .on_click(move |_, window, cx| dismiss(window, cx))
        .child(
            div()
                .id("modal-card")
                .occlude()
                .relative()
                .w(px(width))
                .p(px(24.))
                .rounded(px(16.))
                .border_1()
                .border_color(if p.dark {
                    p.border
                } else {
                    p.border.opacity(0.7)
                })
                .bg(if p.dark { p.surface_2 } else { p.card })
                .shadow_2xl()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(content)
                .child(
                    div()
                        .id("modal-close")
                        .absolute()
                        .top(px(16.))
                        .right(px(16.))
                        .p(px(6.))
                        .rounded_full()
                        .opacity(0.5)
                        .cursor_pointer()
                        .hover(|s| s.opacity(1.).bg(p.muted.opacity(0.5)))
                        .child(icon("x", 16., p.muted_foreground))
                        .on_click(move |_, window, cx| on_dismiss(window, cx)),
                )
                .with_animation(
                    "modal-in",
                    Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                    |d, t| d.opacity(t).mt(px(8. * (1. - t))),
                ),
        )
}

pub fn show_toast(mut toast: Toast, cx: &mut App) {
    let duration = toast.duration.unwrap_or(Duration::from_millis(
        if toast.variant == ToastVariant::Destructive {
            6000
        } else {
            3500
        },
    ));
    toast.duration = Some(duration);
    Overlays::global(cx).update(cx, |o, cx| {
        let id = o.next_id;
        o.next_id += 1;
        o.toasts.push(ActiveToast {
            id,
            toast,
            shown_at: Instant::now(),
            closing: false,
        });
        // Keep the toast stack short.
        if o.toasts.len() > 3 {
            o.toasts.remove(0);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |this, cx| this.dismiss_toast(id, cx));
        })
        .detach();
    });
}

/// Renders the dialog backdrop and card, or nothing.
pub fn render_dialog(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let overlays = Overlays::global(cx);
    let request = overlays.read(cx).dialog.clone()?;
    let p = cx.palette();
    let _ = window;
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
        .w(px(425.))
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(24.))
        .rounded(px(16.))
        .border_1()
        .border_color(if p.dark {
            p.border
        } else {
            p.border.opacity(0.7)
        })
        .bg(if p.dark { p.surface_2 } else { p.card })
        .shadow_2xl()
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(LINE_TITLE))
                        .text_color(p.foreground)
                        .pr(px(24.))
                        .child(request.title.clone()),
                )
                .when_some(request.description.clone(), |d, desc| {
                    d.child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .line_height(px(22.))
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
        .child(
            div()
                .id("dialog-close")
                .absolute()
                .top(px(16.))
                .right(px(16.))
                .p(px(6.))
                .rounded_full()
                .opacity(0.5)
                .cursor_pointer()
                .hover(|s| s.opacity(1.).bg(p.muted.opacity(0.5)))
                .child(icon("x", 16., p.muted_foreground))
                .on_click(close.clone()),
        )
        .with_animation(
            "dialog-in",
            Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
            |d, t| d.opacity(t).mt(px(8. * (1. - t))),
        );

    Some(
        div()
            .id("dialog-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.6))
            .occlude()
            .on_click(close)
            .child(card)
            .with_animation(
                "dialog-backdrop-in",
                Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                |d, t| d.opacity(t),
            )
            .into_any_element(),
    )
}

/// Renders the toast stack in the bottom-right corner: a 300px dark HUD card
/// with a 2px accent bar, hover close button and a 1px countdown.
pub fn render_toasts(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let overlays = Overlays::global(cx);
    let o = overlays.read(cx);
    if o.toasts.is_empty() {
        return None;
    }
    let mut stack = div()
        .absolute()
        .bottom(px(20.))
        .right(px(20.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .w(px(300.));
    for active in &o.toasts {
        let id = active.id;
        let toast = &active.toast;
        let (accent, progress) = match toast.variant {
            ToastVariant::Default => (hsla(0., 0., 1., 0.2), hsla(0., 0., 1., 0.15)),
            ToastVariant::Success => (rgb(0x34d399).into(), Hsla::from(rgb(0x34d399)).opacity(0.3)),
            ToastVariant::Destructive => {
                (rgb(0xf87171).into(), Hsla::from(rgb(0xf87171)).opacity(0.3))
            }
        };
        let accent: Hsla = accent;
        let elapsed = active.shown_at.elapsed().as_secs_f32();
        let total = toast
            .duration
            .unwrap_or(Duration::from_millis(3500))
            .as_secs_f32();
        let remaining = (1. - elapsed / total).clamp(0., 1.);
        let (message, detail) = match &toast.description {
            Some(desc) if !toast.title.is_empty() => (toast.title.clone(), Some(desc.clone())),
            Some(desc) => (desc.clone(), None),
            None => (toast.title.clone(), None),
        };
        let destructive = toast.variant == ToastVariant::Destructive;
        let overlays_close = overlays.clone();
        let action = toast.action.clone();
        let overlays_action = overlays.clone();
        let group: SharedString = format!("toast-{id}").into();
        let card = div()
            .id(("toast", id as usize))
            .group(group.clone())
            .occlude()
            .relative()
            .flex()
            .w_full()
            .rounded(px(5.))
            .border_1()
            .border_color(rgb(0x202225))
            .bg(rgb(0x0a0b0e))
            .shadow_lg()
            .child(div().w(px(2.)).flex_none().rounded_l(px(5.)).bg(accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(TEXT_SMALL))
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_height(px(15.))
                                    .text_color(hsla(0., 0., 1., 0.9))
                                    .child(message),
                            )
                            .when_some(detail, |d, detail| {
                                if destructive {
                                    d.child(
                                        div()
                                            .mt(px(4.))
                                            .px(px(6.))
                                            .py(px(4.))
                                            .rounded(px(3.))
                                            .border_1()
                                            .border_color(hsla(0., 0., 1., 0.06))
                                            .bg(hsla(0., 0., 1., 0.04))
                                            .font_family(FONT_MONO)
                                            .text_size(px(TEXT_SMALL))
                                            .line_height(px(16.5))
                                            .text_color(Hsla::from(rgb(0xfca5a5)).opacity(0.8))
                                            .child(detail),
                                    )
                                } else {
                                    d.child(
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(TEXT_SMALL))
                                            .line_height(px(16.5))
                                            .text_color(hsla(0., 0., 1., 0.45))
                                            .child(detail),
                                    )
                                }
                            }),
                    )
                    .when_some(action, |d, action| {
                        d.child(
                            div()
                                .id("toast-action")
                                .flex_none()
                                .self_center()
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(4.))
                                .bg(hsla(0., 0., 1., 0.1))
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(hsla(0., 0., 1., 0.9))
                                .cursor_pointer()
                                .hover(|s| s.bg(hsla(0., 0., 1., 0.16)))
                                .child(action.label.clone())
                                .on_click(move |_, window, cx| {
                                    (action.on_click)(window, cx);
                                    overlays_action.update(cx, |o, cx| o.dismiss_toast(id, cx));
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .id("toast-close")
                    .absolute()
                    .top(px(-8.))
                    .left(px(-8.))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.1))
                    .bg(hsla(0., 0., 0.2, 0.9))
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .cursor_pointer()
                    .child(icon("x", 12., hsla(0., 0., 1., 0.7)))
                    .on_click(move |_, _, cx| {
                        overlays_close.update(cx, |o, cx| o.dismiss_toast(id, cx));
                    }),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(px(2.))
                    .h(px(1.))
                    .w(relative(remaining))
                    .bg(progress),
            );
        let closing = active.closing;
        let card = card.with_animation(
            ("toast-anim", id as usize * 2 + closing as usize),
            Animation::new(Duration::from_millis(if closing {
                DUR_SNAP
            } else {
                DUR_SHEET
            }))
            .with_easing(if closing { ease_snap } else { ease_out }),
            // Slides in from the edge; leaves quickly with only a nudge.
            move |d, t| {
                if closing {
                    d.opacity(1. - t).ml(px(4. * t))
                } else {
                    d.opacity(t).ml(px(16. * (1. - t)))
                }
            },
        );
        stack = stack.child(card);
    }
    window.request_animation_frame();
    Some(stack.into_any_element())
}
