//! Text cleanup through the user's self-hosted OpenAI-compatible server and
//! model listing.
//!
//! Futures here need the Tokio reactor: run them through
//! [`crate::services::runtime::spawn`].

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::services::retry::{
    LLM_REQUEST_TIMEOUT_CODE, RetryOptions, api_should_retry, with_retry,
};
use crate::services::runtime::http_client;
use crate::text::api_error::extract_api_error_message;
use crate::text::prompts::{
    STRIPPABLE_SHAPED_PARAMS, apply_chat_completions_params, is_truncated_finish_reason,
    wrap_cleanup_transcript,
};
use crate::text::strip_thinking::strip_thinking_tags;
use crate::text::url::{
    build_api_url, ensure_v1_suffix, is_secure_http_endpoint, model_list_base_candidates,
    normalize_base_url,
};

const PROVIDER_NAME: &str = "LAN";

/// Client-side deadline for one LLM request.
pub const LLM_REQUEST_TIMEOUT_SECONDS: u64 = 30;

pub const TRUNCATED_OUTPUT_MESSAGE_KEY: &str = "dictation.errorDescriptions.cleanupTruncated";
pub const EMPTY_OUTPUT_MESSAGE_KEY: &str = "dictation.errorDescriptions.cleanupEmptyReply";
pub const HTTPS_REQUIRED_MESSAGE_KEY: &str = "reasoning.custom.httpsRequired";
const HTTPS_REQUIRED_MESSAGE: &str = "HTTPS required (HTTP allowed for local network only).";
pub const ENDPOINT_WITH_PROTOCOL_MESSAGE_KEY: &str = "reasoning.custom.endpointWithProtocol";
pub const ENDPOINT_UNAUTHORIZED_MESSAGE_KEY: &str = "reasoning.custom.endpointUnauthorized";
pub const UNABLE_TO_LOAD_MODELS_MESSAGE_KEY: &str = "reasoning.custom.unableToLoadModels";

/// Default `max_tokens`: `max(4096, clamp(len * 2, 512, 2048))` is always 4096.
const DEFAULT_MAX_TOKENS: u32 = 4096;
/// Deadline per model-list request, so a hung server cannot leave the
/// settings panel loading forever.
const LIST_MODELS_TIMEOUT: Duration = Duration::from_secs(15);

/// One cleanup request. The caller resolves the system prompt; the text is
/// wrapped in `<transcript>` tags and sent at temperature 0, and a truncated
/// or empty reply fails instead of returning partial text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CleanupRequestConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub system_prompt: String,
    pub disable_thinking: bool,
}

impl CleanupRequestConfig {
    /// The dictation cleanup request, also used by Prompt Studio's test.
    pub fn dictation(
        base_url: &str,
        api_key: Option<&str>,
        model: &str,
        system_prompt: &str,
        disable_thinking: bool,
    ) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.map(str::to_string),
            model: model.to_string(),
            system_prompt: system_prompt.to_string(),
            disable_thinking,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupErrorKind {
    /// Missing, malformed or public-HTTP endpoint; nothing was sent.
    InvalidEndpoint,
    /// Non-2xx reply; `status` holds the code.
    Http,
    /// No HTTP answer (refused, reset, unreadable body).
    Network,
    /// The client-side deadline expired.
    Timeout,
    /// The caller cancelled the request.
    Cancelled,
    /// The reply had no usable `choices`.
    InvalidResponse,
    /// Output hit the token cap.
    Truncated,
    /// The reply had no text.
    EmptyResponse,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct CleanupError {
    pub kind: CleanupErrorKind,
    pub message: String,
    pub message_key: Option<&'static str>,
    pub status: Option<u16>,
}

impl CleanupError {
    fn new(kind: CleanupErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            message_key: None,
            status: None,
        }
    }

    fn with_key(mut self, key: &'static str) -> Self {
        self.message_key = Some(key);
        self
    }

    fn http(message: String, status: u16) -> Self {
        Self {
            status: Some(status),
            ..Self::new(CleanupErrorKind::Http, message)
        }
    }

    fn cancelled() -> Self {
        Self {
            status: Some(499),
            ..Self::new(CleanupErrorKind::Cancelled, "Request cancelled")
        }
    }

    /// `LLM_REQUEST_TIMEOUT` for the deadline error.
    pub fn code(&self) -> Option<&'static str> {
        (self.kind == CleanupErrorKind::Timeout).then_some(LLM_REQUEST_TIMEOUT_CODE)
    }

    fn should_retry(&self) -> bool {
        api_should_retry(self.status, self.code())
    }
}

/// The deadline error ("Request timed out after 30s"); never retried.
pub fn llm_request_timeout_error(timeout_seconds: u64) -> CleanupError {
    CleanupError::new(
        CleanupErrorKind::Timeout,
        format!("Request timed out after {timeout_seconds}s"),
    )
}

/// Output cut off at the token cap.
pub fn truncated_output_error(message: &str) -> CleanupError {
    CleanupError::new(CleanupErrorKind::Truncated, message).with_key(TRUNCATED_OUTPUT_MESSAGE_KEY)
}

/// A reply with no text at all.
pub fn empty_output_error() -> CleanupError {
    CleanupError::new(
        CleanupErrorKind::EmptyResponse,
        "Model returned an empty response",
    )
    .with_key(EMPTY_OUTPUT_MESSAGE_KEY)
}

/// `{base}/v1`, refusing anything but HTTPS or private-network HTTP. Never
/// falls back: the prompt and key must not go to a host the user never chose.
pub fn resolve_self_hosted_openai_base(configured: &str) -> Result<String, CleanupError> {
    let normalized = ensure_v1_suffix(configured.trim());
    if normalized.is_empty() || !is_secure_http_endpoint(&normalized) {
        return Err(
            CleanupError::new(CleanupErrorKind::InvalidEndpoint, HTTPS_REQUIRED_MESSAGE)
                .with_key(HTTPS_REQUIRED_MESSAGE_KEY),
        );
    }
    Ok(normalized)
}

/// A fully read HTTP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub status_text: String,
    pub body: String,
}

impl HttpReply {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Fetch with a bounded param-stripping ladder for 400/422 rejections.
/// Rung 1 (blind): drop a `reasoning` object and retry once. Rung 2 (named):
/// strip exactly the shaped params the error body names and retry once.
/// `do_fetch` receives the serialized body so each retry re-serializes.
pub async fn fetch_with_param_fallback<F, Fut, E>(
    mut do_fetch: F,
    body: &mut Map<String, Value>,
    mut log_rejection: impl FnMut(u16, &[String]),
) -> Result<HttpReply, E>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<HttpReply, E>>,
{
    let rejected = |r: &HttpReply| r.status == 400 || r.status == 422;
    let serialize = |b: &Map<String, Value>| Value::Object(b.clone()).to_string();
    let mut res = do_fetch(serialize(body)).await?;
    if !rejected(&res) {
        return Ok(res);
    }
    if body.get("reasoning").is_some_and(is_truthy) {
        log_rejection(res.status, &["reasoning".to_string()]);
        body.remove("reasoning");
        res = do_fetch(serialize(body)).await?;
        if !rejected(&res) {
            return Ok(res);
        }
    }
    let named: Vec<String> = STRIPPABLE_SHAPED_PARAMS
        .iter()
        .filter(|p| body.contains_key(**p) && res.body.contains(**p))
        .map(|p| p.to_string())
        .collect();
    if !named.is_empty() {
        log_rejection(res.status, &named);
        for param in &named {
            body.remove(param);
        }
        res = do_fetch(serialize(body)).await?;
    }
    Ok(res)
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// Deadlines and backoff, overridable for tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CleanupTuning {
    pub request_timeout: Duration,
    pub retry: RetryOptions,
}

impl Default for CleanupTuning {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(LLM_REQUEST_TIMEOUT_SECONDS),
            retry: RetryOptions::default(),
        }
    }
}

/// Cleans `text` with the default deadlines. Cancelling `cancel` aborts only
/// this request; it then fails with [`CleanupErrorKind::Cancelled`].
pub async fn process_text(
    text: &str,
    config: &CleanupRequestConfig,
    cancel: &CancellationToken,
) -> Result<String, CleanupError> {
    process_text_with(text, config, &CleanupTuning::default(), cancel).await
}

pub async fn process_text_with(
    text: &str,
    config: &CleanupRequestConfig,
    tuning: &CleanupTuning,
    cancel: &CancellationToken,
) -> Result<String, CleanupError> {
    let base = resolve_self_hosted_openai_base(&config.base_url)?;
    let endpoint = build_api_url(&base, "/chat/completions").ok_or_else(|| {
        CleanupError::new(CleanupErrorKind::InvalidEndpoint, HTTPS_REQUIRED_MESSAGE)
            .with_key(HTTPS_REQUIRED_MESSAGE_KEY)
    })?;
    let model = match config.model.trim() {
        "" => "default",
        m => m,
    };
    let api_key = config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    let user_prompt = wrap_cleanup_transcript(text);

    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert(
        "messages".into(),
        json!([
            { "role": "system", "content": config.system_prompt },
            { "role": "user", "content": user_prompt },
        ]),
    );
    apply_chat_completions_params(
        &mut body,
        model,
        config.disable_thinking,
        DEFAULT_MAX_TOKENS,
    );
    log::debug!(
        "cleanup request: endpoint={endpoint} model={model} has_api_key={}",
        api_key.is_some()
    );

    // Stripped params stay stripped across retries.
    let shared_body = Arc::new(Mutex::new(body));
    let attempt = || {
        let shared_body = shared_body.clone();
        let cancel = cancel.clone();
        let endpoint = endpoint.clone();
        async move {
            if cancel.is_cancelled() {
                return Err(CleanupError::cancelled());
            }
            let request = async {
                let mut body = shared_body.lock().clone();
                let reply = fetch_with_param_fallback(
                    |json| post_json(&endpoint, api_key, json),
                    &mut body,
                    |status, stripped| {
                        log::warn!("cleanup param fallback: status={status} stripped={stripped:?}");
                    },
                )
                .await;
                *shared_body.lock() = body;
                let reply = reply?;
                if !reply.is_success() {
                    let data: Value = serde_json::from_str(&reply.body).unwrap_or_else(|_| {
                        let text = if reply.body.is_empty() {
                            reply.status_text.clone()
                        } else {
                            reply.body.clone()
                        };
                        json!({ "error": text })
                    });
                    let message = extract_api_error_message(
                        &data,
                        &format!("{PROVIDER_NAME} API error: {}", reply.status),
                    );
                    log::warn!(
                        "cleanup API error: status={} message={message}",
                        reply.status
                    );
                    return Err(CleanupError::http(message, reply.status));
                }
                // A malformed success body has no status, so it retries like
                // a network fault.
                serde_json::from_str::<Value>(&reply.body)
                    .map_err(|e| CleanupError::new(CleanupErrorKind::Network, e.to_string()))
            };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(CleanupError::cancelled()),
                result = tokio::time::timeout(tuning.request_timeout, request) => match result {
                    Ok(result) => result,
                    Err(_) => Err(llm_request_timeout_error(tuning.request_timeout.as_secs())),
                },
            }
        }
    };
    let response = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(CleanupError::cancelled()),
        result = with_retry(attempt, &tuning.retry, CleanupError::should_retry) => result,
    }?;

    let Some(choice) = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .filter(|c| !c.is_null())
    else {
        return Err(CleanupError::new(
            CleanupErrorKind::InvalidResponse,
            format!("Invalid response structure from {PROVIDER_NAME} API"),
        ));
    };
    let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
    if is_truncated_finish_reason(finish_reason) {
        return Err(truncated_output_error("Model output was truncated"));
    }
    let raw = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    // Reasoning models leak <think> blocks; keep them only when the user
    // explicitly enabled thinking.
    let text = if config.disable_thinking {
        strip_thinking_tags(raw)
    } else {
        raw.to_string()
    };
    if text.is_empty() {
        log::info!("cleanup returned empty response: finish_reason={finish_reason:?}");
        return Err(empty_output_error());
    }
    Ok(text)
}

async fn post_json(
    endpoint: &str,
    api_key: Option<&str>,
    json: String,
) -> Result<HttpReply, CleanupError> {
    let mut request = http_client()
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(json);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| CleanupError::new(CleanupErrorKind::Network, e.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| CleanupError::new(CleanupErrorKind::Network, e.to_string()))?;
    Ok(HttpReply {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or_default().to_string(),
        body,
    })
}

/// A model offered by `/models`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub value: String,
    pub label: String,
    /// The server's description, if any.
    pub description: Option<String>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ListModelsError {
    /// Raw detail (status and body excerpt, or transport error).
    pub message: String,
    /// Localized replacement for `message`, when the UI should show one.
    pub message_key: Option<&'static str>,
}

/// Lists models, trying sibling bases (`/v1`, LM Studio's native API) when
/// the entered base yields none. Returns the base that answered (adopt it as
/// the saved URL when it differs) and the models; an empty base yields an
/// empty list.
pub async fn list_models(
    base_url: &str,
    api_key: Option<&str>,
) -> Result<(String, Vec<ModelOption>), ListModelsError> {
    let normalized = normalize_base_url(base_url);
    if normalized.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    if !normalized.contains("://") {
        return Err(ListModelsError {
            message: "Enter a full base URL including protocol (for example, https://server/v1)."
                .into(),
            message_key: Some(ENDPOINT_WITH_PROTOCOL_MESSAGE_KEY),
        });
    }
    if !is_secure_http_endpoint(&normalized) {
        return Err(ListModelsError {
            message: HTTPS_REQUIRED_MESSAGE.into(),
            message_key: Some(HTTPS_REQUIRED_MESSAGE_KEY),
        });
    }
    let api_key = api_key.map(str::trim).filter(|k| !k.is_empty());

    let mut primary_error: Option<String> = None;
    for candidate in model_list_base_candidates(&normalized) {
        match fetch_model_options(&candidate, api_key).await {
            Ok(options) if !options.is_empty() => return Ok((candidate, options)),
            Ok(_) => {}
            Err(message) => {
                if candidate == normalized {
                    primary_error = Some(message);
                }
            }
        }
    }
    match primary_error {
        None => Ok((normalized, Vec::new())),
        Some(message) => {
            let unauthorized = mentions_unauthorized(&message);
            let message_key = if message.is_empty() {
                Some(UNABLE_TO_LOAD_MODELS_MESSAGE_KEY)
            } else if unauthorized && api_key.is_none() {
                Some(ENDPOINT_UNAUTHORIZED_MESSAGE_KEY)
            } else {
                None
            };
            Err(ListModelsError {
                message,
                message_key,
            })
        }
    }
}

/// `/\b(401|403)\b/`: the status appears as a standalone word.
fn mentions_unauthorized(message: &str) -> bool {
    let bytes = message.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    ["401", "403"].iter().any(|code| {
        message.match_indices(code).any(|(i, _)| {
            let before = i == 0 || !is_word(bytes[i - 1]);
            let after = i + 3 >= bytes.len() || !is_word(bytes[i + 3]);
            before && after
        })
    })
}

async fn fetch_model_options(
    base: &str,
    api_key: Option<&str>,
) -> Result<Vec<ModelOption>, String> {
    let url = build_api_url(base, "/models").ok_or_else(String::new)?;
    let mut request = http_client().get(url);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let fetch = async {
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Ok::<_, String>((status, text))
    };
    let (status, text) = tokio::time::timeout(LIST_MODELS_TIMEOUT, fetch)
        .await
        .map_err(|_| "Request timed out".to_string())??;
    if !status.is_success() {
        let summary = if text.is_empty() {
            format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )
        } else {
            let excerpt: String = text.chars().take(200).collect();
            format!("{} {}", status.as_u16(), excerpt)
        };
        return Err(summary.trim().to_string());
    }
    let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
    let raw = payload
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| payload.get("models").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    Ok(raw.iter().filter_map(model_option).collect())
}

fn model_option(item: &Value) -> Option<ModelOption> {
    let raw = item
        .get("id")
        .filter(|v| !v.is_null())
        .or_else(|| item.get("name"))?;
    let value = match raw {
        Value::Null => return None,
        Value::String(s) if s.is_empty() => return None,
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let owned_by = item
        .get("owned_by")
        .and_then(Value::as_str)
        .map(str::to_string);
    let description = item
        .get("description")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    Some(ModelOption {
        label: value.clone(),
        value,
        description,
        owned_by,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::{MockServer, Reply, run};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn reply(status: u16, body: Value) -> HttpReply {
        HttpReply {
            status,
            status_text: String::new(),
            body: body.to_string(),
        }
    }

    /// Fake fetcher: replies in order, repeating the last; counts calls.
    fn fetcher(
        specs: Vec<HttpReply>,
    ) -> (
        Arc<AtomicUsize>,
        impl FnMut(String) -> std::future::Ready<Result<HttpReply, ()>>,
    ) {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let f = move |_json: String| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(specs[n.min(specs.len() - 1)].clone()))
        };
        (count, f)
    }

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn fallback_passes_2xx_through() {
        let (count, f) = fetcher(vec![reply(200, json!({"ok": true}))]);
        let mut body = obj(json!({"reasoning": {"effort": "none"}}));
        let res = run(fetch_with_param_fallback(f, &mut body, |_, _| {
            panic!("no fallback")
        }))
        .unwrap();
        assert_eq!(res.status, 200);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(body.contains_key("reasoning"));
    }

    #[test]
    fn fallback_strips_reasoning_blind() {
        let (count, f) = fetcher(vec![
            reply(400, json!({"error": "bad request"})),
            reply(200, json!({})),
        ]);
        let mut body = obj(json!({"reasoning": {"effort": "none"}, "max_tokens": 10}));
        let mut logged = Vec::new();
        let res = run(fetch_with_param_fallback(f, &mut body, |s, p| {
            logged.push((s, p.to_vec()))
        }))
        .unwrap();
        assert_eq!(res.status, 200);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert!(!body.contains_key("reasoning"));
        assert_eq!(logged, vec![(400, vec!["reasoning".to_string()])]);
    }

    #[test]
    fn fallback_strips_named_param_only() {
        let (_, f) = fetcher(vec![
            reply(
                400,
                json!({"error": "Harmony does not support reasoning_effort='none'"}),
            ),
            reply(200, json!({})),
        ]);
        let mut body =
            obj(json!({"reasoning_effort": "none", "chat_template_kwargs": {}, "max_tokens": 10}));
        let mut logged = Vec::new();
        let res = run(fetch_with_param_fallback(f, &mut body, |s, p| {
            logged.push((s, p.to_vec()))
        }))
        .unwrap();
        assert_eq!(res.status, 200);
        assert!(!body.contains_key("reasoning_effort"));
        assert!(body.contains_key("chat_template_kwargs"));
        assert!(body.contains_key("max_tokens"));
        assert_eq!(logged, vec![(400, vec!["reasoning_effort".to_string()])]);
    }

    #[test]
    fn fallback_strips_temperature_on_422() {
        let (_, f) = fetcher(vec![
            reply(
                422,
                json!({"error": "`temperature` is deprecated for this model"}),
            ),
            reply(200, json!({})),
        ]);
        let mut body = obj(json!({"temperature": 0.3, "max_tokens": 10}));
        let res = run(fetch_with_param_fallback(f, &mut body, |_, _| {})).unwrap();
        assert_eq!(res.status, 200);
        assert!(!body.contains_key("temperature"));
    }

    #[test]
    fn fallback_gives_up_without_strippable_params_and_is_bounded() {
        let (count, f) = fetcher(vec![reply(400, json!({"error": "model not found"}))]);
        let mut body = obj(json!({"max_tokens": 10}));
        let res = run(fetch_with_param_fallback(f, &mut body, |_, _| {
            panic!("no fallback")
        }))
        .unwrap();
        assert_eq!(res.status, 400);
        assert_eq!(count.load(Ordering::SeqCst), 1);

        let (count, f) = fetcher(vec![reply(
            400,
            json!({"error": "reasoning_effort and thinking rejected"}),
        )]);
        let mut body = obj(
            json!({"reasoning": {}, "reasoning_effort": "low", "thinking": {"type": "disabled"}}),
        );
        let res = run(fetch_with_param_fallback(f, &mut body, |_, _| {})).unwrap();
        assert_eq!(res.status, 400);
        assert_eq!(count.load(Ordering::SeqCst), 3);

        let (count, f) = fetcher(vec![reply(
            500,
            json!({"error": "reasoning_effort exploded"}),
        )]);
        let mut body = obj(json!({"reasoning_effort": "low"}));
        let res = run(fetch_with_param_fallback(f, &mut body, |_, _| {
            panic!("no fallback")
        }))
        .unwrap();
        assert_eq!(res.status, 500);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(body.contains_key("reasoning_effort"));
    }

    #[test]
    fn deadline_error_shape() {
        let err = llm_request_timeout_error(30);
        assert_eq!(err.to_string(), "Request timed out after 30s");
        assert_eq!(err.code(), Some(LLM_REQUEST_TIMEOUT_CODE));
        assert!(!err.should_retry());
    }

    fn fast_tuning() -> CleanupTuning {
        CleanupTuning {
            request_timeout: Duration::from_secs(5),
            retry: RetryOptions {
                initial_delay: Duration::from_millis(1),
                ..RetryOptions::default()
            },
        }
    }

    async fn process(
        text: &str,
        config: &CleanupRequestConfig,
        tuning: &CleanupTuning,
    ) -> Result<String, CleanupError> {
        process_text_with(text, config, tuning, &CancellationToken::new()).await
    }

    fn completion(content: &str, finish: &str) -> String {
        json!({"choices": [{"message": {"content": content}, "finish_reason": finish}]}).to_string()
    }

    fn config_for(server: &MockServer) -> CleanupRequestConfig {
        CleanupRequestConfig {
            base_url: server.url(""),
            api_key: Some("fixture-key".into()),
            model: "qwen3.5:9b".into(),
            system_prompt: "Clean the transcript.".into(),
            disable_thinking: true,
        }
    }

    #[test]
    fn uses_endpoint_model_key_and_prompt() {
        let server =
            MockServer::start(|_| Reply::json(200, &completion("Cleaned transcript", "stop")));
        let result = run(process(
            "raw transcript",
            &config_for(&server),
            &fast_tuning(),
        ));
        assert_eq!(result.unwrap(), "Cleaned transcript");
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/chat/completions");
        assert_eq!(
            requests[0].header("authorization").as_deref(),
            Some("Bearer fixture-key")
        );
        let body = requests[0].json();
        assert_eq!(body["model"], "qwen3.5:9b");
        assert_eq!(body["messages"][0]["content"], "Clean the transcript.");
        assert_eq!(
            body["messages"][1]["content"],
            wrap_cleanup_transcript("raw transcript")
        );
        assert_eq!(body["temperature"], json!(0.0));
        assert_eq!(body["max_tokens"], json!(4096));
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": false})
        );
    }

    #[test]
    fn dictation_config_wraps_transcript_and_strips_thinking() {
        let server = MockServer::start(|_| {
            Reply::json(200, &completion("<think>hmm</think>Hello, world.", "stop"))
        });
        let config =
            CleanupRequestConfig::dictation(&server.url("/v1"), None, "qwen3.5:9b", "SYS", true);
        let result = run(process("hello world", &config, &fast_tuning()));
        assert_eq!(result.unwrap(), "Hello, world.");
        let request = &server.requests()[0];
        assert_eq!(request.header("authorization"), None);
        assert_eq!(
            request.json()["messages"][1]["content"],
            wrap_cleanup_transcript("hello world")
        );
    }

    #[test]
    fn rejects_missing_or_unsafe_endpoints_before_dispatch() {
        for url in ["", "http://public.example.com/v1", "ftp://192.168.1.20/v1"] {
            let config = CleanupRequestConfig {
                base_url: url.into(),
                ..CleanupRequestConfig::default()
            };
            let err = run(process("x", &config, &fast_tuning())).unwrap_err();
            assert_eq!(err.kind, CleanupErrorKind::InvalidEndpoint, "{url}");
            assert_eq!(err.message, HTTPS_REQUIRED_MESSAGE);
            assert_eq!(err.message_key, Some(HTTPS_REQUIRED_MESSAGE_KEY));
        }
    }

    #[test]
    fn rejects_incomplete_and_empty_output() {
        for (content, finish, key) in [
            ("Partial", "length", TRUNCATED_OUTPUT_MESSAGE_KEY),
            ("", "stop", EMPTY_OUTPUT_MESSAGE_KEY),
            (
                "<think>only thoughts</think>",
                "stop",
                EMPTY_OUTPUT_MESSAGE_KEY,
            ),
        ] {
            let body = completion(content, finish);
            let server = MockServer::start(move |_| Reply::json(200, &body));
            let err = run(process("raw", &config_for(&server), &fast_tuning())).unwrap_err();
            assert_eq!(err.message_key, Some(key), "{content:?}");
        }
    }

    #[test]
    fn keeps_thinking_when_enabled() {
        let server =
            MockServer::start(|_| Reply::json(200, &completion("<think>a</think>B", "stop")));
        let config = CleanupRequestConfig {
            disable_thinking: false,
            ..config_for(&server)
        };
        let out = run(process("raw", &config, &fast_tuning())).unwrap();
        assert_eq!(out, "<think>a</think>B");
    }

    #[test]
    fn times_out_without_retrying() {
        let server = MockServer::start(|_| {
            Reply::json(200, &completion("late", "stop")).delayed(Duration::from_millis(1500))
        });
        let tuning = CleanupTuning {
            request_timeout: Duration::from_millis(200),
            ..fast_tuning()
        };
        let err = run(process("raw", &config_for(&server), &tuning)).unwrap_err();
        assert_eq!(err.kind, CleanupErrorKind::Timeout);
        assert_eq!(err.message, "Request timed out after 0s");
        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn retries_5xx_then_succeeds_and_fails_fast_on_4xx() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let server = MockServer::start(move |_| {
            if counter.fetch_add(1, Ordering::SeqCst) < 2 {
                Reply::json(503, r#"{"error":{"message":"loading model"}}"#)
            } else {
                Reply::json(200, &completion("ok", "stop"))
            }
        });
        let out = run(process("raw", &config_for(&server), &fast_tuning()));
        assert_eq!(out.unwrap(), "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        let server = MockServer::start(|_| Reply::json(401, r#"{"error":{"message":"bad key"}}"#));
        let err = run(process("raw", &config_for(&server), &fast_tuning())).unwrap_err();
        assert_eq!(err.kind, CleanupErrorKind::Http);
        assert_eq!(err.status, Some(401));
        assert_eq!(err.message, "bad key");
        assert_eq!(server.requests().len(), 1);

        let server = MockServer::start(|_| Reply::json(500, ""));
        let err = run(process("raw", &config_for(&server), &fast_tuning())).unwrap_err();
        assert_eq!(err.message, "Internal Server Error");
        assert_eq!(
            server.requests().len(),
            4,
            "initial attempt plus three retries"
        );
    }

    #[test]
    fn param_fallback_persists_across_the_request() {
        let server = MockServer::start(|req| {
            if req.json().get("reasoning").is_some() {
                Reply::json(400, r#"{"error":"unknown field"}"#)
            } else {
                Reply::json(200, &completion("ok", "stop"))
            }
        });
        let out = run(process("raw", &config_for(&server), &fast_tuning()));
        assert_eq!(out.unwrap(), "ok");
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn cancelling_one_request_leaves_others_running() {
        let slow = MockServer::start(|_| {
            Reply::json(200, &completion("late", "stop")).delayed(Duration::from_secs(3))
        });
        let other = MockServer::start(|_| {
            Reply::json(200, &completion("fresh", "stop")).delayed(Duration::from_millis(400))
        });
        let (slow_config, other_config) = (config_for(&slow), config_for(&other));
        let tuning = fast_tuning();
        let started = std::time::Instant::now();
        let (cancelled, kept) = run(async {
            let cancel = CancellationToken::new();
            let canceller = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                canceller.cancel();
            });
            tokio::join!(
                process_text_with("raw", &slow_config, &tuning, &cancel),
                process("raw", &other_config, &tuning),
            )
        });
        assert_eq!(cancelled.unwrap_err().kind, CleanupErrorKind::Cancelled);
        assert_eq!(kept.unwrap(), "fresh");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn invalid_structure_is_reported() {
        let server = MockServer::start(|_| Reply::json(200, r#"{"choices":[]}"#));
        let err = run(process("raw", &config_for(&server), &fast_tuning())).unwrap_err();
        assert_eq!(err.kind, CleanupErrorKind::InvalidResponse);
        assert_eq!(err.message, "Invalid response structure from LAN API");
    }

    #[test]
    fn lists_models_with_base_fallback() {
        let server = MockServer::start(|req| match req.path.as_str() {
            "/v1/models" => Reply::json(
                200,
                r#"{"data":[{"id":"qwen3.5:9b","owned_by":"library"},{"id":7},{"name":"named","description":"Desc"},{"id":""}]}"#,
            ),
            _ => Reply::json(404, "not here"),
        });
        let (base, models) = run(list_models(&server.url(""), None)).unwrap();
        assert_eq!(base, server.url("/v1"));
        assert_eq!(
            models,
            vec![
                ModelOption {
                    value: "qwen3.5:9b".into(),
                    label: "qwen3.5:9b".into(),
                    description: None,
                    owned_by: Some("library".into()),
                },
                ModelOption {
                    value: "7".into(),
                    label: "7".into(),
                    description: None,
                    owned_by: None,
                },
                ModelOption {
                    value: "named".into(),
                    label: "named".into(),
                    description: Some("Desc".into()),
                    owned_by: None,
                },
            ]
        );
    }

    #[test]
    fn list_models_reads_models_array_and_sends_key() {
        let server = MockServer::start(|_| Reply::json(200, r#"{"models":[{"name":"llama"}]}"#));
        let (base, models) = run(list_models(&server.url("/v1/models"), Some(" k "))).unwrap();
        assert_eq!(base, server.url("/v1"));
        assert_eq!(models[0].value, "llama");
        assert_eq!(
            server.requests()[0].header("authorization").as_deref(),
            Some("Bearer k")
        );
    }

    #[test]
    fn list_models_errors() {
        let server = MockServer::start(|_| Reply::json(401, "Unauthorized"));
        let err = run(list_models(&server.url("/v1"), None)).unwrap_err();
        assert_eq!(err.message, "401 Unauthorized");
        assert_eq!(err.message_key, Some(ENDPOINT_UNAUTHORIZED_MESSAGE_KEY));
        let err = run(list_models(&server.url("/v1"), Some("key"))).unwrap_err();
        assert_eq!(err.message_key, None);

        let err = run(list_models("http://example.com/v1", None)).unwrap_err();
        assert_eq!(err.message_key, Some(HTTPS_REQUIRED_MESSAGE_KEY));
        let err = run(list_models("localhost:8080", None)).unwrap_err();
        assert_eq!(err.message_key, Some(ENDPOINT_WITH_PROTOCOL_MESSAGE_KEY));
        assert_eq!(
            run(list_models("  ", None)).unwrap(),
            (String::new(), Vec::new())
        );
    }
}
