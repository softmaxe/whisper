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

use crate::i18n::t;
use crate::settings::PanelStartPosition;

use crate::app_state::{AppState, DictationSnapshot};
use crate::backend;
use crate::dictation::{Dictation, DictationNotice};
use crate::macos;

use super::components::{f32_tx, icon};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SLOW, DUR_SNAP, FONT_UI, FONT_VOICE, LINE_READING, PaletteExt,
    TEXT_READING, ease_out, ease_overshoot, ease_snap, progress,
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
const TOAST_W: f32 = 300.;
const FINISH: Duration = Duration::from_millis(DUR_BASE);
/// How long the pill stays for the delivered check: pop, hold, then exit.
const RESOLVE: Duration = Duration::from_millis(RESOLVE_EXIT_AT + DUR_SNAP);
const RESOLVE_EXIT_AT: u64 = 620;
const RECOVERY_HIDE: Duration = Duration::from_millis(5000);

// Waveform constants.
const BARS: usize = 10;
const BAR_W: f32 = 2.5;
const BAR_GAP: f32 = 3.;
const BAR_MIN: f32 = 3.;
const BAR_MAX: f32 = 18.;
const ENVELOPE: [f32; BARS] = [0.35, 0.55, 0.75, 0.92, 1., 1., 0.92, 0.75, 0.55, 0.35];
/// Input levels mapped onto the bars: the quiet end of a laptop mic's
/// speech range shows, and close speech fills them.
const LEVEL_FLOOR_DB: f32 = -55.;
const LEVEL_CEIL_DB: f32 = -20.;
/// Peak height of the idle breath while no speech is heard.
const BREATH: f32 = 0.05;
const RISE: f32 = 0.45;
const FALL: f32 = 0.16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Live,
    Sweep,
    Wave,
}

/// Maps an RMS level to 0..1 linearly in decibels between
/// [`LEVEL_FLOOR_DB`] and [`LEVEL_CEIL_DB`].
fn bar_level(rms: f32) -> f32 {
    if rms <= 0. {
        return 0.;
    }
    let db = 20. * rms.log10();
    ((db - LEVEL_FLOOR_DB) / (LEVEL_CEIL_DB - LEVEL_FLOOR_DB)).clamp(0., 1.)
}

fn live_target(rms: f32, index: usize, now_ms: f32) -> f32 {
    let phase = now_ms * 0.012 * (1. + index as f32 * 0.37) + index as f32 * 1.7;
    let drift = 0.5 + 0.5 * phase.sin();
    let level = bar_level(rms);
    let speech = level * ENVELOPE[index] * (0.55 + 0.45 * drift);
    let breath = BREATH * (0.5 + 0.5 * ((now_ms / 2400.) * TAU).sin());
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

/// The pill's one-shot motion at an instant, layered over the waveform.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PillMotion {
    scale: f32,
    alpha: f32,
    bars: f32,
    /// Scale and opacity of the delivered check, once it shows.
    check: Option<(f32, f32)>,
    running: bool,
}

/// Samples the enter pop (scale .8 → 1 with overshoot), the dip when
/// recording hands over to processing (1 → .94 → 1), and the delivered
/// sequence: the bars fade, a check pops in, the pill swells to 1.06, holds,
/// then shrinks to .9 as it fades. Each input is the time since that event.
fn pill_motion(
    entered: Option<Duration>,
    dipped: Option<Duration>,
    resolved: Option<Duration>,
    reduce: bool,
) -> PillMotion {
    let ms = |d: Duration| d.as_secs_f32() * 1000.;
    let mut m = PillMotion {
        scale: 1.,
        alpha: 1.,
        bars: 1.,
        check: None,
        running: false,
    };
    if reduce {
        if let Some(r) = resolved {
            m.bars = 0.;
            m.check = Some((1., 1.));
            if ms(r) >= RESOLVE_EXIT_AT as f32 {
                m.alpha = 0.;
            }
            // Still draws the frame where the check goes.
            m.running = r < RESOLVE;
        }
        return m;
    }
    if let Some(e) = entered.filter(|e| ms(*e) < DUR_BASE as f32) {
        m.scale *= 0.8 + 0.2 * ease_overshoot(progress(e, DUR_BASE));
        m.running = true;
    }
    if let Some(d) = dipped.map(ms).filter(|d| *d < 160.) {
        m.scale *= if d < 80. {
            1. - 0.06 * ease_out(d / 80.)
        } else {
            0.94 + 0.06 * ease_overshoot((d - 80.) / 80.)
        };
        m.running = true;
    }
    if let Some(r) = resolved {
        let r_ms = ms(r);
        m.bars = 1. - ease_snap(progress(r, DUR_SNAP));
        if r_ms > 60. {
            let k = ((r_ms - 60.) / 260.).clamp(0., 1.);
            m.check = Some((0.6 + 0.4 * ease_overshoot(k), ease_out(k)));
        }
        m.scale *= if r_ms < 80. {
            1. + 0.06 * ease_out(r_ms / 80.)
        } else if r_ms < 200. {
            1.06 - 0.06 * ease_overshoot((r_ms - 80.) / 120.)
        } else {
            1.
        };
        if r_ms >= RESOLVE_EXIT_AT as f32 {
            let x = ease_snap(((r_ms - RESOLVE_EXIT_AT as f32) / DUR_SNAP as f32).min(1.));
            m.scale *= 1. - 0.1 * x;
            m.alpha = 1. - x;
        }
        m.running = r < RESOLVE;
    }
    m
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
    /// Start of the enter pop, the processing dip and the delivered check.
    entered_at: Option<Instant>,
    /// The enter pop waits for the first frame drawn after the window is
    /// ordered in, so it plays on screen rather than before the window shows.
    enter_pending: bool,
    dipped_at: Option<Instant>,
    resolved_at: Option<Instant>,
    was_processing: bool,
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
            entered_at: None,
            enter_pending: false,
            dipped_at: None,
            resolved_at: None,
            was_processing: false,
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
        let processing = snapshot.is_processing;
        if active && !self.was_active {
            self.finishing_until = None;
            self.lanes = [0.; BARS];
            self.error = None;
            self.entered_at = None;
            self.enter_pending = true;
            self.dipped_at = None;
            self.resolved_at = None;
        }
        if active && processing && !self.was_processing && self.was_active {
            self.dipped_at = Some(Instant::now());
        }
        self.was_processing = active && processing;
        if !active && self.was_active {
            // Keep the window for the finish fade, or for the delivered check.
            let hold = if self.resolved_at.is_some() {
                RESOLVE
            } else {
                FINISH
            };
            self.finish_after(hold, window, cx);
        }
        self.was_active = active;
        self.update_visibility(window, cx);
        cx.notify();
    }

    /// Keeps the window up for `hold` after Dictation ends, then hides it.
    fn finish_after(&mut self, hold: Duration, window: &mut Window, cx: &mut Context<Self>) {
        self.finishing_until = Some(Instant::now() + hold);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(hold + Duration::from_millis(20))
                .await;
            let _ = this.update_in(cx, |this, window, cx| this.update_visibility(window, cx));
        })
        .detach();
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
            if !over {
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
                    title: crate::i18n::t_with("app.toasts.addedToDict", &[("words", &list)]),
                    description: None,
                    destructive: false,
                    success: true,
                    shown_at: Instant::now(),
                    duration: Duration::from_millis(6000),
                    undo_words: Some(words),
                    hint: false,
                });
            }
            DictationNotice::Delivered => {
                self.resolved_at = Some(Instant::now());
                // The phase is already idle in the shared state when this
                // arrives, though the pill may not have run its finish yet.
                // Start (or stretch) the finish here so the visibility check
                // below keeps the window up for the check.
                if !Self::snapshot(cx).is_active()
                    && (self.was_active || self.finishing_until.is_some_and(|t| Instant::now() < t))
                {
                    self.finish_after(RESOLVE, window, cx);
                }
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
        motion_fx: PillMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let finishing = !snapshot.is_active();
        let motion = if finishing {
            None
        } else if snapshot.is_processing {
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
                    .bg(hsla(0., 0., 1., opacity[i] * motion_fx.bars)),
            );
        }
        let processing = snapshot.is_processing;
        let check = motion_fx.check.map(|(scale, alpha)| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon("check", 15., hsla(0., 0., 1., 0.92 * alpha))
                        .with_transformation(Transformation::scale(size(scale, scale))),
                )
        });
        let scale = motion_fx.scale;
        // GPUI has no paint transforms on divs, so the pill scales by size
        // inside a fixed slot, which keeps it centered and the row still.
        let pill = div()
            .id("voice-pill")
            .relative()
            .flex_none()
            .w(px(PILL_W * scale))
            .h(px(PILL_H * scale))
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
            .when(!processing && !finishing, |d| d.cursor_pointer())
            .child(bars)
            .children(check)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.drag_origin = Some(e.position);
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
                    // A click on the finish or the delivered check starts nothing.
                    if this.drag_origin.take().is_some() && Self::snapshot(cx).is_active() {
                        Dictation::global(cx).update(cx, |d, cx| d.toggle_listening(cx));
                    }
                }),
            );
        div()
            .w(px(PILL_W))
            .h(px(PILL_H))
            .flex()
            .items_center()
            .justify_center()
            .child(pill)
            .into_any_element()
    }

    /// The cancel control and how far it has emerged: it pops in from .8
    /// over 200 ms and leaves in 60 ms.
    fn render_cancel(
        &mut self,
        visible: bool,
        recording: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, f32) {
        let t_in = f32_tx(
            "cancel-emerge",
            if visible { 1. } else { 0. },
            if visible { DUR_BASE } else { DUR_FAST },
            if visible { ease_out } else { ease_snap },
            window,
            cx,
        );
        let scale = 0.8 + 0.2 * t_in;
        let button = div()
            .id("pill-cancel")
            .flex_none()
            .size(px(CANCEL_SIZE * scale))
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
            .child(
                icon("x", 12., hsla(0., 0., 1., 0.85))
                    .with_transformation(Transformation::scale(size(scale, scale))),
            )
            .when(visible, |d| {
                d.on_click(cx.listener(move |_, _, _, cx| {
                    Dictation::global(cx).update(cx, |d, cx| {
                        if recording {
                            d.cancel_recording(cx)
                        } else {
                            d.cancel_processing(cx)
                        }
                    });
                }))
            });
        let slot = div()
            .size(px(CANCEL_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .child(button)
            .into_any_element();
        (slot, t_in)
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
                    Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
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
                    Animation::new(Duration::from_millis(DUR_SLOW)).with_easing(ease_out),
                    |d, t| d.opacity((t * 2.).min(1.)).mt(px(6. * (1. - t))),
                )
                .into_any_element(),
        )
    }

    fn render_toasts(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let mut stack = div().flex().flex_col().gap(px(6.)).w(px(TOAST_W));
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
                            // Let long descriptions wrap inside the card.
                            .min_w_0()
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
        let now = Instant::now();
        if self.enter_pending && self.shown {
            self.enter_pending = false;
            self.entered_at = Some(now);
        }
        let resolving = self.resolved_at.is_some_and(|t| now - t < RESOLVE);
        let motion_fx = pill_motion(
            if self.enter_pending {
                Some(Duration::ZERO)
            } else {
                self.entered_at.map(|t| now - t)
            },
            self.dipped_at.map(|t| now - t),
            self.resolved_at.map(|t| now - t),
            cx.reduce_motion(),
        );
        if motion_fx.running {
            window.request_animation_frame();
        }
        // The pill fades out for the finish and while an error/recovery card
        // owns the window; the delivered check runs its own exit.
        let pill_shown = (active || resolving) && !error_or_recovery;
        let pill_alpha = f32_tx(
            "pill-alpha",
            if pill_shown { 1. } else { 0. },
            if pill_shown { DUR_BASE } else { DUR_SNAP },
            if pill_shown { ease_out } else { ease_snap },
            window,
            cx,
        ) * motion_fx.alpha;
        let show_pill = (active || finishing) && pill_alpha > 0.001;
        let cancel_visible = self.hovered && (snapshot.is_recording || snapshot.is_processing);
        let recording = snapshot.is_recording;

        let mut root = div().size_full().relative().font_family(FONT_UI);

        if show_pill || cancel_visible {
            let pill = self.render_pill(&snapshot, motion_fx, window, cx);
            let (cancel, cancel_t) = self.render_cancel(cancel_visible, recording, window, cx);
            let cancel = div()
                .when(cancel_t < 0.001, |d| d.invisible())
                .child(cancel);
            let left_dock = dock == PanelStartPosition::BottomLeft;
            let mut row = div().flex().items_center().gap(px(CANCEL_GAP));
            // The cancel control emerges on the side away from the screen edge.
            if left_dock {
                row = row.child(pill).child(cancel);
            } else {
                row = row.child(cancel).child(pill);
            }
            let tracked = self.tracked(row).opacity(pill_alpha);
            let positioned = div().absolute().bottom(px(DOCK_INSET)).map(|d| match dock {
                PanelStartPosition::BottomRight => d.right(px(DOCK_INSET)),
                PanelStartPosition::BottomLeft => d.left(px(DOCK_INSET)),
                // Center the pill itself; the cancel slot sits to its left.
                PanelStartPosition::Center => {
                    d.left(WINDOW_SIZE.width / 2. - px(PILL_W / 2. + CANCEL_SIZE + CANCEL_GAP))
                }
            });
            root = root.child(positioned.child(tracked));

            // Tooltip above the pill while hovered.
            if self.hovered && active {
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
                            Animation::new(Duration::from_millis(DUR_SNAP)),
                            |d, t| d.opacity(t),
                        ),
                );
            }
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
                    .map(|d| match dock {
                        PanelStartPosition::Center => {
                            d.left((WINDOW_SIZE.width - px(TOAST_W)) / 2.)
                        }
                        _ => d.right(px(DOCK_INSET)),
                    })
                    .child(tracked),
            );
            window.request_animation_frame();
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{BREATH, DUR_BASE, RESOLVE, bar_level, live_target, pill_motion};

    fn at(ms: u64) -> Option<Duration> {
        Some(Duration::from_millis(ms))
    }

    fn db(level: f32) -> f32 {
        10f32.powf(level / 20.)
    }

    #[test]
    fn bar_level_maps_decibels_onto_the_bars() {
        assert_eq!(bar_level(0.), 0.);
        assert_eq!(bar_level(db(-70.)), 0.);
        assert!((bar_level(db(-55.))).abs() < 1e-4);
        assert!((bar_level(db(-37.5)) - 0.5).abs() < 1e-4);
        assert!((bar_level(db(-20.)) - 1.).abs() < 1e-4);
        assert_eq!(bar_level(1.), 1.);
        // Close speech into a laptop mic (about -30 dBFS) clears 70%.
        assert!(bar_level(db(-30.)) > 0.7);
    }

    #[test]
    fn silence_only_breathes_and_speech_stands_out() {
        let peak = |rms: f32| {
            (0..240)
                .map(|t| live_target(rms, 4, t as f32 * 10.))
                .fold(0., f32::max)
        };
        assert!(peak(0.) <= BREATH + 1e-4, "{}", peak(0.));
        assert!(peak(db(-30.)) > 0.6, "{}", peak(db(-30.)));
    }

    #[test]
    fn enter_pops_from_point_eight_past_one() {
        let start = pill_motion(at(0), None, None, false);
        assert!((start.scale - 0.8).abs() < 1e-3 && start.running);
        let peak = (1..20)
            .map(|i| pill_motion(at(i * 10), None, None, false).scale)
            .fold(0., f32::max);
        assert!(peak > 1.0, "{peak}");
        assert_eq!(pill_motion(at(DUR_BASE), None, None, false).scale, 1.);
    }

    #[test]
    fn processing_dips_then_recovers() {
        let low = pill_motion(None, at(80), None, false).scale;
        assert!((low - 0.94).abs() < 1e-3, "{low}");
        let done = pill_motion(None, at(160), None, false);
        assert_eq!(done.scale, 1.);
        assert!(!done.running);
    }

    #[test]
    fn delivered_swaps_bars_for_a_check_then_exits() {
        let early = pill_motion(None, None, at(80), false);
        assert!(early.scale > 1.05 && early.check.is_some() && early.bars < 0.5);
        let hold = pill_motion(None, None, at(400), false);
        assert_eq!((hold.scale, hold.alpha, hold.bars), (1., 1., 0.));
        assert_eq!(hold.check.map(|c| c.1), Some(1.));
        let gone = pill_motion(None, None, Some(RESOLVE), false);
        assert!(gone.alpha < 1e-3 && (gone.scale - 0.9).abs() < 1e-3 && !gone.running);
    }

    #[test]
    fn reduced_motion_keeps_scale_and_shows_the_check_still() {
        let m = pill_motion(at(10), at(10), at(100), true);
        assert_eq!((m.scale, m.alpha, m.bars), (1., 1., 0.));
        assert_eq!(m.check, Some((1., 1.)));
        assert_eq!(pill_motion(None, None, Some(RESOLVE), true).alpha, 0.);
    }
}
