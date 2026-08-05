//! Usage: Match final upstream HTTP errors and build protocol-compatible client responses.

use crate::settings::{
    UpstreamErrorMessageBehavior, UpstreamErrorResponseMatchMode, UpstreamErrorResponseRule,
    UpstreamErrorStatusBehavior, MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS, MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY, MAX_UPSTREAM_ERROR_RESPONSE_RULE_PROVIDER_IDS,
    MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES,
};
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;

#[derive(Debug, Clone)]
pub(super) struct UpstreamErrorResponseRewrite {
    pub(super) rule_id: String,
    pub(super) rule_name: String,
    pub(super) provider_id: i64,
    pub(super) provider_name: String,
    pub(super) upstream_status: u16,
    pub(super) client_status: StatusCode,
    pub(super) status_mode: &'static str,
    pub(super) message_mode: &'static str,
    message: String,
    retry_after: Option<HeaderValue>,
}

impl UpstreamErrorResponseRewrite {
    pub(super) fn build_response(&self, cli_key: &str, trace_id: &str) -> Option<Response> {
        let payload = match cli_key {
            "claude" => serde_json::json!({
                "type": "error",
                "error": {
                    "type": "upstream_error",
                    "message": self.message.as_str(),
                }
            }),
            "codex" | "grok" => serde_json::json!({
                "error": {
                    "type": "upstream_error",
                    "code": "upstream_error",
                    "message": self.message.as_str(),
                }
            }),
            "gemini" => serde_json::json!({
                "error": {
                    "code": self.client_status.as_u16(),
                    "status": "UNKNOWN",
                    "message": self.message.as_str(),
                }
            }),
            _ => return None,
        };
        let body = serde_json::to_vec(&payload).ok()?;
        let trace_header = HeaderValue::from_str(trace_id).ok()?;
        let mut builder = Response::builder()
            .status(self.client_status)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )
            .header("x-trace-id", trace_header);
        if let Some(retry_after) = self.retry_after.as_ref() {
            builder = builder.header(header::RETRY_AFTER, retry_after.clone());
        }
        builder.body(Body::from(body)).ok()
    }

    pub(super) fn special_setting(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "upstream_error_response_rule",
            "scope": "response",
            "ruleId": self.rule_id.as_str(),
            "ruleName": self.rule_name.as_str(),
            "providerId": self.provider_id,
            "providerName": self.provider_name.as_str(),
            "upstreamStatus": self.upstream_status,
            "clientStatus": self.client_status.as_u16(),
            "statusMode": self.status_mode,
            "messageMode": self.message_mode,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConditionResult {
    Match,
    NoMatch,
    Unknown,
}

pub(super) fn needs_bounded_body_observation(
    rules: &[UpstreamErrorResponseRule],
    cli_key: &str,
    provider_id: i64,
    status: StatusCode,
) -> bool {
    if !(status.is_client_error() || status.is_server_error()) {
        return false;
    }

    rules.iter().any(|rule| {
        runtime_rule_is_safe(rule)
            && rule_applies_to_scope(rule, cli_key, provider_id)
            && (!rule.keywords.is_empty()
                || matches!(
                    &rule.message_behavior,
                    UpstreamErrorMessageBehavior::Passthrough
                ))
    })
}

pub(super) fn supports_bounded_body_observation(headers: &HeaderMap) -> bool {
    let values = headers.get_all(header::CONTENT_ENCODING);
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return true;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(value) = value.to_str() else {
        return false;
    };

    let mut gzip_layers = 0usize;
    let mut encoding_tokens = 0usize;
    for encoding in value
        .split(',')
        .map(str::trim)
        .filter(|encoding| !encoding.is_empty())
    {
        encoding_tokens = encoding_tokens.saturating_add(1);
        if encoding.eq_ignore_ascii_case("identity") {
            continue;
        }
        if encoding.eq_ignore_ascii_case("gzip") {
            gzip_layers = gzip_layers.saturating_add(1);
            if gzip_layers > 1 {
                return false;
            }
            continue;
        }
        return false;
    }
    encoding_tokens > 0
}

fn safe_retry_after(headers: &HeaderMap) -> Option<HeaderValue> {
    let values = headers.get_all(header::RETRY_AFTER);
    let mut values = values.iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?.trim();
    if value.is_empty() || value.len() > 128 {
        return None;
    }

    let valid_delta_seconds =
        value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok();
    let valid_http_date = chrono::DateTime::parse_from_rfc2822(value).is_ok();
    (valid_delta_seconds || valid_http_date)
        .then(|| HeaderValue::from_str(value).ok())
        .flatten()
}

pub(super) fn match_response_rule(
    rules: &[UpstreamErrorResponseRule],
    cli_key: &str,
    provider_id: i64,
    provider_name: &str,
    upstream_status: StatusCode,
    body: Option<&[u8]>,
    upstream_headers: &HeaderMap,
) -> Option<UpstreamErrorResponseRewrite> {
    if !(upstream_status.is_client_error() || upstream_status.is_server_error()) {
        return None;
    }

    let mut ordered_rules: Vec<(usize, &UpstreamErrorResponseRule)> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule_applies_to_scope(rule, cli_key, provider_id))
        .collect();
    ordered_rules.sort_by_key(|(index, rule)| (rule.priority, *index));

    for (_, rule) in ordered_rules {
        if !runtime_rule_is_safe(rule) {
            return None;
        }
        match evaluate_rule(rule, upstream_status.as_u16(), body) {
            ConditionResult::NoMatch => continue,
            ConditionResult::Unknown => return None,
            ConditionResult::Match => {}
        }

        let (client_status, status_mode) = match &rule.status_behavior {
            UpstreamErrorStatusBehavior::Passthrough => (upstream_status, "passthrough"),
            UpstreamErrorStatusBehavior::Override { status_code } => {
                (StatusCode::from_u16(*status_code).ok()?, "override")
            }
        };
        if !(client_status.is_client_error() || client_status.is_server_error()) {
            return None;
        }

        let (message, message_mode) = match &rule.message_behavior {
            UpstreamErrorMessageBehavior::Passthrough => {
                (extract_upstream_message(body?)?, "passthrough")
            }
            UpstreamErrorMessageBehavior::Override { message } => {
                let trimmed = message.trim();
                if trimmed.is_empty() {
                    return None;
                }
                (
                    truncate_chars(trimmed, MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS),
                    "override",
                )
            }
        };

        return Some(UpstreamErrorResponseRewrite {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            provider_id,
            provider_name: provider_name.to_string(),
            upstream_status: upstream_status.as_u16(),
            client_status,
            status_mode,
            message_mode,
            message,
            retry_after: safe_retry_after(upstream_headers),
        });
    }

    None
}

fn rule_applies_to_scope(
    rule: &UpstreamErrorResponseRule,
    cli_key: &str,
    provider_id: i64,
) -> bool {
    rule.enabled
        && (rule.cli_keys.is_empty() || rule.cli_keys.iter().any(|key| key == cli_key))
        && (rule.provider_ids.is_empty() || rule.provider_ids.contains(&provider_id))
}

fn has_disallowed_control(value: &str, allow_multiline: bool) -> bool {
    value.chars().any(|character| {
        character.is_control() && !(allow_multiline && matches!(character, '\n' | '\t'))
    })
}

fn runtime_rule_is_safe(rule: &UpstreamErrorResponseRule) -> bool {
    let valid_id = {
        let bytes = rule.id.as_bytes();
        bytes.len() == 36
            && bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[18] == b'-'
            && bytes[23] == b'-'
            && bytes[14] == b'4'
            && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
            && bytes.iter().enumerate().all(|(index, byte)| {
                matches!(index, 8 | 13 | 18 | 23)
                    || byte.is_ascii_digit()
                    || matches!(*byte, b'a'..=b'f')
            })
    };
    let valid_status_behavior = match &rule.status_behavior {
        UpstreamErrorStatusBehavior::Passthrough => true,
        UpstreamErrorStatusBehavior::Override { status_code } => (400..=599).contains(status_code),
    };
    let valid_message_behavior = match &rule.message_behavior {
        UpstreamErrorMessageBehavior::Passthrough => true,
        UpstreamErrorMessageBehavior::Override { message } => {
            !message.trim().is_empty()
                && message.chars().count() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS
                && !has_disallowed_control(message, true)
        }
    };

    valid_id
        && !rule.name.trim().is_empty()
        && rule.name.chars().count() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_NAME_CHARS
        && !has_disallowed_control(rule.name.as_str(), false)
        && rule.description.chars().count() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_DESCRIPTION_CHARS
        && !has_disallowed_control(rule.description.as_str(), false)
        && rule.priority <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_PRIORITY
        && (!rule.status_codes.is_empty() || !rule.keywords.is_empty())
        && rule.status_codes.len() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_STATUS_CODES
        && rule
            .status_codes
            .iter()
            .all(|status| (400..=599).contains(status))
        && rule.keywords.len() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORDS
        && rule.keywords.iter().all(|keyword| {
            !keyword.trim().is_empty()
                && keyword.chars().count() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_KEYWORD_CHARS
                && !has_disallowed_control(keyword.as_str(), false)
        })
        && rule
            .cli_keys
            .iter()
            .all(|key| crate::shared::cli_key::is_supported_cli_key(key.as_str()))
        && rule.provider_ids.len() <= MAX_UPSTREAM_ERROR_RESPONSE_RULE_PROVIDER_IDS
        && rule.provider_ids.iter().all(|provider_id| *provider_id > 0)
        && valid_status_behavior
        && valid_message_behavior
}

fn evaluate_rule(
    rule: &UpstreamErrorResponseRule,
    upstream_status: u16,
    body: Option<&[u8]>,
) -> ConditionResult {
    let status_configured = !rule.status_codes.is_empty();
    let status_matches = status_configured && rule.status_codes.contains(&upstream_status);
    let keyword_configured = !rule.keywords.is_empty();
    let keyword_matches = if keyword_configured {
        let Some(body) = body else {
            return match rule.match_mode {
                UpstreamErrorResponseMatchMode::Any if status_matches => ConditionResult::Match,
                UpstreamErrorResponseMatchMode::All if status_configured && !status_matches => {
                    ConditionResult::NoMatch
                }
                _ => ConditionResult::Unknown,
            };
        };
        let Ok(body_text) = std::str::from_utf8(body) else {
            return ConditionResult::Unknown;
        };
        let normalized_body = body_text.to_lowercase();
        rule.keywords
            .iter()
            .any(|keyword| normalized_body.contains(&keyword.to_lowercase()))
    } else {
        false
    };

    let matched = match rule.match_mode {
        UpstreamErrorResponseMatchMode::Any => {
            (status_configured && status_matches) || (keyword_configured && keyword_matches)
        }
        UpstreamErrorResponseMatchMode::All => {
            (!status_configured || status_matches) && (!keyword_configured || keyword_matches)
        }
    };

    if matched {
        ConditionResult::Match
    } else {
        ConditionResult::NoMatch
    }
}

fn extract_upstream_message(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        return extract_message_from_value(&value, 0).and_then(normalize_extracted_message);
    }

    let text = std::str::from_utf8(body).ok()?;
    normalize_extracted_message(text.to_string())
}

fn extract_message_from_value(value: &serde_json::Value, depth: usize) -> Option<String> {
    if depth > 2 {
        return None;
    }
    if let Some(message) = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        return Some(message.to_string());
    }
    if let Some(detail) = value
        .get("error")
        .and_then(|error| error.get("detail"))
        .and_then(serde_json::Value::as_str)
    {
        return Some(detail.to_string());
    }
    for key in ["message", "detail"] {
        if let Some(message) = value.get(key).and_then(serde_json::Value::as_str) {
            return Some(message.to_string());
        }
    }
    if let Some(error) = value.get("error") {
        if let Some(message) = error.as_str() {
            if let Ok(nested) = serde_json::from_str::<serde_json::Value>(message) {
                return extract_message_from_value(&nested, depth + 1)
                    .or_else(|| Some(message.to_string()));
            }
            return Some(message.to_string());
        }
    }
    value.as_str().map(str::to_string)
}

fn normalize_extracted_message(message: String) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(
        trimmed,
        MAX_UPSTREAM_ERROR_RESPONSE_RULE_MESSAGE_CHARS,
    ))
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    fn rule() -> UpstreamErrorResponseRule {
        UpstreamErrorResponseRule {
            id: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51".to_string(),
            name: "quota".to_string(),
            description: String::new(),
            enabled: true,
            priority: 10,
            status_codes: vec![429],
            keywords: vec!["quota".to_string()],
            match_mode: UpstreamErrorResponseMatchMode::All,
            cli_keys: vec!["codex".to_string()],
            provider_ids: vec![7],
            status_behavior: UpstreamErrorStatusBehavior::Override { status_code: 503 },
            message_behavior: UpstreamErrorMessageBehavior::Passthrough,
        }
    }

    #[test]
    fn matches_all_groups_and_extracts_nested_message() {
        let matched = match_response_rule(
            &[rule()],
            "codex",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            Some(br#"{"error":{"message":"quota exhausted"}}"#),
            &HeaderMap::new(),
        )
        .expect("rule should match");

        assert_eq!(matched.client_status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(matched.upstream_status, 429);
        assert_eq!(matched.message, "quota exhausted");
    }

    #[test]
    fn missing_body_stops_before_lower_priority_rule() {
        let mut uncertain = rule();
        uncertain.priority = 1;
        uncertain.status_codes = vec![429];
        let mut lower = rule();
        lower.priority = 2;
        lower.status_codes = vec![429];
        lower.keywords.clear();
        lower.match_mode = UpstreamErrorResponseMatchMode::Any;
        lower.message_behavior = UpstreamErrorMessageBehavior::Override {
            message: "lower".to_string(),
        };

        assert!(match_response_rule(
            &[uncertain, lower],
            "codex",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            None,
            &HeaderMap::new(),
        )
        .is_none());
    }

    #[test]
    fn non_utf8_body_fails_open() {
        let mut candidate = rule();
        candidate.status_codes = vec![500];
        candidate.message_behavior = UpstreamErrorMessageBehavior::Override {
            message: "busy".to_string(),
        };
        assert!(match_response_rule(
            &[candidate],
            "codex",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            Some(&[0xff, 0xfe]),
            &HeaderMap::new(),
        )
        .is_none());
    }

    #[test]
    fn status_match_can_satisfy_any_without_body() {
        let mut candidate = rule();
        candidate.match_mode = UpstreamErrorResponseMatchMode::Any;
        candidate.message_behavior = UpstreamErrorMessageBehavior::Override {
            message: "busy".to_string(),
        };

        let matched = match_response_rule(
            &[candidate],
            "codex",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            None,
            &HeaderMap::new(),
        );
        assert!(matched.is_some());
    }

    #[test]
    fn supports_every_status_and_message_behavior_combination() {
        for (override_status, override_message, expected_status, expected_message) in [
            (false, false, 429, "upstream"),
            (false, true, 429, "configured"),
            (true, false, 503, "upstream"),
            (true, true, 503, "configured"),
        ] {
            let mut candidate = rule();
            candidate.keywords.clear();
            candidate.match_mode = UpstreamErrorResponseMatchMode::Any;
            candidate.status_behavior = if override_status {
                UpstreamErrorStatusBehavior::Override { status_code: 503 }
            } else {
                UpstreamErrorStatusBehavior::Passthrough
            };
            candidate.message_behavior = if override_message {
                UpstreamErrorMessageBehavior::Override {
                    message: "configured".to_string(),
                }
            } else {
                UpstreamErrorMessageBehavior::Passthrough
            };

            let matched = match_response_rule(
                &[candidate],
                "codex",
                7,
                "provider",
                StatusCode::TOO_MANY_REQUESTS,
                Some(br#"{"error":{"message":"upstream"}}"#),
                &HeaderMap::new(),
            )
            .expect("rule should match");

            assert_eq!(matched.client_status.as_u16(), expected_status);
            assert_eq!(matched.message, expected_message);
        }
    }

    #[test]
    fn scope_and_success_status_do_not_match() {
        let candidate = rule();
        assert!(match_response_rule(
            std::slice::from_ref(&candidate),
            "claude",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            Some(b"quota"),
            &HeaderMap::new(),
        )
        .is_none());
        assert!(match_response_rule(
            &[candidate],
            "codex",
            7,
            "provider",
            StatusCode::OK,
            Some(b"quota"),
            &HeaderMap::new(),
        )
        .is_none());
    }

    #[test]
    fn malformed_runtime_rule_fails_open() {
        let mut candidate = rule();
        candidate.status_codes.clear();
        candidate.keywords.clear();
        candidate.message_behavior = UpstreamErrorMessageBehavior::Override {
            message: "should not apply".to_string(),
        };
        assert!(match_response_rule(
            &[candidate],
            "codex",
            7,
            "provider",
            StatusCode::TOO_MANY_REQUESTS,
            None,
            &HeaderMap::new(),
        )
        .is_none());
    }

    #[test]
    fn body_observation_rejects_unknown_or_stacked_encodings() {
        let mut headers = HeaderMap::new();
        assert!(supports_bounded_body_observation(&headers));
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(supports_bounded_body_observation(&headers));
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        assert!(!supports_bounded_body_observation(&headers));
        headers.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("gzip, gzip"),
        );
        assert!(!supports_bounded_body_observation(&headers));

        let mut repeated = HeaderMap::new();
        repeated.append(
            header::CONTENT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        repeated.append(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(!supports_bounded_body_observation(&repeated));
    }

    #[test]
    fn message_extraction_does_not_expose_unrecognized_json_body() {
        assert!(extract_upstream_message(br#"{"unexpected":"secret"}"#).is_none());
        assert_eq!(
            extract_upstream_message(b"plain upstream error").as_deref(),
            Some("plain upstream error")
        );
    }

    #[test]
    fn retry_after_requires_one_valid_standard_value() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("120"));
        assert_eq!(
            safe_retry_after(&headers)
                .as_ref()
                .and_then(|value| value.to_str().ok()),
            Some("120")
        );

        headers.insert(header::RETRY_AFTER, HeaderValue::from_static("not-a-delay"));
        assert!(safe_retry_after(&headers).is_none());

        let mut repeated = HeaderMap::new();
        repeated.append(header::RETRY_AFTER, HeaderValue::from_static("1"));
        repeated.append(header::RETRY_AFTER, HeaderValue::from_static("2"));
        assert!(safe_retry_after(&repeated).is_none());
    }

    #[tokio::test]
    async fn builds_protocol_specific_error_envelopes() {
        for cli_key in ["claude", "codex", "grok", "gemini"] {
            let mut candidate = rule();
            candidate.cli_keys.clear();
            candidate.keywords.clear();
            candidate.match_mode = UpstreamErrorResponseMatchMode::Any;
            candidate.message_behavior = UpstreamErrorMessageBehavior::Override {
                message: "busy".to_string(),
            };
            let rewrite = match_response_rule(
                &[candidate],
                cli_key,
                7,
                "provider",
                StatusCode::TOO_MANY_REQUESTS,
                None,
                &HeaderMap::new(),
            )
            .expect("rule should match");
            let response = rewrite
                .build_response(cli_key, "trace-1")
                .expect("response should build");
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(response.headers().get("x-trace-id").unwrap(), "trace-1");
            let body = to_bytes(response.into_body(), 8 * 1024)
                .await
                .expect("response body");
            let payload: serde_json::Value =
                serde_json::from_slice(body.as_ref()).expect("response JSON");
            assert_eq!(
                payload
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(serde_json::Value::as_str),
                Some("busy")
            );
            if cli_key == "gemini" {
                assert_eq!(payload["error"]["code"], 503);
            }
        }
    }
}
