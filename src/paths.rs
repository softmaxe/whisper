//! Filesystem locations shared by every subsystem.
//!
//! App data (settings, the database and audio) lives under
//! `~/Library/Application Support/com.softmaxe.whisper/`. `WHISPER_USER_DATA_DIR`
//! overrides the root, which tests and e2e runs use to stay away from real user
//! data.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Name of the data directory under `~/Library/Application Support`.
const APP_DATA_DIR_NAME: &str = "com.softmaxe.whisper";

/// Root directory for app data. Created on first access.
pub fn data_dir() -> &'static Path {
    DATA_DIR.get_or_init(|| {
        let dir = std::env::var_os("WHISPER_USER_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(APP_DATA_DIR_NAME)
            });
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
}

pub fn database_path() -> PathBuf {
    data_dir().join("transcriptions.db")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn audio_dir() -> PathBuf {
    data_dir().join("audio")
}

/// Directory holding bundled helper binaries (`macos-globe-listener`, ffmpeg, ...).
///
/// In a packaged `.app` they live in `Contents/Resources/bin`; in development
/// they are built into `resources/bin` of the repository.
pub fn helpers_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("WHISPER_HELPERS_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(contents) = exe.parent().and_then(|p| p.parent())
    {
        let bundled = contents.join("Resources").join("bin");
        if bundled.is_dir() {
            return bundled;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("bin")
}

/// Path to a bundled helper binary.
pub fn helper(name: &str) -> PathBuf {
    helpers_dir().join(name)
}
