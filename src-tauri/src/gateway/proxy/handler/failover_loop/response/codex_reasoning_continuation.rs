//! Usage: Native Codex Responses continuation repair helpers.

use axum::body::Bytes;
use serde_json::{Map, Value};

pub(super) const ENCRYPTED_REASONING_INCLUDE: &str = "reasoning.encrypted_content";
pub(super) const CONTINUATION_MARKER_TEXT: &str = "Continue thinking...";

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

pub(super) fn fold_responses_to_sse(responses: &[Value]) -> Result<Bytes, String> {
    let Some(last) = responses.last() else {
        return Err("cannot fold empty continuation response list".to_string());
    };
    let mut folded = last.clone();
    let output_items = merged_output_items(responses);

    {
        let object = folded
            .as_object_mut()
            .ok_or_else(|| "folded continuation response is not an object".to_string())?;
        object.insert("output".to_string(), Value::Array(output_items.clone()));
        if let Some(usage) = summed_usage(responses) {
            object.insert("usage".to_string(), usage);
        }
    }

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

fn merged_output_items(responses: &[Value]) -> Vec<Value> {
    let mut output = Vec::new();
    for response in responses {
        let Some(items) = response.get("output").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            upsert_output_item(&mut output, item.clone());
        }
    }
    output
}

fn upsert_output_item(output: &mut Vec<Value>, item: Value) {
    let item_id = item.get("id").and_then(Value::as_str);
    if let Some(item_id) = item_id {
        if let Some(existing) = output
            .iter_mut()
            .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(item_id))
        {
            *existing = item;
            return;
        }
    }
    if let Some(item_text) = visible_message_output_text(&item) {
        if let Some((index, relationship)) = output
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                visible_message_output_text(candidate).map(|candidate_text| {
                    (
                        index,
                        message_text_relationship(candidate_text.as_str(), item_text.as_str()),
                    )
                })
            })
            .find(|(_, relationship)| *relationship != MessageTextRelationship::Distinct)
        {
            match relationship {
                MessageTextRelationship::ReplaceExisting => output[index] = item,
                MessageTextRelationship::KeepExisting => {}
                MessageTextRelationship::Distinct => unreachable!(),
            }
            return;
        }
    }
    output.push(item);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageTextRelationship {
    Distinct,
    KeepExisting,
    ReplaceExisting,
}

fn message_text_relationship(existing: &str, next: &str) -> MessageTextRelationship {
    let existing = normalize_visible_message_text(existing);
    let next = normalize_visible_message_text(next);
    if existing.is_empty() || next.is_empty() {
        return MessageTextRelationship::Distinct;
    }
    if existing == next || next.starts_with(&existing) {
        return MessageTextRelationship::ReplaceExisting;
    }
    if existing.starts_with(&next) {
        return MessageTextRelationship::KeepExisting;
    }
    MessageTextRelationship::Distinct
}

fn normalize_visible_message_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn visible_message_output_text(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if item
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "assistant")
    {
        return None;
    }
    if item.get("phase").and_then(Value::as_str) == Some("commentary") {
        return None;
    }
    let text = item
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    (!text.is_empty()).then_some(text)
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
    fn folded_sse_contains_single_completed_response_with_merged_output_and_usage() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [{"id": "rs_1", "type": "reasoning", "encrypted_content": "enc"}],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [{"id": "msg_1", "type": "message", "content": [{"type": "output_text", "text": "ok"}]}],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let raw = fold_responses_to_sse(&[first, second]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();

        assert_eq!(text.matches("event: response.completed").count(), 1);
        assert!(text.contains("\"id\":\"rs_1\""));
        assert!(text.contains("\"id\":\"msg_1\""));
        assert!(text.contains("\"output_tokens\":13"));
        assert!(text.contains("\"reasoning_tokens\":518"));
    }

    #[test]
    fn folded_sse_deduplicates_repeated_visible_message_text_across_rounds() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "rs_1", "type": "reasoning", "encrypted_content": "enc"},
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "收到。现在先不创建新 worktree，也不改代码。"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_second", "type": "message", "content": [{"type": "output_text", "text": "收到。现在先不创建新 worktree，也不改代码。"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let raw = fold_responses_to_sse(&[first, second]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate folded");
        let output = aggregated["output"].as_array().expect("output array");

        assert!(!text.contains("\"id\":\"msg_first\""));
        assert!(text.contains("\"id\":\"msg_second\""));
        assert_eq!(
            output
                .iter()
                .filter(|item| item["type"] == "message")
                .count(),
            1
        );
        assert!(text.contains("\"output_tokens\":13"));
        assert!(text.contains("\"reasoning_tokens\":518"));
    }

    #[test]
    fn folded_sse_keeps_one_extended_visible_message_text_across_rounds() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "答案是 21。"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_second", "type": "message", "content": [{"type": "output_text", "text": "答案是 21。最少取出 21 个糖果。"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let raw = fold_responses_to_sse(&[first, second]).expect("fold");
        let text = std::str::from_utf8(raw.as_ref()).unwrap();
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate folded");
        let output = aggregated["output"].as_array().expect("output array");
        let message = output
            .iter()
            .find(|item| item["type"] == "message")
            .expect("message item");

        assert!(!text.contains("\"id\":\"msg_first\""));
        assert!(text.contains("\"id\":\"msg_second\""));
        assert_eq!(
            message.pointer("/content/0/text").and_then(Value::as_str),
            Some("答案是 21。最少取出 21 个糖果。")
        );
    }

    #[test]
    fn folded_sse_preserves_distinct_visible_message_text_across_rounds() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "答案是 21。"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_second", "type": "message", "content": [{"type": "output_text", "text": "还需要说明最坏情况的抽取策略。"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);
        let messages: Vec<&Value> = output
            .iter()
            .filter(|item| item["type"] == "message")
            .collect();

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|item| item["id"] == "msg_first"));
        assert!(messages.iter().any(|item| item["id"] == "msg_second"));
    }

    #[test]
    fn folded_sse_preserves_quoted_visible_message_text_across_rounds() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_first", "type": "message", "content": [{"type": "output_text", "text": "答案是 21。"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_second", "type": "message", "content": [{"type": "output_text", "text": "你前面说“答案是 21。”，这里还要补充最坏情况证明。"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);
        let messages: Vec<&Value> = output
            .iter()
            .filter(|item| item["type"] == "message")
            .collect();

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|item| item["id"] == "msg_first"));
        assert!(messages.iter().any(|item| item["id"] == "msg_second"));
    }

    #[test]
    fn folded_sse_preserves_non_visible_items_and_commentary_markers() {
        let commentary = commentary_marker_item();
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "rs_first", "type": "reasoning", "summary": [{"type": "summary_text", "text": "same"}], "encrypted_content": "enc_1"},
                {"id": "call_first", "type": "function_call", "name": "lookup", "call_id": "call_1", "arguments": "{}"},
                commentary
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "rs_second", "type": "reasoning", "summary": [{"type": "summary_text", "text": "same"}], "encrypted_content": "enc_2"},
                {"id": "call_second", "type": "function_call", "name": "lookup", "call_id": "call_2", "arguments": "{}"},
                {"id": "msg_visible", "type": "message", "content": [{"type": "output_text", "text": CONTINUATION_MARKER_TEXT}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);

        assert_eq!(
            output
                .iter()
                .filter(|item| item["type"] == "reasoning")
                .count(),
            2
        );
        assert_eq!(
            output
                .iter()
                .filter(|item| item["type"] == "function_call")
                .count(),
            2
        );
        assert_eq!(
            output
                .iter()
                .filter(|item| item["phase"] == "commentary")
                .count(),
            1
        );
        assert!(output.iter().any(|item| item["id"] == "msg_visible"));
    }

    #[test]
    fn folded_sse_does_not_dedupe_explicit_non_assistant_messages() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_user_like_first", "type": "message", "role": "user", "content": [{"type": "output_text", "text": "same visible text"}]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_user_like_second", "type": "message", "role": "user", "content": [{"type": "output_text", "text": "same visible text"}]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);
        let user_messages: Vec<&Value> = output
            .iter()
            .filter(|item| item["role"] == "user")
            .collect();

        assert_eq!(user_messages.len(), 2);
        assert!(user_messages
            .iter()
            .any(|item| item["id"] == "msg_user_like_first"));
        assert!(user_messages
            .iter()
            .any(|item| item["id"] == "msg_user_like_second"));
    }

    #[test]
    fn folded_sse_compares_all_visible_output_text_segments() {
        let first = json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"id": "msg_first", "type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "共同开头。"},
                    {"type": "output_text", "text": "第一条后续。"}
                ]}
            ],
            "usage": {"output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 516}}
        });
        let second = json!({
            "id": "resp_2",
            "status": "completed",
            "output": [
                {"id": "msg_second", "type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "共同开头。"},
                    {"type": "output_text", "text": "第二条后续。"}
                ]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);
        let messages: Vec<&Value> = output
            .iter()
            .filter(|item| item["type"] == "message")
            .collect();

        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|item| item["id"] == "msg_first"));
        assert!(messages.iter().any(|item| item["id"] == "msg_second"));
    }
}
