//! Whisper: macOS dictation with self-hosted speech recognition and text
//! cleanup, built with Rust and GPUI.
//!
//! The library holds everything that does not need a window so it can be unit
//! and integration tested; `main.rs` wires it into the GPUI application.

pub mod analytics;
pub mod audio;
pub mod audio_storage;
pub mod db;
pub mod i18n;
pub mod native;
pub mod paths;
pub mod pipeline;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod text;
