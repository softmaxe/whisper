//! FFmpeg process helpers.
//!
//! Uses the bundled `ffmpeg` from [`crate::paths::helper`] when present, then
//! Homebrew locations, then `PATH`. Conversions are cancellable: cancelling
//! kills the process (SIGKILL) and yields [`FfmpegError::Cancelled`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

static CACHED_PATH: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));

const SYSTEM_CANDIDATES: &[&str] = &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FfmpegError {
    #[error(
        "FFmpeg not found - the bundled FFmpeg is missing from this install and no system FFmpeg was found on PATH"
    )]
    NotFound,
    #[error("Cancelled")]
    Cancelled,
    #[error("FFmpeg process error: {0}")]
    Process(String),
    #[error("FFmpeg exited with code {code}{}", stderr_suffix(.stderr))]
    Failed { code: i32, stderr: String },
    #[error("FFmpeg conversion produced no output file")]
    NoOutput,
    #[error("FFmpeg conversion produced empty output file")]
    EmptyOutput,
}

fn stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Resolves the FFmpeg binary; found paths are cached.
pub fn ffmpeg_path() -> Option<PathBuf> {
    let mut cached = CACHED_PATH.lock();
    if let Some(path) = cached.as_ref() {
        return Some(path.clone());
    }
    let bundled = crate::paths::helper("ffmpeg");
    let found = std::iter::once(bundled)
        .chain(SYSTEM_CANDIDATES.iter().map(PathBuf::from))
        .chain(
            std::env::var_os("PATH")
                .map(|p| {
                    std::env::split_paths(&p)
                        .map(|d| d.join("ffmpeg"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        )
        .find(|p| is_executable(p));
    if found.is_none() {
        log::debug!("FFmpeg not found");
    }
    cached.clone_from(&found);
    found
}

/// Last 500 characters of stderr, trimmed.
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(500);
    chars[start..].iter().collect::<String>().trim().to_string()
}

/// Runs FFmpeg with `args`; fails on a non-zero exit.
async fn run(args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), FfmpegError> {
    if cancel.is_cancelled() {
        return Err(FfmpegError::Cancelled);
    }
    let path = ffmpeg_path().ok_or(FfmpegError::NotFound)?;
    let child = tokio::process::Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| FfmpegError::Process(e.to_string()))?;
    // Dropping the wait future on cancel kills the child (kill_on_drop).
    let output = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(FfmpegError::Cancelled),
        output = child.wait_with_output() => output.map_err(|e| FfmpegError::Process(e.to_string()))?,
    };
    if !output.status.success() {
        let stderr = stderr_tail(&output.stderr);
        log::debug!("FFmpeg failed: {stderr}");
        return Err(FfmpegError::Failed {
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }
    Ok(())
}

/// One-shot conversion that must leave a non-empty `output`.
async fn run_conversion(
    mut args: Vec<OsString>,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<(), FfmpegError> {
    args.push("-y".into());
    args.push(output.as_os_str().to_owned());
    run(args, cancel).await?;
    match std::fs::metadata(output) {
        Err(_) => Err(FfmpegError::NoOutput),
        Ok(meta) if meta.len() == 0 => Err(FfmpegError::EmptyOutput),
        Ok(_) => Ok(()),
    }
}

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// Mono 16 kHz MP3 with any video stream dropped (default bitrate "64k").
pub async fn convert_to_mp3(
    input: &Path,
    output: &Path,
    audio_bitrate: &str,
    cancel: &CancellationToken,
) -> Result<(), FfmpegError> {
    let mut args = vec![OsString::from("-i"), input.as_os_str().to_owned()];
    args.extend(os_args(&[
        "-vn",
        "-c:a",
        "libmp3lame",
        "-b:a",
        audio_bitrate,
        "-ar",
        "16000",
        "-ac",
        "1",
    ]));
    run_conversion(args, output, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::run as block;

    #[test]
    fn converts_and_cancels() {
        if ffmpeg_path().is_none() {
            eprintln!("skipping: FFmpeg is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("tone.wav");
        let args = os_args(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-t",
            "0.5",
        ]);
        block(run_conversion(args, &wav, &CancellationToken::new())).unwrap();
        let mp3 = dir.path().join("tone.mp3");
        block(convert_to_mp3(&wav, &mp3, "64k", &CancellationToken::new())).unwrap();
        assert!(std::fs::metadata(&mp3).unwrap().len() > 0);

        let missing = dir.path().join("missing.wav");
        let err = block(convert_to_mp3(
            &missing,
            &dir.path().join("o.mp3"),
            "64k",
            &CancellationToken::new(),
        ))
        .unwrap_err();
        assert!(matches!(err, FfmpegError::Failed { .. }));

        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = block(convert_to_mp3(&wav, &mp3, "64k", &cancel)).unwrap_err();
        assert_eq!(err, FfmpegError::Cancelled);
    }
}
