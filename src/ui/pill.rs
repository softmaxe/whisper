//! The Recording pill window.
//!
//! A borderless, non-activating, transparent window docked near the bottom of
//! the screen. It has no idle presence: it appears for a Dictation and its
//! feedback and orders out afterwards. Clicks fall through everywhere except
//! over the pill and its panels.

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use whisper::i18n::t;
use whisper::settings::PanelStartPosition;

use crate::app_state::{AppState, DictationSnapshot};
use crate::backend;
use crate::dictation::{Dictation, DictationNotice};
use crate::macos;

use super::components::{f32_tx, icon};
use super::theme::{
    FONT_UI, FONT_VOICE, LINE_READING, PaletteExt, TEXT_READING, ease_grow, ease_out,
};

pub const WINDOW_SIZE: Size<Pixels> = Size {
    width: px(466.),
    height: px(560.),
};
const DOCK_INSET: f32 = 12.;
const SCREEN_MARGIN: f32 = 4.;
const PILL_W: f32 = 84.;
const PILL_H: f32 = 30.;
const CANCEL_SIZE: f32 = 28.;
const CANCEL_GAP: f32 = 8.;
const FINISH: Duration = Duration::from_millis(180);
const RECOVERY_HIDE: Duration = Duration::from_millis(5000);

// Waveform constants.
const BARS: usize = 10;
const BAR_W: f32 = 2.5;
const BAR_GAP: f32 = 3.;
const BAR_MIN: f32 = 3.;
const BAR_MAX: f32 = 18.;
const ENVELOPE: [f32; BARS] = [0.35, 0.55, 0.75, 0.92, 1., 1., 0.92, 0.75, 0.55, 0.35];
const RISE: f32 = 0.45;
const FALL: f32 = 0.16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Live,
    Sweep,
    Wave,
}

fn bar_level(rms: f32) -> f32 {
    ((rms.max(0.) * 8.).powf(0.75)).min(1.)
}

fn live_target(rms: f32, index: usize, now_ms: f32) -> f32 {
    let phase = now_ms * 0.012 * (1. + index as f32 * 0.37) + index as f32 * 1.7;
    let drift = 0.5 + 0.5 * phase.sin();
    let level = bar_level(rms);
    let speech = level * ENVELOPE[index] * (0.55 + 0.45 * drift);
    let breath = 0.12 * (0.5 + 0.5 * ((now_ms / 2400.) * TAU).sin());
    speech + (1. - level) * breath
}

fn sweep_opacity(index: usize, now_ms: f32) -> f32 {
    let overrun = 3.;
    let span = BARS as f32 + overrun * 2.;
    let head = (now_ms / 70.) % span - overrun;
    let d = index as f32 - head;
    0.16 + 0.84 * (-(d * d) / 2.2).exp()
}

fn wave_phase(index: usize, now_ms: f32) -> f32 {
    0.5 + 0.5 * (((now_ms - index as f32 * 69.) / 700.) * TAU).sin()
}

struct ErrorCard {
    title: String,
    description: String,
    transcript: Option<String>,
    shown_at: Instant,
    duration: Duration,
}

struct Recovery {
    text: String,
    copied: bool,
    copy_failed: bool,
    shown_at: Instant,
}

struct PillToast {
    title: String,
    description: Option<String>,
    destructive: bool,
    success: bool,
    shown_at: Instant,
    duration: Duration,
    undo_words: Option<Vec<String>>,
    hint: bool,
}

/// Duration of the dictation error card, by message length.
fn error_duration(title: &str, description: &str) -> Duration {
    let len = format!("{title} {description}").trim().chars().count();
    Duration::from_millis(if len <= 80 {
        3000
    } else if len <= 160 {
        4000
    } else {
        5000
    })
}

pub struct PillView {
    lanes: [f32; BARS],
    last_frame: Option<Instant>,
    started: Instant,
    hovered: bool,
    menu_open: bool,
    finishing_until: Option<Instant>,
    was_active: bool,
    error: Option<ErrorCard>,
    recovery: Option<Recovery>,
    /// Whether Escape is registered for the open copy-recovery panel.
    recovery_escape: bool,
    toasts: Vec<PillToast>,
    shown: bool,
    regions: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    drag_origin: Option<Point<Pixels>>,
    /// Whether the pointer poll is running; it only runs while the pill is shown.
    polling: bool,
}

impl PillView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        macos::make_borderless(window, cx);
        let state = AppState::global(cx);
        cx.observe_in(&state, window, |this, _, window, cx| this.sync(window, cx))
            .detach();
        let dictation = Dictation::global(cx);
        cx.subscribe_in(
            &dictation,
            window,
            |this, _, notice: &DictationNotice, window, cx| {
                this.on_notice(notice.clone(), window, cx)
            },
        )
        .detach();
        Self {
            lanes: [0.; BARS],
            last_frame: None,
            started: Instant::now(),
            hovered: false,
            menu_open: false,
            finishing_until: None,
            was_active: false,
            error: None,
            recovery: None,
            recovery_escape: false,
            toasts: Vec::new(),
            shown: false,
            regions: Rc::new(RefCell::new(Vec::new())),
            drag_origin: None,
            polling: false,
        }
    }

    fn snapshot(cx: &App) -> DictationSnapshot {
        AppState::global(cx).read(cx).dictation.clone()
    }

    fn has_content(&self, cx: &App) -> bool {
        Self::snapshot(cx).is_active()
            || self.finishing_until.is_some_and(|t| Instant::now() < t)
            || self.error.is_some()
            || self.recovery.is_some()
            || !self.toasts.is_empty()
    }

    fn position_window(&self, window: &mut Window, cx: &App) {
        let Some(display) = cx.primary_display() else {
            return;
        };
        let area = display.visible_bounds();
        let w = WINDOW_SIZE.width;
        let h = WINDOW_SIZE.height;
        let margin = px(SCREEN_MARGIN);
        let x = match AppState::settings(cx).panel_start_position {
            PanelStartPosition::BottomLeft => area.origin.x + margin,
            PanelStartPosition::Center => area.origin.x + (area.size.width - w) / 2.,
            PanelStartPosition::BottomRight => area.origin.x + area.size.width - w - margin,
        };
        let y = area.origin.y + area.size.height - h - margin;
        macos::set_origin(window, cx, point(x, y), h);
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = Self::snapshot(cx);
        let active = snapshot.is_active();
        if active && !self.was_active {
            self.finishing_until = None;
            self.lanes = [0.; BARS];
            self.error = None;
        }
        if !active && self.was_active {
            // Keep the window for the finish fade (VOICE_PILL_FINISH_MS).
            self.finishing_until = Some(Instant::now() + FINISH);
            self.menu_open = false;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(FINISH + Duration::from_millis(20))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| this.update_visibility(window, cx));
            })
            .detach();
        }
        self.was_active = active;
        self.update_visibility(window, cx);
        cx.notify();
    }

    fn update_visibility(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.has_content(cx);
        if want && !self.shown {
            self.position_window(window, cx);
            macos::set_ignores_mouse_events(window, true);
            macos::order_front(window, cx);
            self.shown = true;
            self.start_poll(window, cx);
        } else if !want && self.shown {
            macos::order_out(window, cx);
            self.shown = false;
            self.hovered = false;
        }
        cx.notify();
    }

    /// Polls the pointer so the transparent window only takes clicks over its
    /// visible parts. The loop ends once the pill hides, so an idle app does
    /// not wake up for it.
    fn start_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                let keep = this.update_in(cx, |this, window, cx| {
                    this.poll_pointer(window, cx);
                    this.polling = this.shown;
                    this.shown
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.shown {
            return;
        }
        // Expire timed surfaces.
        let now = Instant::now();
        let before = (
            self.error.is_some(),
            self.recovery.is_some(),
            self.toasts.len(),
        );
        let hovering_panel = self.hovered;
        if let Some(error) = &self.error
            && !hovering_panel
            && now.duration_since(error.shown_at) > error.duration
        {
            self.error = None;
        }
        if let Some(recovery) = &self.recovery
            && !hovering_panel
            && now.duration_since(recovery.shown_at) > RECOVERY_HIDE
        {
            self.recovery = None;
        }
        self.toasts
            .retain(|toast| now.duration_since(toast.shown_at) < toast.duration);
        if before
            != (
                self.error.is_some(),
                self.recovery.is_some(),
                self.toasts.len(),
            )
        {
            self.update_visibility(window, cx);
        }

        let Some(frame) = macos::window_frame(window) else {
            return;
        };
        let mouse = macos::mouse_location();
        let local = point(mouse.x - frame.origin.x, mouse.y - frame.origin.y);
        let over = self.regions.borrow().iter().any(|r| r.contains(&local));
        macos::set_ignores_mouse_events(window, !over);
        if over != self.hovered {
            self.hovered = over;
            if !over && !self.menu_open {
                self.drag_origin = None;
            }
            cx.notify();
        }
    }

    fn on_notice(&mut self, notice: DictationNotice, window: &mut Window, cx: &mut Context<Self>) {
        match notice {
            DictationNotice::DismissError => self.error = None,
            DictationNotice::DismissRecovery => self.recovery = None,
            DictationNotice::Error { title, description } => {
                let duration = error_duration(&title, &description);
                self.error = Some(ErrorCard {
                    title,
                    description,
                    transcript: None,
                    shown_at: Instant::now(),
                    duration,
                });
            }
            DictationNotice::NoAudio => {
                let title = t("dictation.noAudio.title");
                let description = t("dictation.noAudio.description");
                let duration = error_duration(&title, &description);
                self.error = Some(ErrorCard {
                    title,
                    description,
                    transcript: None,
                    shown_at: Instant::now(),
                    duration,
                });
            }
            DictationNotice::PushForceStopped {
                kept_in_clipboard,
                transcript,
            } => {
                let title = t("dictation.pushForceStopped.title");
                let description = t(if kept_in_clipboard {
                    "dictation.pushForceStopped.description"
                } else {
                    "dictation.pushForceStopped.descriptionClipboardFailed"
                });
                let duration = error_duration(&title, &description);
                self.error = Some(ErrorCard {
                    title,
                    description,
                    transcript: Some(transcript).filter(|t| !t.trim().is_empty()),
                    shown_at: Instant::now(),
                    duration,
                });
            }
            DictationNotice::DeliveryFallback { text, copied } => {
                self.recovery = Some(Recovery {
                    text,
                    copied,
                    copy_failed: false,
                    shown_at: Instant::now(),
                });
            }
            DictationNotice::CleanupFailed { title, description } => {
                self.toasts.push(PillToast {
                    title,
                    description,
                    destructive: true,
                    success: false,
                    shown_at: Instant::now(),
                    duration: Duration::from_millis(10_000),
                    undo_words: None,
                    hint: false,
                });
            }
            DictationNotice::LearnedWords(words) => {
                let list = words
                    .iter()
                    .map(|w| format!("“{w}”"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.toasts.push(PillToast {
                    title: whisper::i18n::t_with("app.toasts.addedToDict", &[("words", &list)]),
                    description: None,
                    destructive: false,
                    success: true,
                    shown_at: Instant::now(),
                    duration: Duration::from_millis(6000),
                    undo_words: Some(words),
                    hint: false,
                });
            }
            DictationNotice::HotkeyHint { hold } => {
                // Each hint replaces the last, so repeated presses do not stack.
                self.toasts.retain(|t| !t.hint);
                let key = if hold { "push" } else { "tap" };
                self.toasts.push(PillToast {
                    title: t(&format!("app.toasts.hotkeyHint.{key}.title")),
                    description: Some(t(&format!("app.toasts.hotkeyHint.{key}.description"))),
                    destructive: false,
                    success: false,
                    shown_at: Instant::now(),
                    duration: Duration::from_millis(3000),
                    undo_words: None,
                    hint: true,
                });
            }
        }
        self.update_visibility(window, cx);
    }

    fn dock(cx: &App) -> PanelStartPosition {
        AppState::settings(cx).panel_start_position
    }

    /// Wraps an interactive surface so its bounds count as clickable.
    fn tracked(&self, el: impl IntoElement) -> Div {
        let regions = self.regions.clone();
        div().relative().child(el).child(
            canvas(
                move |bounds, _, _| regions.borrow_mut().push(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    fn update_lanes(&mut self, motion: Option<Motion>, level: Option<f32>) -> [f32; BARS] {
        let now = Instant::now();
        let now_ms = now.duration_since(self.started).as_secs_f32() * 1000.;
        let frames = self
            .last_frame
            .map(|last| (now.duration_since(last).as_secs_f32() * 60.).min(4.))
            .unwrap_or(1.);
        self.last_frame = Some(now);
        let mut opacity = [1.; BARS];
        let Some(motion) = motion else { return opacity };
        for i in 0..BARS {
            let target = match motion {
                Motion::Live => match level {
                    Some(rms) => live_target(rms, i, now_ms),
                    None => 0.,
                },
                Motion::Sweep => {
                    opacity[i] = sweep_opacity(i, now_ms);
                    0.16 * opacity[i]
                }
                Motion::Wave => {
                    let phase = wave_phase(i, now_ms);
                    opacity[i] = 0.45 + 0.55 * phase;
                    0.4 * phase
                }
            };
            let rate = if target > self.lanes[i] { RISE } else { FALL };
            self.lanes[i] += (target - self.lanes[i]) * (1. - (1. - rate).powf(frames));
        }
        opacity
    }

    fn render_pill(
        &mut self,
        snapshot: &DictationSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let finishing = !snapshot.is_active();
        let motion = if finishing {
            None
        } else if snapshot.is_processing || snapshot.is_stopping {
            Some(Motion::Wave)
        } else if snapshot.is_recording {
            Some(Motion::Live)
        } else {
            Some(Motion::Sweep)
        };
        if motion.is_some() {
            window.request_animation_frame();
        }
        let level = Dictation::global(cx).read(cx).level();
        let opacity = self.update_lanes(motion, level);
        let mut bars = div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(BAR_GAP))
            .w(px(52.))
            .h(px(24.));
        for i in 0..BARS {
            bars = bars.child(
                div()
                    .w(px(BAR_W))
                    .h(px(BAR_MIN + self.lanes[i] * (BAR_MAX - BAR_MIN)))
                    .rounded_full()
                    .bg(hsla(0., 0., 1., opacity[i])),
            );
        }
        let processing = snapshot.is_processing || snapshot.is_stopping;
        div()
            .id("voice-pill")
            .w(px(PILL_W))
            .h(px(PILL_H))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hsla(0., 0., 1., 0.16))
            .bg(rgb(0x0c0c0e))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.28),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            }])
            .when(!processing, |d| d.cursor_pointer())
            .child(bars)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.menu_open = false;
                    this.drag_origin = Some(e.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, _| {
                // Past a 5px threshold the press becomes a window drag.
                if let (Some(origin), Some(MouseButton::Left)) =
                    (this.drag_origin, e.pressed_button)
                {
                    let d = e.position - origin;
                    if f32::from(d.x).powi(2) + f32::from(d.y).powi(2) > 25. {
                        this.drag_origin = None;
                        window.start_window_move();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag_origin.take().is_some() {
                        Dictation::global(cx).update(cx, |d, cx| d.toggle_listening(cx));
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.menu_open = !this.menu_open;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    fn render_cancel(
        &mut self,
        visible: bool,
        recording: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t_in = f32_tx(
            "cancel-emerge",
            if visible { 1. } else { 0. },
            300,
            ease_grow,
            window,
            cx,
        );
        div()
            .id("pill-cancel")
            .size(px(CANCEL_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hsla(0., 0., 1., 0.16))
            .bg(rgb(0x0c0c0e))
            .opacity(t_in)
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x1f1f22)))
            .child(icon("x", 12., hsla(0., 0., 1., 0.85)))
            .on_click(cx.listener(move |_, _, _, cx| {
                Dictation::global(cx).update(cx, |d, cx| {
                    if recording {
                        d.cancel_recording(cx)
                    } else {
                        d.cancel_processing(cx)
                    }
                });
            }))
            .into_any_element()
    }

    fn render_menu(&mut self, recording: bool, cx: &mut Context<Self>) -> AnyElement {
        let p = cx.palette();
        let item = |id: &'static str, label: String| {
            div()
                .id(id)
                .w_full()
                .px(px(12.))
                .py(px(8.))
                .text_size(px(14.))
                .cursor_pointer()
                .hover(move |s| s.bg(p.muted))
                .child(label)
        };
        div()
            .w(px(192.))
            .rounded(px(8.))
            .border_1()
            .border_color(p.border)
            .bg(p.popover)
            .text_color(p.foreground)
            .shadow_lg()
            .overflow_hidden()
            .child(
                item(
                    "menu-toggle",
                    if recording {
                        t("app.commandMenu.stopListening")
                    } else {
                        t("app.commandMenu.startListening")
                    },
                )
                .font_weight(FontWeight::MEDIUM)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.menu_open = false;
                    Dictation::global(cx).update(cx, |d, cx| d.toggle_listening(cx));
                })),
            )
            .child(div().h(px(1.)).bg(p.border))
            .child(
                item("menu-hide", t("app.commandMenu.hideForNow")).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.menu_open = false;
                        this.finishing_until = None;
                        macos::order_out(window, cx);
                        this.shown = false;
                        cx.notify();
                    },
                )),
            )
            .into_any_element()
    }

    fn render_error(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette();
        let error = self.error.as_ref()?;
        let elapsed = error.shown_at.elapsed().as_secs_f32();
        let remaining = (1. - elapsed / error.duration.as_secs_f32()).clamp(0., 1.);
        let transcript = error.transcript.clone();
        let action = |id: &'static str, icon_name: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .h(px(32.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .px(px(16.))
                .rounded(px(6.))
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(primary, |d| {
                    d.bg(p.foreground)
                        .text_color(p.background)
                        .hover(move |s| s.bg(p.foreground.opacity(0.9)))
                })
                .when(!primary, |d| {
                    d.bg(p.foreground.opacity(0.15))
                        .text_color(p.foreground)
                        .hover(move |s| s.bg(p.foreground.opacity(0.2)))
                })
                .child(icon(
                    icon_name,
                    14.,
                    if primary { p.background } else { p.foreground },
                ))
                .child(label)
        };
        let retry = action("error-retry", "rotate-ccw", t("common.retry"), true).on_click(
            cx.listener(|this, _, _, cx| {
                this.error = None;
                Dictation::global(cx).update(cx, |d, cx| d.start(cx));
            }),
        );
        let text = div()
            .min_w_0()
            .flex_1()
            .px(px(8.))
            .py(px(4.))
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(px(22.))
                    .text_color(p.foreground)
                    .child(error.title.clone()),
            )
            .when(!error.description.is_empty(), |d| {
                d.child(
                    div()
                        .mt(px(4.))
                        .text_size(px(14.))
                        .line_height(px(19.25))
                        .text_color(p.muted_foreground)
                        .child(error.description.clone()),
                )
            });
        let body = if let Some(transcript) = transcript {
            let view = action(
                "error-transcript",
                "scroll-text",
                t("dictation.errorActions.viewTranscript"),
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.error = None;
                this.recovery = Some(Recovery {
                    text: transcript.clone(),
                    copied: false,
                    copy_failed: false,
                    shown_at: Instant::now(),
                });
                cx.notify();
            }));
            div().px(px(8.)).py(px(12.)).child(text).child(
                div()
                    .mt(px(12.))
                    .flex()
                    .gap(px(8.))
                    .child(retry.flex_1())
                    .child(view.flex_1()),
            )
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(12.))
                .child(text)
                .child(retry)
        };
        Some(
            div()
                .id("dictation-error")
                .relative()
                .w_full()
                .rounded(px(10.))
                .border_1()
                .border_color(p.border)
                .bg(p.surface_0)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.5),
                    offset: point(px(0.), px(20.)),
                    blur_radius: px(40.),
                    spread_radius: px(-16.),
                    inset: false,
                }])
                .child(body)
                // The countdown traces the card's top edge.
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(10.))
                        .h(px(2.))
                        .w(relative(remaining * 0.95))
                        .bg(p.foreground.opacity(0.85))
                        .rounded_full(),
                )
                .with_animation(
                    "error-in",
                    Animation::new(Duration::from_millis(200)).with_easing(ease_out),
                    |d, t| d.opacity(t).mt(px(4. * (1. - t))),
                )
                .into_any_element(),
        )
    }

    fn render_recovery(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = cx.palette();
        let recovery = self.recovery.as_ref()?;
        let ready = !recovery.copy_failed && recovery.copied;
        let notice = if recovery.copy_failed {
            "transcriptionPreview.recovery.copyFailed"
        } else if ready {
            "transcriptionPreview.recovery.pasteFailedCopied"
        } else {
            "transcriptionPreview.recovery.pasteFailed"
        };
        let text = recovery.text.clone();
        let failed = recovery.copy_failed;
        Some(
            div()
                .id("copy-recovery")
                .w_full()
                .max_h(px(420.))
                .flex()
                .flex_col()
                .rounded(px(16.))
                .border_1()
                .border_color(p.border)
                .bg(p.surface_0)
                .shadow_xl()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .px(px(16.))
                        .pt(px(16.))
                        .pb(px(12.))
                        .border_b_1()
                        .border_color(p.border.opacity(0.4))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .line_height(px(20.))
                                        .text_color(p.foreground)
                                        .child(t("transcriptionPreview.recovery.title")),
                                )
                                .child(
                                    div()
                                        .mt(px(4.))
                                        .text_size(px(12.))
                                        .line_height(px(20.))
                                        .text_color(if failed {
                                            p.destructive
                                        } else {
                                            p.muted_foreground
                                        })
                                        .child(t(notice)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .id("recovery-close")
                                        .size(px(28.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .hover(move |s| s.bg(p.surface_2))
                                        .child(icon("x", 16., p.muted_foreground))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.recovery = None;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(p.muted_foreground)
                                        .child("Esc"),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("recovery-text")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(16.))
                        .py(px(12.))
                        .font_family(FONT_VOICE)
                        .text_size(px(TEXT_READING))
                        .line_height(px(LINE_READING))
                        .text_color(p.foreground)
                        .child(text.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .px(px(16.))
                        .py(px(12.))
                        .border_t_1()
                        .border_color(p.border.opacity(0.4))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(p.muted_foreground)
                                .child(if ready {
                                    t("transcriptionPreview.recovery.pasteHint")
                                } else {
                                    String::new()
                                }),
                        )
                        .child(if ready {
                            div()
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(p.muted_foreground)
                                .child(icon("check", 14., p.success))
                                .child(t("transcriptionPreview.recovery.copied"))
                                .into_any_element()
                        } else {
                            div()
                                .id("recovery-copy")
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(p.border.opacity(0.8))
                                .bg(p.surface_3)
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.surface_raised).border_color(p.border_hover))
                                .child(icon("copy", 14., p.foreground))
                                .child(t("transcriptionPreview.recovery.copy"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let ok = backend::write_clipboard(&text);
                                    if let Some(r) = this.recovery.as_mut() {
                                        r.copied = ok;
                                        r.copy_failed = !ok;
                                    }
                                    cx.notify();
                                }))
                                .into_any_element()
                        }),
                )
                .with_animation(
                    "recovery-in",
                    Animation::new(Duration::from_millis(420)).with_easing(ease_out),
                    |d, t| d.opacity((t * 2.).min(1.)).mt(px(6. * (1. - t))),
                )
                .into_any_element(),
        )
    }

    fn render_toasts(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let mut stack = div().flex().flex_col().gap(px(6.)).w(px(300.));
        for (i, toast) in self.toasts.iter().enumerate() {
            let accent: Hsla = if toast.destructive {
                rgb(0xf87171).into()
            } else if toast.success {
                rgb(0x34d399).into()
            } else {
                hsla(0., 0., 1., 0.2)
            };
            let undo = toast.undo_words.clone();
            let remaining = (1.
                - toast.shown_at.elapsed().as_secs_f32() / toast.duration.as_secs_f32())
            .clamp(0., 1.);
            stack = stack.child(
                div()
                    .id(("pill-toast", i))
                    .relative()
                    .flex()
                    .rounded(px(5.))
                    .border_1()
                    .border_color(rgb(0x202225))
                    .bg(rgb(0x0a0b0e))
                    .shadow_lg()
                    .child(div().w(px(2.)).flex_none().bg(accent))
                    .child(
                        div()
                            .flex_1()
                            .px(px(10.))
                            .py(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(hsla(0., 0., 1., 0.9))
                                    .child(toast.title.clone()),
                            )
                            .when_some(toast.description.clone(), |d, desc| {
                                d.child(
                                    div()
                                        .mt(px(4.))
                                        .text_size(px(12.))
                                        .text_color(hsla(0., 0., 1., 0.45))
                                        .child(desc),
                                )
                            }),
                    )
                    .when_some(undo, |d, words| {
                        d.child(
                            div()
                                .id(("toast-undo", i))
                                .flex_none()
                                .self_center()
                                .mr(px(8.))
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(4.))
                                .bg(hsla(0., 0., 1., 0.1))
                                .text_size(px(12.))
                                .text_color(hsla(0., 0., 1., 0.9))
                                .cursor_pointer()
                                .hover(|s| s.bg(hsla(0., 0., 1., 0.16)))
                                .child(t("app.toasts.undo"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    backend::undo_learned_words(&words, cx);
                                    this.toasts
                                        .retain(|t| t.undo_words.as_ref() != Some(&words));
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(2.))
                            .h(px(1.))
                            .w(relative(remaining))
                            .bg(accent.opacity(0.3)),
                    ),
            );
        }
        Some(stack.into_any_element())
    }
}

impl Render for PillView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.regions.borrow_mut().clear();
        if self.recovery.is_some() != self.recovery_escape {
            self.recovery_escape = self.recovery.is_some();
            backend::set_recovery_cancel_hotkey(self.recovery_escape);
        }
        let snapshot = Self::snapshot(cx);
        let dock = Self::dock(cx);
        let active = snapshot.is_active();
        let finishing = self.finishing_until.is_some_and(|t| Instant::now() < t);
        let error_or_recovery = self.error.is_some() || self.recovery.is_some();
        // The pill fades out for the finish and while an error/recovery card owns the window.
        let pill_alpha = f32_tx(
            "pill-alpha",
            if active && !error_or_recovery { 1. } else { 0. },
            150,
            ease_out,
            window,
            cx,
        );
        let show_pill = (active || finishing) && pill_alpha > 0.001;
        let cancel_visible = self.hovered && (snapshot.is_recording || snapshot.is_processing);
        let recording = snapshot.is_recording;

        let mut root = div().size_full().relative().font_family(FONT_UI);

        if show_pill || cancel_visible {
            let pill = self.render_pill(&snapshot, window, cx);
            let cancel = self.render_cancel(cancel_visible, recording, window, cx);
            let left_dock = dock == PanelStartPosition::BottomLeft;
            let mut row = div().flex().items_center().gap(px(CANCEL_GAP));
            // The cancel control emerges on the side away from the screen edge.
            if left_dock {
                row = row
                    .child(pill)
                    .child(div().when(!cancel_visible, |d| d.invisible()).child(cancel));
            } else {
                row = row
                    .child(div().when(!cancel_visible, |d| d.invisible()).child(cancel))
                    .child(pill);
            }
            let tracked = self.tracked(row).opacity(pill_alpha);
            let positioned = div().absolute().bottom(px(DOCK_INSET)).map(|d| match dock {
                PanelStartPosition::BottomRight => d.right(px(DOCK_INSET)),
                PanelStartPosition::BottomLeft => d.left(px(DOCK_INSET)),
                PanelStartPosition::Center => {
                    d.left(WINDOW_SIZE.width / 2. - px((PILL_W + CANCEL_SIZE + CANCEL_GAP) / 2.))
                }
            });
            root = root.child(positioned.child(tracked));

            // Tooltip above the pill while hovered.
            if self.hovered && !self.menu_open && active {
                let p = cx.palette();
                let label = if snapshot.is_recording {
                    t("app.mic.recording")
                } else if snapshot.is_processing {
                    t("app.mic.processing")
                } else {
                    backend::format_hotkey_label(&AppState::settings(cx).effective_dictation_key())
                };
                root = root.child(
                    div()
                        .absolute()
                        .bottom(px(DOCK_INSET + PILL_H + 8.))
                        .map(|d| match dock {
                            PanelStartPosition::BottomLeft => d.left(px(DOCK_INSET)),
                            _ => d.right(px(DOCK_INSET)),
                        })
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(p.toast_surface)
                        .text_size(px(11.))
                        .text_color(hsla(0., 0., 0.96, 1.))
                        .child(label)
                        .with_animation(
                            "pill-tooltip",
                            Animation::new(Duration::from_millis(150)),
                            |d, t| d.opacity(t),
                        ),
                );
            }
        }

        if self.menu_open && (active || finishing) {
            let menu = self.render_menu(recording, cx);
            let tracked = self.tracked(menu);
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(DOCK_INSET + PILL_H + 12.))
                    .map(|d| match dock {
                        PanelStartPosition::BottomLeft => d.left(px(DOCK_INSET)),
                        _ => d.right(px(DOCK_INSET)),
                    })
                    .child(tracked),
            );
        }

        let card = self.render_error(cx).or_else(|| self.render_recovery(cx));
        if let Some(card) = card {
            let width = WINDOW_SIZE.width - px(2. * DOCK_INSET);
            let tracked = self.tracked(card).w(width);
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(DOCK_INSET))
                    .left(px(DOCK_INSET))
                    .child(tracked),
            );
            window.request_animation_frame();
        }

        if let Some(toasts) = self.render_toasts(cx) {
            let tracked = self.tracked(toasts);
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(if active {
                        DOCK_INSET + PILL_H + 16.
                    } else {
                        DOCK_INSET
                    }))
                    .right(px(DOCK_INSET))
                    .child(tracked),
            );
            window.request_animation_frame();
        }
        root
    }
}
