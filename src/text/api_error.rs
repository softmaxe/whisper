//! Reads a human-readable message out of an OpenAI-compatible error body.

use serde_json::Value;

const MAX_STRINGIFIED_LENGTH: usize = 500;

fn as_non_empty_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// FastAPI/Pydantic payloads: `{detail: "..."}` or `{detail: [{loc, msg}]}`.
fn from_detail(container: Option<&Value>) -> Option<String> {
    let container = container?.as_object()?;
    let detail = container.get("detail");
    if let Some(direct) = as_non_empty_string(detail) {
        return Some(direct);
    }
    let entries = detail?.as_array()?;
    let parts: Vec<String> = entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|entry| {
            let msg = as_non_empty_string(entry.get("msg"))?;
            let field = entry
                .get("loc")
                .and_then(Value::as_array)
                .and_then(|loc| as_non_empty_string(loc.last()));
            Some(match field {
                Some(field) => format!("{field}: {msg}"),
                None => msg,
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// JSON text capped at 500 UTF-16 units plus an ellipsis.
fn stringify(value: &Value) -> Option<String> {
    let json = serde_json::to_string(value).ok()?;
    if json.encode_utf16().count() <= MAX_STRINGIFIED_LENGTH {
        return Some(json);
    }
    let mut units = 0;
    let mut cut = 0;
    for (i, c) in json.char_indices() {
        units += c.len_utf16();
        if units > MAX_STRINGIFIED_LENGTH {
            break;
        }
        cut = i + c.len_utf8();
    }
    Some(format!("{}…", &json[..cut]))
}

pub fn extract_api_error_message(error_data: &Value, fallback: &str) -> String {
    let safe_fallback = if fallback.trim().is_empty() {
        "Unknown API error".to_string()
    } else {
        fallback.trim().to_string()
    };
    let Some(obj) = error_data.as_object() else {
        return safe_fallback;
    };
    let error = obj.get("error");
    let message = obj.get("message");

    let nested = error
        .and_then(Value::as_object)
        .and_then(|e| as_non_empty_string(e.get("message")));
    if let Some(nested) = nested {
        return nested;
    }
    if let Some(flat) = as_non_empty_string(message) {
        return flat;
    }

    // Nested containers first: a top-level detail usually just restates the
    // HTTP status, while the wrapped payload carries the upstream error.
    for container in [message, error, Some(error_data)] {
        if let Some(detail) = from_detail(container) {
            return detail;
        }
    }

    if let Some(error_string) = as_non_empty_string(error) {
        return error_string;
    }

    for value in [message, obj.get("detail"), error].into_iter().flatten() {
        if !(value.is_object() || value.is_array()) {
            continue;
        }
        if let Some(stringified) = stringify(value) {
            return stringified;
        }
    }
    safe_fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ex(v: Value) -> String {
        extract_api_error_message(&v, "fallback")
    }

    #[test]
    fn mistral_validation_body() {
        let body = json!({
            "object": "error",
            "message": {"detail": [
                {"type": "extra_forbidden", "loc": ["body", "max_completion_tokens"],
                 "msg": "Extra inputs are not permitted", "input": 4096},
                {"type": "extra_forbidden", "loc": ["body", "chat_template_kwargs"],
                 "msg": "Extra inputs are not permitted", "input": {"enable_thinking": false}}
            ]},
            "type": "invalid_request_error"
        });
        assert_eq!(
            ex(body),
            "max_completion_tokens: Extra inputs are not permitted; chat_template_kwargs: Extra inputs are not permitted"
        );
    }

    #[test]
    fn common_shapes() {
        assert_eq!(
            ex(json!({"error": {"message": "Invalid API key"}})),
            "Invalid API key"
        );
        assert_eq!(ex(json!({"message": "Rate limited"})), "Rate limited");
        assert_eq!(
            ex(json!({"message": {"weird": "object"}})),
            r#"{"weird":"object"}"#
        );
        assert_eq!(
            ex(json!({"error": "Service Unavailable"})),
            "Service Unavailable"
        );
        assert_eq!(
            ex(json!({"detail": "Model 'llama3' not found"})),
            "Model 'llama3' not found"
        );
    }

    #[test]
    fn long_message_is_capped() {
        let message = ex(json!({"message": {"blob": "x".repeat(2000)}}));
        assert!(message.encode_utf16().count() <= 501);
        assert!(message.ends_with('…'));
    }

    #[test]
    fn detail_entries_degrade_gracefully() {
        let body = json!({"message": {"detail": [
            {"loc": ["body", "field_a"], "msg": 42},
            {"loc": [], "msg": "Field is required"},
            {"loc": ["body", {"nested": true}], "msg": "Bad nesting"},
            {"msg": "No loc at all"},
            "not an object",
            null
        ]}});
        assert_eq!(ex(body), "Field is required; Bad nesting; No loc at all");
    }

    #[test]
    fn unusable_detail_surfaces_raw_body() {
        assert_eq!(ex(json!({"message": {"detail": []}})), r#"{"detail":[]}"#);
        assert_eq!(
            ex(json!({"error": {"code": "invalid_request_error", "type": "bad_request"}})),
            r#"{"code":"invalid_request_error","type":"bad_request"}"#
        );
        assert_eq!(
            ex(json!({"detail": {"reason": "upstream timeout"}})),
            r#"{"reason":"upstream timeout"}"#
        );
    }

    #[test]
    fn non_object_bodies_fall_back() {
        assert_eq!(ex(Value::Null), "fallback");
        assert_eq!(ex(json!("just a string")), "fallback");
        assert_eq!(ex(json!([])), "fallback");
        assert_eq!(ex(json!({})), "fallback");
        assert_eq!(
            extract_api_error_message(&json!({}), ""),
            "Unknown API error"
        );
    }

    #[test]
    fn detail_lists_and_nesting() {
        assert_eq!(
            ex(json!({"detail": [{"loc": ["body", "model"], "msg": "field required"}]})),
            "model: field required"
        );
        assert_eq!(
            ex(json!({"message": {"detail": "Rate limit exceeded"}})),
            "Rate limit exceeded"
        );
        assert_eq!(
            ex(json!({"error": {"detail": "Rate limit exceeded"}})),
            "Rate limit exceeded"
        );
        assert_eq!(
            ex(json!({
                "detail": "Unprocessable Entity",
                "message": {"detail": [{"loc": ["body", "model"], "msg": "field required"}]}
            })),
            "model: field required"
        );
    }
}
