//! Usage: Native Codex Responses continuation repair helpers.

use axum::body::Bytes;
use serde_json::{json, Map, Value};

pub(super) const ENCRYPTED_REASONING_INCLUDE: &str = "reasoning.encrypted_content";
pub(super) const CONTINUATION_MARKER_TEXT: &str = "Continue thinking. Preserve any prior assistant-visible answer verbatim as a prefix. If the prior answer is already complete, repeat it exactly; do not rewrite, summarize, or produce an alternative wording.";

pub(super) struct IncludeMergeInput<'a> {
    pub(super) repair_enabled: bool,
    pub(super) cli_key: &'a str,
    pub(super) upstream_forwarded_path: &'a str,
    pub(super) body: &'a [u8],
    pub(super) active_bridge_type: Option<&'a str>,
    pub(super) oauth_adapter_present: bool,
    pub(super) gemini_oauth_response_mode_present: bool,
    pub(super) use_codex_chatgpt_backend: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IncludeMergeOutcome {
    pub(super) eligible: bool,
    pub(super) changed: bool,
    pub(super) body: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncludeMergeStatus {
    Changed,
    AlreadyPresent,
    Unsupported,
}

pub(super) fn ensure_encrypted_reasoning_include(
    input: IncludeMergeInput<'_>,
) -> IncludeMergeOutcome {
    if !input.repair_enabled {
        return unchanged(input.body, false);
    }
    let forwarded_path = input.upstream_forwarded_path.trim_end_matches('/');
    if input.cli_key != "codex"
        || !matches!(forwarded_path, "/v1/responses" | "/responses")
        || input.active_bridge_type.is_some()
        || input.oauth_adapter_present
        || input.gemini_oauth_response_mode_present
        || input.use_codex_chatgpt_backend
    {
        return unchanged(input.body, false);
    }

    let Ok(mut root) = serde_json::from_slice::<Value>(input.body) else {
        return unchanged(input.body, false);
    };
    if !root.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        return unchanged(input.body, false);
    }

    let Some(object) = root.as_object_mut() else {
        return unchanged(input.body, false);
    };
    match merge_include_value(object.entry("include").or_insert(Value::Array(Vec::new()))) {
        IncludeMergeStatus::AlreadyPresent => return unchanged(input.body, true),
        IncludeMergeStatus::Unsupported => return unchanged(input.body, false),
        IncludeMergeStatus::Changed => {}
    }

    match serde_json::to_vec(&root) {
        Ok(bytes) => IncludeMergeOutcome {
            eligible: true,
            changed: true,
            body: Bytes::from(bytes),
        },
        Err(_) => unchanged(input.body, true),
    }
}

pub(super) fn is_truncation_continuation_pattern(tokens: Option<i64>) -> bool {
    let Some(tokens) = tokens else {
        return false;
    };
    tokens >= 516 && tokens.checked_add(2).is_some_and(|value| value % 518 == 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuationReplayPolicy {
    StableEncryptedReplay,
}

impl ContinuationReplayPolicy {
    pub(super) fn from_post_match_strategy(
        strategy: crate::settings::CodexReasoningGuardPostMatchStrategy,
    ) -> Option<Self> {
        match strategy {
            crate::settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepair => {
                Some(Self::StableEncryptedReplay)
            }
            crate::settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider => None,
        }
    }

    pub(super) fn requires_encrypted_reasoning(self) -> bool {
        true
    }

    pub(super) fn next_replay_tail(
        self,
        stable_tail: &mut Vec<Value>,
        current: &Value,
    ) -> Vec<Value> {
        match self {
            Self::StableEncryptedReplay => {
                stable_tail.extend(reasoning_items(current));
                stable_tail.push(commentary_marker_item());
                stable_tail.clone()
            }
        }
    }
}

pub(super) fn request_reasoning_enabled(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|root| root.get("reasoning").cloned())
        .is_none_or(|reasoning| reasoning != Value::Bool(false))
}

pub(super) fn continuation_input_items(base_body: &[u8], replay_tail: &[Value]) -> Vec<Value> {
    let mut input_items = serde_json::from_slice::<Value>(base_body)
        .ok()
        .and_then(|root| root.get("input").cloned())
        .map(|input| match input {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            other => vec![other],
        })
        .unwrap_or_default();
    input_items.extend(replay_tail.iter().cloned());
    input_items
}

pub(super) fn build_continuation_payload(
    base_body: &[u8],
    replay_tail: &[Value],
) -> Result<Bytes, String> {
    let mut root = serde_json::from_slice::<Value>(base_body)
        .map_err(|err| format!("invalid continuation base request json: {err}"))?;
    let input_items = continuation_input_items(base_body, replay_tail);
    let object = root
        .as_object_mut()
        .ok_or_else(|| "continuation base request is not a JSON object".to_string())?;

    object.insert("stream".to_string(), Value::Bool(true));
    object.insert("input".to_string(), Value::Array(input_items));
    object.remove("previous_response_id");
    merge_include_value(
        object
            .entry("include".to_string())
            .or_insert(Value::Array(Vec::new())),
    );

    serde_json::to_vec(&root)
        .map(Bytes::from)
        .map_err(|err| format!("failed to encode continuation request json: {err}"))
}

pub(super) fn reasoning_items(response: &Value) -> Vec<Value> {
    response
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn latest_reasoning_has_encrypted_content(response: &Value) -> bool {
    reasoning_items(response)
        .last()
        .and_then(|item| item.get("encrypted_content"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn commentary_marker_item() -> Value {
    serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": CONTINUATION_MARKER_TEXT}],
        "phase": "commentary",
    })
}

pub(super) fn output_tokens(response: &Value) -> u64 {
    response
        .pointer("/usage/output_tokens")
        .or_else(|| response.pointer("/response/usage/output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuationRepairRoundKind {
    Initial,
    Continuation,
}

impl ContinuationRepairRoundKind {
    fn as_str(self) -> &'static str {
        match self {
            ContinuationRepairRoundKind::Initial => "initial",
            ContinuationRepairRoundKind::Continuation => "continuation",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ContinuationRepairRound {
    pub(super) kind: ContinuationRepairRoundKind,
    pub(super) raw_sse: Bytes,
    pub(super) aggregated: Value,
    pub(super) duration_ms: Option<u128>,
}

impl ContinuationRepairRound {
    pub(super) fn new(
        kind: ContinuationRepairRoundKind,
        raw_sse: Bytes,
        aggregated: Value,
        duration_ms: Option<u128>,
    ) -> Self {
        Self {
            kind,
            raw_sse,
            aggregated,
            duration_ms,
        }
    }
}

pub(super) fn reconstruction_round_trace(rounds: &[ContinuationRepairRound]) -> Vec<Value> {
    rounds
        .iter()
        .enumerate()
        .map(|(index, round)| {
            let usage =
                crate::usage::parse_usage_from_json_or_sse_bytes("codex", round.raw_sse.as_ref());
            let reasoning =
                super::codex_reasoning_features::extract_reasoning_tokens(&round.aggregated);
            json!({
                "index": index,
                "kind": round.kind.as_str(),
                "responseId": round.aggregated.get("id").and_then(Value::as_str),
                "terminalKind": "completed",
                "status": round.aggregated.get("status").and_then(Value::as_str),
                "reasoningTokens": reasoning.map(|value| value.reasoning_tokens),
                "reasoningTokensPointer": reasoning.map(|value| value.pointer),
                "outputTokens": output_tokens(&round.aggregated),
                "usage": usage.as_ref().and_then(|usage| {
                    serde_json::from_str::<Value>(&usage.usage_json).ok()
                }),
                "byteCount": round.raw_sse.len(),
                "durationMs": round.duration_ms.map(|value| value.min(u128::from(u64::MAX)) as u64),
            })
        })
        .collect()
}

pub(super) fn fold_responses_to_sse(responses: &[Value]) -> Result<Bytes, String> {
    let Some(last) = responses.last() else {
        return Err("cannot fold empty continuation response list".to_string());
    };
    let mut folded = strip_encrypted_content(last.clone());
    let output_items = response_output_items(&folded);

    {
        let object = folded
            .as_object_mut()
            .ok_or_else(|| "folded continuation response is not an object".to_string())?;
        object.insert("output".to_string(), Value::Array(output_items.clone()));
    }
    folded = strip_encrypted_content(folded);

    let mut created_response = folded.clone();
    if let Some(created) = created_response.as_object_mut() {
        created.insert(
            "status".to_string(),
            Value::String("in_progress".to_string()),
        );
        created.insert("output".to_string(), Value::Array(Vec::new()));
        created.remove("usage");
    }

    let mut raw = String::new();
    push_sse_event(
        &mut raw,
        "response.created",
        serde_json::json!({"type": "response.created", "response": created_response}),
    )?;
    for (index, item) in output_items.iter().enumerate() {
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            serde_json::json!({
                "type": "response.output_item.done",
                "output_index": index,
                "item": item,
            }),
        )?;
    }
    push_sse_event(
        &mut raw,
        "response.completed",
        serde_json::json!({"type": "response.completed", "response": folded}),
    )?;
    Ok(Bytes::from(raw))
}

fn unchanged(body: &[u8], eligible: bool) -> IncludeMergeOutcome {
    IncludeMergeOutcome {
        eligible,
        changed: false,
        body: Bytes::copy_from_slice(body),
    }
}

fn merge_include_value(include: &mut Value) -> IncludeMergeStatus {
    match include {
        Value::Array(items) => {
            if include_items_contain_encrypted_reasoning(items) {
                return IncludeMergeStatus::AlreadyPresent;
            }
            items.push(Value::String(ENCRYPTED_REASONING_INCLUDE.to_string()));
            IncludeMergeStatus::Changed
        }
        Value::String(existing) => {
            let existing = existing.trim();
            if existing == ENCRYPTED_REASONING_INCLUDE {
                return IncludeMergeStatus::AlreadyPresent;
            }

            let mut values = Vec::new();
            if !existing.is_empty() {
                values.push(Value::String(existing.to_string()));
            }
            values.push(Value::String(ENCRYPTED_REASONING_INCLUDE.to_string()));
            *include = Value::Array(values);
            IncludeMergeStatus::Changed
        }
        Value::Null => {
            *include = Value::Array(vec![Value::String(ENCRYPTED_REASONING_INCLUDE.to_string())]);
            IncludeMergeStatus::Changed
        }
        _ => IncludeMergeStatus::Unsupported,
    }
}

fn include_items_contain_encrypted_reasoning(items: &[Value]) -> bool {
    items
        .iter()
        .filter_map(Value::as_str)
        .any(|item| item == ENCRYPTED_REASONING_INCLUDE)
}

fn response_output_items(response: &Value) -> Vec<Value> {
    response
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| strip_encrypted_content(item.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn strip_encrypted_content(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.into_iter().map(strip_encrypted_content).collect())
        }
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter_map(|(key, value)| {
                    (key != "encrypted_content").then(|| (key, strip_encrypted_content(value)))
                })
                .collect(),
        ),
        other => other,
    }
}

fn summed_usage(responses: &[Value]) -> Option<Value> {
    let mut total = Value::Object(Map::new());
    let mut saw_usage = false;
    for response in responses {
        let Some(usage) = response
            .get("usage")
            .or_else(|| response.pointer("/response/usage"))
        else {
            continue;
        };
        saw_usage = true;
        merge_usage_value(&mut total, usage);
    }
    saw_usage.then_some(total)
}

pub(super) fn summed_provider_repair_usage(
    responses: &[Value],
) -> Option<crate::usage::UsageExtract> {
    let usage = strip_encrypted_content(summed_usage(responses)?);
    let body = serde_json::to_vec(&usage).ok()?;
    crate::usage::parse_usage_from_json_bytes(&body)
}

fn merge_usage_value(total: &mut Value, next: &Value) {
    match next {
        Value::Object(next_object) => {
            if !total.is_object() {
                *total = Value::Object(Map::new());
            }
            let Some(total_object) = total.as_object_mut() else {
                return;
            };
            for (key, value) in next_object {
                merge_usage_value(
                    total_object
                        .entry(key.clone())
                        .or_insert_with(|| zero_like(value)),
                    value,
                );
            }
        }
        Value::Number(next_number) => {
            let total_value = total.as_u64().unwrap_or(0);
            let next_value = next_number.as_u64().unwrap_or(0);
            *total = Value::Number(serde_json::Number::from(
                total_value.saturating_add(next_value),
            ));
        }
        value if total.is_null() => {
            *total = value.clone();
        }
        _ => {}
    }
}

fn zero_like(value: &Value) -> Value {
    match value {
        Value::Object(_) => Value::Object(Map::new()),
        Value::Number(_) => Value::Number(serde_json::Number::from(0)),
        _ => Value::Null,
    }
}

fn push_sse_event(raw: &mut String, event: &str, data: Value) -> Result<(), String> {
    let data = serde_json::to_string(&data)
        .map_err(|err| format!("failed to encode continuation SSE event: {err}"))?;
    raw.push_str("event: ");
    raw.push_str(event);
    raw.push('\n');
    raw.push_str("data: ");
    raw.push_str(&data);
    raw.push_str("\n\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merge_body(repair_enabled: bool, body: Value) -> IncludeMergeOutcome {
        let bytes = serde_json::to_vec(&body).unwrap();
        ensure_encrypted_reasoning_include(IncludeMergeInput {
            repair_enabled,
            cli_key: "codex",
            upstream_forwarded_path: "/v1/responses",
            body: &bytes,
            active_bridge_type: None,
            oauth_adapter_present: false,
            gemini_oauth_response_mode_present: false,
            use_codex_chatgpt_backend: false,
        })
    }

    fn merged_json(outcome: &IncludeMergeOutcome) -> Value {
        serde_json::from_slice(outcome.body.as_ref()).unwrap()
    }

    fn folded_output_items(responses: &[Value]) -> Vec<Value> {
        let raw = fold_responses_to_sse(responses).expect("fold");
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate folded");
        aggregated["output"]
            .as_array()
            .expect("output array")
            .clone()
    }

    fn merge_body_with_path(path: &str, body: Value) -> IncludeMergeOutcome {
        let bytes = serde_json::to_vec(&body).unwrap();
        ensure_encrypted_reasoning_include(IncludeMergeInput {
            repair_enabled: true,
            cli_key: "codex",
            upstream_forwarded_path: path,
            body: &bytes,
            active_bridge_type: None,
            oauth_adapter_present: false,
            gemini_oauth_response_mode_present: false,
            use_codex_chatgpt_backend: false,
        })
    }

    #[test]
    fn include_merge_disabled_returns_unchanged_body() {
        let body = json!({"model": "gpt-5.5", "stream": true});

        let outcome = merge_body(false, body.clone());

        assert!(!outcome.eligible);
        assert!(!outcome.changed);
        assert_eq!(merged_json(&outcome), body);
    }

    #[test]
    fn include_merge_enabled_adds_missing_include() {
        let outcome = merge_body(true, json!({"model": "gpt-5.5", "stream": true}));

        assert!(outcome.eligible);
        assert!(outcome.changed);
        assert_eq!(
            merged_json(&outcome).get("include").unwrap(),
            &json!([ENCRYPTED_REASONING_INCLUDE])
        );
    }

    #[test]
    fn include_merge_enabled_accepts_native_responses_trailing_slash_paths() {
        for path in ["/v1/responses/", "/responses/"] {
            let outcome = merge_body_with_path(path, json!({"model": "gpt-5.5", "stream": true}));

            assert!(outcome.eligible, "path should be eligible: {path}");
            assert!(outcome.changed, "path should merge include: {path}");
            assert_eq!(
                merged_json(&outcome).get("include").unwrap(),
                &json!([ENCRYPTED_REASONING_INCLUDE])
            );
        }
    }

    #[test]
    fn include_merge_enabled_preserves_existing_array_without_duplicates() {
        let body = json!({
            "model": "gpt-5.5",
            "stream": true,
            "include": ["foo", ENCRYPTED_REASONING_INCLUDE]
        });

        let outcome = merge_body(true, body.clone());

        assert!(outcome.eligible);
        assert!(!outcome.changed);
        assert_eq!(merged_json(&outcome), body);
    }

    #[test]
    fn include_merge_enabled_appends_to_existing_string() {
        let outcome = merge_body(
            true,
            json!({"model": "gpt-5.5", "stream": true, "include": "foo"}),
        );

        assert!(outcome.eligible);
        assert!(outcome.changed);
        assert_eq!(
            merged_json(&outcome).get("include").unwrap(),
            &json!(["foo", ENCRYPTED_REASONING_INCLUDE])
        );
    }

    #[test]
    fn include_merge_enabled_preserves_existing_encrypted_reasoning_string() {
        let body = json!({
            "model": "gpt-5.5",
            "stream": true,
            "include": ENCRYPTED_REASONING_INCLUDE
        });

        let outcome = merge_body(true, body.clone());

        assert!(outcome.eligible);
        assert!(!outcome.changed);
        assert_eq!(merged_json(&outcome), body);
    }

    #[test]
    fn include_merge_unsupported_existing_include_type_is_not_eligible() {
        let body = json!({
            "model": "gpt-5.5",
            "stream": true,
            "include": {"unexpected": true}
        });

        let outcome = merge_body(true, body.clone());

        assert!(!outcome.eligible);
        assert!(!outcome.changed);
        assert_eq!(merged_json(&outcome), body);
    }

    #[test]
    fn include_merge_excludes_non_native_or_translated_paths() {
        let body = serde_json::to_vec(&json!({"stream": true})).unwrap();
        for input in [
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "claude",
                upstream_forwarded_path: "/v1/responses",
                body: &body,
                active_bridge_type: None,
                oauth_adapter_present: false,
                gemini_oauth_response_mode_present: false,
                use_codex_chatgpt_backend: false,
            },
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "codex",
                upstream_forwarded_path: "/v1/chat/completions",
                body: &body,
                active_bridge_type: None,
                oauth_adapter_present: false,
                gemini_oauth_response_mode_present: false,
                use_codex_chatgpt_backend: false,
            },
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "codex",
                upstream_forwarded_path: "/v1/responses",
                body: &body,
                active_bridge_type: Some("cx2cc"),
                oauth_adapter_present: false,
                gemini_oauth_response_mode_present: false,
                use_codex_chatgpt_backend: false,
            },
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "codex",
                upstream_forwarded_path: "/v1/responses",
                body: &body,
                active_bridge_type: None,
                oauth_adapter_present: true,
                gemini_oauth_response_mode_present: false,
                use_codex_chatgpt_backend: false,
            },
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "codex",
                upstream_forwarded_path: "/v1/responses",
                body: &body,
                active_bridge_type: None,
                oauth_adapter_present: false,
                gemini_oauth_response_mode_present: true,
                use_codex_chatgpt_backend: false,
            },
            IncludeMergeInput {
                repair_enabled: true,
                cli_key: "codex",
                upstream_forwarded_path: "/v1/responses",
                body: &body,
                active_bridge_type: None,
                oauth_adapter_present: false,
                gemini_oauth_response_mode_present: false,
                use_codex_chatgpt_backend: true,
            },
        ] {
            let outcome = ensure_encrypted_reasoning_include(input);
            assert!(!outcome.eligible);
            assert!(!outcome.changed);
            assert_eq!(outcome.body, Bytes::copy_from_slice(&body));
        }
    }

    #[test]
    fn continuation_pattern_matches_518n_minus_two() {
        for tokens in [516, 1034, 1552] {
            assert!(is_truncation_continuation_pattern(Some(tokens)));
        }
        for tokens in [0, 300, 517, 1035] {
            assert!(!is_truncation_continuation_pattern(Some(tokens)));
        }
        assert!(!is_truncation_continuation_pattern(Some(i64::MAX)));
        assert!(!is_truncation_continuation_pattern(None));
    }

    #[test]
    fn continuation_payload_replays_input_reasoning_marker_and_drops_previous_response_id() {
        let base = serde_json::to_vec(&json!({
            "model": "gpt-5.5",
            "stream": true,
            "previous_response_id": "resp_old",
            "include": ["foo"],
            "input": [{"role": "user", "content": "hello"}],
            "reasoning": {"effort": "high"}
        }))
        .unwrap();
        let replay_tail = vec![
            json!({"id": "rs_1", "type": "reasoning", "encrypted_content": "enc"}),
            commentary_marker_item(),
        ];

        let body = build_continuation_payload(&base, &replay_tail).expect("payload");
        let value: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(value.get("previous_response_id"), None);
        assert_eq!(value["stream"], json!(true));
        assert_eq!(value["reasoning"], json!({"effort": "high"}));
        assert_eq!(
            value["include"],
            json!(["foo", ENCRYPTED_REASONING_INCLUDE])
        );
        let input = value["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[2]["phase"], "commentary");
    }

    #[test]
    fn folded_sse_uses_final_round_output_and_summed_usage() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "model": "gpt-cut",
            "output": [
                {"id": "rs_1", "type": "reasoning", "encrypted_content": "enc"},
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "tentative answer"}]},
                {"id": "call_first", "type": "function_call", "name": "lookup", "call_id": "call_1", "arguments": "{}"}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "model": "gpt-clean",
            "output": [
                {"id": "msg_final", "type": "message", "content": [{"type": "output_text", "text": "final answer"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let raw = fold_responses_to_sse(&[first, second]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate folded");
        let output = aggregated["output"].as_array().expect("output array");

        assert_eq!(text.matches("event: response.completed").count(), 1);
        assert!(text.contains("\"id\":\"resp_2\""));
        assert!(text.contains("\"model\":\"gpt-clean\""));
        assert!(!text.contains("resp_1"));
        assert!(!text.contains("gpt-cut"));
        assert!(!text.contains("tentative answer"));
        assert!(!text.contains("call_first"));
        assert!(!text.contains("\"type\":\"reasoning\""));
        assert!(!text.contains("encrypted_content"));
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["id"], "msg_final");
        assert!(text.contains("\"output_tokens\":3"));
        assert!(text.contains("\"reasoning_tokens\":2"));
    }

    #[test]
    fn folded_sse_discards_all_intermediate_rounds_in_multi_hop_repair() {
        let first = json!({
            "id": "resp_cut_1",
            "status": "completed",
            "model": "gpt-cut-516",
            "output": [
                {"id": "rs_1", "type": "reasoning", "encrypted_content": "enc_1"},
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "multi-hop-a"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_cut_2",
            "status": "completed",
            "model": "gpt-cut-1034",
            "output": [
                {"id": "rs_2", "type": "reasoning", "encrypted_content": "enc_2"},
                {"id": "msg_second", "type": "message", "content": [{"type": "output_text", "text": "multi-hop-b"}]}
            ],
            "usage": {"output_tokens": 6, "output_tokens_details": {"reasoning_tokens": 1034}}
        });
        let third = json!({
            "id": "resp_clean_3",
            "status": "completed",
            "model": "gpt-clean-128",
            "output": [
                {"id": "msg_final", "type": "message", "content": [{"type": "output_text", "text": "multi-hop-clean"}]}
            ],
            "usage": {"output_tokens": 4, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let raw = fold_responses_to_sse(&[first, second, third]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();

        assert!(text.contains("resp_clean_3"));
        assert!(text.contains("gpt-clean-128"));
        assert!(text.contains("multi-hop-clean"));
        assert!(!text.contains("resp_cut_1"));
        assert!(!text.contains("resp_cut_2"));
        assert!(!text.contains("gpt-cut-516"));
        assert!(!text.contains("gpt-cut-1034"));
        assert!(!text.contains("multi-hop-a"));
        assert!(!text.contains("multi-hop-b"));
        assert!(!text.contains("enc_1"));
        assert!(!text.contains("enc_2"));
        assert!(text.contains("\"output_tokens\":4"));
        assert!(text.contains("\"reasoning_tokens\":2"));
    }

    #[test]
    fn folded_sse_strips_encrypted_content_from_final_round_recursively() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [{"id": "rs_1", "type": "reasoning", "encrypted_content": "enc_1"}],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {
                    "id": "rs_final",
                    "type": "reasoning",
                    "encrypted_content": "enc_final",
                    "summary": [
                        {"type": "summary_text", "text": "safe"},
                        {"type": "summary_text", "encrypted_content": "nested_secret", "text": "also safe"}
                    ]
                },
                {"id": "msg_final", "type": "message", "content": [{"type": "output_text", "text": "ok"}]}
            ],
            "usage": {
                "output_tokens": 3,
                "output_tokens_details": {"reasoning_tokens": 2},
                "debug": {"encrypted_content": "usage_secret", "note": "kept"}
            }
        });

        let raw = fold_responses_to_sse(&[first, second]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate folded");
        let output = aggregated["output"].as_array().expect("output array");
        let reasoning = output
            .iter()
            .find(|item| item["id"] == "rs_final")
            .expect("final reasoning item");

        assert!(!text.contains("encrypted_content"));
        assert!(!text.contains("enc_final"));
        assert!(!text.contains("nested_secret"));
        assert!(!text.contains("usage_secret"));
        assert_eq!(reasoning["summary"][0]["text"], "safe");
        assert_eq!(reasoning["summary"][1]["text"], "also safe");
        assert_eq!(reasoning.get("encrypted_content"), None);
        assert_eq!(reasoning["summary"][1].get("encrypted_content"), None);
        assert_eq!(
            aggregated
                .pointer("/usage/debug/note")
                .and_then(Value::as_str),
            Some("kept")
        );
        assert_eq!(aggregated.pointer("/usage/debug/encrypted_content"), None);
    }

    #[test]
    fn summed_provider_repair_usage_keeps_aggregate_accounting_separate_from_client_fold() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [{"id": "rs_1", "type": "reasoning", "encrypted_content": "enc_1"}],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 10,
                "total_tokens": 110,
                "output_tokens_details": {"reasoning_tokens": 516},
                "input_tokens_details": {"cached_tokens": 20}
            }
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_final", "type": "message", "content": [{"type": "output_text", "text": "ok"}]}
            ],
            "usage": {
                "input_tokens": 7,
                "output_tokens": 3,
                "total_tokens": 10,
                "output_tokens_details": {"reasoning_tokens": 2},
                "input_tokens_details": {"cached_tokens": 1},
                "debug": {"encrypted_content": "usage_secret"}
            }
        });

        let raw = fold_responses_to_sse(&[first.clone(), second.clone()]).expect("fold");
        let client_usage = crate::usage::parse_usage_from_json_or_sse_bytes("codex", raw.as_ref())
            .expect("client folded usage");
        let provider_usage =
            summed_provider_repair_usage(&[first, second]).expect("provider repair usage");

        assert_eq!(client_usage.metrics.input_tokens, Some(7));
        assert_eq!(client_usage.metrics.output_tokens, Some(3));
        assert_eq!(client_usage.metrics.total_tokens, Some(10));
        assert_eq!(client_usage.metrics.reasoning_tokens, Some(2));
        assert_eq!(client_usage.metrics.cache_read_input_tokens, Some(1));

        assert_eq!(provider_usage.metrics.input_tokens, Some(107));
        assert_eq!(provider_usage.metrics.output_tokens, Some(13));
        assert_eq!(provider_usage.metrics.total_tokens, Some(120));
        assert_eq!(provider_usage.metrics.reasoning_tokens, Some(518));
        assert_eq!(provider_usage.metrics.cache_read_input_tokens, Some(21));
        assert!(!provider_usage.usage_json.contains("encrypted_content"));
        assert!(!provider_usage.usage_json.contains("usage_secret"));
    }

    #[test]
    fn folded_sse_preserves_final_round_mixed_items_without_prior_leakage() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_mixed", "type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "partial visible answer"},
                    {"type": "refusal", "refusal": "hidden refusal branch"}
                ]},
                {"id": "refusal_item", "type": "refusal", "refusal": "visible refusal item"}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_final", "type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "final visible answer"}
                ]},
                {"id": "refusal_final", "type": "refusal", "refusal": "final refusal item"}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);

        assert!(!output.iter().any(|item| item["id"] == "msg_mixed"));
        assert!(!output.iter().any(|item| item["id"] == "refusal_item"));
        assert!(output.iter().any(|item| item["id"] == "msg_final"));
        assert!(output.iter().any(|item| item["id"] == "refusal_final"));
    }
}
