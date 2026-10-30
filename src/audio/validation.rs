//! Degenerate-recording checks.
//!
//! The gate is on the encoded WAV size (44-byte header + PCM): an accidental
//! double tap yields under 256 bytes, which is only a few milliseconds of
//! audio.

pub const MIN_AUDIO_BYTES: usize = 256;

/// Whether a finished recording carries real audio worth transcribing: some
/// frame was delivered and the encoded file reaches [`MIN_AUDIO_BYTES`].
pub fn is_usable_recording(encoded_len: usize, received_audio_data: bool) -> bool {
    received_audio_data && encoded_len >= MIN_AUDIO_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_recordings() {
        assert!(is_usable_recording(50_000, true));
        assert!(is_usable_recording(256, true));
        assert!(is_usable_recording(1_674_472, true));
        assert!(!is_usable_recording(50_000, false));
        assert!(!is_usable_recording(0, false));
        assert!(!is_usable_recording(255, true));
        assert!(!is_usable_recording(110, true));
        assert!(!is_usable_recording(0, true));
    }
}
