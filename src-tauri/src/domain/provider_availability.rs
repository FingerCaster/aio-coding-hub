//! Usage: Lightweight provider availability probe.
//!
//! Sends a minimal API request to verify that a provider's base URL + credentials
//! are reachable and functional. Supports all CLI types (claude, codex, gemini).

use crate::providers::{is_supported_bridge_type, ModelMapping, CX2CC_BRIDGE_TYPE};
use crate::shared::error::AppResult;
use crate::{blocking, db};
use reqwest::header::{HeaderMap, HeaderValue};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_RESPONSE_BODY_LIMIT: usize = 64 * 1024;
const PROBE_RESPONSE_PREVIEW_LIMIT: usize = 500;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ProviderAvailabilityResult {
    pub ok: bool,
    pub provider_id: i64,
    pub provider_name: String,
    pub base_url: String,
    pub status: Option<u16>,
    pub latency_ms: i64,
    pub error: Option<String>,
    pub response_preview: Option<String>,
}

struct LoadedProvider {
    id: i64,
    transport_provider_id: i64,
    cli_key: String,
    name: String,
    base_urls: Vec<String>,
    api_key_plaintext: String,
    availability_test_model: Option<String>,
    model_mapping: ModelMapping,
    auth_mode: String,
    oauth_provider_type: Option<String>,
    source_provider_id: Option<i64>,
    bridge_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeResponseBody {
    bytes: Vec<u8>,
    truncated: bool,
    limit: usize,
}

fn append_probe_response_chunk(bytes: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    let remaining = limit.saturating_sub(bytes.len());
    if remaining == 0 {
        return !chunk.is_empty();
    }

    let keep = chunk.len().min(remaining);
    bytes.extend_from_slice(&chunk[..keep]);
    keep < chunk.len()
}

async fn read_probe_response_body_with_limit(
    mut resp: reqwest::Response,
    limit: usize,
) -> Result<ProbeResponseBody, String> {
    let content_length = resp.content_length();
    let mut truncated = content_length.is_some_and(|len| len > limit as u64);
    let capacity = content_length
        .and_then(|len| usize::try_from(len).ok())
        .unwrap_or_default()
        .min(limit);
    let mut bytes = Vec::with_capacity(capacity);

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("failed to read probe response: {e}"))?
    {
        if append_probe_response_chunk(&mut bytes, chunk.as_ref(), limit) {
            truncated = true;
            break;
        }
        if bytes.len() >= limit && content_length != Some(limit as u64) {
            truncated = true;
            break;
        }
    }

    Ok(ProbeResponseBody {
        bytes,
        truncated,
        limit,
    })
}

fn probe_response_preview(body: &ProbeResponseBody) -> String {
    let preview_len = body.bytes.len().min(PROBE_RESPONSE_PREVIEW_LIMIT);
    let mut preview = String::from_utf8_lossy(&body.bytes[..preview_len]).to_string();
    if body.truncated {
        if !preview.is_empty() {
            preview.push('\n');
        }
        preview.push_str(&format!(
            "[probe response truncated after {} bytes]",
            body.limit
        ));
    }
    preview
}

async fn load_provider_for_test(db: db::Db, provider_id: i64) -> AppResult<LoadedProvider> {
    blocking::run("provider_availability_load", move || -> AppResult<LoadedProvider> {
        if provider_id <= 0 {
            return Err(format!("SEC_INVALID_INPUT: invalid provider_id={provider_id}").into());
        }

        let conn = db.open_connection()?;
        #[allow(clippy::type_complexity)]
        let row: Option<(
            i64,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
        )> = conn
            .query_row(
                r#"
SELECT id, cli_key, name, base_url, base_urls_json, api_key_plaintext, availability_test_model, model_mapping_json, auth_mode, oauth_provider_type, source_provider_id, bridge_type
FROM providers
WHERE id = ?1
"#,
                rusqlite::params![provider_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| format!("DB_ERROR: {e}"))?;

        let Some((id, cli_key, name, base_url_fallback, base_urls_json, api_key_plaintext, availability_test_model, model_mapping_json, auth_mode, oauth_provider_type, source_provider_id, bridge_type)) = row else {
            return Err("DB_NOT_FOUND: provider not found".into());
        };

        let mut base_urls: Vec<String> = serde_json::from_str::<Vec<String>>(&base_urls_json)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();

        if base_urls.is_empty() {
            let fallback = base_url_fallback.trim().to_string();
            if !fallback.is_empty() {
                base_urls.push(fallback);
            }
        }

        Ok(LoadedProvider {
            id,
            transport_provider_id: id,
            cli_key,
            name,
            base_urls,
            api_key_plaintext,
            availability_test_model: normalize_probe_model(availability_test_model.as_deref()),
            model_mapping: model_mapping_from_json(&model_mapping_json),
            auth_mode,
            oauth_provider_type,
            source_provider_id,
            bridge_type,
        })
    })
    .await
}

async fn load_effective_provider_for_test(
    db: db::Db,
    provider_id: i64,
) -> AppResult<LoadedProvider> {
    let provider = load_provider_for_test(db.clone(), provider_id).await?;
    let Some(bridge_type) = provider.bridge_type.as_deref() else {
        return Ok(provider);
    };

    if bridge_type == CX2CC_BRIDGE_TYPE && provider.source_provider_id.is_none() {
        return Ok(provider);
    }

    let Some(source_provider_id) = provider.source_provider_id else {
        return Ok(provider);
    };

    let (source, source_cli_key) =
        crate::providers::get_source_provider_for_gateway(&db, source_provider_id, bridge_type)?;

    Ok(LoadedProvider {
        id: provider.id,
        transport_provider_id: source.id,
        cli_key: source_cli_key,
        name: provider.name,
        base_urls: source.base_urls,
        api_key_plaintext: source.api_key_plaintext,
        availability_test_model: provider.availability_test_model,
        model_mapping: provider.model_mapping,
        auth_mode: source.auth_mode,
        oauth_provider_type: source.oauth_provider_type,
        source_provider_id: provider.source_provider_id,
        bridge_type: provider.bridge_type,
    })
}

impl LoadedProvider {
    fn transport_context(&self) -> crate::providers::ProviderTransportContext {
        crate::providers::ProviderTransportContext {
            provider_id: self.transport_provider_id,
            base_urls: self.base_urls.clone(),
            api_key_plaintext: self.api_key_plaintext.clone(),
            auth_mode: self.auth_mode.clone(),
            oauth_provider_type: self.oauth_provider_type.clone(),
        }
    }

    fn resolved_base_url(&self) -> AppResult<String> {
        crate::gateway::resolve_transport_base_url(&self.transport_context(), &self.cli_key)
            .map_err(Into::into)
    }
}

fn normalize_probe_model(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn model_mapping_from_json(raw: &str) -> ModelMapping {
    let mapping = serde_json::from_str::<ModelMapping>(raw)
        .ok()
        .unwrap_or_default();
    ModelMapping {
        default_model: normalize_probe_model(mapping.default_model.as_deref()),
        exact: mapping
            .exact
            .into_iter()
            .filter_map(|(key, value)| {
                let key = normalize_probe_model(Some(&key))?;
                let value = normalize_probe_model(Some(&value))?;
                Some((key, value))
            })
            .collect(),
    }
}

fn resolve_codex_probe_model_from_sources(
    provider_override: Option<&str>,
    global_setting: Option<&str>,
) -> String {
    normalize_probe_model(provider_override)
        .or_else(|| normalize_probe_model(global_setting))
        .unwrap_or_else(|| crate::settings::DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string())
}

fn build_probe_request(
    cli_key: &str,
    base_url: &str,
    api_key: &str,
    model_override: Option<&str>,
) -> AppResult<(String, HeaderMap, serde_json::Value)> {
    let base = base_url.trim_end_matches('/');

    match cli_key {
        "claude" => {
            let url = format!("{base}/v1/messages");
            let mut headers = HeaderMap::new();
            if let Ok(v) = HeaderValue::from_str(api_key) {
                headers.insert("x-api-key", v);
            }
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            headers.insert("content-type", HeaderValue::from_static("application/json"));
            let body = serde_json::json!({
                "model": model_override.unwrap_or("claude-sonnet-4-6"),
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            Ok((url, headers, body))
        }
        "codex" => {
            let url = format!("{base}/v1/chat/completions");
            let mut headers = HeaderMap::new();
            let bearer = format!("Bearer {api_key}");
            if let Ok(v) = HeaderValue::from_str(&bearer) {
                headers.insert("authorization", v);
            }
            headers.insert("content-type", HeaderValue::from_static("application/json"));
            let body = serde_json::json!({
                "model": model_override.unwrap_or(crate::settings::DEFAULT_CODEX_PROVIDER_TEST_MODEL),
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}]
            });
            Ok((url, headers, body))
        }
        "gemini" => {
            let url =
                format!("{base}/v1beta/models/gemini-2.0-flash:generateContent?key={api_key}");
            let mut headers = HeaderMap::new();
            headers.insert("content-type", HeaderValue::from_static("application/json"));
            let body = serde_json::json!({
                "contents": [{"parts": [{"text": "ping"}]}],
                "generationConfig": {"maxOutputTokens": 1}
            });
            Ok((url, headers, body))
        }
        _ => Err(format!("UNSUPPORTED_CLI_KEY: {cli_key}").into()),
    }
}

fn build_probe_request_with_body(
    cli_key: &str,
    base_url: &str,
    api_key: &str,
    target_path: &str,
    body: serde_json::Value,
) -> AppResult<(String, HeaderMap, serde_json::Value)> {
    let base = base_url.trim_end_matches('/');
    let path = if target_path.starts_with('/') {
        target_path.to_string()
    } else {
        format!("/{target_path}")
    };
    let mut url = format!("{base}{path}");
    let mut headers = HeaderMap::new();

    match cli_key {
        "claude" => {
            if let Ok(v) = HeaderValue::from_str(api_key) {
                headers.insert("x-api-key", v);
            }
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        "codex" => {
            let bearer = format!("Bearer {api_key}");
            if let Ok(v) = HeaderValue::from_str(&bearer) {
                headers.insert("authorization", v);
            }
        }
        "gemini" => {
            let separator = if url.contains('?') { '&' } else { '?' };
            url.push(separator);
            url.push_str("key=");
            url.push_str(api_key);
        }
        _ => return Err(format!("UNSUPPORTED_CLI_KEY: {cli_key}").into()),
    }

    headers.insert("content-type", HeaderValue::from_static("application/json"));
    Ok((url, headers, body))
}

fn build_bridge_probe_request(
    provider: &LoadedProvider,
    base_url: &str,
    api_key: &str,
    source_model: &str,
) -> AppResult<(String, HeaderMap, serde_json::Value)> {
    let bridge_type = provider
        .bridge_type
        .as_deref()
        .ok_or_else(|| "BRIDGE_MISSING_TYPE: bridge provider missing bridge_type".to_string())?;
    let (target_path, translated_body) = crate::gateway::build_translated_bridge_probe(
        bridge_type,
        provider.model_mapping.clone(),
        source_model,
    )?;
    build_probe_request_with_body(
        &provider.cli_key,
        base_url,
        api_key,
        &target_path,
        translated_body,
    )
}

fn redact_key_param(msg: &str) -> String {
    regex::Regex::new(r"([?&])key=[^&\s]*")
        .map(|re| re.replace_all(msg, "${1}key=***").to_string())
        .unwrap_or_else(|_| msg.to_string())
}

fn looks_like_auth_failure(status: u16, response_text: &str) -> bool {
    if matches!(status, 401 | 403) {
        return true;
    }

    let lower = response_text.to_ascii_lowercase();
    [
        "api key not valid",
        "invalid api key",
        "invalid_api_key",
        "invalid x-api-key",
        "authentication",
        "unauthorized",
        "permission denied",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_probe_available_status(status: u16, response_text: &str) -> bool {
    status < 500 && !looks_like_auth_failure(status, response_text)
}

fn should_map_bridge_probe_model(bridge_type: Option<&str>) -> bool {
    matches!(bridge_type, Some(value) if value != CX2CC_BRIDGE_TYPE && is_supported_bridge_type(value))
}

pub async fn test_provider_availability<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: db::Db,
    provider_id: i64,
) -> AppResult<ProviderAvailabilityResult> {
    let provider = load_effective_provider_for_test(db.clone(), provider_id).await?;

    if let Some(bridge_type) = provider.bridge_type.as_deref() {
        let bridge_label = if bridge_type == CX2CC_BRIDGE_TYPE {
            "CX2CC"
        } else if is_supported_bridge_type(bridge_type) {
            "转译桥接"
        } else {
            "未知桥接"
        };
        if bridge_type == CX2CC_BRIDGE_TYPE && provider.source_provider_id.is_none() {
            return Ok(ProviderAvailabilityResult {
                ok: false,
                provider_id: provider.id,
                provider_name: provider.name,
                base_url: provider.base_urls.first().cloned().unwrap_or_default(),
                status: None,
                latency_ms: 0,
                error: Some(format!("{bridge_label}供应商需通过其源供应商测试可用性")),
                response_preview: None,
            });
        }
    }

    let base_url = provider.resolved_base_url()?;
    if base_url.is_empty() {
        return Ok(ProviderAvailabilityResult {
            ok: false,
            provider_id: provider.id,
            provider_name: provider.name,
            base_url,
            status: None,
            latency_ms: 0,
            error: Some("供应商未配置 Base URL".into()),
            response_preview: None,
        });
    }

    if provider.auth_mode != "oauth" && provider.api_key_plaintext.trim().is_empty() {
        return Ok(ProviderAvailabilityResult {
            ok: false,
            provider_id: provider.id,
            provider_name: provider.name,
            base_url,
            status: None,
            latency_ms: 0,
            error: Some("供应商未配置 API Key".into()),
            response_preview: None,
        });
    }

    let effective_credential = crate::providers::resolve_effective_transport_credential(
        &db,
        &crate::gateway::http_client::get(),
        &provider.cli_key,
        &provider.transport_context(),
    )
    .await?;

    let bridge_probe_source_model =
        if should_map_bridge_probe_model(provider.bridge_type.as_deref()) {
            let settings = crate::settings::read(app)?;
            Some(resolve_codex_probe_model_from_sources(
                provider.availability_test_model.as_deref(),
                Some(settings.codex_provider_test_model.as_str()),
            ))
        } else {
            None
        };
    let regular_probe_model = if bridge_probe_source_model.is_none() && provider.cli_key == "codex"
    {
        match normalize_probe_model(provider.availability_test_model.as_deref()) {
            Some(model) => Some(model),
            None => {
                let settings = crate::settings::read(app)?;
                Some(resolve_codex_probe_model_from_sources(
                    None,
                    Some(settings.codex_provider_test_model.as_str()),
                ))
            }
        }
    } else {
        None
    };
    let (url, headers, body) = if let Some(source_model) = bridge_probe_source_model.as_deref() {
        build_bridge_probe_request(&provider, &base_url, &effective_credential, source_model)?
    } else {
        build_probe_request(
            &provider.cli_key,
            &base_url,
            &effective_credential,
            regular_probe_model.as_deref(),
        )?
    };

    let client = reqwest::Client::builder()
        .user_agent(format!(
            "aio-coding-hub-probe/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP_CLIENT_INIT: {e}"))?;

    let started = Instant::now();
    let result = client.post(&url).headers(headers).json(&body).send().await;

    let latency_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = read_probe_response_body_with_limit(resp, PROBE_RESPONSE_BODY_LIMIT)
                .await
                .unwrap_or_else(|_| ProbeResponseBody {
                    bytes: Vec::new(),
                    truncated: false,
                    limit: PROBE_RESPONSE_BODY_LIMIT,
                });
            let preview = probe_response_preview(&body);
            // Provider is "available" if the endpoint responds without an auth
            // failure or upstream 5xx. 400/404 model errors and 429 rate limits
            // still prove the configured base URL and credential reached the
            // provider, but Gemini invalid API keys are reported as 400 and must
            // not be treated as available.
            let ok = is_probe_available_status(status, &preview);

            let error = if ok {
                None
            } else {
                let msg = serde_json::from_slice::<serde_json::Value>(&body.bytes)
                    .ok()
                    .and_then(|v| {
                        v.get("error").and_then(|e| {
                            e.get("message")
                                .and_then(|m| m.as_str().map(String::from))
                                .or_else(|| e.as_str().map(String::from))
                        })
                    })
                    .unwrap_or_else(|| format!("HTTP {status}"));
                Some(msg)
            };

            Ok(ProviderAvailabilityResult {
                ok,
                provider_id: provider.id,
                provider_name: provider.name,
                base_url,
                status: Some(status),
                latency_ms,
                error,
                response_preview: if ok { None } else { Some(preview) },
            })
        }
        Err(err) => {
            let error_message = if err.is_timeout() {
                "请求超时（15秒）".to_string()
            } else if err.is_connect() {
                redact_key_param(&format!("连接失败: {err}"))
            } else {
                redact_key_param(&format!("请求失败: {err}"))
            };

            Ok(ProviderAvailabilityResult {
                ok: false,
                provider_id: provider.id,
                provider_name: provider.name,
                base_url,
                status: None,
                latency_ms,
                error: Some(error_message),
                response_preview: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        upsert, DailyResetMode, ProviderAuthMode, ProviderBaseUrlMode, ProviderUpsertParams,
        CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE, CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
        CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn default_provider_params(name: &str) -> ProviderUpsertParams {
        ProviderUpsertParams {
            provider_id: None,
            cli_key: "codex".to_string(),
            name: name.to_string(),
            base_urls: vec!["https://api.example.com/v1".to_string()],
            base_url_mode: ProviderBaseUrlMode::Order,
            auth_mode: Some(ProviderAuthMode::ApiKey),
            api_key: Some("sk-test".to_string()),
            enabled: true,
            cost_multiplier: 1.0,
            priority: Some(100),
            claude_models: None,
            availability_test_model: None,
            limit_5h_usd: None,
            limit_daily_usd: None,
            daily_reset_mode: Some(DailyResetMode::Fixed),
            daily_reset_time: Some("00:00:00".to_string()),
            limit_weekly_usd: None,
            limit_monthly_usd: None,
            limit_total_usd: None,
            tags: None,
            note: None,
            source_provider_id: None,
            bridge_type: None,
            stream_idle_timeout_seconds: None,
            model_mapping: None,
            extension_values: None,
            upstream_retry_policy_override: None,
            upstream_retry_policy_override_specified: false,
        }
    }

    async fn response_from_request_capture(
        expected_path: &'static str,
        response_status: u16,
        response_body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("test server addr");
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).await.expect("read request");
                if read == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..read]);
                if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            if let Some(header_end) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_ascii_lowercase();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or_default();
                let body_start = header_end + 4;
                while buf.len().saturating_sub(body_start) < content_length {
                    let read = stream.read(&mut chunk).await.expect("read request body");
                    if read == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..read]);
                }
            }

            let request = String::from_utf8_lossy(&buf).to_string();
            assert!(
                request.contains(&format!("POST {expected_path} ")),
                "unexpected request path: {request}"
            );

            let response = format!(
                "HTTP/1.1 {response_status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            let _ = stream.shutdown().await;
            request
        });

        (format!("http://{addr}"), task)
    }

    fn header_value(headers: &HeaderMap, key: &str) -> String {
        headers
            .get(key)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn build_probe_request_for_claude_uses_messages_endpoint_and_x_api_key() {
        let (url, headers, body) =
            build_probe_request("claude", "https://api.example.com/", "sk-claude", None)
                .expect("claude request");

        assert_eq!(url, "https://api.example.com/v1/messages");
        assert_eq!(header_value(&headers, "x-api-key"), "sk-claude");
        assert_eq!(header_value(&headers, "anthropic-version"), "2023-06-01");
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["messages"][0]["content"], "ping");
    }

    #[test]
    fn build_probe_request_for_claude_uses_model_override() {
        let (_, _, body) = build_probe_request(
            "claude",
            "https://api.example.com/",
            "sk-claude",
            Some("claude-test-model"),
        )
        .expect("claude request");

        assert_eq!(body["model"], "claude-test-model");
    }

    #[test]
    fn build_probe_request_for_codex_uses_chat_completions_and_bearer_auth() {
        let (url, headers, body) = build_probe_request(
            "codex",
            "https://api.example.com",
            "sk-openai",
            Some("gpt-test"),
        )
        .expect("codex request");

        assert_eq!(url, "https://api.example.com/v1/chat/completions");
        assert_eq!(header_value(&headers, "authorization"), "Bearer sk-openai");
        assert_eq!(body["messages"][0]["content"], "ping");
        assert_eq!(body["model"], "gpt-test");
    }

    #[test]
    fn build_probe_request_for_gemini_uses_generate_content_key_param() {
        let (url, headers, body) = build_probe_request(
            "gemini",
            "https://generativelanguage.googleapis.com/",
            "sk-google",
            None,
        )
        .expect("gemini request");

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent?key=sk-google"
        );
        assert_eq!(header_value(&headers, "content-type"), "application/json");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "ping");
    }

    #[test]
    fn build_probe_request_rejects_unsupported_cli_key() {
        let err = build_probe_request("unknown", "https://api.example.com", "secret", None)
            .unwrap_err()
            .to_string();

        assert_eq!(err, "UNSUPPORTED_CLI_KEY: unknown");
    }

    #[test]
    fn resolve_codex_probe_model_from_sources_prefers_provider_override_then_global_then_default() {
        assert_eq!(
            resolve_codex_probe_model_from_sources(Some("gpt-provider"), Some("gpt-global")),
            "gpt-provider"
        );
        assert_eq!(
            resolve_codex_probe_model_from_sources(Some("   "), Some("gpt-global")),
            "gpt-global"
        );
        assert_eq!(
            resolve_codex_probe_model_from_sources(None, Some("   ")),
            crate::settings::DEFAULT_CODEX_PROVIDER_TEST_MODEL
        );
    }

    #[test]
    fn redact_key_param_preserves_delimiters_and_hides_gemini_key() {
        let redacted =
            redact_key_param("连接失败: https://host/v1beta/models?alt=sse&key=sk-secret&other=1");

        assert_eq!(
            redacted,
            "连接失败: https://host/v1beta/models?alt=sse&key=***&other=1"
        );
        assert!(!redacted.contains("sk-secret"));
    }

    #[test]
    fn append_probe_response_chunk_keeps_bounded_prefix() {
        let mut bytes = b"abcd".to_vec();
        let truncated = append_probe_response_chunk(&mut bytes, b"efgh", 6);

        assert_eq!(bytes, b"abcdef");
        assert!(truncated);
    }

    #[test]
    fn probe_response_preview_marks_truncated_payloads() {
        let preview = probe_response_preview(&ProbeResponseBody {
            bytes: b"upstream error".to_vec(),
            truncated: true,
            limit: 12,
        });

        assert_eq!(
            preview,
            "upstream error\n[probe response truncated after 12 bytes]"
        );
    }

    #[test]
    fn probe_status_rejects_5xx_and_auth_errors_but_allows_model_or_rate_limit_errors() {
        assert!(is_probe_available_status(
            400,
            r#"{"error":{"message":"model not found"}}"#
        ));
        assert!(is_probe_available_status(404, "model not found"));
        assert!(is_probe_available_status(429, "rate limit exceeded"));

        assert!(!is_probe_available_status(500, "upstream error"));
        assert!(!is_probe_available_status(401, "unauthorized"));
        assert!(!is_probe_available_status(
            400,
            r#"{"error":{"message":"API key not valid. Please pass a valid API key."}}"#
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_bridge_availability_uses_source_provider_transport() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp.path().join("provider-availability.sqlite3");
        let db = crate::db::init_for_tests(&db_path).expect("init db");
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let (source_base_url, server_task) = response_from_request_capture(
            "/v1/messages",
            400,
            r#"{"error":{"message":"model not found"}}"#,
        )
        .await;

        let mut source_params = default_provider_params("Claude source");
        source_params.cli_key = "claude".to_string();
        source_params.base_urls = vec![source_base_url.clone()];
        source_params.api_key = Some("sk-claude".to_string());
        let source = upsert(&db, source_params).expect("insert source");

        let mut bridge_params = default_provider_params("Codex bridge");
        bridge_params.base_urls = vec![];
        bridge_params.api_key = None;
        bridge_params.source_provider_id = Some(source.id);
        bridge_params.bridge_type = Some(CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE.to_string());
        let bridge = upsert(&db, bridge_params).expect("insert bridge");

        let result = test_provider_availability(&app_handle, db, bridge.id)
            .await
            .expect("availability result");

        assert!(result.ok);
        assert_eq!(result.provider_id, bridge.id);
        assert_eq!(result.provider_name, "Codex bridge");
        assert_eq!(result.base_url, source_base_url);
        assert_eq!(result.status, Some(400));
        assert!(result.error.is_none());

        let request = server_task.await.expect("server task");
        assert!(request
            .to_ascii_lowercase()
            .contains("x-api-key: sk-claude"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_bridge_availability_maps_configured_test_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp
            .path()
            .join("provider-availability-mapped-model.sqlite3");
        let db = crate::db::init_for_tests(&db_path).expect("init db");
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let (source_base_url, server_task) = response_from_request_capture(
            "/v1/messages",
            400,
            r#"{"error":{"message":"model not found"}}"#,
        )
        .await;

        let mut source_params = default_provider_params("Claude source");
        source_params.cli_key = "claude".to_string();
        source_params.base_urls = vec![source_base_url.clone()];
        source_params.api_key = Some("sk-claude".to_string());
        let source = upsert(&db, source_params).expect("insert source");

        let mut bridge_params = default_provider_params("Codex bridge");
        bridge_params.base_urls = vec![];
        bridge_params.api_key = None;
        bridge_params.availability_test_model = Some("gpt-5.5".to_string());
        bridge_params.model_mapping = Some(crate::providers::ModelMapping {
            default_model: Some("claude-default".to_string()),
            exact: std::collections::BTreeMap::from([(
                "gpt-5.5".to_string(),
                "claude-opus-test".to_string(),
            )]),
        });
        bridge_params.source_provider_id = Some(source.id);
        bridge_params.bridge_type = Some(CODEX_TO_ANTHROPIC_MESSAGES_BRIDGE_TYPE.to_string());
        let bridge = upsert(&db, bridge_params).expect("insert bridge");

        let result = test_provider_availability(&app_handle, db, bridge.id)
            .await
            .expect("availability result");

        assert!(result.ok);
        let request = server_task.await.expect("server task");
        assert!(
            request.contains(r#""model":"claude-opus-test""#),
            "mapped test model was not used: {request}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_bridge_availability_uses_disabled_source_transport() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp
            .path()
            .join("provider-availability-disabled-source.sqlite3");
        let db = crate::db::init_for_tests(&db_path).expect("init db");
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let (source_base_url, server_task) = response_from_request_capture(
            "/v1/responses",
            400,
            r#"{"error":{"message":"model not found"}}"#,
        )
        .await;

        let mut source_params = default_provider_params("Disabled source");
        source_params.cli_key = "codex".to_string();
        source_params.base_urls = vec![source_base_url.clone()];
        source_params.api_key = Some("sk-disabled-source".to_string());
        let source = upsert(&db, source_params).expect("insert source");

        let mut bridge_params = default_provider_params("Codex bridge");
        bridge_params.base_urls = vec![];
        bridge_params.api_key = None;
        bridge_params.source_provider_id = Some(source.id);
        bridge_params.bridge_type = Some(CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE.to_string());
        let bridge = upsert(&db, bridge_params).expect("insert bridge");
        crate::providers::set_enabled(&db, source.id, false).expect("disable source");

        let result = test_provider_availability(&app_handle, db, bridge.id)
            .await
            .expect("availability result");

        assert!(result.ok);
        assert_eq!(result.provider_id, bridge.id);
        assert_eq!(result.provider_name, "Codex bridge");
        assert_eq!(result.base_url, source_base_url);
        assert_eq!(result.status, Some(400));
        assert!(result.error.is_none());

        let request = server_task.await.expect("server task");
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-disabled-source"));
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "current_thread")]
    async fn codex_bridge_availability_uses_oauth_source_credential() {
        let _env_lock = crate::test_support::test_env_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let db_path = temp
            .path()
            .join("provider-availability-oauth-source.sqlite3");
        let db = crate::db::init_for_tests(&db_path).expect("init db");
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();

        let (source_base_url, server_task) = response_from_request_capture(
            "/v1/chat/completions",
            400,
            r#"{"error":{"message":"model not found"}}"#,
        )
        .await;
        std::env::set_var(
            "AIO_CODING_HUB_TEST_CODEX_OAUTH_BASE_URL",
            source_base_url.clone(),
        );

        let mut source_params = default_provider_params("OAuth codex source");
        source_params.base_urls = vec![source_base_url.clone()];
        source_params.auth_mode = Some(ProviderAuthMode::Oauth);
        source_params.api_key = None;
        let source = upsert(&db, source_params).expect("insert oauth source");

        crate::providers::update_oauth_tokens(
            &db,
            source.id,
            "oauth",
            "codex_oauth",
            "oauth-access-token",
            Some("oauth-refresh-token"),
            None,
            "https://auth.openai.com/oauth/token",
            "test-client-id",
            None,
            Some(crate::shared::time::now_unix_seconds() + 3_600),
            Some("oauth@example.com"),
        )
        .expect("seed oauth token");

        let mut bridge_params = default_provider_params("Codex bridge");
        bridge_params.base_urls = vec![];
        bridge_params.api_key = None;
        bridge_params.source_provider_id = Some(source.id);
        bridge_params.bridge_type = Some(CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE.to_string());
        let bridge = upsert(&db, bridge_params).expect("insert bridge");

        let result = test_provider_availability(&app_handle, db, bridge.id)
            .await
            .expect("availability result");

        assert!(result.ok);
        assert_eq!(result.provider_id, bridge.id);
        assert_eq!(result.provider_name, "Codex bridge");
        assert_eq!(result.base_url, source_base_url);
        assert_eq!(result.status, Some(400));
        assert!(result.error.is_none());

        let request = server_task.await.expect("server task");
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer oauth-access-token"));
        std::env::remove_var("AIO_CODING_HUB_TEST_CODEX_OAUTH_BASE_URL");
    }
}
