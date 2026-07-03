//! Usage: Privacy-safe Codex reasoning feature samples for guard evaluation and logs.

use crate::gateway::response_fixer;
use crate::settings::CodexReasoningGuardRuleMode;
use axum::http::HeaderMap;
use std::sync::{Arc, Mutex};

pub(super) const CODEX_REASONING_FEATURES_TYPE: &str = "codex_reasoning_features";
pub(super) const REQUEST_KIND_CONTEXT_COMPACTION: &str = "context_compaction";
pub(super) const EXEMPT_REASON_CONTEXT_COMPACTION: &str = "context_compaction";
pub(super) const RESPONSE_CLASSIFICATION_COMPLETE: &str = "complete";
pub(super) const RESPONSE_CLASSIFICATION_REQUEST_ONLY: &str = "request_only";
pub(super) const SKIPPED_GUARD_DISABLED_STREAM_NOT_BUFFERED: &str =
    "guard_disabled_stream_not_buffered";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexReasoningFeatureSample {
    pub(super) rule_mode: CodexReasoningGuardRuleMode,
    pub(super) reasoning_tokens: Option<i64>,
    pub(super) reasoning_tokens_pointer: Option<&'static str>,
    pub(super) request_reasoning_effort: Option<String>,
    pub(super) raw_request_reasoning_effort: Option<String>,
    pub(super) request_reasoning_effort_pointer: Option<String>,
    pub(super) request_kind: Option<&'static str>,
    pub(super) intercept_exempt_reason: Option<&'static str>,
    pub(super) response_classification: &'static str,
    pub(super) classification_skipped_reason: Option<&'static str>,
    pub(super) has_final_answer: Option<bool>,
    pub(super) has_commentary: Option<bool>,
    pub(super) commentary_observed: Option<bool>,
    pub(super) has_tool_call: Option<bool>,
    pub(super) has_reasoning_item: Option<bool>,
    pub(super) has_output_text: Option<bool>,
    pub(super) final_answer_only: Option<bool>,
}

impl CodexReasoningFeatureSample {
    pub(super) fn to_special_setting(&self) -> serde_json::Value {
        serde_json::json!({
            "type": CODEX_REASONING_FEATURES_TYPE,
            "scope": "attempt",
            "ruleMode": self.rule_mode,
            "reasoningTokens": self.reasoning_tokens,
            "reasoningTokensPointer": self.reasoning_tokens_pointer,
            "requestReasoningEffort": self.request_reasoning_effort,
            "rawRequestReasoningEffort": self.raw_request_reasoning_effort,
            "requestReasoningEffortPointer": self.request_reasoning_effort_pointer,
            "requestKind": self.request_kind,
            "interceptExemptReason": self.intercept_exempt_reason,
            "responseClassification": self.response_classification,
            "classificationSkippedReason": self.classification_skipped_reason,
            "hasFinalAnswer": self.has_final_answer,
            "hasCommentary": self.has_commentary,
            "commentaryObserved": self.commentary_observed,
            "hasToolCall": self.has_tool_call,
            "hasReasoningItem": self.has_reasoning_item,
            "hasOutputText": self.has_output_text,
            "finalAnswerOnly": self.final_answer_only,
        })
    }
}

pub(super) fn special_settings_snapshot(
    shared: &Arc<Mutex<Vec<serde_json::Value>>>,
) -> Vec<serde_json::Value> {
    shared
        .lock()
        .ok()
        .map(|settings| settings.clone())
        .unwrap_or_default()
}

pub(super) fn push_special_setting(
    shared: &Arc<Mutex<Vec<serde_json::Value>>>,
    sample: &CodexReasoningFeatureSample,
) {
    response_fixer::push_special_setting(shared, sample.to_special_setting());
}

pub(super) fn build_complete_sample(
    cli_key: &str,
    rule_mode: CodexReasoningGuardRuleMode,
    request_headers: Option<&HeaderMap>,
    request_json: Option<&serde_json::Value>,
    special_settings: &[serde_json::Value],
    response_json: &serde_json::Value,
) -> Option<CodexReasoningFeatureSample> {
    if cli_key != "codex" {
        return None;
    }

    let request_kind = detect_request_kind(request_headers, request_json);
    let effort = extract_request_effort_from_special_settings(special_settings);
    let token = extract_reasoning_tokens(response_json);
    let structure = classify_response_structure(response_json);

    Some(CodexReasoningFeatureSample {
        rule_mode,
        reasoning_tokens: token.map(|token| token.reasoning_tokens),
        reasoning_tokens_pointer: token.map(|token| token.pointer),
        request_reasoning_effort: effort.as_ref().and_then(|effort| effort.effort.clone()),
        raw_request_reasoning_effort: effort.as_ref().map(|effort| effort.raw_effort.clone()),
        request_reasoning_effort_pointer: effort.map(|effort| effort.pointer),
        request_kind: request_kind.request_kind,
        intercept_exempt_reason: resolve_compaction_exemption(
            request_kind.request_kind,
            token.as_ref().map(|token| token.reasoning_tokens),
        ),
        response_classification: RESPONSE_CLASSIFICATION_COMPLETE,
        classification_skipped_reason: None,
        has_final_answer: Some(structure.has_final_answer),
        has_commentary: Some(structure.has_commentary),
        commentary_observed: Some(structure.commentary_observed),
        has_tool_call: Some(structure.has_tool_call),
        has_reasoning_item: Some(structure.has_reasoning_item),
        has_output_text: Some(structure.has_output_text),
        final_answer_only: Some(structure.final_answer_only()),
    })
}

pub(super) fn build_request_only_sample(
    cli_key: &str,
    rule_mode: CodexReasoningGuardRuleMode,
    request_headers: Option<&HeaderMap>,
    request_json: Option<&serde_json::Value>,
    special_settings: &[serde_json::Value],
    classification_skipped_reason: &'static str,
) -> Option<CodexReasoningFeatureSample> {
    if cli_key != "codex" {
        return None;
    }

    let request_kind = detect_request_kind(request_headers, request_json);
    let effort = extract_request_effort_from_special_settings(special_settings);
    Some(CodexReasoningFeatureSample {
        rule_mode,
        reasoning_tokens: None,
        reasoning_tokens_pointer: None,
        request_reasoning_effort: effort.as_ref().and_then(|effort| effort.effort.clone()),
        raw_request_reasoning_effort: effort.as_ref().map(|effort| effort.raw_effort.clone()),
        request_reasoning_effort_pointer: effort.map(|effort| effort.pointer),
        request_kind: request_kind.request_kind,
        intercept_exempt_reason: resolve_compaction_exemption(request_kind.request_kind, None),
        response_classification: RESPONSE_CLASSIFICATION_REQUEST_ONLY,
        classification_skipped_reason: Some(classification_skipped_reason),
        has_final_answer: None,
        has_commentary: None,
        commentary_observed: None,
        has_tool_call: None,
        has_reasoning_item: None,
        has_output_text: None,
        final_answer_only: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExtractedReasoningTokens {
    pub(super) reasoning_tokens: i64,
    pub(super) pointer: &'static str,
}

pub(super) const REASONING_TOKEN_POINTERS: &[&str] = &[
    "/usage/output_tokens_details/reasoning_tokens",
    "/usage/completion_tokens_details/reasoning_tokens",
    "/response/usage/output_tokens_details/reasoning_tokens",
    "/response/usage/completion_tokens_details/reasoning_tokens",
];

pub(super) fn extract_reasoning_tokens(
    value: &serde_json::Value,
) -> Option<ExtractedReasoningTokens> {
    for pointer in REASONING_TOKEN_POINTERS {
        let Some(raw) = value.pointer(pointer) else {
            continue;
        };
        let reasoning_tokens = match raw {
            serde_json::Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|v| i64::try_from(v).ok())),
            _ => None,
        };
        if let Some(reasoning_tokens) = reasoning_tokens {
            return Some(ExtractedReasoningTokens {
                reasoning_tokens,
                pointer,
            });
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExtractedRequestEffort {
    pub(super) effort: Option<String>,
    pub(super) raw_effort: String,
    pub(super) pointer: String,
}

pub(super) fn extract_request_effort_from_special_settings(
    special_settings: &[serde_json::Value],
) -> Option<ExtractedRequestEffort> {
    special_settings.iter().rev().find_map(|setting| {
        if setting.get("type").and_then(serde_json::Value::as_str) != Some("codex_reasoning_effort")
        {
            return None;
        }
        let raw_effort = setting
            .get("rawEffort")
            .or_else(|| setting.get("effort"))
            .and_then(serde_json::Value::as_str)?
            .trim()
            .to_string();
        let effort = setting
            .get("effort")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let pointer = setting
            .get("pointer")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        Some(ExtractedRequestEffort {
            effort,
            raw_effort,
            pointer,
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct RequestKindDetection {
    pub(super) request_kind: Option<&'static str>,
}

pub(super) fn detect_request_kind(
    headers: Option<&HeaderMap>,
    request_json: Option<&serde_json::Value>,
) -> RequestKindDetection {
    if headers
        .map(headers_contain_compaction_marker)
        .unwrap_or(false)
        || request_json
            .map(request_json_contains_compaction_marker)
            .unwrap_or(false)
    {
        return RequestKindDetection {
            request_kind: Some(REQUEST_KIND_CONTEXT_COMPACTION),
        };
    }
    RequestKindDetection::default()
}

/// Mirrors the upstream fix: only an explicit context_compaction request kind
/// whose response carried reasoning_tokens == 0 is exempt from the active
/// guard. Non-zero (516/1034/1552) or null reasoning still hits the active rule.
pub(super) fn resolve_compaction_exemption(
    request_kind: Option<&'static str>,
    reasoning_tokens: Option<i64>,
) -> Option<&'static str> {
    if request_kind == Some(REQUEST_KIND_CONTEXT_COMPACTION) && reasoning_tokens == Some(0) {
        Some(EXEMPT_REASON_CONTEXT_COMPACTION)
    } else {
        None
    }
}

fn headers_contain_compaction_marker(headers: &HeaderMap) -> bool {
    headers.iter().any(|(name, value)| {
        let name = name.as_str().to_ascii_lowercase();
        let value = value
            .to_str()
            .ok()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        matches!(
            name.as_str(),
            "x-codex-request-kind" | "x-codex-purpose" | "x-codex-turn-metadata"
        ) && contains_compaction_marker(&value)
    })
}

fn request_json_contains_compaction_marker(value: &serde_json::Value) -> bool {
    for key in ["codex_request_kind", "request_kind", "purpose"] {
        if value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(contains_compaction_marker)
            .unwrap_or(false)
        {
            return true;
        }
    }
    value
        .get("metadata")
        .map(metadata_contains_compaction_marker)
        .unwrap_or(false)
}

fn metadata_contains_compaction_marker(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => contains_compaction_marker(value),
        serde_json::Value::Array(values) => values.iter().any(metadata_contains_compaction_marker),
        serde_json::Value::Object(map) => map.values().any(metadata_contains_compaction_marker),
        _ => false,
    }
}

fn contains_compaction_marker(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase().replace('-', "_");
    value.contains("context_compaction") || value.contains("remote_compaction")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ResponseStructure {
    pub(super) has_final_answer: bool,
    pub(super) has_commentary: bool,
    pub(super) commentary_observed: bool,
    pub(super) has_tool_call: bool,
    pub(super) has_reasoning_item: bool,
    pub(super) has_output_text: bool,
}

impl ResponseStructure {
    fn final_answer_only(&self) -> bool {
        self.has_final_answer
            && !self.commentary_observed
            && !self.has_tool_call
            && !self.has_reasoning_item
    }
}

pub(super) fn classify_response_structure(value: &serde_json::Value) -> ResponseStructure {
    let mut structure = ResponseStructure::default();
    visit_response_value(value, &mut structure);
    structure.has_commentary = structure.commentary_observed;
    structure
}

fn visit_response_value(value: &serde_json::Value, structure: &mut ResponseStructure) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                visit_response_value(value, structure);
            }
        }
        serde_json::Value::Object(map) => {
            let type_value = map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_ascii_lowercase());
            let role_value = map
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_ascii_lowercase());
            let channel_value = map
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .map(|value| value.to_ascii_lowercase());

            if channel_value.as_deref() == Some("commentary")
                || role_value.as_deref() == Some("commentary")
            {
                structure.commentary_observed = true;
            }

            if let Some(type_value) = type_value.as_deref() {
                if type_value == "reasoning" || type_value.contains("reasoning") {
                    structure.has_reasoning_item = true;
                }
                if type_value.contains("tool")
                    || type_value == "function_call"
                    || type_value.contains("function_call")
                {
                    structure.has_tool_call = true;
                }
                if type_value == "output_text" || type_value == "text" {
                    structure.has_output_text = true;
                    structure.has_final_answer = true;
                }
                if type_value.contains("commentary") {
                    structure.commentary_observed = true;
                }
            }

            if map.contains_key("tool_calls")
                || map.contains_key("tool_call_id")
                || map.contains_key("function_call")
            {
                structure.has_tool_call = true;
            }
            if map.contains_key("output_text") || map.contains_key("final_answer") {
                structure.has_output_text = true;
                structure.has_final_answer = true;
            }
            if role_value.as_deref() == Some("assistant") && map.contains_key("content") {
                structure.has_final_answer = true;
            }

            for value in map.values() {
                visit_response_value(value, structure);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    #[test]
    fn extracts_reasoning_tokens_from_supported_shapes() {
        for (pointer, value) in [
            (
                "/usage/output_tokens_details/reasoning_tokens",
                json!({"usage":{"output_tokens_details":{"reasoning_tokens":516}}}),
            ),
            (
                "/usage/completion_tokens_details/reasoning_tokens",
                json!({"usage":{"completion_tokens_details":{"reasoning_tokens":1034}}}),
            ),
            (
                "/response/usage/output_tokens_details/reasoning_tokens",
                json!({"response":{"usage":{"output_tokens_details":{"reasoning_tokens":1552}}}}),
            ),
            (
                "/response/usage/completion_tokens_details/reasoning_tokens",
                json!({"response":{"usage":{"completion_tokens_details":{"reasoning_tokens":1}}}}),
            ),
        ] {
            let extracted = extract_reasoning_tokens(&value).expect("reasoning tokens");
            assert_eq!(extracted.pointer, pointer);
        }
    }

    #[test]
    fn classifies_final_answer_only_response() {
        let value = json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "redacted"}]
            }]
        });

        let structure = classify_response_structure(&value);
        assert!(structure.has_final_answer);
        assert!(structure.has_output_text);
        assert!(structure.final_answer_only());
    }

    #[test]
    fn final_answer_only_rejects_commentary_tool_and_reasoning() {
        for value in [
            json!({"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x"}]},{"type":"message","channel":"commentary","content":[{"type":"output_text","text":"y"}]}]}),
            json!({"output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x"}]},{"type":"function_call","name":"tool"}]}),
            json!({"output":[{"type":"reasoning","summary":[]},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x"}]}]}),
        ] {
            let structure = classify_response_structure(&value);
            assert!(!structure.final_answer_only());
        }
    }

    #[test]
    fn detects_compaction_only_from_explicit_request_kind_signals() {
        // The remote_compaction_v2 beta header is just a Codex Desktop beta
        // feature marker and must NOT mark a normal turn as context compaction.
        let mut beta_headers = HeaderMap::new();
        beta_headers.insert(
            "x-codex-beta-features",
            HeaderValue::from_static("remote_compaction_v2"),
        );
        assert_eq!(
            detect_request_kind(Some(&beta_headers), None).request_kind,
            None
        );

        // Explicit request-kind headers do mark compaction.
        let mut kind_headers = HeaderMap::new();
        kind_headers.insert(
            "x-codex-request-kind",
            HeaderValue::from_static("context_compaction"),
        );
        assert_eq!(
            detect_request_kind(Some(&kind_headers), None).request_kind,
            Some(REQUEST_KIND_CONTEXT_COMPACTION)
        );

        // Body metadata purpose also marks compaction.
        let body = json!({"metadata":{"purpose":"context_compaction"}});
        assert_eq!(
            detect_request_kind(None, Some(&body)).request_kind,
            Some(REQUEST_KIND_CONTEXT_COMPACTION)
        );
    }

    #[test]
    fn resolve_compaction_exemption_only_for_zero_reasoning_tokens() {
        assert_eq!(
            resolve_compaction_exemption(Some(REQUEST_KIND_CONTEXT_COMPACTION), Some(0)),
            Some(EXEMPT_REASON_CONTEXT_COMPACTION)
        );
        assert_eq!(
            resolve_compaction_exemption(Some(REQUEST_KIND_CONTEXT_COMPACTION), Some(516)),
            None
        );
        assert_eq!(
            resolve_compaction_exemption(Some(REQUEST_KIND_CONTEXT_COMPACTION), None),
            None
        );
        assert_eq!(resolve_compaction_exemption(None, Some(0)), None);
    }

    #[test]
    fn reads_existing_codex_reasoning_effort_special_setting() {
        let settings = vec![json!({
            "type": "codex_reasoning_effort",
            "effort": "high",
            "rawEffort": "High",
            "pointer": "/reasoning/effort"
        })];

        let extracted = extract_request_effort_from_special_settings(&settings).expect("effort");
        assert_eq!(extracted.effort.as_deref(), Some("high"));
        assert_eq!(extracted.raw_effort, "High");
        assert_eq!(extracted.pointer, "/reasoning/effort");
    }

    #[test]
    fn request_only_sample_does_not_include_response_structure_fields() {
        let sample = build_request_only_sample(
            "codex",
            CodexReasoningGuardRuleMode::ReasoningTokens,
            None,
            Some(&json!({"request_kind":"context_compaction"})),
            &[],
            SKIPPED_GUARD_DISABLED_STREAM_NOT_BUFFERED,
        )
        .expect("sample");

        assert_eq!(
            sample.response_classification,
            RESPONSE_CLASSIFICATION_REQUEST_ONLY
        );
        assert_eq!(
            sample.classification_skipped_reason,
            Some(SKIPPED_GUARD_DISABLED_STREAM_NOT_BUFFERED)
        );
        assert_eq!(sample.final_answer_only, None);
        assert_eq!(sample.request_kind, Some(REQUEST_KIND_CONTEXT_COMPACTION));
        // request-only samples cannot confirm reasoning_tokens == 0, so they
        // are never marked exempt.
        assert_eq!(sample.intercept_exempt_reason, None);
    }
}
