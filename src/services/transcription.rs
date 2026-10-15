//! Self-hosted speech-to-text.
//!
//! Every request goes to the user's OpenAI-compatible server. A HEAD probe
//! with a short deadline proves the server is reachable before any audio is
//! uploaded; the upload itself, including the response body, then has its own
//! longer deadline. Futures here need the Tokio reactor: run them through
//! [`crate::services::runtime::spawn`].

use std::time::Duration;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::services::runtime::http_client;
use crate::settings::Settings;
use crate::text::language::get_base_language_code;
use crate::text::url::{build_api_url, is_secure_http_endpoint, normalize_base_url};

pub const CUSTOM_ENDPOINT_INVALID_CODE: &str = "CUSTOM_ENDPOINT_INVALID";
pub const CUSTOM_ENDPOINT_INVALID_MESSAGE_KEY: &str =
    "dictation.errorDescriptions.customEndpointInvalid";
pub const CONNECTION_FAILED_CODE: &str = "TRANSCRIPTION_CONNECTION_FAILED";
pub const CONNECTION_FAILED_MESSAGE_KEY: &str =
    "dictation.errorDescriptions.transcriptionConnectionFailed";
pub const REQUEST_TIMEOUT_CODE: &str = "TRANSCRIPTION_REQUEST_TIMEOUT";
pub const REQUEST_TIMEOUT_MESSAGE_KEY: &str = "dictation.errorDescriptions.transcriptionTimeout";
pub const RATE_LIMITED_CODE: &str = "PROVIDER_RATE_LIMITED";
pub const RATE_LIMITED_MESSAGE_KEY: &str = "dictation.errorDescriptions.providerRateLimited";
pub const INVALID_KEY_CODE: &str = "INVALID_KEY";
pub const SERVER_ERROR_CODE: &str = "SERVER_ERROR";

/// Reachability deadline for the HEAD probe.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3);
/// Deadline for the upload and its response body.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a batch transcription request goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Full `.../audio/transcriptions` URL.
    pub endpoint: String,
    pub model: Option<String>,
    /// Base language code ("en", "zh"); `None` for auto-detect.
    pub language: Option<String>,
}

/// The transcription server is missing or unusable; nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RouteError {
    pub code: &'static str,
    pub message: String,
    pub message_key: &'static str,
}

impl RouteError {
    fn new(message: &str) -> Self {
        Self {
            code: CUSTOM_ENDPOINT_INVALID_CODE,
            message: message.to_string(),
            message_key: CUSTOM_ENDPOINT_INVALID_MESSAGE_KEY,
        }
    }
}

/// Resolves the dictation route from saved settings.
pub fn resolve_transcription_route(settings: &Settings) -> Result<Route, RouteError> {
    resolve_route(
        &settings.remote_transcription_url,
        &settings.remote_transcription_model,
        &settings.preferred_language,
        None,
    )
}

/// Resolves a route; `effective_language` (when non-empty) wins over the preference.
pub fn resolve_route(
    url: &str,
    model: &str,
    preferred_language: &str,
    effective_language: Option<&str>,
) -> Result<Route, RouteError> {
    let language = match effective_language.filter(|l| !l.is_empty()) {
        Some(lang) => Some(lang.to_string()),
        None => get_base_language_code(preferred_language.trim()).map(str::to_string),
    };
    // Fail closed: without a usable server URL no audio leaves the device.
    let raw = url.trim();
    if raw.is_empty() {
        return Err(RouteError::new(
            "Self-hosted transcription URL is not configured",
        ));
    }
    let base = normalize_base_url(raw);
    let endpoint = (!base.is_empty() && is_secure_http_endpoint(&base))
        .then(|| build_api_url(&base, "/audio/transcriptions"))
        .flatten()
        .ok_or_else(|| {
            RouteError::new("Self-hosted transcription URL is invalid or unsupported")
        })?;
    let model = Some(model.trim())
        .filter(|m| !m.is_empty())
        .map(str::to_string);
    Ok(Route {
        endpoint,
        model,
        language,
    })
}

/// File extension the server sees for a recording of `mime_type`.
pub fn audio_extension_for_mime(mime_type: &str) -> &'static str {
    if mime_type.contains("ogg") {
        "ogg"
    } else if mime_type.contains("mp4") {
        "mp4"
    } else if mime_type.contains("mpeg") {
        "mp3"
    } else if mime_type.contains("wav") {
        "wav"
    } else {
        "webm"
    }
}

/// One multipart upload to `{base}/audio/transcriptions`.
#[derive(Debug, Clone)]
pub struct TranscriptionRequest {
    pub endpoint: String,
    pub model: Option<String>,
    pub language: Option<String>,
    /// Already-fitted dictionary/script prompt; omitted when empty.
    pub prompt: Option<String>,
    pub audio: Vec<u8>,
    pub file_name: String,
    pub mime_type: String,
}

impl TranscriptionRequest {
    /// A request for `route` carrying `audio` (e.g. a WAV from [`crate::audio::wav`]).
    pub fn new(route: &Route, audio: Vec<u8>, mime_type: &str, prompt: Option<String>) -> Self {
        Self {
            endpoint: route.endpoint.clone(),
            model: route.model.clone(),
            language: route.language.clone(),
            prompt,
            audio,
            file_name: format!("audio.{}", audio_extension_for_mime(mime_type)),
            mime_type: mime_type.to_string(),
        }
    }
}

/// Deadlines, overridable for tests.
#[derive(Debug, Clone, Copy)]
pub struct TranscribeOptions {
    pub connection_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            connection_timeout: CONNECTION_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptionResponse {
    /// Server text as returned (not trimmed); guaranteed to contain non-whitespace.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptionError {
    #[error("{0}")]
    Route(RouteError),
    #[error("Could not reach the transcription server within 3 seconds")]
    ConnectionFailed,
    #[error("Transcription request timed out after 30 seconds")]
    RequestTimeout,
    #[error("API Error: {status} {body}")]
    Http { status: u16, body: String },
    /// The upload failed after the server was reachable (reset, refused, ...).
    #[error("{0}")]
    Network(String),
    #[error("Failed to parse API response: {0}")]
    InvalidResponse(String),
    #[error("No text transcribed - audio may be too short, silent, or in an unsupported format")]
    EmptyText,
    /// Cancelled by the caller; not an error to report.
    #[error("Transcription cancelled")]
    Cancelled,
}

impl TranscriptionError {
    /// Error code stored with failed History rows and used by the UI.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Route(e) => Some(e.code),
            Self::ConnectionFailed => Some(CONNECTION_FAILED_CODE),
            Self::RequestTimeout => Some(REQUEST_TIMEOUT_CODE),
            Self::Http { status: 401, .. } => Some(INVALID_KEY_CODE),
            Self::Http { status: 429, .. } => Some(RATE_LIMITED_CODE),
            Self::Http { status, .. } if *status >= 500 => Some(SERVER_ERROR_CODE),
            _ => None,
        }
    }

    /// Localization key for the toast description, when one exists.
    pub fn message_key(&self) -> Option<&'static str> {
        match self {
            Self::Route(e) => Some(e.message_key),
            Self::ConnectionFailed => Some(CONNECTION_FAILED_MESSAGE_KEY),
            Self::RequestTimeout => Some(REQUEST_TIMEOUT_MESSAGE_KEY),
            Self::Http { status: 429, .. } => Some(RATE_LIMITED_MESSAGE_KEY),
            _ => None,
        }
    }
}

/// Transcribes with the default deadlines. Cancelling `cancel` aborts the
/// probe or upload immediately and yields [`TranscriptionError::Cancelled`].
pub async fn transcribe(
    request: TranscriptionRequest,
    cancel: CancellationToken,
) -> Result<TranscriptionResponse, TranscriptionError> {
    transcribe_with(request, cancel, TranscribeOptions::default()).await
}

pub async fn transcribe_with(
    request: TranscriptionRequest,
    cancel: CancellationToken,
    options: TranscribeOptions,
) -> Result<TranscriptionResponse, TranscriptionError> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(TranscriptionError::Cancelled),
        result = run(request, options) => result,
    }
}

async fn run(
    request: TranscriptionRequest,
    options: TranscribeOptions,
) -> Result<TranscriptionResponse, TranscriptionError> {
    let client = http_client();
    log::debug!(
        "transcription request starting: model={:?} bytes={} mime={} language={:?}",
        request.model,
        request.audio.len(),
        request.mime_type,
        request.language
    );

    // Any HTTP response (404/405 included) proves reachability; slow inference
    // must not be mistaken for an unreachable server.
    let probe = client.head(&request.endpoint).send();
    match tokio::time::timeout(options.connection_timeout, probe).await {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => {
            log::warn!("transcription server unreachable: {err}");
            return Err(TranscriptionError::ConnectionFailed);
        }
        Err(_) => return Err(TranscriptionError::ConnectionFailed),
    }

    let upload = async {
        let mut part =
            reqwest::multipart::Part::bytes(request.audio).file_name(request.file_name.clone());
        part = match part.mime_str(&request.mime_type) {
            Ok(p) => p,
            Err(err) => return Err(TranscriptionError::Network(err.to_string())),
        };
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(model) = request.model.filter(|m| !m.is_empty()) {
            form = form.text("model", model);
        }
        if let Some(language) = request.language.filter(|l| !l.is_empty()) {
            form = form.text("language", language);
        }
        if let Some(prompt) = request.prompt.filter(|p| !p.is_empty()) {
            form = form.text("prompt", prompt);
        }
        let response = client
            .post(&request.endpoint)
            .multipart(form)
            .send()
            .await
            .map_err(|e| TranscriptionError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        // The deadline covers the body as well as the headers.
        let body = response
            .text()
            .await
            .map_err(|e| TranscriptionError::Network(e.to_string()))?;
        Ok((status, body))
    };
    let (status, body) = match tokio::time::timeout(options.request_timeout, upload).await {
        Ok(result) => result?,
        Err(_) => return Err(TranscriptionError::RequestTimeout),
    };

    if !(200..300).contains(&status) {
        log::error!("transcription API error response: status={status}");
        return Err(TranscriptionError::Http { status, body });
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| TranscriptionError::InvalidResponse(e.to_string()))?;
    let text = json
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if text.trim().is_empty() {
        log::info!("transcription returned empty text");
        return Err(TranscriptionError::EmptyText);
    }
    Ok(TranscriptionResponse { text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::{MockServer, Reply, run};
    use std::time::Instant;

    fn settings(url: &str, model: &str, language: &str) -> Settings {
        Settings {
            remote_transcription_url: url.into(),
            remote_transcription_model: model.into(),
            preferred_language: language.into(),
            ..Settings::default()
        }
    }

    #[test]
    fn routes_to_the_configured_server() {
        let route =
            resolve_transcription_route(&settings("http://192.168.1.5:11434/v1", "whisper-1", ""))
                .unwrap();
        assert_eq!(
            route.endpoint,
            "http://192.168.1.5:11434/v1/audio/transcriptions"
        );
        assert_eq!(route.model.as_deref(), Some("whisper-1"));
    }

    #[test]
    fn missing_url_fails_closed() {
        let err = resolve_transcription_route(&settings("  ", "", "auto")).unwrap_err();
        assert_eq!(err.code, "CUSTOM_ENDPOINT_INVALID");
        assert!(err.message.contains("not configured"));
    }

    #[test]
    fn invalid_or_public_http_fails_closed() {
        for url in ["not a url", "ftp://localhost:8080", "http://example.com/v1"] {
            let err = resolve_transcription_route(&settings(url, "", "auto")).unwrap_err();
            assert!(err.message.contains("invalid or unsupported"), "{url}");
            assert_eq!(err.message_key, CUSTOM_ENDPOINT_INVALID_MESSAGE_KEY);
        }
    }

    #[test]
    fn bare_origin_gains_the_path_and_language_rules() {
        let route = resolve_transcription_route(&settings(
            "https://stt.internal.example.com",
            "tiny",
            "auto",
        ))
        .unwrap();
        assert_eq!(
            route.endpoint,
            "https://stt.internal.example.com/audio/transcriptions"
        );
        assert_eq!(route.language, None);

        let route = resolve_route("http://localhost:8000/v1", " ", "de-DE", Some("ja")).unwrap();
        assert_eq!(route.model, None);
        assert_eq!(route.language.as_deref(), Some("ja"));
        let route = resolve_route("http://localhost:8000/v1", "", "de-DE", None).unwrap();
        assert_eq!(route.language.as_deref(), Some("de"));
    }

    #[test]
    fn error_codes_and_keys() {
        let http = |status| TranscriptionError::Http {
            status,
            body: String::new(),
        };
        assert_eq!(http(429).message_key(), Some(RATE_LIMITED_MESSAGE_KEY));
        assert_eq!(http(404).code(), None);
        assert_eq!(
            TranscriptionError::ConnectionFailed.code(),
            Some("TRANSCRIPTION_CONNECTION_FAILED")
        );
        assert_eq!(
            TranscriptionError::RequestTimeout.message_key(),
            Some(REQUEST_TIMEOUT_MESSAGE_KEY)
        );
        assert_eq!(
            TranscriptionError::EmptyText.to_string(),
            "No text transcribed - audio may be too short, silent, or in an unsupported format"
        );
        assert_eq!(http(500).to_string(), "API Error: 500 ");
    }

    fn request_for(server: &MockServer) -> TranscriptionRequest {
        let route = resolve_route(&server.url("/v1"), "self-hosted-model", "zh-CN", None).unwrap();
        TranscriptionRequest::new(
            &route,
            vec![1, 2, 3, 4],
            "audio/wav",
            Some("Whisper, GPUI".into()),
        )
    }

    #[test]
    fn posts_multipart_fields_after_a_head_probe() {
        run(async {
            let server = MockServer::start(|req| {
                if req.method == "HEAD" {
                    Reply::status(405)
                } else {
                    Reply::json(200, r#"{"text":"self-hosted text"}"#)
                }
            });
            let result = transcribe(request_for(&server), CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(result.text, "self-hosted text");
            let requests = server.requests();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0].method, "HEAD");
            assert!(
                requests[0].body.is_empty(),
                "the probe must not upload audio"
            );
            assert_eq!(requests[0].path, "/v1/audio/transcriptions");
            let post = &requests[1];
            assert_eq!(post.method, "POST");
            assert_eq!(post.path, "/v1/audio/transcriptions");
            let content_type = post.header("content-type").unwrap_or_default();
            assert!(content_type.starts_with("multipart/form-data; boundary="));
            let body = String::from_utf8_lossy(&post.body);
            assert!(body.contains("name=\"file\"; filename=\"audio.wav\""));
            assert!(body.contains("Content-Type: audio/wav"));
            assert!(body.contains("name=\"model\"\r\n\r\nself-hosted-model\r\n"));
            assert!(body.contains("name=\"language\"\r\n\r\nzh\r\n"));
            assert!(body.contains("name=\"prompt\"\r\n\r\nWhisper, GPUI\r\n"));
        })
    }

    #[test]
    fn omits_empty_optional_fields() {
        run(async {
            let server = MockServer::start(|_| Reply::json(200, r#"{"text":"ok"}"#));
            let route = resolve_route(&server.url("/v1"), "", "auto", None).unwrap();
            let request =
                TranscriptionRequest::new(&route, vec![0; 8], "audio/webm", Some(String::new()));
            transcribe(request, CancellationToken::new()).await.unwrap();
            let body = String::from_utf8_lossy(&server.requests()[1].body).to_string();
            assert!(body.contains("filename=\"audio.webm\""));
            assert!(!body.contains("name=\"model\""));
            assert!(!body.contains("name=\"language\""));
            assert!(!body.contains("name=\"prompt\""));
        })
    }

    #[test]
    fn refused_connection_reports_unreachable_without_uploading() {
        run(async {
            // Bind then drop a listener so the port refuses connections.
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let route =
                resolve_route(&format!("http://127.0.0.1:{port}/v1"), "", "", None).unwrap();
            let request = TranscriptionRequest::new(&route, vec![1], "audio/wav", None);
            let err = transcribe(request, CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(err, TranscriptionError::ConnectionFailed);
            assert_eq!(err.code(), Some("TRANSCRIPTION_CONNECTION_FAILED"));
        })
    }

    #[test]
    fn slow_probe_hits_the_connection_deadline() {
        run(async {
            let server = MockServer::start(|req| {
                if req.method == "HEAD" {
                    Reply::json(200, "{}").delayed(Duration::from_millis(800))
                } else {
                    Reply::json(200, r#"{"text":"never"}"#)
                }
            });
            let options = TranscribeOptions {
                connection_timeout: Duration::from_millis(150),
                request_timeout: Duration::from_secs(5),
            };
            let err = transcribe_with(request_for(&server), CancellationToken::new(), options)
                .await
                .unwrap_err();
            assert_eq!(err, TranscriptionError::ConnectionFailed);
            assert!(server.requests().iter().all(|r| r.method == "HEAD"));
        })
    }

    #[test]
    fn reachable_server_may_take_longer_than_the_probe_deadline() {
        run(async {
            let server = MockServer::start(|req| {
                if req.method == "HEAD" {
                    Reply::status(405)
                } else {
                    Reply::json(200, r#"{"text":"A completed dictation."}"#)
                        .delayed(Duration::from_millis(400))
                }
            });
            let options = TranscribeOptions {
                connection_timeout: Duration::from_millis(150),
                request_timeout: Duration::from_secs(5),
            };
            let result = transcribe_with(request_for(&server), CancellationToken::new(), options)
                .await
                .unwrap();
            assert_eq!(result.text, "A completed dictation.");
        })
    }

    #[test]
    fn stalled_upload_hits_the_request_deadline() {
        run(async {
            let server = MockServer::start(|req| {
                if req.method == "HEAD" {
                    Reply::status(405)
                } else {
                    Reply::json(200, r#"{"text":"late"}"#).delayed(Duration::from_millis(1500))
                }
            });
            let options = TranscribeOptions {
                connection_timeout: Duration::from_secs(2),
                request_timeout: Duration::from_millis(200),
            };
            let err = transcribe_with(request_for(&server), CancellationToken::new(), options)
                .await
                .unwrap_err();
            assert_eq!(err.code(), Some("TRANSCRIPTION_REQUEST_TIMEOUT"));
        })
    }

    #[test]
    fn maps_http_errors_and_empty_text() {
        run(async {
            for (status, body, code) in [
                (401, "{}", Some("INVALID_KEY")),
                (429, "slow down", Some("PROVIDER_RATE_LIMITED")),
                (502, "bad gateway", Some("SERVER_ERROR")),
                (400, "bad", None),
            ] {
                let server = MockServer::start(move |_| Reply::json(status, body));
                let err = transcribe(request_for(&server), CancellationToken::new())
                    .await
                    .unwrap_err();
                assert_eq!(err.code(), code, "status {status}");
                assert_eq!(err.to_string(), format!("API Error: {status} {body}"));
            }
            let server = MockServer::start(|_| Reply::json(200, r#"{"text":"   "}"#));
            let err = transcribe(request_for(&server), CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(err, TranscriptionError::EmptyText);
            let server = MockServer::start(|_| Reply::json(200, "not json"));
            let err = transcribe(request_for(&server), CancellationToken::new())
                .await
                .unwrap_err();
            assert!(matches!(err, TranscriptionError::InvalidResponse(_)));
        })
    }

    #[test]
    fn cancellation_aborts_the_upload() {
        run(async {
            let server = MockServer::start(|req| {
                if req.method == "HEAD" {
                    Reply::status(405)
                } else {
                    Reply::json(200, r#"{"text":"late"}"#).delayed(Duration::from_secs(3))
                }
            });
            let cancel = CancellationToken::new();
            let trigger = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                trigger.cancel();
            });
            let started = Instant::now();
            let err = transcribe(request_for(&server), cancel).await.unwrap_err();
            assert_eq!(err, TranscriptionError::Cancelled);
            assert!(started.elapsed() < Duration::from_secs(2));
        })
    }
}
