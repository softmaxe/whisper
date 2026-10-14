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

pub use capture::{
    CaptureEvent, CaptureSession, EventSink, FIRST_AUDIO_TIMEOUT, PRE_ROLL_MAX_AGE,
    PREPARED_MAX_AGE, RecordedAudio, Recorder,
};
pub use devices::{InputDeviceInfo, list_input_devices};
pub use gate::{GateReason, SpeechGateDecision};

/// Error code and copy shown when capture fails or the device
/// disappears ("Microphone unavailable").
pub const MICROPHONE_CAPTURE_ERROR_CODE: &str = "MIC_CAPTURE_FAILED";
pub const MICROPHONE_CAPTURE_ERROR_TITLE: &str = "Microphone unavailable";
pub const MICROPHONE_CAPTURE_ERROR_MESSAGE_KEY: &str =
    "dictation.errorDescriptions.microphoneUnavailable";

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

    /// `MIC_CAPTURE_FAILED` for errors the UI shows as "Microphone unavailable".
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::NoDefaultDevice
            | Self::DeviceNotFound(_)
            | Self::PermissionDenied
            | Self::Open(_)
            | Self::DeviceLost(_) => Some(MICROPHONE_CAPTURE_ERROR_CODE),
            _ => None,
        }
    }

    pub fn message_key(&self) -> Option<&'static str> {
        self.code().map(|_| MICROPHONE_CAPTURE_ERROR_MESSAGE_KEY)
    }
}
