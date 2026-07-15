//! Middleware: infers the requested model from path/query/JSON body, computes
//! observe_request flag, and classifies the request kind (Claude `/compact`).
//!
//! Also applies the "large body + missing model" diagnostic heuristic aligned
//! with claude-code-hub's `LARGE_REQUEST_BODY_BYTES`: if the body exceeds
//! `LARGE_REQUEST_BODY_BYTES` and no model can be inferred from any source, we
//! return a 400 with a diagnostic message, because this combination is almost
//! always an upstream-client bug (truncation / non-JSON body / dropped model
//! field) rather than a legitimate request.

use super::{MiddlewareAction, ProxyContext};
use crate::gateway::proxy::compute_observe_request;
use crate::gateway::proxy::handler::early_error::{
    build_early_error_log_ctx, early_error_contract, push_special_setting,
    respond_early_error_with_spawn, EarlyErrorKind,
};
use crate::gateway::proxy::CLAUDE_LOGGED_MESSAGES_PATH;
use crate::gateway::response_fixer;
use crate::gateway::util::{infer_requested_model_info, LARGE_REQUEST_BODY_BYTES};
use axum::http::Method;

/// Claude Code `/compact` replaces the whole system prompt with this marker
/// (verified verbatim against claude-cli 2.1.198). Detection is best-effort:
/// if a future CLI version changes the wording, requests silently fall back to
/// the normal timeout policy.
const COMPACT_SYSTEM_PROMPT_PREFIX: &str =
    "You are a helpful AI assistant tasked with summarizing conversations.";

pub(in crate::gateway::proxy::handler) struct ModelInferenceMiddleware;

impl ModelInferenceMiddleware {
    pub(in crate::gateway::proxy::handler) fn run<R: tauri::Runtime>(
        mut ctx: ProxyContext<R>,
    ) -> MiddlewareAction<R> {
        let model_info = infer_requested_model_info(
            &ctx.forwarded_path,
            ctx.query.as_deref(),
            ctx.introspection_json.as_ref(),
        );
        ctx.requested_model = model_info.model;
        ctx.requested_model_location = model_info.location;

        record_codex_reasoning_effort(
            &ctx.cli_key,
            ctx.introspection_json.as_ref(),
            ctx.requested_model.as_deref(),
            &ctx.special_settings,
        );

        ctx.observe_request = compute_observe_request(
            &ctx.cli_key,
            &ctx.req_method,
            &ctx.forwarded_path,
            &ctx.headers,
            ctx.introspection_json.as_ref(),
        );

        ctx.is_compact_request = is_compact_request(
            &ctx.cli_key,
            &ctx.req_method,
            &ctx.forwarded_path,
            ctx.introspection_json.as_ref(),
        );
        if ctx.is_compact_request {
            push_special_setting(
                &ctx.special_settings,
                serde_json::json!({ "type": "request_kind", "kind": "compact" }),
            );
        }

        if is_large_body_missing_model(ctx.body_bytes.len(), ctx.requested_model.as_deref()) {
            let contract = early_error_contract(EarlyErrorKind::LargeBodyMissingModel);
            let message = large_body_missing_model_message(ctx.body_bytes.len());
            let special_settings_json =
                crate::gateway::response_fixer::special_settings_json(&ctx.special_settings);
            let log_ctx = build_early_error_log_ctx(&ctx);
            let resp = respond_early_error_with_spawn(
                &log_ctx,
                contract,
                message,
                special_settings_json,
                None,
                None,
            );
            return MiddlewareAction::ShortCircuit(resp);
        }

        MiddlewareAction::Continue(Box::new(ctx))
    }
}

/// Detects a Claude Code `/compact` request.
///
/// Only inspects the parsed `system` field (array form, first block's `text`).
/// Never searches the raw body: conversation content may legitimately contain
/// the marker text and must not cause a false positive.
pub(in crate::gateway::proxy::handler) fn is_compact_request(
    cli_key: &str,
    method: &Method,
    forwarded_path: &str,
    introspection_json: Option<&serde_json::Value>,
) -> bool {
    if cli_key != "claude"
        || *method != Method::POST
        || forwarded_path != CLAUDE_LOGGED_MESSAGES_PATH
    {
        return false;
    }

    introspection_json
        .and_then(|root| root.get("system"))
        .and_then(|system| system.as_array())
        .and_then(|blocks| blocks.first())
        .and_then(|block| block.get("text"))
        .and_then(|text| text.as_str())
        .is_some_and(|text| text.starts_with(COMPACT_SYSTEM_PROMPT_PREFIX))
}

pub(in crate::gateway::proxy::handler) fn is_large_body_missing_model(
    body_len: usize,
    requested_model: Option<&str>,
) -> bool {
    body_len >= LARGE_REQUEST_BODY_BYTES && requested_model.map(str::is_empty).unwrap_or(true)
}

pub(in crate::gateway::proxy::handler) fn large_body_missing_model_message(
    body_len: usize,
) -> String {
    let body_mb = body_len as f64 / (1024.0 * 1024.0);
    let threshold_mb = LARGE_REQUEST_BODY_BYTES / (1024 * 1024);
    format!(
        "Missing required field 'model'. Request body ({body_mb:.1} MB) exceeded the \
         gateway's diagnostic threshold ({threshold_mb} MB). If you did send 'model', \
         the body may have been truncated or malformed by an upstream client/proxy. \
         Please verify the request body integrity and JSON format."
    )
}

fn record_codex_reasoning_effort(
    cli_key: &str,
    introspection_json: Option<&serde_json::Value>,
    requested_model: Option<&str>,
    special_settings: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) {
    if cli_key != "codex" {
        return;
    }

    let Some(root) = introspection_json else {
        return;
    };
    let Some(extracted) = extract_codex_reasoning_effort(root) else {
        return;
    };

    response_fixer::push_special_setting(
        special_settings,
        serde_json::json!({
            "type": "codex_reasoning_effort",
            "scope": "request",
            "source": "request",
            "effort": extracted.effort,
            "rawEffort": extracted.raw_effort,
            "requestedModel": requested_model,
            "pointer": extracted.pointer,
        }),
    );
}

struct ExtractedCodexReasoningEffort {
    effort: Option<String>,
    raw_effort: String,
    pointer: &'static str,
}

fn extract_codex_reasoning_effort(
    root: &serde_json::Value,
) -> Option<ExtractedCodexReasoningEffort> {
    for (pointer, value) in [
        (
            "/reasoning/effort",
            root.pointer("/reasoning/effort")
                .and_then(serde_json::Value::as_str),
        ),
        (
            "/reasoning_effort",
            root.get("reasoning_effort")
                .and_then(serde_json::Value::as_str),
        ),
        (
            "/reasoningEffort",
            root.get("reasoningEffort")
                .and_then(serde_json::Value::as_str),
        ),
    ] {
        let Some(raw_effort) = value else {
            continue;
        };
        return Some(ExtractedCodexReasoningEffort {
            effort: normalize_codex_reasoning_effort(raw_effort),
            raw_effort: raw_effort.trim().to_string(),
            pointer,
        });
    }
    None
}

fn normalize_codex_reasoning_effort(value: &str) -> Option<String> {
    let effort = value.trim().to_ascii_lowercase();
    match effort.as_str() {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra" => Some(effort),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_triggers_when_body_large_and_model_missing() {
        assert!(is_large_body_missing_model(LARGE_REQUEST_BODY_BYTES, None));
        assert!(is_large_body_missing_model(
            LARGE_REQUEST_BODY_BYTES + 1,
            None,
        ));
        assert!(is_large_body_missing_model(
            LARGE_REQUEST_BODY_BYTES,
            Some(""),
        ));
    }

    #[test]
    fn heuristic_silent_when_model_present() {
        assert!(!is_large_body_missing_model(
            LARGE_REQUEST_BODY_BYTES,
            Some("claude-sonnet-4"),
        ));
        assert!(!is_large_body_missing_model(
            LARGE_REQUEST_BODY_BYTES * 2,
            Some("gpt-5"),
        ));
    }

    #[test]
    fn heuristic_silent_when_body_below_threshold() {
        assert!(!is_large_body_missing_model(
            LARGE_REQUEST_BODY_BYTES - 1,
            None,
        ));
        assert!(!is_large_body_missing_model(0, None));
    }

    #[test]
    fn diagnostic_message_mentions_actual_size_and_threshold() {
        let message = large_body_missing_model_message(LARGE_REQUEST_BODY_BYTES + 1);
        assert!(message.contains("model"));
        assert!(message.contains(&format!("{} MB", LARGE_REQUEST_BODY_BYTES / (1024 * 1024))));
        assert!(message.contains("truncated"));
    }

    #[test]
    fn extracts_nested_codex_reasoning_effort_first() {
        let root = serde_json::json!({
            "model": "gpt-5.5",
            "reasoning": { "effort": " HIGH " },
            "reasoning_effort": "low",
            "reasoningEffort": "medium"
        });

        let extracted = extract_codex_reasoning_effort(&root).expect("effort");
        assert_eq!(extracted.effort.as_deref(), Some("high"));
        assert_eq!(extracted.raw_effort, "HIGH");
        assert_eq!(extracted.pointer, "/reasoning/effort");
    }

    #[test]
    fn extracts_reasoning_effort_aliases() {
        let snake = serde_json::json!({ "reasoning_effort": "xhigh" });
        let camel = serde_json::json!({ "reasoningEffort": "minimal" });

        let snake_extracted = extract_codex_reasoning_effort(&snake).expect("snake effort");
        let camel_extracted = extract_codex_reasoning_effort(&camel).expect("camel effort");

        assert_eq!(snake_extracted.effort.as_deref(), Some("xhigh"));
        assert_eq!(snake_extracted.raw_effort, "xhigh");
        assert_eq!(snake_extracted.pointer, "/reasoning_effort");
        assert_eq!(camel_extracted.effort.as_deref(), Some("minimal"));
        assert_eq!(camel_extracted.raw_effort, "minimal");
        assert_eq!(camel_extracted.pointer, "/reasoningEffort");
    }

    #[test]
    fn extracts_max_and_ultra_codex_reasoning_effort() {
        let max = serde_json::json!({ "reasoning_effort": "max" });
        let ultra = serde_json::json!({ "reasoningEffort": "Ultra" });

        let max_extracted = extract_codex_reasoning_effort(&max).expect("max effort");
        let ultra_extracted = extract_codex_reasoning_effort(&ultra).expect("ultra effort");

        assert_eq!(max_extracted.effort.as_deref(), Some("max"));
        assert_eq!(max_extracted.raw_effort, "max");
        assert_eq!(max_extracted.pointer, "/reasoning_effort");
        assert_eq!(ultra_extracted.effort.as_deref(), Some("ultra"));
        assert_eq!(ultra_extracted.raw_effort, "Ultra");
        assert_eq!(ultra_extracted.pointer, "/reasoningEffort");
    }

    #[test]
    fn ignores_invalid_codex_reasoning_effort_values() {
        let root = serde_json::json!({
            "reasoning": { "effort": "turbo" },
            "reasoning_effort": "",
            "reasoningEffort": 123
        });

        let extracted = extract_codex_reasoning_effort(&root).expect("explicit effort field");
        assert_eq!(extracted.effort, None);
        assert_eq!(extracted.raw_effort, "turbo");
        assert_eq!(extracted.pointer, "/reasoning/effort");
        assert!(normalize_codex_reasoning_effort("medium").is_some());
        assert!(normalize_codex_reasoning_effort("unknown").is_none());
    }

    fn compact_body() -> serde_json::Value {
        serde_json::json!({
            "model": "claude-3-5-sonnet",
            "system": [
                {
                    "type": "text",
                    "text": format!("{COMPACT_SYSTEM_PROMPT_PREFIX} Follow the instructions."),
                }
            ],
            "messages": [{"role": "user", "content": "summarize"}]
        })
    }

    #[test]
    fn compact_request_detected_for_claude_messages_post() {
        assert!(is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages",
            Some(&compact_body()),
        ));
    }

    #[test]
    fn compact_request_rejects_string_form_system() {
        let body = serde_json::json!({
            "system": COMPACT_SYSTEM_PROMPT_PREFIX,
            "messages": [{"role": "user", "content": "summarize"}]
        });
        assert!(!is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages",
            Some(&body),
        ));
    }

    #[test]
    fn compact_request_ignores_marker_text_inside_messages() {
        let body = serde_json::json!({
            "system": [{"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."}],
            "messages": [
                {"role": "user", "content": COMPACT_SYSTEM_PROMPT_PREFIX}
            ]
        });
        assert!(!is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages",
            Some(&body),
        ));
    }

    #[test]
    fn compact_request_rejects_other_cli_key_path_or_method() {
        let body = compact_body();
        assert!(!is_compact_request(
            "codex",
            &Method::POST,
            "/v1/messages",
            Some(&body),
        ));
        assert!(!is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages/count_tokens",
            Some(&body),
        ));
        assert!(!is_compact_request(
            "claude",
            &Method::GET,
            "/v1/messages",
            Some(&body),
        ));
    }

    #[test]
    fn compact_request_falls_back_false_without_json_or_system() {
        assert!(!is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages",
            None,
        ));
        assert!(!is_compact_request(
            "claude",
            &Method::POST,
            "/v1/messages",
            Some(&serde_json::json!({"messages": []})),
        ));
    }
}
