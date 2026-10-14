//! A held key re-fires a registered shortcut on every autorepeat (macOS: first
//! after 225-375 ms, then every 30-90 ms) and never reports the release. Fires
//! inside the window of the previous fire are repeats and renew the window, so
//! one hold is one press however long it lasts.

pub const HOTKEY_REPEAT_WINDOW_MS: u64 = 600;

#[derive(Debug, Clone)]
pub struct RepeatGate {
    window_ms: u64,
    last_fire_at: Option<u64>,
}

impl Default for RepeatGate {
    fn default() -> Self {
        Self::new(HOTKEY_REPEAT_WINDOW_MS)
    }
}

impl RepeatGate {
    pub fn new(window_ms: u64) -> Self {
        Self {
            window_ms,
            last_fire_at: None,
        }
    }

    /// True when a fire at `now_ms` is a new press rather than an autorepeat.
    pub fn is_press(&mut self, now_ms: u64) -> bool {
        let is_repeat = self
            .last_fire_at
            .is_some_and(|last| now_ms.saturating_sub(last) < self.window_ms);
        self.last_fire_at = Some(now_ms);
        !is_repeat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presses(times: &[u64]) -> Vec<bool> {
        let mut gate = RepeatGate::default();
        times.iter().map(|t| gate.is_press(*t)).collect()
    }

    #[test]
    fn a_held_key_is_one_press() {
        let times = [0, 375, 465, 555, 645, 735, 825, 915, 1005];
        let mut expected = vec![false; times.len()];
        expected[0] = true;
        assert_eq!(presses(&times), expected);
    }

    #[test]
    fn a_deliberate_second_press_after_the_window_counts() {
        assert_eq!(presses(&[0, 700, 1300]), vec![true, true, true]);
    }

    #[test]
    fn a_quick_second_tap_inside_the_window_is_a_repeat() {
        assert_eq!(presses(&[0, 300]), vec![true, false]);
    }
}
