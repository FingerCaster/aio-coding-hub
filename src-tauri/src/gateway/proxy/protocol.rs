//! Wire-only helpers. Client policy, routing identity and accounting stay with the source CLI.

use crate::shared::gateway_protocol::GatewayProtocol;
use axum::http::{header, HeaderMap, HeaderValue, Method};

pub(in crate::gateway) fn is_native_client(cli_key: &str) -> bool {
    matches!(cli_key, "pi" | "omp")
}

pub(in crate::gateway) fn legacy_protocol(cli_key: &str, path: &str) -> Option<GatewayProtocol> {
    match cli_key {
        "claude" => Some(GatewayProtocol::AnthropicMessages),
        "codex" => Some(GatewayProtocol::OpenaiResponses),
        "gemini" => Some(GatewayProtocol::GoogleGenerativeAi),
        "grok" if path.trim_end_matches('/').ends_with("/responses") => {
            Some(GatewayProtocol::OpenaiResponses)
        }
        "grok" => Some(GatewayProtocol::OpenaiCompletions),
        _ => None,
    }
}

/// Only inference endpoints proven by the native SDK captures are routable.
pub(in crate::gateway) fn is_inference(
    protocol: GatewayProtocol,
    method: &Method,
    path: &str,
) -> bool {
    if *method != Method::POST {
        return false;
    }
    let path = path.trim_end_matches('/');
    match protocol {
        GatewayProtocol::AnthropicMessages => path == "/v1/messages",
        GatewayProtocol::OpenaiCompletions => path == "/v1/chat/completions",
        GatewayProtocol::OpenaiResponses => path == "/v1/responses",
        GatewayProtocol::GoogleGenerativeAi => path
            .strip_prefix("/v1beta/models/")
            .or_else(|| path.strip_prefix("/v1/models/"))
            .and_then(|rest| {
                rest.strip_suffix(":generateContent")
                    .or_else(|| rest.strip_suffix(":streamGenerateContent"))
            })
            .is_some_and(|model| !model.is_empty() && !model.contains('/')),
    }
}

/// Native API-key authentication deliberately has no account/OAuth token heuristics.
pub(in crate::gateway) fn inject_api_key(
    protocol: GatewayProtocol,
    api_key: &str,
    headers: &mut HeaderMap,
) {
    crate::gateway::util::clear_all_auth_headers(headers);
    if protocol != GatewayProtocol::AnthropicMessages {
        headers.remove("anthropic-version");
        headers.remove("anthropic-beta");
    }
    let (name, value) = match protocol {
        GatewayProtocol::AnthropicMessages => {
            if !headers.contains_key("anthropic-version") {
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
            ("x-api-key", api_key.trim().to_string())
        }
        GatewayProtocol::OpenaiCompletions | GatewayProtocol::OpenaiResponses => (
            header::AUTHORIZATION.as_str(),
            format!("Bearer {}", api_key.trim()),
        ),
        GatewayProtocol::GoogleGenerativeAi => ("x-goog-api-key", api_key.trim().to_string()),
    };
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert(name, value);
    }
}

pub(in crate::gateway) fn strip_query_credentials(query: Option<&str>) -> Option<String> {
    let query = query?;
    let mut url = reqwest::Url::parse("http://gateway.invalid/").ok()?;
    url.set_query(Some(query));
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| !matches!(key.as_ref(), "key" | "api_key" | "access_token" | "token"))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if pairs.is_empty() {
        return None;
    }
    url.query_pairs_mut().clear().extend_pairs(pairs);
    url.query().map(str::to_string)
}

pub(in crate::gateway) fn usage_tracker(
    cli_key: &str,
    protocol: Option<GatewayProtocol>,
) -> crate::usage::SseUsageTracker {
    match protocol.filter(|_| is_native_client(cli_key)) {
        Some(protocol) => crate::usage::SseUsageTracker::for_protocol(protocol),
        None => crate::usage::SseUsageTracker::new(cli_key),
    }
}

pub(in crate::gateway) fn parse_usage(
    cli_key: &str,
    protocol: Option<GatewayProtocol>,
    body: &[u8],
) -> Option<crate::usage::UsageExtract> {
    match protocol.filter(|_| is_native_client(cli_key)) {
        Some(protocol) => crate::usage::parse_usage_for_protocol(protocol, body),
        None => crate::usage::parse_usage_from_json_or_sse_bytes(cli_key, body),
    }
}

pub(in crate::gateway) fn error_payload(
    protocol: GatewayProtocol,
    status: u16,
    message: &str,
) -> serde_json::Value {
    match protocol {
        GatewayProtocol::AnthropicMessages => {
            serde_json::json!({"type":"error","error":{"type":"upstream_error","message":message}})
        }
        GatewayProtocol::OpenaiCompletions | GatewayProtocol::OpenaiResponses => {
            serde_json::json!({"error":{"type":"upstream_error","code":"upstream_error","message":message}})
        }
        GatewayProtocol::GoogleGenerativeAi => {
            serde_json::json!({"error":{"code":status,"status":"UNKNOWN","message":message}})
        }
    }
}

pub(in crate::gateway) fn error_frame(
    protocol: GatewayProtocol,
    status: u16,
    message: &str,
) -> bytes::Bytes {
    error_payload_frame(protocol, error_payload(protocol, status, message))
}

pub(in crate::gateway) fn error_payload_frame(
    protocol: GatewayProtocol,
    mut payload: serde_json::Value,
) -> bytes::Bytes {
    let event = match protocol {
        GatewayProtocol::AnthropicMessages => "event: error\n",
        GatewayProtocol::OpenaiResponses => {
            payload["type"] = serde_json::json!("error");
            "event: error\n"
        }
        GatewayProtocol::OpenaiCompletions | GatewayProtocol::GoogleGenerativeAi => "",
    };
    bytes::Bytes::from(format!("{event}data: {payload}\n\n"))
}

/// Preserve gateway diagnostics while making local errors consumable by the SDK.
pub(in crate::gateway) async fn adapt_error_response(
    protocol: GatewayProtocol,
    response: axum::response::Response,
) -> axum::response::Response {
    if response.status().is_success()
        || !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/json"))
    {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let value = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let message = value
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Gateway request failed");
    let payload = if value
        .as_ref()
        .is_some_and(|value| value.get("error").is_some())
    {
        value.unwrap()
    } else {
        let mut payload = error_payload(protocol, parts.status.as_u16(), message);
        if let Some(serde_json::Value::Object(original)) = value {
            for key in ["trace_id", "error_code", "attempts", "retry_after"] {
                if let Some(value) = original.get(key) {
                    payload[key] = value.clone();
                }
            }
        }
        payload
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.remove(header::CONTENT_ENCODING);
    axum::response::Response::from_parts(parts, axum::body::Body::from(payload.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_endpoints_do_not_admit_another_protocol_or_auxiliary_route() {
        let routes = [
            (GatewayProtocol::AnthropicMessages, "/v1/messages"),
            (GatewayProtocol::OpenaiCompletions, "/v1/chat/completions"),
            (GatewayProtocol::OpenaiResponses, "/v1/responses"),
            (
                GatewayProtocol::GoogleGenerativeAi,
                "/v1beta/models/native-test:streamGenerateContent",
            ),
        ];
        for (protocol, path) in routes {
            assert!(is_inference(protocol, &Method::POST, path));
            assert!(!is_inference(protocol, &Method::GET, path));
            for (other, _) in routes {
                assert_eq!(is_inference(other, &Method::POST, path), protocol == other);
            }
            for auxiliary in [
                "/v1/models",
                "/v1/responses/compact",
                "/v1/messages/count_tokens",
                "/v1beta/models/:generateContent",
            ] {
                assert!(!is_inference(protocol, &Method::POST, auxiliary));
            }
        }
    }

    #[test]
    fn grok_legacy_protocol_is_path_specific_and_native_clients_have_no_default() {
        assert_eq!(
            legacy_protocol("grok", "/v1/responses/"),
            Some(GatewayProtocol::OpenaiResponses)
        );
        assert_eq!(
            legacy_protocol("grok", "/v1/chat/completions"),
            Some(GatewayProtocol::OpenaiCompletions)
        );
        assert_eq!(legacy_protocol("pi", "/v1/responses"), None);
        assert_eq!(legacy_protocol("omp", "/v1/messages"), None);
    }

    #[test]
    fn native_api_key_auth_clears_every_client_credential() {
        for (protocol, name, expected) in [
            (
                GatewayProtocol::AnthropicMessages,
                "x-api-key",
                "upstream-key",
            ),
            (
                GatewayProtocol::OpenaiCompletions,
                "authorization",
                "Bearer upstream-key",
            ),
            (
                GatewayProtocol::OpenaiResponses,
                "authorization",
                "Bearer upstream-key",
            ),
            (
                GatewayProtocol::GoogleGenerativeAi,
                "x-goog-api-key",
                "upstream-key",
            ),
        ] {
            let mut headers = HeaderMap::new();
            for key in [
                "authorization",
                "x-api-key",
                "x-goog-api-key",
                "chatgpt-account-id",
                "x-goog-api-client",
            ] {
                headers.insert(key, HeaderValue::from_static("client-placeholder"));
            }
            inject_api_key(protocol, "upstream-key", &mut headers);
            assert_eq!(headers[name], expected);
            assert!(!headers.values().any(|value| value == "client-placeholder"));
        }
        let mut headers = HeaderMap::new();
        inject_api_key(
            GatewayProtocol::GoogleGenerativeAi,
            "ya29.literal-api-key",
            &mut headers,
        );
        assert_eq!(headers["x-goog-api-key"], "ya29.literal-api-key");
        assert!(!headers.contains_key(header::AUTHORIZATION));
    }

    #[test]
    fn query_auth_is_removed_but_sdk_query_flags_survive() {
        assert_eq!(
            strip_query_credentials(Some("key=secret&beta=true&alt=sse")),
            Some("beta=true&alt=sse".to_string())
        );
        assert_eq!(
            strip_query_credentials(Some("%6Bey=secret&access_token=secret")),
            None
        );
    }
}
