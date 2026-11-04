//! The Recording pill window.
//!
//! A borderless, non-activating, transparent window docked near the bottom of
//! the screen. It has no idle presence: it appears for a Dictation and its
//! feedback and orders out afterwards. Clicks fall through everywhere except
//! over the pill and its panels.
//!
//! The pill, its panels and its hints share one dark HUD surface in both
//! themes, like the system volume HUD, so they read as one component over any
//! wallpaper or app. The window never takes keyboard focus: Escape reaches it
//! through a global shortcut registered only while something can take it.

use std::cell::RefCell;
use std::f32::consts::{FRAC_PI_2, TAU};
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

use super::components::{KeyboardFocusable, f32_tx, icon, kbd};
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SLOW, DUR_SNAP, FONT_UI, FONT_VOICE, LINE_READING, LINE_SMALL, Palette,
    RADIUS_LG, RADIUS_MD, RADIUS_XL, TEXT_BODY, TEXT_READING, TEXT_SMALL, ease_out, ease_overshoot,
    ease_snap, progress,
};

pub const WINDOW_SIZE: Size<Pixels> = Size {
    width: px(466.),
    height: px(560.),
};
const DOCK_INSET: f32 = 12.;
const SCREEN_MARGIN: f32 = 4.;
const PILL_W: f32 = 92.;
const PILL_H: f32 = 30.;
const CANCEL_SIZE: f32 = 28.;
const CANCEL_GAP: f32 = 8.;
const TOAST_W: f32 = 300.;
/// Inner padding of the error and recovery panels.
const PANEL_PAD: f32 = 16.;
/// The dismiss button of a panel, ringed by its countdown.
const DISMISS_SIZE: f32 = 24.;
const FINISH: Duration = Duration::from_millis(DUR_BASE);
/// How long the pill stays for the delivered check: pop, hold, then exit.
const RESOLVE: Duration = Duration::from_millis(RESOLVE_EXIT_AT + DUR_SNAP);
const RESOLVE_EXIT_AT: u64 = 620;
/// The recovery panel holds the user's words; it stays longer while they
/// still need to copy them than once they are on the clipboard.
const RECOVERY_COPIED: Duration = Duration::from_millis(6000);
const RECOVERY_UNCOPIED: Duration = Duration::from_millis(12_000);

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
/// The record dot that says the microphone is live.
const REC_DOT: f32 = 6.;
const REC_GAP: f32 = 7.;
/// The three-dot motif that replaces the bars while transcribing.
const THINK_DOTS: usize = 3;
const THINK_DOT: f32 = 4.;
const THINK_GAP: f32 = 5.;

/// The pill's surfaces always use the dark palette; see the module docs.
fn hud() -> Palette {
    Palette::dark()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    /// Bars follow the input level.
    Live,
    /// A dim highlight sweeps the bars while the microphone opens.
    Sweep,
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

/// Opacity of transcribing dot `index`: a highlight that walks left to right.
fn think_opacity(index: usize, now_ms: f32, reduce: bool) -> f32 {
    if reduce {
        return 0.8;
    }
    let phase = ((now_ms / 1100.) - index as f32 * 0.18) * TAU;
    0.3 + 0.7 * (0.5 + 0.5 * phase.sin()).powi(2)
}

/// The record dot's slow pulse; held steady when motion is reduced.
fn rec_pulse(now_ms: f32, reduce: bool) -> f32 {
    if reduce {
        return 1.;
    }
    0.7 + 0.3 * (0.5 + 0.5 * ((now_ms / 1600.) * TAU).cos())
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

/// An auto-dismiss timer that stands still while the pointer rests on the
/// pill's surfaces, so nothing leaves while it is being read.
#[derive(Clone, Copy, Debug)]
struct Countdown {
    started: Instant,
    paused_at: Option<Instant>,
    duration: Duration,
}

impl Countdown {
    fn new(duration: Duration, now: Instant) -> Self {
        Self {
            started: now,
            paused_at: None,
            duration,
        }
    }

    fn elapsed(&self, now: Instant) -> Duration {
        self.paused_at
            .unwrap_or(now)
            .saturating_duration_since(self.started)
    }

    fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(at)) => {
                self.started += now.saturating_duration_since(at);
                self.paused_at = None;
            }
            _ => {}
        }
    }

    /// The share of the countdown still to run, from 1 down to 0.
    fn remaining(&self, now: Instant) -> f32 {
        (1. - self.elapsed(now).as_secs_f32() / self.duration.as_secs_f32()).clamp(0., 1.)
    }

    fn expired(&self, now: Instant) -> bool {
        self.elapsed(now) >= self.duration
    }
}

struct ErrorCard {
    title: String,
    description: String,
    transcript: Option<String>,
    countdown: Countdown,
}

impl ErrorCard {
    /// A card shown from now, on screen for as long as its text needs.
    fn new(title: String, description: String, transcript: Option<String>) -> Self {
        let duration = error_duration(&title, &description);
        Self {
            title,
            description,
            transcript,
            countdown: Countdown::new(duration, Instant::now()),
        }
    }
}

struct Recovery {
    text: String,
    copied: bool,
    copy_failed: bool,
    countdown: Countdown,
}

impl Recovery {
    fn new(text: String, copied: bool) -> Self {
        let duration = if copied {
            RECOVERY_COPIED
        } else {
            RECOVERY_UNCOPIED
        };
        Self {
            text,
            copied,
            copy_failed: false,
            countdown: Countdown::new(duration, Instant::now()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Destructive,
    Success,
    Hint,
}

struct PillToast {
    title: String,
    description: Option<String>,
    kind: ToastKind,
    countdown: Countdown,
    undo_words: Option<Vec<String>>,
}

/// Duration of the dictation error card, by message length.
fn error_duration(title: &str, description: &str) -> Duration {
    let len = format!("{title} {description}").trim().chars().count();
    Duration::from_millis(if len <= 80 {
        4000
    } else if len <= 160 {
        5000
    } else {
        6000
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
    /// Whether Escape is registered to dismiss the open error or recovery panel.
    panel_escape: bool,
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
            panel_escape: false,
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
            self.set_hovered(false);
        }
        cx.notify();
    }

    /// Records whether the pointer rests on a surface and holds every
    /// countdown still while it does.
    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
        let now = Instant::now();
        let countdowns = self
            .error
            .iter_mut()
            .map(|e| &mut e.countdown)
            .chain(self.recovery.iter_mut().map(|r| &mut r.countdown))
            .chain(self.toasts.iter_mut().map(|t| &mut t.countdown));
        for countdown in countdowns {
            countdown.set_paused(hovered, now);
        }
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
        // Expire timed surfaces. Paused countdowns never expire.
        let now = Instant::now();
        let before = (
            self.error.is_some(),
            self.recovery.is_some(),
            self.toasts.len(),
        );
        if self
            .error
            .as_ref()
            .is_some_and(|e| e.countdown.expired(now))
        {
            self.error = None;
        }
        if self
            .recovery
            .as_ref()
            .is_some_and(|r| r.countdown.expired(now))
        {
            self.recovery = None;
        }
        self.toasts.retain(|toast| !toast.countdown.expired(now));
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
            self.set_hovered(over);
            if !over {
                self.drag_origin = None;
            }
            cx.notify();
        }
    }

    fn push_toast(
        &mut self,
        title: String,
        description: Option<String>,
        kind: ToastKind,
        duration: Duration,
        undo_words: Option<Vec<String>>,
    ) {
        self.toasts.push(PillToast {
            title,
            description,
            kind,
            countdown: Countdown::new(duration, Instant::now()),
            undo_words,
        });
    }

    fn on_notice(&mut self, notice: DictationNotice, window: &mut Window, cx: &mut Context<Self>) {
        match notice {
            DictationNotice::DismissError => self.error = None,
            // Sent when Escape is pressed. It closes the panel on top: an
            // error card covers the recovery panel while both are open.
            DictationNotice::DismissRecovery => {
                if self.error.take().is_none() {
                    self.recovery = None;
                }
            }
            DictationNotice::Error { title, description } => {
                self.error = Some(ErrorCard::new(title, description, None));
            }
            DictationNotice::NoAudio => {
                self.error = Some(ErrorCard::new(
                    t("dictation.noAudio.title"),
                    t("dictation.noAudio.description"),
                    None,
                ));
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
                self.error = Some(ErrorCard::new(
                    title,
                    description,
                    Some(transcript).filter(|t| !t.trim().is_empty()),
                ));
            }
            DictationNotice::DeliveryFallback { text, copied } => {
                self.recovery = Some(Recovery::new(text, copied));
            }
            DictationNotice::CleanupFailed { title, description } => {
                self.push_toast(
                    title,
                    description,
                    ToastKind::Destructive,
                    Duration::from_millis(10_000),
                    None,
                );
            }
            DictationNotice::LearnedWords(words) => {
                let list = words
                    .iter()
                    .map(|w| format!("“{w}”"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.push_toast(
                    crate::i18n::t_with("app.toasts.addedToDict", &[("words", &list)]),
                    None,
                    ToastKind::Success,
                    Duration::from_millis(6000),
                    Some(words),
                );
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
                self.toasts.retain(|t| t.kind != ToastKind::Hint);
                let key = if hold { "push" } else { "tap" };
                self.push_toast(
                    t(&format!("app.toasts.hotkeyHint.{key}.title")),
                    Some(t(&format!("app.toasts.hotkeyHint.{key}.description"))),
                    ToastKind::Hint,
                    Duration::from_millis(4000),
                    None,
                );
            }
        }
        // A surface that appears under the pointer starts out paused.
        self.set_hovered(self.hovered);
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

    fn update_lanes(
        &mut self,
        motion: Option<Motion>,
        level: Option<f32>,
        reduce: bool,
    ) -> [f32; BARS] {
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
                    opacity[i] = if reduce {
                        0.4
                    } else {
                        sweep_opacity(i, now_ms)
                    };
                    0.
                }
            };
            let rate = if target > self.lanes[i] { RISE } else { FALL };
            self.lanes[i] += (target - self.lanes[i]) * (1. - (1. - rate).powf(frames));
        }
        opacity
    }

    /// The pill. Each phase has its own silhouette, so a still frame tells
    /// them apart without relying on color: a record dot beside level bars
    /// while the microphone is live, a dim sweep over the bars while it
    /// opens, and three pulsing dots alone while transcribing.
    fn render_pill(
        &mut self,
        snapshot: &DictationSnapshot,
        motion_fx: PillMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = hud();
        let reduce = cx.reduce_motion();
        let finishing = !snapshot.is_active();
        let recording = snapshot.is_recording;
        let processing = snapshot.is_processing;
        let motion = if finishing {
            None
        } else if recording || processing {
            Some(Motion::Live)
        } else {
            Some(Motion::Sweep)
        };
        if motion.is_some() {
            window.request_animation_frame();
        }
        let now_ms = self.started.elapsed().as_secs_f32() * 1000.;
        let level = Dictation::global(cx).read(cx).level();
        let opacity = self.update_lanes(motion, level, reduce);
        let thinking = f32_tx(
            "pill-thinking",
            if processing { 1. } else { 0. },
            if reduce { DUR_FAST } else { DUR_BASE },
            ease_out,
            window,
            cx,
        );
        let bars_alpha = (1. - thinking) * motion_fx.bars;

        let mut bars = div().flex().items_center().gap(px(BAR_GAP)).h(px(24.));
        for i in 0..BARS {
            bars = bars.child(
                div()
                    .w(px(BAR_W))
                    .h(px(BAR_MIN + self.lanes[i] * (BAR_MAX - BAR_MIN)))
                    .rounded_full()
                    .bg(p.white.opacity(opacity[i] * bars_alpha)),
            );
        }
        // The dot is red only while the microphone is live; while it opens
        // the dot waits in a dim neutral so the bars do not shift.
        let dot_color = if recording {
            p.destructive
                .opacity(rec_pulse(now_ms, reduce) * bars_alpha)
        } else {
            p.white.opacity(0.3 * bars_alpha)
        };
        let meter = div()
            .flex()
            .items_center()
            .gap(px(REC_GAP))
            .child(div().size(px(REC_DOT)).rounded_full().bg(dot_color))
            .child(bars);

        let think_alpha = thinking * motion_fx.bars;
        let think = (think_alpha > 0.001).then(|| {
            let mut dots = div().flex().items_center().gap(px(THINK_GAP));
            for i in 0..THINK_DOTS {
                dots = dots.child(
                    div().size(px(THINK_DOT)).rounded_full().bg(p
                        .white
                        .opacity(think_opacity(i, now_ms, reduce) * think_alpha)),
                );
            }
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(dots)
        });

        let check = motion_fx.check.map(|(scale, alpha)| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon("check", 15., p.white.opacity(0.92 * alpha))
                        .with_transformation(Transformation::scale(size(scale, scale))),
                )
        });
        let label = if recording {
            t("dictation.pill.recording")
        } else if processing {
            t("dictation.pill.transcribing")
        } else {
            t("dictation.pill.starting")
        };
        let scale = motion_fx.scale;
        // GPUI has no paint transforms on divs, so the pill scales by size
        // inside a fixed slot, which keeps it centered and the row still.
        let pill = div()
            .id("voice-pill")
            .role(Role::Button)
            .aria_label(label)
            .relative()
            .flex_none()
            .w(px(PILL_W * scale))
            .h(px(PILL_H * scale))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(p.border_hover)
            .bg(p.toast_surface)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.28),
                offset: point(px(0.), px(2.)),
                blur_radius: px(6.),
                spread_radius: px(0.),
                inset: false,
            }])
            .when(!processing && !finishing, |d| d.cursor_pointer())
            .child(meter)
            .children(think)
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
        let p = hud();
        let reduce = cx.reduce_motion();
        let t_in = f32_tx(
            "cancel-emerge",
            if visible { 1. } else { 0. },
            if visible && !reduce {
                DUR_BASE
            } else {
                DUR_FAST
            },
            if visible { ease_out } else { ease_snap },
            window,
            cx,
        );
        let scale = if reduce { 1. } else { 0.8 + 0.2 * t_in };
        let button = div()
            .id("pill-cancel")
            .keyboard_focusable("pill-cancel", Role::Button, t("dictation.pill.cancel"), cx)
            .flex_none()
            .size(px(CANCEL_SIZE * scale))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(p.border_hover)
            .bg(p.toast_surface)
            .opacity(t_in)
            .cursor_pointer()
            .hover(move |s| s.bg(p.surface_raised))
            .child(
                icon("x", 12., p.foreground)
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

    /// What the pill is doing and how to act on it, shown above it on hover.
    fn render_hint(snapshot: &DictationSnapshot) -> Div {
        let p = hud();
        let caption = |text: String, color: Hsla| div().text_color(color).child(text);
        let row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(10.))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(p.border_hover)
            .bg(p.toast_surface)
            .text_size(px(TEXT_SMALL))
            .whitespace_nowrap();
        if snapshot.is_recording {
            // Escape is bound globally for as long as the recording runs.
            row.child(caption(t("dictation.pill.finish"), p.foreground))
                .child(caption("·".into(), p.subtle_foreground))
                .child(caption(t("dictation.pill.cancel"), p.muted_foreground))
                .child(kbd("Esc", &p))
        } else if snapshot.is_processing {
            row.child(caption(t("dictation.pill.transcribing"), p.foreground))
        } else {
            row.child(caption(t("dictation.pill.starting"), p.foreground))
        }
    }

    /// The dismiss control shared by both panels: the Escape key chip, then a
    /// close button ringed by the panel's countdown.
    fn dismiss_cluster(
        id: &'static str,
        remaining: f32,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &mut App,
    ) -> Div {
        let p = hud();
        let ring = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let center = bounds.center();
                let r = f32::from(bounds.size.width.min(bounds.size.height)) / 2. - 1.;
                let arc = |share: f32, color: Hsla, window: &mut Window| {
                    let steps = ((48. * share).ceil() as usize).max(2);
                    let mut path = PathBuilder::stroke(px(1.5));
                    for i in 0..=steps {
                        // From twelve o'clock, clockwise.
                        let a = -FRAC_PI_2 + TAU * share * i as f32 / steps as f32;
                        let at = point(center.x + px(r * a.cos()), center.y + px(r * a.sin()));
                        if i == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                };
                arc(1., p.white.opacity(0.1), window);
                if remaining > 0.005 {
                    arc(remaining, p.white.opacity(0.5), window);
                }
            },
        )
        .absolute()
        .inset_0();
        let close = div()
            .id(id)
            .keyboard_focusable(id, Role::Button, t("dictation.pill.dismiss"), cx)
            .relative()
            .flex_none()
            .size(px(DISMISS_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(move |s| s.bg(p.white.opacity(0.08)))
            .child(ring)
            .child(icon("x", 12., p.muted_foreground))
            .on_click(on_close);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(kbd("Esc", &p))
            .child(close)
    }

    /// A compact panel action: white on the HUD when primary, a quiet wash
    /// otherwise.
    fn panel_action(
        id: &'static str,
        icon_name: &'static str,
        label: String,
        primary: bool,
        cx: &mut App,
    ) -> Stateful<Div> {
        let p = hud();
        let (bg, hover, fg) = if primary {
            (p.foreground, p.white, p.toast_surface)
        } else {
            (p.white.opacity(0.1), p.white.opacity(0.16), p.foreground)
        };
        div()
            .id(id)
            .keyboard_focusable(id, Role::Button, label.clone(), cx)
            .h(px(28.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .px(px(10.))
            .rounded(px(RADIUS_LG))
            .bg(bg)
            .hover(move |s| s.bg(hover))
            .text_size(px(TEXT_SMALL))
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .whitespace_nowrap()
            .cursor_pointer()
            .child(icon(icon_name, 14., fg))
            .child(label)
    }

    /// The HUD card both panels sit on.
    fn panel_card(id: &'static str) -> Stateful<Div> {
        let p = hud();
        div()
            .id(id)
            .relative()
            .w_full()
            .rounded(px(RADIUS_XL))
            .border_1()
            .border_color(p.border_hover)
            .bg(p.toast_surface)
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.35),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-12.),
                inset: false,
            }])
    }

    fn render_error(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = hud();
        let reduce = cx.reduce_motion();
        let error = self.error.as_ref()?;
        let remaining = error.countdown.remaining(Instant::now());
        let title = error.title.clone();
        let description = error.description.clone();
        let transcript = error.transcript.clone();

        let retry = Self::panel_action(
            "pill-error-retry",
            "rotate-ccw",
            t("common.retry"),
            true,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.error = None;
            Dictation::global(cx).update(cx, |d, cx| d.start(cx));
        }));
        let view = transcript.map(|transcript| {
            Self::panel_action(
                "pill-error-transcript",
                "scroll-text",
                t("dictation.errorActions.viewTranscript"),
                false,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.error = None;
                this.recovery = Some(Recovery::new(transcript.clone(), false));
                this.set_hovered(this.hovered);
                cx.notify();
            }))
        });
        let dismiss = Self::dismiss_cluster(
            "pill-error-close",
            remaining,
            cx.listener(|this, _, _, cx| {
                this.error = None;
                cx.notify();
            }),
            cx,
        );
        const ICON: f32 = 16.;
        const ICON_GAP: f32 = 10.;
        // The icon and the dismiss cluster center on the title line; the
        // description runs under the cluster so it does not wrap early.
        let header = div()
            .flex()
            .items_start()
            .gap(px(ICON_GAP))
            .child(div().h(px(LINE_SMALL)).flex().items_center().child(icon(
                "alert-circle",
                ICON,
                p.destructive,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(TEXT_BODY))
                    .line_height(px(LINE_SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.foreground)
                    .child(title),
            )
            .child(div().mt(px(-3.)).child(dismiss));
        let description = (!description.is_empty()).then(|| {
            div()
                .mt(px(2.))
                .pl(px(ICON + ICON_GAP))
                .text_size(px(TEXT_SMALL))
                .line_height(px(LINE_SMALL))
                .text_color(p.muted_foreground)
                .child(description)
        });
        let actions = div()
            .mt(px(12.))
            .flex()
            .justify_end()
            .gap(px(8.))
            .children(view)
            .child(retry);
        Some(
            Self::panel_card("pill-error")
                .p(px(PANEL_PAD))
                .child(header)
                .children(description)
                .child(actions)
                .with_animation(
                    "pill-error-in",
                    Animation::new(Duration::from_millis(DUR_BASE)).with_easing(ease_out),
                    move |d, t| {
                        let d = d.opacity(t);
                        if reduce { d } else { d.mt(px(4. * (1. - t))) }
                    },
                )
                .into_any_element(),
        )
    }

    fn render_recovery(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = hud();
        let reduce = cx.reduce_motion();
        let recovery = self.recovery.as_ref()?;
        let remaining = recovery.countdown.remaining(Instant::now());
        let ready = !recovery.copy_failed && recovery.copied;
        let failed = recovery.copy_failed;
        let text = recovery.text.clone();

        let dismiss = Self::dismiss_cluster(
            "pill-recovery-close",
            remaining,
            cx.listener(|this, _, _, cx| {
                this.recovery = None;
                cx.notify();
            }),
            cx,
        );
        let header = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .px(px(PANEL_PAD))
            .pt(px(PANEL_PAD))
            .pb(px(12.))
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(TEXT_BODY))
                            .line_height(px(LINE_SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.foreground)
                            .child(t("transcriptionPreview.recovery.title")),
                    )
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .text_color(p.muted_foreground)
                            .child(t("transcriptionPreview.recovery.pasteFailed")),
                    ),
            )
            .child(div().mt(px(-3.)).child(dismiss));
        let body = div()
            .id("pill-recovery-text")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(PANEL_PAD))
            .py(px(12.))
            .font_family(FONT_VOICE)
            .text_size(px(TEXT_READING))
            .line_height(px(LINE_READING))
            .text_color(p.foreground)
            .child(text.clone());
        // One line says where the text stands; Copy shows only while the
        // clipboard does not hold it.
        let status = if ready {
            Some((
                "check",
                p.success,
                t("transcriptionPreview.recovery.copiedHint"),
            ))
        } else if failed {
            Some((
                "alert-circle",
                p.destructive,
                t("transcriptionPreview.recovery.copyFailed"),
            ))
        } else {
            None
        };
        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .min_h(px(52.))
            .px(px(PANEL_PAD))
            .py(px(12.))
            .border_t_1()
            .border_color(p.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .text_color(p.muted_foreground)
                    .when_some(status, |d, (name, color, label)| {
                        d.child(icon(name, 14., color)).child(label)
                    }),
            )
            .when(!ready, |d| {
                d.child(
                    Self::panel_action(
                        "pill-recovery-copy",
                        "copy",
                        t("transcriptionPreview.recovery.copy"),
                        true,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let ok = backend::write_clipboard(&text);
                        if let Some(r) = this.recovery.as_mut() {
                            r.copied = ok;
                            r.copy_failed = !ok;
                            if ok {
                                // Copied: the panel only needs to stay long
                                // enough to read the paste hint.
                                r.countdown = Countdown::new(RECOVERY_COPIED, Instant::now());
                            }
                        }
                        this.set_hovered(this.hovered);
                        cx.notify();
                    })),
                )
            });
        Some(
            Self::panel_card("pill-recovery")
                .max_h(px(420.))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(header)
                .child(body)
                .child(footer)
                .with_animation(
                    "pill-recovery-in",
                    Animation::new(Duration::from_millis(DUR_SLOW)).with_easing(ease_out),
                    move |d, t| {
                        let d = d.opacity((t * 2.).min(1.));
                        if reduce { d } else { d.mt(px(6. * (1. - t))) }
                    },
                )
                .into_any_element(),
        )
    }

    fn render_toasts(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let p = hud();
        let mut stack = div().flex().flex_col().gap(px(6.)).w(px(TOAST_W));
        for (i, toast) in self.toasts.iter().enumerate() {
            let (icon_name, accent) = match toast.kind {
                ToastKind::Destructive => ("alert-circle", p.destructive),
                ToastKind::Success => ("check", p.success),
                ToastKind::Hint => ("keyboard", p.muted_foreground),
            };
            let undo = toast.undo_words.clone();
            stack = stack.child(
                div()
                    .id(("pill-toast", i))
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(RADIUS_LG))
                    .border_1()
                    .border_color(p.border_hover)
                    .bg(p.toast_surface)
                    .shadow_lg()
                    .child(
                        div()
                            .h(px(LINE_SMALL))
                            .flex()
                            .items_center()
                            .child(icon(icon_name, 14., accent)),
                    )
                    .child(
                        div()
                            .flex_1()
                            // Let long descriptions wrap inside the card.
                            .min_w_0()
                            .text_size(px(TEXT_SMALL))
                            .line_height(px(LINE_SMALL))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.foreground)
                                    .child(toast.title.clone()),
                            )
                            .when_some(toast.description.clone(), |d, desc| {
                                d.child(div().mt(px(2.)).text_color(p.muted_foreground).child(desc))
                            }),
                    )
                    .when_some(undo, |d, words| {
                        let id = ElementId::from(("pill-toast-undo", i));
                        d.child(
                            div()
                                .id(id.clone())
                                .keyboard_focusable(id, Role::Button, t("app.toasts.undo"), cx)
                                .flex_none()
                                .self_center()
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .px(px(8.))
                                .rounded(px(RADIUS_MD))
                                .bg(p.white.opacity(0.1))
                                .text_size(px(TEXT_SMALL))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .cursor_pointer()
                                .hover(move |s| s.bg(p.white.opacity(0.16)))
                                .child(t("app.toasts.undo"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    backend::undo_learned_words(&words, cx);
                                    this.toasts
                                        .retain(|t| t.undo_words.as_ref() != Some(&words));
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }
        Some(stack.into_any_element())
    }
}

impl Render for PillView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.regions.borrow_mut().clear();
        // Escape dismisses whichever panel is open. The binding is global
        // because this window never takes keyboard focus.
        let panel_open = self.error.is_some() || self.recovery.is_some();
        if panel_open != self.panel_escape {
            self.panel_escape = panel_open;
            backend::set_recovery_cancel_hotkey(panel_open);
        }
        let snapshot = Self::snapshot(cx);
        let dock = Self::dock(cx);
        let active = snapshot.is_active();
        let finishing = self.finishing_until.is_some_and(|t| Instant::now() < t);
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
        let pill_shown = (active || resolving) && !panel_open;
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

            // The hint sits above the pill, aligned with it; toasts take
            // that spot when present.
            if self.hovered && active && self.toasts.is_empty() {
                let hint = Self::render_hint(&snapshot).with_animation(
                    "pill-hint",
                    Animation::new(Duration::from_millis(DUR_SNAP)),
                    |d, t| d.opacity(t),
                );
                root = root.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(DOCK_INSET + PILL_H + 8.))
                        .px(px(DOCK_INSET))
                        .flex()
                        .map(|d| match dock {
                            PanelStartPosition::BottomLeft => d.justify_start(),
                            PanelStartPosition::Center => d.justify_center(),
                            PanelStartPosition::BottomRight => d.justify_end(),
                        })
                        .child(hint),
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
                        PanelStartPosition::BottomLeft => d.left(px(DOCK_INSET)),
                        PanelStartPosition::BottomRight => d.right(px(DOCK_INSET)),
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
    use std::time::{Duration, Instant};

    use super::{
        BREATH, Countdown, DUR_BASE, RESOLVE, bar_level, live_target, pill_motion, think_opacity,
    };

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
    fn transcribing_dots_walk_unless_motion_is_reduced() {
        let spread = |reduce: bool| {
            (0..3)
                .map(|i| think_opacity(i, 300., reduce))
                .fold((f32::MAX, 0f32), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (lo, hi) = spread(false);
        assert!(hi - lo > 0.1, "{lo}..{hi}");
        let (lo, hi) = spread(true);
        assert_eq!(lo, hi);
    }

    #[test]
    fn countdown_stands_still_while_paused() {
        let t0 = Instant::now();
        let mut c = Countdown::new(Duration::from_secs(4), t0);
        assert_eq!(c.remaining(t0), 1.);
        c.set_paused(true, t0 + Duration::from_secs(1));
        let later = t0 + Duration::from_secs(10);
        assert!((c.remaining(later) - 0.75).abs() < 1e-4);
        assert!(!c.expired(later));
        c.set_paused(false, later);
        assert!((c.remaining(later) - 0.75).abs() < 1e-4);
        assert!(c.expired(later + Duration::from_secs(3)));
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
