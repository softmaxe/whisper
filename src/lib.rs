//! Whisper: macOS dictation with self-hosted speech recognition and text
//! cleanup, built with Rust and GPUI.
//!
//! Modules are private so the compiler reports unused code; `main.rs` only
//! calls [`run`].

mod analytics;
mod app;
mod app_state;
mod audio;
mod audio_storage;
mod backend;
mod db;
mod dictation;
mod i18n;
mod macos;
mod menu;
mod native;
mod paths;
mod pipeline;
mod secrets;
mod services;
mod settings;
mod text;
mod tray;
mod ui;

pub use app::run;
