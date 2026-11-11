//! Countdown and feedback state for the recording pill.

use super::*;

/// Every error card offers an action, so it stays long enough to reach for
/// it; hovering holds it longer still.
const ERROR_HOLD: Duration = Duration::from_millis(12_000);
/// The recovery panel holds the user's words; it stays longer while they
/// still need to copy them than once they are on the clipboard.
pub(super) const RECOVERY_COPIED: Duration = Duration::from_millis(6000);
pub(super) const RECOVERY_UNCOPIED: Duration = Duration::from_millis(12_000);

/// An auto-dismiss timer that stands still while the pointer rests on the
/// pill's surfaces, so nothing leaves while it is being read.
#[derive(Clone, Copy, Debug)]
pub(super) struct Countdown {
    started: Instant,
    paused_at: Option<Instant>,
    duration: Duration,
}

impl Countdown {
    pub(super) fn new(duration: Duration, now: Instant) -> Self {
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

    pub(super) fn set_paused(&mut self, paused: bool, now: Instant) {
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
    pub(super) fn remaining(&self, now: Instant) -> f32 {
        (1. - self.elapsed(now).as_secs_f32() / self.duration.as_secs_f32()).clamp(0., 1.)
    }

    pub(super) fn expired(&self, now: Instant) -> bool {
        self.elapsed(now) >= self.duration
    }

    /// Whole seconds still to run, rounded up, so it reads 1 until it ends.
    pub(super) fn seconds_left(&self, now: Instant) -> u64 {
        self.duration
            .saturating_sub(self.elapsed(now))
            .as_secs_f32()
            .ceil() as u64
    }
}

pub(super) struct ErrorCard {
    pub(super) title: String,
    pub(super) description: String,
    /// Words the error kept, and whether they are already on the clipboard.
    pub(super) transcript: Option<(String, bool)>,
    pub(super) actions: ErrorActions,
    /// The length of the recording kept for a resend, when known.
    pub(super) kept: Option<Duration>,
    /// `None` while the card holds a recording to resend: closing it drops
    /// the recording, so only the user may close it, never a timer.
    pub(super) countdown: Option<Countdown>,
}

impl ErrorCard {
    /// A card shown from now, for longer when it can resend the recording.
    pub(super) fn new(
        title: String,
        description: String,
        transcript: Option<(String, bool)>,
        actions: ErrorActions,
        kept: Option<Duration>,
    ) -> Self {
        Self {
            title,
            description,
            transcript,
            actions,
            kept,
            countdown: Self::countdown(actions),
        }
    }

    /// Starts the countdown over, for when the user comes back to the card.
    pub(super) fn restart(&mut self) {
        if self.countdown.is_some() {
            self.countdown = Self::countdown(self.actions);
        }
    }

    fn countdown(actions: ErrorActions) -> Option<Countdown> {
        (!actions.retry).then(|| Countdown::new(ERROR_HOLD, Instant::now()))
    }
}

pub(super) struct Recovery {
    /// Names what went wrong, so the panel never reads as a success.
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) text: String,
    pub(super) copied: bool,
    pub(super) copy_failed: bool,
    pub(super) countdown: Countdown,
}

impl Recovery {
    pub(super) fn expired(&self, now: Instant) -> bool {
        self.copied && self.countdown.expired(now)
    }

    pub(super) fn keep_if_uncopied(self, copy: impl FnOnce(&str) -> bool) -> Option<Self> {
        if self.copied || copy(&self.text) {
            None
        } else {
            Some(self)
        }
    }

    pub(super) fn new(
        title: String,
        description: Option<String>,
        text: String,
        copied: bool,
    ) -> Self {
        let duration = if copied {
            RECOVERY_COPIED
        } else {
            RECOVERY_UNCOPIED
        };
        Self {
            title,
            description,
            text,
            copied,
            copy_failed: false,
            countdown: Countdown::new(duration, Instant::now()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ToastKind {
    Destructive,
    Success,
    Hint,
}

pub(super) struct PillToast {
    pub(super) title: String,
    pub(super) description: Option<String>,
    pub(super) kind: ToastKind,
    pub(super) countdown: Countdown,
    pub(super) undo_words: Option<Vec<String>>,
}

/// Only the front card's visible, advancing countdown needs continuous frames.
/// Toast expiry and hidden countdowns are handled by the pointer poll.
pub(super) fn card_needs_animation_frame(
    error: Option<&ErrorCard>,
    recovery: Option<&Recovery>,
    now: Instant,
) -> bool {
    let countdown = if let Some(error) = error {
        error.countdown.as_ref()
    } else {
        recovery.filter(|r| r.copied).map(|r| &r.countdown)
    };
    countdown.is_some_and(|c| c.paused_at.is_none() && !c.expired(now))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        Countdown, ERROR_HOLD, ErrorActions, ErrorCard, Recovery, card_needs_animation_frame,
    };

    #[test]
    fn card_frames_follow_only_the_visible_countdown() {
        let now = Instant::now();
        let mut recovery = Recovery::new("Recovery".into(), None, "words".into(), false);
        recovery.countdown = Countdown::new(Duration::from_secs(4), now);
        assert!(!card_needs_animation_frame(None, None, now));
        assert!(!card_needs_animation_frame(None, Some(&recovery), now));
        recovery.copied = true;
        assert!(card_needs_animation_frame(None, Some(&recovery), now));
        let error = ErrorCard::new(
            "Retry".into(),
            "Kept audio".into(),
            None,
            ErrorActions {
                retry: true,
                open_settings: false,
            },
            None,
        );
        assert!(!card_needs_animation_frame(
            Some(&error),
            Some(&recovery),
            now
        ));
        assert!(!card_needs_animation_frame(
            None,
            Some(&recovery),
            now + Duration::from_secs(4)
        ));
    }

    #[test]
    fn card_frames_stop_when_paused_and_restart_when_resumed() {
        let now = Instant::now();
        let mut error = ErrorCard::new(
            "Error".into(),
            "Details".into(),
            None,
            ErrorActions::default(),
            None,
        );
        error.countdown = Some(Countdown::new(Duration::from_secs(4), now));
        assert!(card_needs_animation_frame(Some(&error), None, now));
        error.countdown.as_mut().unwrap().set_paused(true, now);
        let later = now + Duration::from_secs(10);
        assert!(!card_needs_animation_frame(Some(&error), None, later));
        error.countdown.as_mut().unwrap().set_paused(false, later);
        assert!(card_needs_animation_frame(Some(&error), None, later));
        assert!(!card_needs_animation_frame(
            Some(&error),
            None,
            later + Duration::from_secs(4)
        ));
        let mut recovery = Recovery::new("Recovery".into(), None, "words".into(), true);
        recovery.countdown = Countdown::new(Duration::from_secs(4), now);
        recovery.countdown.set_paused(true, now);
        assert!(!card_needs_animation_frame(None, Some(&recovery), later));
        recovery.countdown.set_paused(false, later);
        assert!(card_needs_animation_frame(None, Some(&recovery), later));
    }

    #[test]
    fn new_recording_preserves_recovery_when_copying_fails() {
        let recovery = super::Recovery::new("Recovery".into(), None, "unsaved words".into(), false);
        let deferred = recovery
            .keep_if_uncopied(|text| {
                assert_eq!(text, "unsaved words");
                false
            })
            .unwrap();
        assert_eq!(deferred.text, "unsaved words");
        assert!(deferred.keep_if_uncopied(|_| true).is_none());
        let copied = super::Recovery::new("Recovery".into(), None, "copied words".into(), true);
        assert!(
            copied
                .keep_if_uncopied(|_| panic!("must not overwrite a newer copy"))
                .is_none()
        );
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
    fn countdown_names_whole_seconds_left() {
        let t0 = Instant::now();
        let c = Countdown::new(Duration::from_secs(12), t0);
        assert_eq!(c.seconds_left(t0), 12);
        assert_eq!(c.seconds_left(t0 + Duration::from_millis(8_200)), 4);
        assert_eq!(c.seconds_left(t0 + Duration::from_millis(11_990)), 1);
        assert_eq!(c.seconds_left(t0 + Duration::from_secs(13)), 0);
    }

    #[test]
    fn a_card_holding_the_recording_never_times_out() {
        let retry = ErrorActions {
            retry: true,
            open_settings: true,
        };
        // Closing drops the recording, so only the user may close it.
        assert!(ErrorCard::countdown(retry).is_none());
        let plain = ErrorCard::countdown(ErrorActions::default()).unwrap();
        assert_eq!(plain.duration, ERROR_HOLD);
    }

    #[test]
    fn uncopied_recovery_survives_time_and_settings_reopening() {
        let mut card = ErrorCard::new(
            "Review".into(),
            "Kept words".into(),
            Some(("Supabase".into(), false)),
            ErrorActions::default(),
            None,
        );
        card.countdown = None;
        card.restart();
        assert!(card.countdown.is_none());
        let mut recovery = super::Recovery::new("Review".into(), None, "Supabase".into(), false);
        let later = Instant::now() + Duration::from_secs(60);
        assert!(!recovery.expired(later));
        recovery.copied = true;
        assert!(recovery.expired(later));
    }
}
