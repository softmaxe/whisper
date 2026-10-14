//! macOS integration: global hotkeys, automatic paste, the post-paste edit
//! monitor, media pause/resume, microphone selection, permissions, launch at
//! login and the Dock icon.
//!
//! The Accessibility- and CoreAudio-heavy work runs in small Swift helper
//! processes (`resources/native/*.swift`, built by `build.rs` into
//! `resources/bin`) driven over stdin/stdout.
//!
//! Threading: functions that touch AppKit state (`dock`, `HotkeyManager`
//! registration) must run on the main thread; everything that spawns helpers
//! or sleeps (`paste`, `text_monitor`, `media`, `mic`) blocks and belongs on a
//! background thread.

pub mod autostart;
pub mod dock;
pub mod helper;
pub mod hotkeys;
pub mod media;
pub mod mic;
pub mod paste;
pub mod permissions;
pub mod text_monitor;
pub mod timer;
