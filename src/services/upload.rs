//! File transcription.
//!
//! The self-hosted path uploads the whole file in one request (the user's own
//! server, so no third-party size cap and no chunking). Containers the server
//! may not accept are re-encoded to mono 16 kHz MP3 first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::services::ffmpeg::{FfmpegError, convert_to_mp3};
use crate::services::runtime::http_client;
use crate::services::transcription::resolve_route;

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

pub const UPLOAD_CANCELLED_CODE: &str = "UPLOAD_CANCELLED";

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
    convert_to_mp3(source, &prepared.path, "64k", cancel).await?;
    Ok(prepared)
}

/// Operations registered under one request id.
type OperationSet = Arc<Mutex<Vec<(u64, CancellationToken)>>>;

/// In-flight uploads keyed by request id; one token per operation.
#[derive(Default)]
pub struct UploadCancelRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<String, OperationSet>>,
}

/// One registered operation. Dropping it (or calling `release`) unregisters.
pub struct UploadRegistration<'a> {
    registry: &'a UploadCancelRegistry,
    request_id: Option<String>,
    set: Option<OperationSet>,
    id: u64,
    /// `None` when registered without a request id (not cancellable).
    pub token: Option<CancellationToken>,
}

impl UploadRegistration<'_> {
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        let (Some(request_id), Some(set)) = (self.request_id.take(), self.set.take()) else {
            return;
        };
        let mut entries = self.registry.entries.lock();
        let mut members = set.lock();
        members.retain(|(id, _)| *id != self.id);
        // Never delete a successor set registered after a cancel.
        if members.is_empty()
            && entries
                .get(&request_id)
                .is_some_and(|s| Arc::ptr_eq(s, &set))
        {
            entries.remove(&request_id);
        }
    }
}

impl Drop for UploadRegistration<'_> {
    fn drop(&mut self) {
        self.release_inner();
    }
}

impl UploadCancelRegistry {
    pub fn register(&self, request_id: Option<&str>) -> UploadRegistration<'_> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let Some(request_id) = request_id.filter(|r| !r.is_empty()) else {
            return UploadRegistration {
                registry: self,
                request_id: None,
                set: None,
                id,
                token: None,
            };
        };
        let token = CancellationToken::new();
        let set = self
            .entries
            .lock()
            .entry(request_id.to_string())
            .or_default()
            .clone();
        set.lock().push((id, token.clone()));
        UploadRegistration {
            registry: self,
            request_id: Some(request_id.to_string()),
            set: Some(set),
            id,
            token: Some(token),
        }
    }

    /// Cancels every operation under `request_id`; returns how many.
    pub fn cancel(&self, request_id: &str) -> usize {
        let Some(set) = self.entries.lock().remove(request_id) else {
            return 0;
        };
        let members = set.lock();
        for (_, token) in members.iter() {
            token.cancel();
        }
        members.len()
    }
}

static REGISTRY: Lazy<UploadCancelRegistry> = Lazy::new(UploadCancelRegistry::default);

/// `cancel-upload-transcription`: true when something was cancelled.
pub fn cancel_upload_transcription(request_id: &str) -> bool {
    REGISTRY.cancel(request_id) > 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTranscriptionRequest {
    pub file_path: PathBuf,
    /// Base language code ("en"), or empty for auto-detect.
    pub language: String,
    pub remote_transcription_url: String,
    pub remote_transcription_model: String,
    /// Enables [`cancel_upload_transcription`] for this upload.
    pub request_id: Option<String>,
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

    pub fn is_cancelled(&self) -> bool {
        self.code.as_deref() == Some(UPLOAD_CANCELLED_CODE)
    }
}

/// Transcribes an audio or video file on the self-hosted server and returns
/// its text. Cancellable by request id; a cancel yields `UPLOAD_CANCELLED`.
pub async fn transcribe_audio_file(
    request: FileTranscriptionRequest,
) -> Result<String, UploadError> {
    let registration = REGISTRY.register(request.request_id.as_deref());
    let cancel = registration.token.clone().unwrap_or_default();
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled_error()),
        result = transcribe_file_inner(&request, &cancel) => result,
    };
    drop(registration);
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
    UploadError {
        message: "Cancelled".into(),
        code: Some(UPLOAD_CANCELLED_CODE.into()),
        message_key: None,
    }
}

async fn transcribe_file_inner(
    request: &FileTranscriptionRequest,
    cancel: &CancellationToken,
) -> Result<String, UploadError> {
    // Fail closed: a misconfigured route never falls through to a default.
    let route = resolve_route(
        &request.remote_transcription_url,
        &request.remote_transcription_model,
        "",
        Some(&request.language),
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
        return Err(UploadError {
            message: format!("Server error {status}: {excerpt}"),
            code: Some("SERVER_ERROR".into()),
            message_key: None,
        });
    };
    if status != 200 {
        let message = data
            .pointer("/error/message")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .or_else(|| {
                data.get("error")
                    .and_then(Value::as_str)
                    .filter(|m| !m.is_empty())
            })
            .map(str::to_string)
            .unwrap_or_else(|| format!("Self-hosted API Error: {status}"));
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
    fn extension_list_and_matching() {
        let lower: Vec<String> = UPLOAD_AUDIO_EXTENSIONS
            .iter()
            .map(|e| e.to_lowercase())
            .collect();
        assert_eq!(lower, UPLOAD_AUDIO_EXTENSIONS);
        let unique: std::collections::HashSet<_> = UPLOAD_AUDIO_EXTENSIONS.iter().collect();
        assert_eq!(unique.len(), UPLOAD_AUDIO_EXTENSIONS.len());
        for ext in [
            "mp3", "wav", "m4a", "webm", "ogg", "oga", "flac", "aac", "opus",
        ] {
            assert!(UPLOAD_AUDIO_EXTENSIONS.contains(&ext));
        }
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

    #[test]
    fn registry_cancels_every_operation_under_an_id() {
        let registry = UploadCancelRegistry::default();
        let first = registry.register(Some("req-1"));
        let second = registry.register(Some("req-1"));
        assert_eq!(registry.cancel("req-1"), 2);
        assert!(first.token.as_ref().unwrap().is_cancelled());
        assert!(second.token.as_ref().unwrap().is_cancelled());
    }

    #[test]
    fn registry_release_semantics() {
        let registry = UploadCancelRegistry::default();
        let first = registry.register(Some("req-1"));
        let second = registry.register(Some("req-1"));
        let first_token = first.token.clone().unwrap();
        first.release();
        assert_eq!(registry.cancel("req-1"), 1);
        assert!(!first_token.is_cancelled());
        assert!(second.token.as_ref().unwrap().is_cancelled());

        let a = registry.register(Some("req-2"));
        let b = registry.register(Some("req-2"));
        drop(a);
        drop(b);
        assert_eq!(registry.cancel("req-2"), 0);
        assert_eq!(registry.cancel("never-registered"), 0);
    }

    #[test]
    fn registry_without_id_is_inert() {
        let registry = UploadCancelRegistry::default();
        for id in [None, Some("")] {
            let reg = registry.register(id);
            assert!(reg.token.is_none());
            reg.release();
        }
    }

    #[test]
    fn registration_after_cancel_starts_a_fresh_set() {
        let registry = UploadCancelRegistry::default();
        let first = registry.register(Some("req-1"));
        registry.cancel("req-1");
        let second = registry.register(Some("req-1"));
        assert!(!second.token.as_ref().unwrap().is_cancelled());
        // The pre-cancel registration's release must not disturb the successor set.
        first.release();
        assert_eq!(registry.cancel("req-1"), 1);
        assert!(second.token.as_ref().unwrap().is_cancelled());
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
        let text = run(transcribe_audio_file(FileTranscriptionRequest {
            file_path: path,
            language: "en".into(),
            remote_transcription_url: server.url("/v1"),
            remote_transcription_model: "whisper-large-v3".into(),
            request_id: Some("upload-request".into()),
        }))
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
        let err = run(transcribe_audio_file(FileTranscriptionRequest {
            file_path: path,
            ..Default::default()
        }))
        .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("CUSTOM_ENDPOINT_INVALID"));
    }

    #[test]
    fn maps_server_errors() {
        let (_dir, path) = temp_audio("mp3");
        for (status, body, expected, code) in [
            (500, r#"{"error":{"message":"boom"}}"#, "boom", None),
            (400, r#"{"error":"bad file"}"#, "bad file", None),
            (404, r#"{}"#, "Self-hosted API Error: 404", None),
            (
                413,
                "Request Entity Too Large",
                "Server error 413: Request Entity Too Large",
                Some("SERVER_ERROR"),
            ),
        ] {
            let server = MockServer::start(move |_| Reply::json(status, body));
            let err = run(transcribe_audio_file(FileTranscriptionRequest {
                file_path: path.clone(),
                remote_transcription_url: server.url("/v1"),
                ..Default::default()
            }))
            .unwrap_err();
            assert_eq!(err.message, expected);
            assert_eq!(err.code.as_deref(), code);
        }
    }

    #[test]
    fn cancel_by_request_id() {
        let server = MockServer::start(|_| {
            Reply::json(200, r#"{"text":"late"}"#).delayed(Duration::from_secs(3))
        });
        let (_dir, path) = temp_audio("wav");
        let request = FileTranscriptionRequest {
            file_path: path,
            remote_transcription_url: server.url("/v1"),
            request_id: Some("cancel-me".into()),
            ..Default::default()
        };
        let err = run(async {
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                assert!(cancel_upload_transcription("cancel-me"));
            });
            transcribe_audio_file(request).await
        })
        .unwrap_err();
        assert!(err.is_cancelled());
        assert_eq!(err.message, "Cancelled");
        assert!(
            !cancel_upload_transcription("cancel-me"),
            "finished uploads unregister"
        );
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
