//! macOS integration: global hotkeys, automatic paste, the post-paste edit
//! monitor, media pause/resume, microphone selection, permissions, launch at
//! login and the Dock icon.
//!
//! The Accessibility-heavy work (and the media and lid-state queries) runs in
//! small Swift helper processes (`resources/native/*.swift`, built by
//! `build.rs` into `resources/bin`) driven over stdin/stdout; CoreAudio device
//! queries run in-process (`mic::coreaudio`). One-shot calls go through
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

/// A four-character code (`FourCharCode`, e.g. `b"oapp"`) as its big-endian `u32`.
const fn fourcc(code: &[u8; 4]) -> u32 {
    ((code[0] as u32) << 24) | ((code[1] as u32) << 16) | ((code[2] as u32) << 8) | code[3] as u32
}
