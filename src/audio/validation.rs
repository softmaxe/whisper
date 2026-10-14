//! Degenerate-recording checks.
//!
//! The gate is on the encoded WAV size (44-byte header + PCM): an accidental
//! double tap yields under 256 bytes, which is only a few milliseconds of
//! audio.

use serde::Serialize;

pub const MIN_AUDIO_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnusableReason {
    /// No audio frame was ever delivered.
    NoAudioData,
    /// Frames arrived but the encoded file is essentially empty.
    EmptyContainer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecordingCheck {
    pub usable: bool,
    pub reason: Option<UnusableReason>,
}

/// True for missing sizes and anything under [`MIN_AUDIO_BYTES`].
pub fn is_empty_recording(size: Option<usize>) -> bool {
    size.unwrap_or(0) < MIN_AUDIO_BYTES
}

/// Whether a finished recording carries real audio worth transcribing.
pub fn evaluate_finished_recording(
    size: Option<usize>,
    received_audio_data: bool,
) -> RecordingCheck {
    if !received_audio_data {
        return RecordingCheck {
            usable: false,
            reason: Some(UnusableReason::NoAudioData),
        };
    }
    if is_empty_recording(size) {
        return RecordingCheck {
            usable: false,
            reason: Some(UnusableReason::EmptyContainer),
        };
    }
    RecordingCheck {
        usable: true,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_recordings() {
        assert_eq!(
            evaluate_finished_recording(Some(50_000), true),
            RecordingCheck {
                usable: true,
                reason: None
            }
        );
        assert_eq!(
            evaluate_finished_recording(Some(0), false).reason,
            Some(UnusableReason::NoAudioData)
        );
        assert_eq!(
            evaluate_finished_recording(Some(110), true).reason,
            Some(UnusableReason::EmptyContainer)
        );
        assert_eq!(
            evaluate_finished_recording(None, false).reason,
            Some(UnusableReason::NoAudioData)
        );
        assert_eq!(
            evaluate_finished_recording(None, true).reason,
            Some(UnusableReason::EmptyContainer)
        );
    }

    #[test]
    fn size_threshold() {
        assert!(is_empty_recording(Some(110)));
        assert!(is_empty_recording(Some(0)));
        assert!(is_empty_recording(Some(255)));
        assert!(!is_empty_recording(Some(256)));
        assert!(!is_empty_recording(Some(3000)));
        assert!(!is_empty_recording(Some(1_674_472)));
        assert!(is_empty_recording(None));
    }
}
