//! One wire protocol for live and file speech transcription.
//!
//! Callers own payload construction, reachability, cancellation and deadlines.

use reqwest::multipart::{Form, Part};
use serde_json::Value;

use super::http_body::{BodyError, read_body};
use crate::text::api_error::extract_api_error_message;

pub const MAX_ASR_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("{0}")]
    Network(#[from] reqwest::Error),
    #[error("{message}")]
    Http { status: u16, message: String },
    #[error("{0}")]
    InvalidResponse(String),
}

/// Strip request URLs, which can contain credentials, before an error escapes.
pub fn request_error(error: reqwest::Error) -> AsrError {
    AsrError::Network(error.without_url())
}

pub async fn transcribe(
    client: &reqwest::Client,
    endpoint: &str,
    part: Part,
    model: Option<&str>,
    language: Option<&str>,
    prompt: Option<&str>,
) -> Result<String, AsrError> {
    let mut form = Form::new().part("file", part);
    for (name, value) in [("model", model), ("language", language), ("prompt", prompt)] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            form = form.text(name, value.to_owned());
        }
    }
    let response = client
        .post(endpoint)
        .multipart(form)
        .send()
        .await
        .map_err(request_error)?;
    let status = response.status();
    let body = read_body(response, MAX_ASR_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BodyError::Network(error) => request_error(error),
            BodyError::TooLarge(limit) => {
                AsrError::InvalidResponse(format!("Transcription response exceeds {limit} bytes"))
            }
        })?;
    decode_response(status.as_u16(), &body)
}

fn decode_response(status: u16, body: &str) -> Result<String, AsrError> {
    let payload = serde_json::from_str::<Value>(body);
    if !(200..300).contains(&status) {
        let message = match &payload {
            Ok(payload) => {
                extract_api_error_message(payload, &format!("Self-hosted API Error: {status}"))
            }
            Err(_) => format!(
                "Server error {status}: {}",
                body.chars().take(120).collect::<String>()
            ),
        };
        return Err(AsrError::Http { status, message });
    }
    let payload = payload.map_err(|error| AsrError::InvalidResponse(error.to_string()))?;
    payload
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| AsrError::InvalidResponse("Expected a string field `text`".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_server::{MockServer, Reply, run};

    #[test]
    fn response_contract_accepts_all_2xx_and_requires_string_text() {
        for status in [200, 201, 202, 299] {
            assert_eq!(
                decode_response(status, r#"{"text":"verbatim "}"#).unwrap(),
                "verbatim "
            );
            for body in ["{}", r#"{"text":null}"#, r#"{"text":7}"#, "not json"] {
                assert!(matches!(
                    decode_response(status, body),
                    Err(AsrError::InvalidResponse(_))
                ));
            }
        }
    }

    #[test]
    fn request_errors_remove_the_endpoint_url() {
        let error = run(reqwest::Client::new()
            .get("http://127.0.0.1:1/?private-query")
            .send())
        .unwrap_err();
        let message = request_error(error).to_string();
        assert!(!message.contains("private-query"));
        assert!(!message.contains("127.0.0.1"));
    }

    #[test]
    fn rejects_large_success_and_error_bodies() {
        for status in [200, 500] {
            let body = "x".repeat(MAX_ASR_RESPONSE_BYTES + 1);
            let server = MockServer::start(move |_| Reply::json(status, &body));
            let error = run(transcribe(
                &reqwest::Client::new(),
                &server.url("/asr"),
                Part::bytes(vec![1]),
                None,
                None,
                None,
            ))
            .unwrap_err();
            assert!(matches!(error, AsrError::InvalidResponse(_)));
        }
    }
}
