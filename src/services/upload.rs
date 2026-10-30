//! File transcription.
//!
//! The self-hosted path uploads the whole file in one request (the user's own
//! server, so no third-party size cap and no chunking). Containers the server
//! may not accept are re-encoded to mono 16 kHz MP3 first.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::services::ffmpeg::{FfmpegError, convert_to_mp3};
use crate::services::runtime::http_client;
use crate::services::transcription::resolve_route;
use crate::text::api_error::extract_api_error_message;

/// Extensions the upload picker admits.
pub const UPLOAD_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "wav", "m4a", "webm", "ogg", "oga", "flac", "aac", "opus", "mpeg", "mpg", "mpga", "mp2",
    "mp4", "m4v", "m4b", "mov", "mkv", "mka", "3gp", "avi", "wmv", "wma", "aiff", "aif", "aifc",
    "caf", "amr", "ac3", "au", "snd", "wv", "ape",
];

/// Containers uploaded as-is, with the content type sent for each.
pub const AUDIO_MIME_TYPES: &[(&str, &str)] = &[
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("webm", "audio/webm"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("flac", "audio/flac"),
    ("aac", "audio/aac"),
    ("opus", "audio/ogg"),
];

/// True when the final extension of `file_name` is an admitted format.
pub fn is_supported_upload_file(file_name: &str) -> bool {
    match file_name.rfind('.') {
        Some(dot) if dot > 0 => {
            let ext = file_name[dot + 1..].to_lowercase();
            UPLOAD_AUDIO_EXTENSIONS.contains(&ext.as_str())
        }
        _ => false,
    }
}

fn upload_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

fn native_mime(path: &Path) -> Option<&'static str> {
    let ext = upload_extension(path);
    AUDIO_MIME_TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, mime)| *mime)
}

/// Content type for an upload; `audio/mpeg` for anything not native.
pub fn provider_content_type(path: &Path) -> &'static str {
    native_mime(path).unwrap_or("audio/mpeg")
}

/// The file to upload; a temporary MP3 is deleted on drop.
#[derive(Debug)]
pub struct PreparedUpload {
    pub path: PathBuf,
    temporary: bool,
}

impl Drop for PreparedUpload {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Uses native containers as-is and re-encodes anything else to MP3.
pub async fn prepare_provider_upload(
    source: &Path,
    cancel: &CancellationToken,
) -> Result<PreparedUpload, FfmpegError> {
    if native_mime(source).is_some() {
        return Ok(PreparedUpload {
            path: source.to_path_buf(),
            temporary: false,
        });
    }
    let mp3 = std::env::temp_dir().join(format!(
        "whisper-upload-{}-{}.mp3",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    // Owning the path before conversion removes partial output on failure.
    let prepared = PreparedUpload {
        path: mp3,
        temporary: true,
    };
    convert_to_mp3(source, &prepared.path, cancel).await?;
    Ok(prepared)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTranscriptionRequest {
    pub file_path: PathBuf,
    /// The saved language preference ("en-US", "auto", ...).
    pub preferred_language: String,
    pub remote_transcription_url: String,
    pub remote_transcription_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct UploadError {
    pub message: String,
    pub code: Option<String>,
    pub message_key: Option<String>,
}

impl UploadError {
    fn plain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            message_key: None,
        }
    }
}

/// Transcribes an audio or video file on the self-hosted server and returns
/// its text. Cancelling `cancel` aborts the conversion or upload.
pub async fn transcribe_audio_file(
    request: FileTranscriptionRequest,
    cancel: CancellationToken,
) -> Result<String, UploadError> {
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled_error()),
        result = transcribe_file_inner(&request, &cancel) => result,
    };
    match result {
        Err(_) if cancel.is_cancelled() => {
            log::debug!("audio file transcription cancelled");
            Err(cancelled_error())
        }
        Err(err) => {
            log::error!("audio file transcription error: {}", err.message);
            Err(err)
        }
        ok => ok,
    }
}

fn cancelled_error() -> UploadError {
    UploadError::plain("Cancelled")
}

async fn transcribe_file_inner(
    request: &FileTranscriptionRequest,
    cancel: &CancellationToken,
) -> Result<String, UploadError> {
    // Fail closed: a misconfigured route never falls through to a default.
    let route = resolve_route(
        &request.remote_transcription_url,
        &request.remote_transcription_model,
        &request.preferred_language,
    )
    .map_err(|e| UploadError {
        message: e.message.clone(),
        code: Some(e.code.to_string()),
        message_key: Some(e.message_key.to_string()),
    })?;
    let source = std::fs::canonicalize(&request.file_path)
        .map_err(|_| UploadError::plain("Invalid file path"))?;

    let upload = prepare_provider_upload(&source, cancel)
        .await
        .map_err(|e| UploadError::plain(e.to_string()))?;
    let bytes = tokio::fs::read(&upload.path)
        .await
        .map_err(|e| UploadError::plain(e.to_string()))?;
    let file_name = upload
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".into());
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(provider_content_type(&upload.path))
        .map_err(|e| UploadError::plain(e.to_string()))?;
    let mut form = reqwest::multipart::Form::new().part("file", part);
    if let Some(model) = route.model.clone() {
        form = form.text("model", model);
    }
    if let Some(language) = route.language.clone() {
        form = form.text("language", language);
    }

    let response = http_client()
        .post(&route.endpoint)
        .multipart(form)
        .send()
        .await
        .map_err(|e| UploadError::plain(e.to_string()))?;
    let status = response.status().as_u16();
    let text = response
        .text()
        .await
        .map_err(|e| UploadError::plain(e.to_string()))?;
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        // Proxies and gateways return non-JSON error pages (413, 504).
        let excerpt: String = text.chars().take(120).collect();
        return Err(UploadError::plain(format!(
            "Server error {status}: {excerpt}"
        )));
    };
    if status != 200 {
        let message = extract_api_error_message(&data, &format!("Self-hosted API Error: {status}"));
        return Err(UploadError::plain(message));
    }
    Ok(data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// Opens the system file picker filtered to upload formats. `None` when the
/// user cancels. Call from the UI thread's async context.
pub async fn select_audio_files(multiple: bool) -> Option<Vec<PathBuf>> {
    let dialog =
        rfd::AsyncFileDialog::new().add_filter("Audio and Video Files", UPLOAD_AUDIO_EXTENSIONS);
    let paths: Vec<PathBuf> = if multiple {
        dialog
            .pick_files()
            .await?
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    } else {
        vec![dialog.pick_file().await?.path().to_path_buf()]
    };
    (!paths.is_empty()).then_some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::{MockServer, Reply, run};
    use std::time::Duration;

    #[test]
    fn matches_supported_extensions() {
        assert!(is_supported_upload_file("standup.mp3"));
        assert!(is_supported_upload_file("Screen Recording.MOV"));
        assert!(is_supported_upload_file("lecture.mpeg"));
        assert!(is_supported_upload_file("voice memo.m4a"));
        assert!(is_supported_upload_file("interview.2024.aiff"));
        assert!(!is_supported_upload_file("notes.txt"));
        assert!(!is_supported_upload_file("clip.mp4.part"));
        assert!(!is_supported_upload_file("README"));
        assert!(!is_supported_upload_file(".mp3"));
    }

    #[test]
    fn content_types() {
        assert_eq!(provider_content_type(Path::new("a.M4A")), "audio/mp4");
        assert_eq!(provider_content_type(Path::new("a.opus")), "audio/ogg");
        assert_eq!(provider_content_type(Path::new("a.mov")), "audio/mpeg");
    }

    fn temp_audio(ext: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("clip.{ext}"));
        std::fs::write(&path, b"RIFF0000WAVEfake").unwrap();
        (dir, path)
    }

    #[test]
    fn uploads_native_files_with_route_fields() {
        let server = MockServer::start(|_| Reply::json(200, r#"{"text":"self-hosted"}"#));
        let (_dir, path) = temp_audio("wav");
        let text = run(transcribe_audio_file(
            FileTranscriptionRequest {
                file_path: path,
                preferred_language: "en-US".into(),
                remote_transcription_url: server.url("/v1"),
                remote_transcription_model: "whisper-large-v3".into(),
            },
            CancellationToken::new(),
        ))
        .unwrap();
        assert_eq!(text, "self-hosted");
        let requests = server.requests();
        assert_eq!(requests.len(), 1, "uploads skip the reachability probe");
        assert_eq!(requests[0].path, "/v1/audio/transcriptions");
        let body = String::from_utf8_lossy(&requests[0].body).to_string();
        assert!(body.contains("filename=\"clip.wav\""));
        assert!(body.contains("Content-Type: audio/wav"));
        assert!(body.contains("name=\"model\"\r\n\r\nwhisper-large-v3"));
        assert!(body.contains("name=\"language\"\r\n\r\nen"));
        assert!(!body.contains("name=\"prompt\""));
    }

    #[test]
    fn missing_server_fails_before_any_request() {
        let (_dir, path) = temp_audio("wav");
        let err = run(transcribe_audio_file(
            FileTranscriptionRequest {
                file_path: path,
                ..Default::default()
            },
            CancellationToken::new(),
        ))
        .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("CUSTOM_ENDPOINT_INVALID"));
    }

    #[test]
    fn maps_server_errors() {
        let (_dir, path) = temp_audio("mp3");
        for (status, body, expected) in [
            (500, r#"{"error":{"message":"boom"}}"#, "boom"),
            (400, r#"{"error":"bad file"}"#, "bad file"),
            (
                404,
                r#"{"detail":"Model 'x' is not installed"}"#,
                "Model 'x' is not installed",
            ),
            (404, r#"{}"#, "Self-hosted API Error: 404"),
            (
                413,
                "Request Entity Too Large",
                "Server error 413: Request Entity Too Large",
            ),
        ] {
            let server = MockServer::start(move |_| Reply::json(status, body));
            let err = run(transcribe_audio_file(
                FileTranscriptionRequest {
                    file_path: path.clone(),
                    remote_transcription_url: server.url("/v1"),
                    ..Default::default()
                },
                CancellationToken::new(),
            ))
            .unwrap_err();
            assert_eq!(err.message, expected);
            assert_eq!(err.code, None);
        }
    }

    #[test]
    fn cancelling_the_token_aborts_the_upload() {
        let server = MockServer::start(|_| {
            Reply::json(200, r#"{"text":"late"}"#).delayed(Duration::from_secs(3))
        });
        let (_dir, path) = temp_audio("wav");
        let request = FileTranscriptionRequest {
            file_path: path,
            remote_transcription_url: server.url("/v1"),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let err = run(async {
            let cancel = CancellationToken::new();
            let trigger = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                trigger.cancel();
            });
            transcribe_audio_file(request, cancel).await
        })
        .unwrap_err();
        assert_eq!(err.message, "Cancelled");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn non_native_containers_are_reencoded_to_mp3() {
        if crate::services::ffmpeg::ffmpeg_path().is_none() {
            eprintln!("skipping: FFmpeg is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let aiff = dir.path().join("memo.aiff");
        let status = std::process::Command::new(crate::services::ffmpeg::ffmpeg_path().unwrap())
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=16000",
                "-t",
                "0.3",
                "-y",
            ])
            .arg(&aiff)
            .output()
            .unwrap()
            .status;
        assert!(status.success());
        let prepared = run(prepare_provider_upload(&aiff, &CancellationToken::new())).unwrap();
        assert_eq!(upload_extension(&prepared.path), "mp3");
        let temp_path = prepared.path.clone();
        assert!(temp_path.exists());
        drop(prepared);
        assert!(!temp_path.exists(), "temporary MP3 is removed");
    }
}
