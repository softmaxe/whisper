//! Confirm/alert dialogs and toasts for the control panel. Any view can raise
//! one through the global `Overlays` entity; the control panel renders them
//! above everything else.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::components::{Button, ButtonVariant, backdrop, dialog_shadow, icon};
use super::theme::{
    DUR_BASE, DUR_SHEET, DUR_SNAP, FONT_MONO, LINE_SMALL, PaletteExt, RADIUS_2XL, RADIUS_LG,
    RADIUS_SM, TEXT_BODY, TEXT_SMALL, ease_out, ease_snap,
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
            cancel_label: Some(crate::i18n::t("common.cancel").into()),
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
pub struct Toast {
    pub title: SharedString,
    pub description: Option<SharedString>,
    pub variant: ToastVariant,
    /// `None` uses the defaults: 6 s for destructive, else 3.5 s.
    pub duration: Option<Duration>,
}

impl Toast {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            variant: ToastVariant::Default,
            duration: None,
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
    /// The toast's duration with the variant default applied.
    duration: Duration,
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
                .child(
                    close_button("modal-close", &p)
                        .on_click(move |_, window, cx| on_dismiss(window, cx)),
                )
                .with_animation(
                    "modal-in",
                    Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                    |d, t| d.opacity(t).mt(px(8. * (1. - t))),
                ),
        )
}

pub fn show_toast(toast: Toast, cx: &mut App) {
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
pub fn render_dialog(cx: &mut App) -> Option<AnyElement> {
    let overlays = Overlays::global(cx);
    let request = overlays.read(cx).dialog.clone()?;
    let p = cx.palette();
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
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(px(24.))
                        .text_color(p.foreground)
                        .pr(px(28.))
                        .child(request.title.clone()),
                )
                .when_some(request.description.clone(), |d, desc| {
                    d.child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .line_height(px(20.))
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
        .child(close_button("dialog-close", &p).on_click(close.clone()))
        .with_animation(
            "dialog-in",
            Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
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
                Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                |d, t| d.opacity(t),
            )
            .into_any_element(),
    )
}

/// A 28 px ghost close button pinned to a card's top-right corner.
fn close_button(id: &'static str, p: &super::theme::Palette) -> Stateful<Div> {
    let hover = p.hover_wash(if p.dark { 0.06 } else { 0.05 });
    div()
        .id(id)
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
    let o = overlays.read(cx);
    if o.toasts.is_empty() {
        return None;
    }
    let p = cx.palette();
    let mut stack = div()
        .absolute()
        .bottom(px(24.))
        .right(px(24.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .w(px(360.));
    for active in &o.toasts {
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
        let elapsed = active.shown_at.elapsed().as_secs_f32();
        let total = active.duration.as_secs_f32();
        let remaining = (1. - elapsed / total).clamp(0., 1.);
        let (message, detail) = match &toast.description {
            Some(desc) if !toast.title.is_empty() => (toast.title.clone(), Some(desc.clone())),
            Some(desc) => (desc.clone(), None),
            None => (toast.title.clone(), None),
        };
        let destructive = toast.variant == ToastVariant::Destructive;
        let overlays_close = overlays.clone();
        let group: SharedString = format!("toast-{id}").into();
        let close_hover = p.hover_wash(0.08);
        let card = div()
            .id(("toast", id as usize))
            .group(group.clone())
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
                            .line_height(px(20.))
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
                                    .text_size(px(TEXT_SMALL))
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
            .child(
                div()
                    .id("toast-close")
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_SM))
                    .opacity(0.)
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
                    d.opacity(t).mt(px(12. * (1. - t)))
                }
            },
        );
        stack = stack.child(card);
    }
    window.request_animation_frame();
    Some(stack.into_any_element())
}
