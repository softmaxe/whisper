//! macOS integration: global hotkeys, automatic paste, the post-paste edit
//! monitor, media pause/resume, microphone selection, permissions, launch at
//! login and the Dock icon.
//!
//! The Accessibility- and CoreAudio-heavy work runs in small Swift helper
//! processes (`resources/native/*.swift`, built by `build.rs` into
//! `resources/bin`) driven over stdin/stdout. One-shot calls go through
//! `helper`; the long-running ones (Globe listener, lid monitor, edit
//! monitoring) run under `supervised_child`.
//!
//! Threading: functions that touch AppKit state (`dock`, `HotkeyManager`
//! registration) must run on the main thread; everything that spawns helpers
//! or sleeps (`paste`, `text_monitor`, `media`) blocks and belongs on a
//! background thread. Microphone selection reads the lid monitor's latest
//! sample and does not wait for the helper.

pub mod autostart;
pub mod dock;
pub mod helper;
pub mod hotkeys;
pub mod media;
pub mod mic;
pub mod paste;
pub mod permissions;
pub mod supervised_child;
pub mod text_monitor;
pub mod timer;
