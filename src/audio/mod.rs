//! Microphone capture, speech gate, WAV encoding and dictation cues.
//!
//! cpal streams live on dedicated threads (they are `!Send` on macOS); the
//! UI talks to them through [`Recorder`].

pub mod capture;
pub mod cues;
pub mod devices;
pub mod gate;
pub mod validation;
pub mod wav;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("No microphone is available")]
    NoDefaultDevice,
    #[error("Selected microphone is unavailable: {0}")]
    DeviceNotFound(String),
    #[error("Microphone access was denied")]
    PermissionDenied,
    #[error("Could not open the microphone: {0}")]
    Open(String),
    /// The stream opened but never delivered a frame; the UI reports
    /// "no audio", not an error.
    #[error("The selected microphone did not deliver audio")]
    FirstAudioUnavailable,
    #[error("Microphone disconnected: {0}")]
    DeviceLost(String),
    /// Cancelled or superseded by another request.
    #[error("Capture request ended")]
    Cancelled,
    #[error("A recording is already in progress")]
    Busy,
    #[error("Audio output unavailable: {0}")]
    Output(String),
}

impl AudioError {
    /// True for the soft "no audio" outcome.
    pub fn is_no_audio(&self) -> bool {
        matches!(self, Self::FirstAudioUnavailable)
    }
}
