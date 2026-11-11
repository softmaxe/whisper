//! The Recording pill window.
//!
//! A borderless, non-activating, transparent window docked near the bottom of
//! the screen. It has no idle presence: it appears for a Dictation and its
//! feedback and orders out afterwards. Clicks fall through everywhere except
//! over the pill and its panels.
//!
//! The pill, its panels and its hints share one dark HUD surface in both
//! light and neutral dark themes, with a matching surface for Tokyo Night,
//! so they read as one component over any wallpaper or app. The window never
//! takes keyboard focus: Escape reaches it
//! through a global shortcut registered only while the microphone is live or
//! the lossless copy-recovery panel is open. The error card never takes it,
//! since it can hold the only copy of a recording.

mod cards;
mod state;
mod waveform;

use state::*;
use waveform::*;

use std::cell::RefCell;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::i18n::t;
use crate::settings::{PanelStartPosition, ThemePreference};

use crate::app_state::{AppState, DictationSnapshot};
use crate::backend;
use crate::dictation::{Dictation, DictationNotice, ErrorActions};
use crate::macos;

use super::components::{KeyboardFocusable, f32_tx, icon, kbd};
use super::format::format_elapsed;
use super::theme::{
    DUR_BASE, DUR_FAST, DUR_SLOW, DUR_SNAP, FONT_UI, FONT_VOICE, LINE_READING, LINE_SMALL, Palette,
    RADIUS_LG, RADIUS_MD, RADIUS_XL, TEXT_BODY, TEXT_READING, TEXT_SMALL, ease_out, ease_overshoot,
    ease_snap, motion, progress, tabular_nums,
};

pub const WINDOW_SIZE: Size<Pixels> = Size {
    width: px(466.),
    height: px(560.),
};
const DOCK_INSET: f32 = 12.;
const SCREEN_MARGIN: f32 = 4.;
const PILL_W: f32 = 92.;
const PILL_H: f32 = 30.;
const TIMER_GAP: f32 = 8.;
/// The widest time the timer slot is sized for, so a recording that passes
/// ten minutes keeps the capsule's width.
const TIMER_WIDEST: &str = "00:00";
/// The pill's two labels, the timer and "Transcribing…", sit a step below
/// full strength so the record dot stays the pill's loudest mark.
const PILL_LABEL: f32 = 0.8;
/// The cancel control beside the pill. It stays from the first frame of a
/// Dictation so aborting never depends on discovering a hover state. Its
/// glyph rests a step below full strength, so the pill stays the focus
/// while the control still reads as enabled (4.5:1 on the HUD).
// The pill's own height, so the two share top and bottom edges.
const CANCEL_SIZE: f32 = PILL_H;
const CANCEL_GAP: f32 = 6.;
const CANCEL_REST: f32 = 0.65;
/// Least side padding of the processing pill, around its dots and label.
const PILL_PAD: f32 = 10.;
const THINK_LABEL_GAP: f32 = 8.;
const TOAST_W: f32 = 300.;
const FINISH: Duration = Duration::from_millis(DUR_BASE);
/// How long the pill stays for the delivered check: pop, hold, then exit.
const RESOLVE: Duration = Duration::from_millis(RESOLVE_EXIT_AT + DUR_SNAP);
const RESOLVE_EXIT_AT: u64 = 620;

/// The floating HUD remains dark over any wallpaper, while honoring named themes.
fn hud(cx: &App) -> Palette {
    if cx.global::<super::theme::Theme>().preference == ThemePreference::TokyoNight {
        Palette::tokyo_night()
    } else {
        Palette::dark()
    }
}

/// The HUD's outline. Brighter than the dark palette's borders so the
/// near-black surface keeps a visible edge on dark wallpapers and
/// full-screen dark apps, where its fill alone barely separates.
fn hud_edge() -> Hsla {
    hsla(0., 0., 1., 0.2)
}

/// A drop shadow for a HUD surface plus a 1px top highlight that lifts it
/// off dark backgrounds the shadow cannot darken.
fn hud_shadow(y: f32, blur: f32, spread: f32, alpha: f32) -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., alpha),
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(spread),
            inset: false,
        },
        BoxShadow {
            color: hsla(0., 0., 1., 0.08),
            offset: point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        },
    ]
}

/// A tooltip on the HUD surface, so it matches the panel it labels in
/// both themes, with the control's key shortcut as a chip.
struct HudTip {
    label: SharedString,
    key: Option<&'static str>,
}

impl Render for HudTip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = hud(cx);
        div()
            .font_family(FONT_UI)
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(8.))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(hud_edge())
            .bg(p.toast_surface)
            .text_size(px(TEXT_SMALL))
            .text_color(p.foreground)
            .child(self.label.clone())
            .when_some(self.key, |d, key| d.child(kbd(key, &p)))
    }
}

/// How long a dev scene pretends the microphone has already been live, so
/// a screenshot shows the elapsed timer.
#[cfg(debug_assertions)]
fn dev_head_start() -> Duration {
    super::dev_scene::pill_head_start()
}

#[cfg(not(debug_assertions))]
fn dev_head_start() -> Duration {
    Duration::ZERO
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
    /// Failed clipboard writes remain retrievable after the live dictation.
    deferred_recoveries: Vec<Recovery>,
    /// Whether Escape is registered to dismiss the copy-recovery panel.
    panel_escape: bool,
    /// Whether the Settings sheet is open. The error card's countdown stands
    /// still meanwhile, since Open Settings sends the user there to fix the
    /// server and the card must still hold the recording when they return.
    settings_open: bool,
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
    /// When the microphone went live, and the time it ran for, which the
    /// timer keeps showing while it fades out for transcribing.
    recording_since: Option<Instant>,
    /// Actual readiness time, independent of a dev scene's elapsed timer.
    ready_at: Option<Instant>,
    recorded: Duration,
    /// Whether the pointer rests on the cancel control, so the hint above
    /// the pill speaks for it instead of for the pill.
    cancel_hovered: bool,
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
            deferred_recoveries: Vec::new(),
            panel_escape: false,
            settings_open: false,
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
            recording_since: None,
            ready_at: None,
            recorded: Duration::ZERO,
            cancel_hovered: false,
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
            || !self.deferred_recoveries.is_empty()
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
            if let Some(recovery) = self.recovery.take()
                && let Some(recovery) = recovery.keep_if_uncopied(backend::write_clipboard)
            {
                self.deferred_recoveries.push(recovery);
            }
            self.panel_escape = false;
            backend::set_panel_cancel_hotkey(false);
            self.finishing_until = None;
            self.lanes = [0.; BARS];
            self.error = None;
            self.entered_at = None;
            self.enter_pending = true;
            self.dipped_at = None;
            self.resolved_at = None;
            self.recording_since = None;
            self.ready_at = None;
            self.recorded = Duration::ZERO;
        }
        if snapshot.is_recording && self.recording_since.is_none() {
            self.recording_since = Some(Instant::now() - dev_head_start());
            self.ready_at = Some(Instant::now());
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
    /// countdown still while it does. The error card also holds while the
    /// Settings sheet is open.
    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
        let now = Instant::now();
        if let Some(countdown) = self.error.as_mut().and_then(|e| e.countdown.as_mut()) {
            countdown.set_paused(hovered || self.settings_open, now);
        }
        let countdowns = self
            .recovery
            .iter_mut()
            .map(|r| &mut r.countdown)
            .chain(self.toasts.iter_mut().map(|t| &mut t.countdown));
        for countdown in countdowns {
            countdown.set_paused(hovered, now);
        }
    }

    /// Follows the Settings sheet. Closing it gives the error card a fresh
    /// countdown, so the user has the full time to resend after a fix.
    fn track_settings(&mut self, cx: &mut Context<Self>) {
        let open = crate::app::settings_open(cx);
        if open == self.settings_open {
            return;
        }
        self.settings_open = open;
        if !open && let Some(error) = self.error.as_mut() {
            error.restart();
        }
        self.set_hovered(self.hovered);
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
        self.track_settings(cx);
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
            .and_then(|e| e.countdown.as_ref())
            .is_some_and(|c| c.expired(now))
        {
            self.close_error(cx);
        }
        if self.recovery.as_ref().is_some_and(|r| r.expired(now)) {
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

    /// Closes the error card by the user's hand or its timer. A recording
    /// kept for it goes with it, so the words never outlive the offer.
    fn close_error(&mut self, cx: &mut App) {
        if let Some(error) = self.error.take()
            && error.actions.retry
        {
            Dictation::global(cx).update(cx, |d, _| d.discard_failed());
        }
    }

    fn on_notice(&mut self, notice: DictationNotice, window: &mut Window, cx: &mut Context<Self>) {
        match notice {
            // Dictation sends this as it starts over or resends, having
            // already settled the kept recording itself.
            DictationNotice::DismissError => self.error = None,
            // Sent when Escape is pressed. Only the recovery panel takes it,
            // and not while an error card covers it.
            DictationNotice::DismissRecovery => {
                if self.error.is_none() {
                    self.recovery = None;
                }
            }
            DictationNotice::Error {
                title,
                description,
                actions,
                kept,
            } => {
                self.error = Some(ErrorCard::new(title, description, None, actions, kept));
            }
            DictationNotice::TranscriptRecovery {
                title,
                description,
                text,
                kept,
            } => {
                let mut error = ErrorCard::new(
                    title,
                    description,
                    Some((text, false)),
                    ErrorActions {
                        retry: kept.is_some(),
                        open_settings: false,
                    },
                    kept,
                );
                // This card may hold the only text, even without audio.
                error.countdown = None;
                self.error = Some(error);
            }
            DictationNotice::NoAudio => {
                self.error = Some(ErrorCard::new(
                    t("dictation.noAudio.title"),
                    t("dictation.noAudio.description"),
                    None,
                    ErrorActions::default(),
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
                    Some((transcript, kept_in_clipboard)).filter(|(t, _)| !t.trim().is_empty()),
                    ErrorActions::default(),
                    None,
                ));
            }
            DictationNotice::DeliveryFallback { text, copied } => {
                if let Some(previous) = self.recovery.take().filter(|r| !r.copied) {
                    self.deferred_recoveries.push(previous);
                }
                self.recovery = Some(Recovery::new(
                    t("transcriptionPreview.recovery.title"),
                    Some(t("transcriptionPreview.recovery.pasteFailed")),
                    text,
                    copied,
                ));
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
                let resolved_at = Instant::now();
                self.resolved_at = Some(resolved_at);
                // The check holds still before its exit, so wake once at the
                // exit boundary instead of drawing throughout the hold.
                cx.spawn_in(window, async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(RESOLVE_EXIT_AT))
                        .await;
                    let _ = this.update_in(cx, |this, _, cx| {
                        if this.resolved_at == Some(resolved_at) {
                            cx.notify();
                        }
                    });
                })
                .detach();
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
}

impl Render for PillView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.regions.borrow_mut().clear();
        let snapshot = Self::snapshot(cx);
        if !snapshot.is_active()
            && self.error.is_none()
            && self.recovery.is_none()
            && let Some(mut recovery) = self.deferred_recoveries.pop()
        {
            recovery.countdown = Countdown::new(RECOVERY_UNCOPIED, Instant::now());
            self.recovery = Some(recovery);
        }
        // Escape closes the copy-recovery panel, which loses nothing: its
        // words are in History and usually on the clipboard. The binding is
        // global because this window never takes keyboard focus, so the
        // error card, which may hold the only copy of a recording, never
        // takes it; a stray Escape meant for another app must not lose it.
        let panel_open = self.error.is_some() || self.recovery.is_some();
        let escape = !snapshot.is_active() && self.error.is_none() && self.recovery.is_some();
        if escape != self.panel_escape {
            self.panel_escape = escape;
            backend::set_panel_cancel_hotkey(escape);
        }
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
        if show_pill && motion_fx.running {
            window.request_animation_frame();
        }
        let recording = snapshot.is_recording;
        let cancel_visible = (recording || snapshot.is_processing) && !panel_open;
        if !cancel_visible || !self.hovered {
            self.cancel_hovered = false;
        }
        self.update_recorded(&snapshot);

        let mut root = div().size_full().relative().font_family(FONT_UI);

        if show_pill {
            let pill = self.render_pill(&snapshot, motion_fx, window, cx);
            let (cancel, cancel_t) = self.render_cancel(cancel_visible, recording, window, cx);
            // The slot is held while the microphone opens, so the pill does
            // not shift when the control arrives.
            let cancel = div()
                .when(cancel_t < 0.001, |d| d.invisible())
                .child(cancel);
            // Cancel trails the pill, where a close control sits on a HUD.
            let row = div()
                .flex()
                .items_center()
                .gap(px(CANCEL_GAP))
                .child(pill)
                .child(cancel);
            let tracked = self.tracked(row).opacity(pill_alpha);
            let positioned = div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(DOCK_INSET))
                .px(px(DOCK_INSET))
                .flex()
                .map(|d| match dock {
                    PanelStartPosition::BottomLeft => d.justify_start(),
                    // A leading spacer as wide as the cancel slot keeps the
                    // pill itself on the screen's center line.
                    PanelStartPosition::Center => d
                        .justify_center()
                        .child(div().flex_none().w(px(CANCEL_SIZE + CANCEL_GAP))),
                    PanelStartPosition::BottomRight => d.justify_end(),
                });
            root = root.child(positioned.child(tracked));

            // The hint sits above the pill, aligned with it; toasts take
            // that spot when present.
            let hint = (self.hovered && active && self.toasts.is_empty())
                .then(|| Self::render_hint(&snapshot, self.cancel_hovered, cx))
                .flatten();
            if let Some(hint) = hint {
                let hint =
                    hint.with_animation("pill-hint", motion(DUR_SNAP, cx), |d, t| d.opacity(t));
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
            if card_needs_animation_frame(self.error.as_ref(), self.recovery.as_ref(), now) {
                window.request_animation_frame();
            }
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
        }
        root
    }
}
