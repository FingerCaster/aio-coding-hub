//! Usage: Native Codex Responses continuation repair helpers.

use axum::body::Bytes;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) const ENCRYPTED_REASONING_INCLUDE: &str = "reasoning.encrypted_content";
pub(super) const CONTINUATION_MARKER_TEXT: &str = "Continue thinking. Preserve any prior assistant-visible answer verbatim as a prefix. If the prior answer is already complete, repeat it exactly; do not rewrite, summarize, or produce an alternative wording.";
const EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT: &str = "Continue solving from the original user prompt. Treat any prior assistant-visible draft in this repair as untrusted and not delivered to the user. Recompute from scratch, check the final answer against the original constraints, and return one concise final answer. Do not use tools. Do not preserve, quote, or summarize prior drafts; use a short direct derivation instead of extended hidden deliberation.";
const EXPERIMENTAL_CONTINUATION_RETRY_PREFIX: &str =
    "Previous experimental repair attempts still ended at the hidden truncation boundary";
const SYNTHETIC_CONTINUATION_MARKERS: [&str; 3] = [
    CONTINUATION_MARKER_TEXT,
    EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT,
    EXPERIMENTAL_CONTINUATION_RETRY_PREFIX,
];
pub(super) const BPLUS_CLIENT_CONTRACT_VERSION: &str = "bplus_protocol_reconstruction_v8";
pub(super) const BPLUS_RESPONSE_ID_CONTINUITY: &str = "final_raw_response_id_validated";
pub(super) const BPLUS_CLEAN_APPEND_ENABLED: bool = false;

pub(super) struct IncludeMergeInput<'a> {
    pub(super) repair_enabled: bool,
    pub(super) auto_add_encrypted_reasoning_include: bool,
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
    if !input.auto_add_encrypted_reasoning_include {
        return unchanged(input.body, true);
    }
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
    ExperimentalSafeReplay,
}

impl ContinuationReplayPolicy {
    pub(super) fn from_post_match_strategy(
        strategy: crate::settings::CodexReasoningGuardPostMatchStrategy,
    ) -> Option<Self> {
        match strategy {
            crate::settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepair => {
                Some(Self::StableEncryptedReplay)
            }
            crate::settings::CodexReasoningGuardPostMatchStrategy::ContinuationRepairExperimental => {
                Some(Self::ExperimentalSafeReplay)
            }
            crate::settings::CodexReasoningGuardPostMatchStrategy::RetrySameProvider => None,
        }
    }

    pub(super) fn auto_add_encrypted_reasoning_include(self) -> bool {
        true
    }

    pub(super) fn requires_encrypted_reasoning(self) -> bool {
        true
    }

    pub(super) fn is_experimental(self) -> bool {
        matches!(self, Self::ExperimentalSafeReplay)
    }

    pub(super) fn payload_mode(self) -> ContinuationPayloadMode {
        match self {
            Self::StableEncryptedReplay => ContinuationPayloadMode::StableEncryptedReplay,
            Self::ExperimentalSafeReplay => ContinuationPayloadMode::ExperimentalSafeReplay,
        }
    }

    pub(super) fn next_replay_tail(
        self,
        stable_tail: &mut Vec<Value>,
        current: &Value,
        raw_sse: &[u8],
        sent_rounds: u32,
    ) -> Vec<Value> {
        match self {
            Self::StableEncryptedReplay => {
                stable_tail.extend(reasoning_items(current));
                stable_tail.push(commentary_marker_item());
                stable_tail.clone()
            }
            Self::ExperimentalSafeReplay => {
                let _ = raw_sse;
                append_unique_experimental_replay_items(stable_tail, reasoning_items(current));
                stable_tail.push(experimental_continuation_instruction_item(
                    sent_rounds,
                    Some(current),
                ));
                stable_tail.clone()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuationPayloadMode {
    StableEncryptedReplay,
    ExperimentalSafeReplay,
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
    mode: ContinuationPayloadMode,
) -> Result<Bytes, String> {
    let mut root = serde_json::from_slice::<Value>(base_body)
        .map_err(|err| format!("invalid continuation base request json: {err}"))?;
    let input_items = match mode {
        ContinuationPayloadMode::StableEncryptedReplay => {
            continuation_input_items(base_body, replay_tail)
        }
        ContinuationPayloadMode::ExperimentalSafeReplay => {
            experimental_continuation_input_items(base_body, replay_tail)
        }
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| "continuation base request is not a JSON object".to_string())?;

    object.insert("stream".to_string(), Value::Bool(true));
    object.insert("input".to_string(), Value::Array(input_items));
    object.remove("previous_response_id");
    match mode {
        ContinuationPayloadMode::StableEncryptedReplay => {
            merge_include_value(
                object
                    .entry("include".to_string())
                    .or_insert(Value::Array(Vec::new())),
            );
        }
        ContinuationPayloadMode::ExperimentalSafeReplay => {
            apply_experimental_safe_payload_options(object);
        }
    }

    serde_json::to_vec(&root)
        .map(Bytes::from)
        .map_err(|err| format!("failed to encode continuation request json: {err}"))
}

fn experimental_continuation_input_items(base_body: &[u8], replay_tail: &[Value]) -> Vec<Value> {
    let mut input_items = serde_json::from_slice::<Value>(base_body)
        .ok()
        .and_then(|root| root.get("input").cloned())
        .map(|input| match input {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            other => vec![other],
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| sanitize_responses_input_item_for_continuation_replay(item, false))
        .collect::<Vec<_>>();
    input_items.extend(
        replay_tail
            .iter()
            .cloned()
            .filter_map(|item| sanitize_responses_input_item_for_continuation_replay(item, true)),
    );
    input_items
}

fn sanitize_responses_input_item_for_continuation_replay(
    item: Value,
    allow_encrypted_reasoning: bool,
) -> Option<Value> {
    let normalized = normalize_responses_input_item_for_continuation(item)?;
    if normalized.get("type").and_then(Value::as_str) == Some("reasoning") {
        if allow_encrypted_reasoning && reasoning_item_has_encrypted_content(&normalized) {
            return Some(normalized);
        }
        return None;
    }
    Some(strip_encrypted_content(normalized))
}

fn reasoning_item_has_encrypted_content(item: &Value) -> bool {
    item.get("encrypted_content")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn normalize_responses_input_item_for_continuation(item: Value) -> Option<Value> {
    match item {
        Value::Null => None,
        Value::String(text) => Some(json!({
            "type": "message",
            "role": "user",
            "content": text,
        })),
        other => Some(other),
    }
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

fn experimental_safe_replay_output_items(response: &Value) -> Vec<Value> {
    response
        .get("output")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .cloned()
                .filter_map(sanitize_response_output_item_for_experimental_replay)
                .collect()
        })
        .unwrap_or_default()
}

fn experimental_safe_replay_raw_lifecycle_output_items(raw: &[u8]) -> Vec<Value> {
    let mut cursor = 0usize;
    let mut items = Vec::new();
    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let Ok(frame_text) = std::str::from_utf8(frame) else {
            continue;
        };
        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        else {
            continue;
        };
        if !matches!(
            event_name.as_str(),
            "response.created" | "response.in_progress" | "response.completed"
        ) {
            continue;
        }
        let output = data
            .get("response")
            .and_then(|response| response.get("output"))
            .or_else(|| data.get("output"))
            .and_then(Value::as_array);
        let Some(output) = output else {
            continue;
        };
        items.extend(
            output
                .iter()
                .cloned()
                .filter_map(sanitize_response_output_item_for_experimental_replay),
        );
    }
    dedupe_experimental_replay_items(items)
}

fn dedupe_experimental_replay_items(items: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let key = serde_json::to_string(&item).unwrap_or_default();
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn append_unique_experimental_replay_items(target: &mut Vec<Value>, items: Vec<Value>) {
    let mut seen = target
        .iter()
        .filter_map(|item| serde_json::to_string(item).ok())
        .collect::<std::collections::BTreeSet<_>>();
    for item in items {
        let key = serde_json::to_string(&item).unwrap_or_default();
        if seen.insert(key) {
            target.push(item);
        }
    }
}

fn sanitize_response_output_item_for_experimental_replay(item: Value) -> Option<Value> {
    let normalized = normalize_responses_input_item_for_continuation(item)?;
    let item_type = normalized.get("type").and_then(Value::as_str);
    if item_type == Some("reasoning") || is_responses_native_tool_context_item(&normalized) {
        return None;
    }
    if item_type != Some("message") {
        return None;
    }
    if normalized.get("phase").and_then(Value::as_str) == Some("commentary") {
        return None;
    }
    if normalized
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "assistant")
    {
        return None;
    }
    match visible_message_output_text(&normalized) {
        Ok(Some(_)) => Some(strip_encrypted_content(normalized)),
        Ok(None) | Err(_) => None,
    }
}

fn apply_experimental_safe_payload_options(object: &mut Map<String, Value>) {
    merge_include_value(
        object
            .entry("include".to_string())
            .or_insert(Value::Array(Vec::new())),
    );
    object.remove("tools");
    object.remove("tool_choice");
    object.remove("parallel_tool_calls");
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

fn experimental_continuation_instruction_text(sent_rounds: u32, current: Option<&Value>) -> String {
    if sent_rounds == 0 {
        return EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT.to_string();
    }
    let token_detail = current
        .and_then(super::codex_reasoning_features::extract_reasoning_tokens)
        .map(|token| {
            format!(
                "; the last hidden reasoning token count was {}",
                token.reasoning_tokens
            )
        })
        .unwrap_or_default();
    format!(
        "{EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT}\n\n{EXPERIMENTAL_CONTINUATION_RETRY_PREFIX}{token_detail}. None of those attempts were delivered to the user. For repair attempt {}, deliberately change strategy: stop exploring alternatives, avoid repeating the same hidden reasoning path, and answer from the original prompt with the shortest complete proof you can.",
        sent_rounds.saturating_add(1)
    )
}

fn experimental_continuation_instruction_item(sent_rounds: u32, current: Option<&Value>) -> Value {
    serde_json::json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": experimental_continuation_instruction_text(sent_rounds, current)}],
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

#[derive(Debug, Clone)]
pub(super) struct ContinuationReconstruction {
    pub(super) client_raw: Bytes,
    pub(super) client_usage: crate::usage::UsageExtract,
    pub(super) provider_repair_usage: crate::usage::UsageExtract,
    pub(super) round_trace: Vec<Value>,
    pub(super) reconstruction_status: &'static str,
    pub(super) visible_assembly_kind: &'static str,
    pub(super) canonical_response_id: String,
    pub(super) canonical_response_id_continuity: &'static str,
    pub(super) aggregate_raw_bytes: usize,
}

#[derive(Debug, Clone)]
struct RoundVisibility {
    has_visible_client_output: bool,
    assistant_message_text: Option<String>,
    assistant_message_source: Option<&'static str>,
    visible_text_len: usize,
    visible_text_hash: Option<String>,
    normalized_visible_text_len: usize,
    normalized_visible_text_hash: Option<String>,
    has_tool_call: bool,
    has_reasoning: bool,
    has_commentary_marker: bool,
    non_visible_reasoning_count: usize,
    non_visible_commentary_count: usize,
    unsafe_reason: Option<&'static str>,
}

pub(super) fn reconstruct_bplus_client_sse(
    rounds: &[ContinuationRepairRound],
    aggregate_cap_bytes: usize,
) -> Result<ContinuationReconstruction, String> {
    if rounds.len() < 2 {
        return Err("continuation repair requires at least one continuation round".to_string());
    }
    let aggregate_raw_bytes = rounds
        .iter()
        .try_fold(0usize, |total, round| {
            total.checked_add(round.raw_sse.len())
        })
        .ok_or_else(|| "aggregate repair raw bytes overflowed".to_string())?;
    if aggregate_raw_bytes > aggregate_cap_bytes {
        return Err(format!(
            "aggregate repair raw bytes exceeded cap ({aggregate_raw_bytes} > {aggregate_cap_bytes})"
        ));
    }
    let final_round = rounds
        .last()
        .ok_or_else(|| "missing final continuation round".to_string())?;
    let final_response_id = final_round
        .aggregated
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "missing final response id".to_string())?
        .to_string();

    let visibility = rounds
        .iter()
        .map(classify_round_visibility_for_reconstruction)
        .collect::<Vec<_>>();
    for (index, (round, round_visibility)) in rounds.iter().zip(visibility.iter()).enumerate() {
        if let Err(reason) = validate_raw_round_for_reconstruction(
            round,
            round_visibility,
            index == rounds.len().saturating_sub(1),
        ) {
            let raw_kind = if is_raw_tool_or_function_reason(&reason) {
                "raw tool/function call"
            } else {
                "raw event-stream"
            };
            return Err(format!("round {index} {raw_kind} is unsafe: {reason}"));
        }
    }
    let final_visibility = visibility
        .last()
        .ok_or_else(|| "missing final visibility".to_string())?;

    let visible_assembly_kind = select_visible_assembly_kind(&visibility)?;
    if visible_assembly_kind == "clean_append_disabled" && !BPLUS_CLEAN_APPEND_ENABLED {
        return Err("clean append is disabled without Phase 0 samples".to_string());
    }

    if !final_visibility.has_visible_client_output
        && !final_visibility.has_tool_call
        && final_visibility.unsafe_reason.is_none()
    {
        return Err("final round has no client-visible output".to_string());
    }
    let provider_repair_usage = provider_usage_from_rounds(rounds)?;
    let final_usage =
        crate::usage::parse_usage_from_json_or_sse_bytes("codex", final_round.raw_sse.as_ref())
            .ok_or_else(|| "final client usage missing".to_string())?;
    let first_usage =
        crate::usage::parse_usage_from_json_or_sse_bytes("codex", rounds[0].raw_sse.as_ref())
            .ok_or_else(|| "initial client usage missing".to_string())?;
    let client_usage = client_usage_from_rounds(&first_usage, &final_usage);

    let remaining_reconstructed_bytes = aggregate_cap_bytes
        .checked_sub(aggregate_raw_bytes)
        .ok_or_else(|| "aggregate repair raw bytes exceeded cap".to_string())?;
    let (client_raw, reconstruction_status) =
        match validate_final_round_for_passthrough(rounds, &visibility) {
            Ok(()) => (
                patch_terminal_completed_usage(
                    &final_round.raw_sse,
                    &client_usage,
                    remaining_reconstructed_bytes,
                )?,
                "final_full_passthrough",
            ),
            Err(reason)
                if final_visibility_can_use_sanitized_rebuild(final_visibility, &reason) =>
            {
                validate_final_raw_for_sanitized_rebuild(final_round, final_visibility)?;
                (
                    build_sanitized_final_client_sse(
                        final_round,
                        final_visibility,
                        &client_usage,
                        remaining_reconstructed_bytes,
                    )?,
                    "final_sanitized_rebuild",
                )
            }
            Err(reason) => return Err(reason),
        };
    let reconstructed_total = aggregate_raw_bytes
        .checked_add(client_raw.len())
        .ok_or_else(|| "aggregate repair reconstructed bytes overflowed".to_string())?;
    if reconstructed_total > aggregate_cap_bytes {
        return Err(format!(
            "aggregate repair raw/reconstructed bytes exceeded cap ({reconstructed_total} > {aggregate_cap_bytes})"
        ));
    }
    let reparsed_client_usage =
        crate::usage::parse_usage_from_json_or_sse_bytes("codex", client_raw.as_ref())
            .ok_or_else(|| "patched client usage missing".to_string())?;
    if reparsed_client_usage.usage_json != client_usage.usage_json {
        return Err("patched terminal usage does not match client usage".to_string());
    }

    Ok(ContinuationReconstruction {
        client_raw,
        client_usage,
        provider_repair_usage,
        round_trace: build_round_trace(rounds, &visibility),
        reconstruction_status,
        visible_assembly_kind,
        canonical_response_id: final_response_id,
        canonical_response_id_continuity: BPLUS_RESPONSE_ID_CONTINUITY,
        aggregate_raw_bytes: reconstructed_total,
    })
}

fn select_visible_assembly_kind(visibility: &[RoundVisibility]) -> Result<&'static str, String> {
    let Some(final_visibility) = visibility.last() else {
        return Err("missing final visibility".to_string());
    };
    if let Some(reason) = final_visibility.unsafe_reason {
        if !final_unsafe_reason_can_be_sanitized(final_visibility, reason) {
            return Err(format!("final round unsafe: {reason}"));
        }
    }

    for round in visibility.iter().take(visibility.len().saturating_sub(1)) {
        if round.has_tool_call {
            return Err("non-final tool/function call is unsafe".to_string());
        }
        if let Some(reason) = round.unsafe_reason {
            if !non_final_unsafe_reason_can_be_suppressed(reason) {
                return Err(format!("non-final visible payload is unsafe: {reason}"));
            }
        }
    }
    if final_visibility.has_tool_call {
        return Err(
            "final tool/function call is unsafe in experimental continuation repair".to_string(),
        );
    }

    let non_final_visible = visibility
        .iter()
        .take(visibility.len().saturating_sub(1))
        .filter(|round| {
            round.has_visible_client_output
                && (round.assistant_message_text.is_some()
                    || !round
                        .unsafe_reason
                        .is_some_and(non_final_unsafe_reason_can_be_suppressed))
        })
        .collect::<Vec<_>>();
    if non_final_visible.is_empty() {
        return Ok("empty_prior");
    }

    let visible_texts = visibility
        .iter()
        .filter_map(|round| round.assistant_message_text.as_deref())
        .map(normalize_visible_message_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if visible_texts.len() < 2 {
        return Err("visible comparison missing final assistant message text".to_string());
    }
    let Some(final_text) = visible_texts.last() else {
        return Err("missing final visible text".to_string());
    };
    if visible_texts.iter().all(|text| text == final_text) {
        return Ok("exact_duplicate");
    }
    let replacement_allowed = intercepted_prior_visible_can_be_replaced(&visibility);
    let mut saw_strict_prefix = false;
    for pair in visible_texts.windows(2) {
        let previous = &pair[0];
        let next = &pair[1];
        if previous == next {
            continue;
        }
        if next.starts_with(previous) {
            saw_strict_prefix = true;
            continue;
        }
        if replacement_allowed {
            return Ok("intercepted_prior_replaced");
        }
        return Err(visible_text_chain_mismatch_reason(
            visibility, previous, next,
        ));
    }
    if saw_strict_prefix {
        Ok("final_superset")
    } else {
        Err("visible text chain did not prove a safe branch".to_string())
    }
}

fn intercepted_prior_visible_can_be_replaced(visibility: &[RoundVisibility]) -> bool {
    let Some(final_visibility) = visibility.last() else {
        return false;
    };
    let Some(final_text) = final_visibility
        .assistant_message_text
        .as_deref()
        .map(normalize_visible_message_text)
        .filter(|text| !text.is_empty())
    else {
        return false;
    };
    if final_visibility.has_tool_call || final_visibility.has_commentary_marker {
        return false;
    }
    if let Some(reason) = final_visibility.unsafe_reason {
        if !final_unsafe_reason_can_be_sanitized(final_visibility, reason) {
            return false;
        }
    }

    let mut saw_replaceable_prior = false;
    for round in visibility.iter().take(visibility.len().saturating_sub(1)) {
        if round.has_tool_call {
            return false;
        }
        if let Some(reason) = round.unsafe_reason {
            if !non_final_unsafe_reason_can_be_suppressed(reason) {
                return false;
            }
        }
        let Some(prior_text) = round
            .assistant_message_text
            .as_deref()
            .map(normalize_visible_message_text)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        if final_text.contains(&prior_text) && !final_text.starts_with(&prior_text) {
            return false;
        }
        saw_replaceable_prior = true;
    }

    saw_replaceable_prior
}

fn visible_text_chain_mismatch_reason(
    visibility: &[RoundVisibility],
    previous: &str,
    next: &str,
) -> String {
    let previous_normalized = normalize_visible_message_text(previous);
    let next_normalized = normalize_visible_message_text(next);
    let common_prefix_chars = previous_normalized
        .chars()
        .zip(next_normalized.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let previous_round = visibility
        .iter()
        .filter(|round| round.assistant_message_text.is_some())
        .find(|round| {
            round
                .assistant_message_text
                .as_deref()
                .map(normalize_visible_message_text)
                == Some(previous_normalized.clone())
        });
    let next_round = visibility
        .iter()
        .filter(|round| round.assistant_message_text.is_some())
        .find(|round| {
            round
                .assistant_message_text
                .as_deref()
                .map(normalize_visible_message_text)
                == Some(next_normalized.clone())
        });
    format!(
        concat!(
            "visible text chain is distinct or non-prefix: ",
            "previousSource={};previousLen={};previousHash={};previousNormalizedLen={};previousNormalizedHash={};",
            "nextSource={};nextLen={};nextHash={};nextNormalizedLen={};nextNormalizedHash={};",
            "commonPrefixChars={}"
        ),
        previous_round
            .and_then(|round| round.assistant_message_source)
            .unwrap_or("unknown"),
        previous.chars().count(),
        stable_visible_text_hash(previous),
        previous_normalized.chars().count(),
        stable_visible_text_hash(&previous_normalized),
        next_round
            .and_then(|round| round.assistant_message_source)
            .unwrap_or("unknown"),
        next.chars().count(),
        stable_visible_text_hash(next),
        next_normalized.chars().count(),
        stable_visible_text_hash(&next_normalized),
        common_prefix_chars,
    )
}

fn non_final_unsafe_reason_can_be_suppressed(reason: &str) -> bool {
    reason == "reasoning_visible_payload"
}

fn final_unsafe_reason_can_be_sanitized(visibility: &RoundVisibility, reason: &str) -> bool {
    reason == "reasoning_visible_payload"
        && visibility.assistant_message_text.is_some()
        && !visibility.has_tool_call
}

fn final_visibility_can_use_sanitized_rebuild(
    visibility: &RoundVisibility,
    passthrough_error: &str,
) -> bool {
    if visibility.assistant_message_text.is_none() || visibility.has_tool_call {
        return false;
    }
    if let Some(reason) = visibility.unsafe_reason {
        return final_unsafe_reason_can_be_sanitized(visibility, reason);
    }
    final_passthrough_error_can_use_sanitized_rebuild(passthrough_error)
}

fn final_passthrough_error_can_use_sanitized_rebuild(reason: &str) -> bool {
    matches!(
        reason,
        "final raw has unsafe pre-completed event"
            | "final raw has unknown pre-completed event"
            | "final raw output_item.added contains visible or unknown payload"
    )
}

fn classify_round_visibility_for_reconstruction(
    round: &ContinuationRepairRound,
) -> RoundVisibility {
    let mut visibility = classify_round_visibility(&round.aggregated);
    if let Some(text) = raw_lifecycle_assistant_message_text(round.raw_sse.as_ref()) {
        visibility.has_visible_client_output = true;
        visibility.visible_text_len = text.chars().count();
        visibility.visible_text_hash = Some(stable_visible_text_hash(&text));
        let normalized = normalize_visible_message_text(&text);
        visibility.normalized_visible_text_len = normalized.chars().count();
        visibility.normalized_visible_text_hash = Some(stable_visible_text_hash(&normalized));
        visibility.assistant_message_text = Some(text);
        visibility.assistant_message_source = Some("raw_lifecycle");
    }
    if raw_lifecycle_output_has_tool_call(round.raw_sse.as_ref()) {
        visibility.has_tool_call = true;
        visibility
            .unsafe_reason
            .get_or_insert("raw_lifecycle_tool_call");
    }
    visibility
}

fn raw_lifecycle_assistant_message_text(raw: &[u8]) -> Option<String> {
    let mut cursor = 0usize;
    let mut latest_text = None;
    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let frame_text = std::str::from_utf8(frame).ok()?;
        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        else {
            continue;
        };
        if !matches!(
            event_name.as_str(),
            "response.created" | "response.in_progress" | "response.completed"
        ) {
            continue;
        }
        for item in lifecycle_raw_output_items(&data) {
            if item.get("type").and_then(Value::as_str) != Some("message") {
                continue;
            }
            if item
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| role != "assistant")
            {
                continue;
            }
            if let Ok(Some(text)) = visible_message_output_text(item) {
                if !text.trim().is_empty() {
                    latest_text = Some(text);
                }
            }
        }
    }
    latest_text
}

fn raw_lifecycle_output_has_tool_call(raw: &[u8]) -> bool {
    let mut cursor = 0usize;
    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let Ok(frame_text) = std::str::from_utf8(frame) else {
            continue;
        };
        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        else {
            continue;
        };
        if !matches!(
            event_name.as_str(),
            "response.created" | "response.in_progress" | "response.completed"
        ) {
            continue;
        }
        if lifecycle_raw_output_items(&data)
            .into_iter()
            .any(is_responses_native_tool_context_item)
        {
            return true;
        }
    }
    false
}

fn lifecycle_raw_output_items(data: &Value) -> Vec<&Value> {
    data.get("response")
        .and_then(|response| response.get("output"))
        .or_else(|| data.get("output"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn is_responses_native_tool_call_item(item: &Value) -> bool {
    is_replayable_responses_tool_call_item(item) || is_non_replayable_native_tool_call_item(item)
}

fn is_replayable_responses_tool_call_item(item: &Value) -> bool {
    crate::gateway::proxy::protocol_bridge::response_cache::is_tool_call_context_item(item)
}

fn is_responses_native_tool_output_item(item: &Value) -> bool {
    crate::gateway::proxy::protocol_bridge::response_cache::is_tool_output_item(item)
}

fn is_non_replayable_native_tool_call_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("image_generation_call" | "web_search_call")
    )
}

fn is_responses_native_tool_context_item(item: &Value) -> bool {
    is_responses_native_tool_call_item(item) || is_responses_native_tool_output_item(item)
}

fn classify_round_visibility(response: &Value) -> RoundVisibility {
    let mut visible_text_parts = Vec::new();
    let mut has_visible_client_output = false;
    let mut has_tool_call = false;
    let mut has_reasoning = false;
    let mut has_commentary_marker = false;
    let mut non_visible_reasoning_count = 0usize;
    let mut non_visible_commentary_count = 0usize;
    let mut unsafe_reason = None;

    for item in response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "reasoning" => {
                has_reasoning = true;
                non_visible_reasoning_count = non_visible_reasoning_count.saturating_add(1);
                if reasoning_item_has_visible_payload(item) {
                    has_visible_client_output = true;
                    unsafe_reason.get_or_insert("reasoning_visible_payload");
                }
            }
            _ if is_responses_native_tool_context_item(item) => {
                has_tool_call = true;
                if is_responses_native_tool_output_item(item) {
                    unsafe_reason.get_or_insert("tool_output_item");
                } else if is_non_replayable_native_tool_call_item(item) {
                    unsafe_reason.get_or_insert("native_tool_call_item");
                } else if let Err(reason) = validate_final_tool_call_item(item) {
                    unsafe_reason.get_or_insert(reason);
                }
            }
            "message" => {
                if item.get("phase").and_then(Value::as_str) == Some("commentary") {
                    if item_contains_synthetic_continuation_marker(item) {
                        has_commentary_marker = true;
                        non_visible_commentary_count =
                            non_visible_commentary_count.saturating_add(1);
                    } else {
                        has_visible_client_output = true;
                        unsafe_reason.get_or_insert("commentary_message_visible");
                    }
                    continue;
                }
                if item
                    .get("role")
                    .and_then(Value::as_str)
                    .is_some_and(|role| role != "assistant")
                {
                    has_visible_client_output = true;
                    unsafe_reason.get_or_insert("non_assistant_message_visible");
                    continue;
                }
                match visible_message_output_text(item) {
                    Ok(Some(text)) => {
                        has_visible_client_output = true;
                        visible_text_parts.push(text);
                    }
                    Ok(None) => {
                        has_visible_client_output = true;
                        unsafe_reason.get_or_insert("message_without_output_text");
                    }
                    Err(reason) => {
                        has_visible_client_output = true;
                        unsafe_reason.get_or_insert(reason);
                    }
                }
            }
            "refusal" | "output_text" => {
                has_visible_client_output = true;
                unsafe_reason.get_or_insert("non_message_visible_payload");
            }
            "" => {
                has_visible_client_output = true;
                unsafe_reason.get_or_insert("missing_output_item_type");
            }
            _ => {
                has_visible_client_output = true;
                unsafe_reason.get_or_insert("unknown_visible_output_item");
            }
        }
    }

    let assistant_message_text = if !visible_text_parts.is_empty()
        && (unsafe_reason.is_none() || unsafe_reason == Some("reasoning_visible_payload"))
    {
        Some(visible_text_parts.join(""))
    } else {
        None
    };
    let visible_text_len = assistant_message_text
        .as_ref()
        .map(|text| text.chars().count())
        .unwrap_or(0);
    let visible_text_hash = assistant_message_text
        .as_ref()
        .map(|text| stable_visible_text_hash(text));
    let normalized_visible_text = assistant_message_text
        .as_deref()
        .map(normalize_visible_message_text)
        .unwrap_or_default();
    let normalized_visible_text_len = normalized_visible_text.chars().count();
    let normalized_visible_text_hash = (!normalized_visible_text.is_empty())
        .then(|| stable_visible_text_hash(&normalized_visible_text));

    RoundVisibility {
        has_visible_client_output,
        assistant_message_text,
        assistant_message_source: visible_text_hash.as_ref().map(|_| "aggregated_output"),
        visible_text_len,
        visible_text_hash,
        normalized_visible_text_len,
        normalized_visible_text_hash,
        has_tool_call,
        has_reasoning,
        has_commentary_marker,
        non_visible_reasoning_count,
        non_visible_commentary_count,
        unsafe_reason,
    }
}

fn validate_final_round_for_passthrough(
    rounds: &[ContinuationRepairRound],
    visibility: &[RoundVisibility],
) -> Result<(), String> {
    let final_round = rounds
        .last()
        .ok_or_else(|| "missing final continuation round".to_string())?;
    let final_visibility = visibility
        .last()
        .ok_or_else(|| "missing final visibility".to_string())?;
    validate_final_raw_for_passthrough(final_round, final_visibility)?;
    if final_visibility.unsafe_reason == Some("commentary_message_visible")
        || final_visibility.has_commentary_marker
    {
        return Err("final output contains commentary marker or visible commentary".to_string());
    }
    if let Some(reason) = final_visibility.unsafe_reason {
        return Err(format!("final output is unsafe: {reason}"));
    }
    reject_echoed_prior_reasoning(rounds)?;
    validate_final_tool_calls(final_round)?;
    Ok(())
}

fn build_sanitized_final_client_sse(
    final_round: &ContinuationRepairRound,
    final_visibility: &RoundVisibility,
    client_usage: &crate::usage::UsageExtract,
    max_output_bytes: usize,
) -> Result<Bytes, String> {
    let output_items = sanitized_final_output_items(final_round, final_visibility)?;
    let usage = serde_json::from_str::<Value>(&client_usage.usage_json)
        .map_err(|err| format!("client usage json was not parseable: {err}"))?;
    let output_text = output_items
        .iter()
        .filter_map(folded_visible_message_output_text)
        .collect::<Vec<_>>()
        .join("");

    let mut sanitized = strip_encrypted_content(final_round.aggregated.clone());
    let object = sanitized
        .as_object_mut()
        .ok_or_else(|| "sanitized final response is not an object".to_string())?;
    object.insert("output".to_string(), Value::Array(output_items));
    object.insert("usage".to_string(), usage);
    object.insert("output_text".to_string(), Value::String(output_text));
    object.remove("reasoning");
    object.remove("summary");
    object.remove("commentary");

    let raw = fold_responses_to_sse(&[sanitized])?;
    if raw.len() > max_output_bytes {
        return Err(format!(
            "sanitized client SSE exceeded reconstruction cap ({} > {max_output_bytes})",
            raw.len()
        ));
    }
    let reparsed = crate::usage::parse_usage_from_json_or_sse_bytes("codex", raw.as_ref())
        .ok_or_else(|| "sanitized client usage missing".to_string())?;
    if reparsed.usage_json != client_usage.usage_json {
        return Err("sanitized terminal usage does not match client usage".to_string());
    }
    let raw_text = String::from_utf8_lossy(raw.as_ref());
    for forbidden in [
        "encrypted_content",
        "\"type\":\"reasoning\"",
        "\"summary\"",
        "\"commentary\"",
    ] {
        if raw_text.contains(forbidden) {
            return Err(format!(
                "sanitized client SSE retained forbidden marker: {forbidden}"
            ));
        }
    }
    for forbidden in SYNTHETIC_CONTINUATION_MARKERS {
        if raw_text.contains(forbidden) {
            return Err(format!(
                "sanitized client SSE retained forbidden marker: {forbidden}"
            ));
        }
    }
    Ok(raw)
}

fn sanitized_final_output_items(
    final_round: &ContinuationRepairRound,
    final_visibility: &RoundVisibility,
) -> Result<Vec<Value>, String> {
    let mut output_items = experimental_safe_replay_output_items(&final_round.aggregated);
    output_items.extend(experimental_safe_replay_raw_lifecycle_output_items(
        final_round.raw_sse.as_ref(),
    ));
    let expected = final_visibility
        .assistant_message_text
        .as_deref()
        .ok_or_else(|| "sanitized final output missing assistant message text".to_string())?;
    let expected_normalized = normalize_visible_message_text(expected);
    let output_items = output_items
        .into_iter()
        .filter(|item| {
            folded_visible_message_output_text(item)
                .map(|text| normalize_visible_message_text(&text) == expected_normalized)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let merged = merged_output_items(&[json!({ "output": output_items })]);
    let text = merged
        .iter()
        .filter_map(folded_visible_message_output_text)
        .collect::<Vec<_>>()
        .join("");
    if normalize_visible_message_text(&text) != normalize_visible_message_text(expected) {
        return Err("sanitized final output text does not match final visibility".to_string());
    }
    if merged.is_empty() {
        return Err("sanitized final output has no safe assistant message".to_string());
    }
    Ok(merged)
}

fn validate_final_raw_for_sanitized_rebuild(
    round: &ContinuationRepairRound,
    final_visibility: &RoundVisibility,
) -> Result<(), String> {
    let raw_text = std::str::from_utf8(round.raw_sse.as_ref())
        .map_err(|err| format!("final raw SSE is not utf-8: {err}"))?;
    for marker in SYNTHETIC_CONTINUATION_MARKERS {
        if raw_text.contains(marker) {
            return Err("final raw contains synthetic continuation marker".to_string());
        }
    }

    let mut cursor = 0usize;
    let mut created_count = 0usize;
    let mut completed_count = 0usize;
    let mut created_id: Option<String> = None;
    let mut completed_id: Option<String> = None;
    let mut raw_visible = FinalRawVisibleStream::default();
    while let Some(relative_end) =
        crate::gateway::proxy::sse::find_sse_event_end(&round.raw_sse[cursor..])
    {
        let event_end = cursor + relative_end;
        let frame = &round.raw_sse[cursor..event_end];
        cursor = event_end;
        let frame_text =
            std::str::from_utf8(frame).map_err(|err| format!("invalid final SSE frame: {err}"))?;
        let parsed = crate::gateway::proxy::sse::parse_sse_frame(frame_text);
        if completed_count > 0 {
            if parsed.is_some() {
                return Err("final raw has semantic event after response.completed".to_string());
            }
            if !sse_frame_is_nonsemantic_after_completed(frame_text) {
                return Err(
                    "final raw has unparseable SSE frame after response.completed".to_string(),
                );
            }
            continue;
        }
        let Some((event_name, data)) = parsed else {
            if sse_frame_is_comment_only(frame_text) {
                continue;
            }
            return Err(
                "final raw has unparseable SSE frame before response.completed".to_string(),
            );
        };
        if final_sanitized_internal_stream_event_is_suppressible(&event_name, &data) {
            continue;
        }
        if !is_allowed_pre_completed_final_raw_event(&event_name)
            && (sse_event_name_has_internal_payload(&event_name)
                || value_contains_internal_payload_key(&data))
        {
            return Err("final raw has unsafe pre-completed event".to_string());
        }
        if !is_allowed_pre_completed_final_raw_event(&event_name)
            && !pre_completed_event_is_nonsemantic(&event_name, &data)
            && !response_metadata_event_is_suppressible(&event_name, &data)
        {
            return Err("sanitized final raw has unknown pre-completed event".to_string());
        }
        if matches!(
            event_name.as_str(),
            "response.failed" | "response.incomplete" | "error"
        ) {
            return Err("final raw has unsafe terminal event".to_string());
        }
        if event_name.starts_with("response.function_call_arguments.") {
            return Err("final raw has function_call_arguments event".to_string());
        }
        validate_sanitized_final_lifecycle_output_payload(&event_name, &data)?;
        if matches!(
            event_name.as_str(),
            "response.output_item.added" | "response.output_item.done"
        ) {
            let item = data.get("item").unwrap_or(&data);
            validate_sanitized_final_output_item_event(&event_name, item)?;
        }
        raw_visible.record_event(&event_name, &data, "final raw")?;
        match event_name.as_str() {
            "response.created" => {
                created_count = created_count.saturating_add(1);
                let id = lifecycle_response_id(&data)
                    .ok_or_else(|| "final created response missing id".to_string())?;
                created_id = Some(id.to_string());
            }
            "response.completed" => {
                completed_count = completed_count.saturating_add(1);
                let id = lifecycle_response_id(&data)
                    .ok_or_else(|| "final completed response missing id".to_string())?;
                completed_id = Some(id.to_string());
            }
            _ => {}
        }
    }
    if !round.raw_sse[cursor..].iter().all(u8::is_ascii_whitespace) {
        return Err("final raw has trailing partial SSE data".to_string());
    }
    if created_count != 1 {
        return Err(format!(
            "final raw must contain exactly one response.created event, found {created_count}"
        ));
    }
    if completed_count != 1 {
        return Err(format!(
            "final raw must contain exactly one response.completed event, found {completed_count}"
        ));
    }
    if created_id != completed_id {
        return Err("final response.created id does not match response.completed id".to_string());
    }
    let aggregated_id = round
        .aggregated
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "final aggregated response missing id".to_string())?;
    if created_id.as_deref() != Some(aggregated_id) {
        return Err("final raw response id does not match aggregated response id".to_string());
    }
    raw_visible.validate_against_final_output(round, final_visibility)?;
    Ok(())
}

fn validate_sanitized_final_lifecycle_output_payload(
    event_name: &str,
    data: &Value,
) -> Result<(), String> {
    if !matches!(
        event_name,
        "response.created" | "response.in_progress" | "response.completed"
    ) {
        return Ok(());
    }
    for item in lifecycle_raw_output_items(data) {
        validate_sanitizable_final_output_item(item)?;
    }
    Ok(())
}

fn validate_sanitized_final_output_item_event(
    event_name: &str,
    item: &Value,
) -> Result<(), String> {
    if event_name == "response.output_item.added"
        && item.get("type").and_then(Value::as_str) == Some("reasoning")
        && !sanitized_final_raw_output_item_added_is_suppressible_reasoning_metadata(item)
    {
        return Err("final raw has unsafe pre-completed event".to_string());
    }
    if event_name == "response.output_item.added"
        && sanitized_final_raw_output_item_added_is_visible_or_unknown(item)
    {
        return Err("final raw output_item.added contains visible or unknown payload".to_string());
    }
    validate_sanitizable_final_output_item(item)
}

fn sanitized_final_raw_output_item_added_is_visible_or_unknown(item: &Value) -> bool {
    non_final_raw_output_item_added_is_visible_or_unknown(item)
        && !sanitized_final_raw_output_item_added_is_suppressible_message_metadata(item)
        && !sanitized_final_raw_output_item_added_is_suppressible_reasoning_metadata(item)
}

fn sanitized_final_raw_output_item_added_is_suppressible_message_metadata(item: &Value) -> bool {
    non_final_raw_output_item_added_is_suppressible_message_metadata(item)
        && item
            .as_object()
            .is_some_and(|object| object.contains_key("internal_chat_message_metadata_passthrough"))
}

fn sanitized_final_raw_output_item_added_is_suppressible_reasoning_metadata(item: &Value) -> bool {
    non_final_raw_output_item_added_is_suppressible_reasoning(item)
        && !value_contains_tool_or_function_marker(item)
}

fn final_sanitized_internal_stream_event_is_suppressible(event_name: &str, data: &Value) -> bool {
    if !matches!(
        event_name,
        "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
    ) {
        return false;
    }
    if value_contains_tool_or_function_marker(data) {
        return false;
    }
    let Some(object) = data.as_object() else {
        return false;
    };
    object.iter().all(|(key, value)| {
        final_sanitized_internal_stream_field_is_suppressible(event_name, key, value)
    })
}

fn final_sanitized_internal_stream_field_is_suppressible(
    event_name: &str,
    key: &str,
    value: &Value,
) -> bool {
    match key {
        "type" => value.as_str() == Some(event_name),
        "response_id" | "item_id" => value.as_str().is_some_and(|text| !text.trim().is_empty()),
        "output_index" | "summary_index" | "sequence_number" | "sequence" => {
            metadata_sequence_value_is_safe(value)
        }
        "delta" | "text" => value.as_str().is_some(),
        "part" | "summary_part" => final_sanitized_internal_stream_part_is_suppressible(value),
        _ => false,
    }
}

fn final_sanitized_internal_stream_part_is_suppressible(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "type" => value.as_str().is_some_and(|part_type| {
            matches!(part_type, "reasoning_text" | "summary_text" | "text")
        }),
        "text" => value.as_str().is_some(),
        "index" | "sequence_number" | "sequence" => metadata_sequence_value_is_safe(value),
        _ => false,
    })
}

fn validate_sanitizable_final_output_item(item: &Value) -> Result<(), String> {
    if is_responses_native_tool_context_item(item) {
        return Err("final raw has tool/function output item".to_string());
    }
    match item.get("type").and_then(Value::as_str) {
        Some("reasoning") => Ok(()),
        Some("message") => visible_message_output_text(item)
            .map(|_| ())
            .map_err(str::to_string),
        Some(_) => Err("final raw has unsupported output item".to_string()),
        None => Err("final raw output item is missing type".to_string()),
    }
}

fn validate_final_raw_for_passthrough(
    round: &ContinuationRepairRound,
    final_visibility: &RoundVisibility,
) -> Result<(), String> {
    let raw_text = std::str::from_utf8(round.raw_sse.as_ref())
        .map_err(|err| format!("final raw SSE is not utf-8: {err}"))?;
    for marker in SYNTHETIC_CONTINUATION_MARKERS {
        if raw_text.contains(marker) {
            return Err("final raw contains synthetic continuation marker".to_string());
        }
    }

    let mut cursor = 0usize;
    let mut created_count = 0usize;
    let mut completed_count = 0usize;
    let mut created_id: Option<String> = None;
    let mut completed_id: Option<String> = None;
    let mut raw_visible = FinalRawVisibleStream::default();
    while let Some(relative_end) =
        crate::gateway::proxy::sse::find_sse_event_end(&round.raw_sse[cursor..])
    {
        let event_end = cursor + relative_end;
        let frame = &round.raw_sse[cursor..event_end];
        cursor = event_end;
        let frame_text =
            std::str::from_utf8(frame).map_err(|err| format!("invalid final SSE frame: {err}"))?;
        let parsed = crate::gateway::proxy::sse::parse_sse_frame(frame_text);
        if completed_count > 0 {
            if parsed.is_some() {
                return Err("final raw has semantic event after response.completed".to_string());
            }
            if !sse_frame_is_nonsemantic_after_completed(frame_text) {
                return Err(
                    "final raw has unparseable SSE frame after response.completed".to_string(),
                );
            }
            continue;
        }
        let Some((event_name, data)) = parsed else {
            if sse_frame_is_comment_only(frame_text) {
                continue;
            }
            return Err(
                "final raw has unparseable SSE frame before response.completed".to_string(),
            );
        };
        validate_pre_completed_final_raw_event(&event_name, &data)?;
        validate_lifecycle_output_payload(&event_name, &data, round)?;
        if event_name == "response.output_item.added" {
            let item = data.get("item").unwrap_or(&data);
            if final_raw_output_item_added_is_visible_or_unknown(item) {
                return Err(
                    "final raw output_item.added contains visible or unknown payload".to_string(),
                );
            }
        }
        raw_visible.record_event(&event_name, &data, "final raw")?;
        match event_name.as_str() {
            "response.created" => {
                created_count = created_count.saturating_add(1);
                let response = data.get("response").unwrap_or(&data);
                let id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "final created response missing id".to_string())?;
                created_id = Some(id.to_string());
            }
            "response.completed" => {
                completed_count = completed_count.saturating_add(1);
                let response = data.get("response").unwrap_or(&data);
                let id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "final completed response missing id".to_string())?;
                completed_id = Some(id.to_string());
            }
            "response.failed" | "response.incomplete" | "error" => {
                return Err("final raw has unsafe terminal event".to_string());
            }
            _ => {}
        }
    }
    if !round.raw_sse[cursor..].iter().all(u8::is_ascii_whitespace) {
        return Err("final raw has trailing partial SSE data".to_string());
    }
    if created_count != 1 {
        return Err(format!(
            "final raw must contain exactly one response.created event, found {created_count}"
        ));
    }
    if completed_count != 1 {
        return Err(format!(
            "final raw must contain exactly one response.completed event, found {completed_count}"
        ));
    }
    if created_id != completed_id {
        return Err("final response.created id does not match response.completed id".to_string());
    }
    let aggregated_id = round
        .aggregated
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "final aggregated response missing id".to_string())?;
    if created_id.as_deref() != Some(aggregated_id) {
        return Err("final raw response id does not match aggregated response id".to_string());
    }
    raw_visible.validate_against_final_output(round, final_visibility)?;

    Ok(())
}

#[derive(Default)]
struct FinalRawVisibleStream {
    output_text_delta: String,
    saw_output_text_delta: bool,
    output_text_done: Vec<String>,
    content_part_done: Vec<String>,
    function_call_argument_delta: BTreeMap<String, String>,
    function_call_argument_done: Vec<(String, String)>,
}

impl FinalRawVisibleStream {
    fn record_event(
        &mut self,
        event_name: &str,
        data: &Value,
        raw_label: &str,
    ) -> Result<(), String> {
        match event_name {
            "response.output_text.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{raw_label} output_text.delta missing delta"))?;
                self.saw_output_text_delta = true;
                self.output_text_delta.push_str(delta);
            }
            "response.output_text.done" => {
                if let Some(text) = data.get("text").and_then(Value::as_str) {
                    self.output_text_done.push(text.to_string());
                }
            }
            "response.content_part.added"
                if content_part_visible_text(data, raw_label)?
                    .is_some_and(|text| !text.is_empty()) =>
            {
                return Err(format!(
                    "{raw_label} content_part.added contains visible text"
                ));
            }
            "response.content_part.done" => {
                if let Some(text) = content_part_visible_text(data, raw_label)? {
                    self.content_part_done.push(text.to_string());
                }
            }
            "response.function_call_arguments.delta" => {
                let key = function_call_event_key(data).ok_or_else(|| {
                    "final raw function_call_arguments.delta missing call key".to_string()
                })?;
                let delta = data.get("delta").and_then(Value::as_str).ok_or_else(|| {
                    "final raw function_call_arguments.delta missing delta".to_string()
                })?;
                self.function_call_argument_delta
                    .entry(key)
                    .or_default()
                    .push_str(delta);
            }
            "response.function_call_arguments.done" => {
                let key = function_call_event_key(data).ok_or_else(|| {
                    "final raw function_call_arguments.done missing call key".to_string()
                })?;
                let arguments = data
                    .get("arguments")
                    .and_then(argument_value_to_string)
                    .ok_or_else(|| {
                        "final raw function_call_arguments.done missing arguments".to_string()
                    })?;
                self.function_call_argument_done.push((key, arguments));
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_against_final_output(
        &self,
        round: &ContinuationRepairRound,
        final_visibility: &RoundVisibility,
    ) -> Result<(), String> {
        self.validate_against_output(round, final_visibility, "final raw")
    }

    fn validate_against_output(
        &self,
        round: &ContinuationRepairRound,
        visibility: &RoundVisibility,
        raw_label: &str,
    ) -> Result<(), String> {
        let final_text = visibility.assistant_message_text.as_deref();
        let visible_label = if raw_label == "final raw" {
            "final visible message"
        } else {
            "visible message"
        };
        if self.saw_output_text_delta {
            let expected = final_text.ok_or_else(|| {
                format!("{raw_label} output_text.delta exists without {visible_label}")
            })?;
            if self.output_text_delta != expected {
                return Err(format!(
                    "{raw_label} output_text.delta does not match {visible_label}"
                ));
            }
        }
        let expected = final_text;
        validate_visible_done_texts(
            "output_text.done",
            &self.output_text_done,
            expected,
            raw_label,
            visible_label,
        )?;
        validate_visible_done_texts(
            "content_part.done",
            &self.content_part_done,
            expected,
            raw_label,
            visible_label,
        )?;

        for text in self
            .output_text_done
            .iter()
            .chain(self.content_part_done.iter())
        {
            if text.is_empty() {
                return Err(format!(
                    "{raw_label} visible done event contains empty text"
                ));
            }
        }

        for (key, arguments) in &self.function_call_argument_delta {
            let expected = final_tool_call_arguments_for_key(round, key)?;
            if arguments != &expected {
                return Err(
                    "final raw function_call_arguments.delta does not match final tool call"
                        .to_string(),
                );
            }
        }
        for (key, arguments) in &self.function_call_argument_done {
            let expected = final_tool_call_arguments_for_key(round, key)?;
            if arguments != &expected {
                return Err(
                    "final raw function_call_arguments.done does not match final tool call"
                        .to_string(),
                );
            }
        }

        Ok(())
    }
}

fn validate_visible_done_texts(
    event_kind: &str,
    texts: &[String],
    expected: Option<&str>,
    raw_label: &str,
    visible_label: &str,
) -> Result<(), String> {
    if texts.is_empty() {
        return Ok(());
    }
    let expected = expected
        .ok_or_else(|| format!("{raw_label} {event_kind} exists without {visible_label}"))?;
    let joined = texts.join("");
    if joined != expected {
        return Err(format!(
            "{raw_label} {event_kind} does not match {visible_label}"
        ));
    }
    Ok(())
}

fn content_part_visible_text<'a>(
    data: &'a Value,
    raw_label: &str,
) -> Result<Option<&'a str>, String> {
    let Some(part) = data
        .get("part")
        .or_else(|| data.get("content_part"))
        .or_else(|| data.get("content"))
    else {
        return Ok(None);
    };
    let part_type = part.get("type").and_then(Value::as_str);
    if part_type == Some("refusal")
        || part
            .get("refusal")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(format!("{raw_label} content_part contains refusal text"));
    }
    if matches!(part_type, Some("output_text" | "text")) {
        return Ok(part.get("text").and_then(Value::as_str));
    }
    Ok(None)
}

fn function_call_event_key(data: &Value) -> Option<String> {
    data.get("item_id")
        .or_else(|| data.get("call_id"))
        .or_else(|| data.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            data.get("output_index")
                .and_then(Value::as_u64)
                .map(|index| format!("output_index:{index}"))
        })
}

fn final_tool_call_arguments_for_key(
    round: &ContinuationRepairRound,
    key: &str,
) -> Result<String, String> {
    for (index, item) in round
        .aggregated
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        if !is_replayable_responses_tool_call_item(item) {
            continue;
        }
        let key_matches = item
            .get("id")
            .or_else(|| item.get("call_id"))
            .and_then(Value::as_str)
            == Some(key)
            || item.get("call_id").and_then(Value::as_str) == Some(key)
            || key == format!("output_index:{index}");
        if key_matches {
            return item
                .get("arguments")
                .and_then(argument_value_to_string)
                .ok_or_else(|| "final tool call missing arguments".to_string());
        }
    }
    Err("final raw function_call_arguments event has no matching final tool call".to_string())
}

fn argument_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(_) | Value::Array(_) | Value::Null => serde_json::to_string(value).ok(),
        _ => None,
    }
}

fn validate_pre_completed_final_raw_event(event_name: &str, data: &Value) -> Result<(), String> {
    if sse_event_name_has_internal_payload(event_name) || value_contains_internal_payload_key(data)
    {
        return Err("final raw has unsafe pre-completed event".to_string());
    }
    if !is_allowed_pre_completed_final_raw_event(event_name) {
        if pre_completed_event_is_nonsemantic(event_name, data) {
            return Ok(());
        }
        return Err("final raw has unknown pre-completed event".to_string());
    }
    Ok(())
}

fn is_allowed_pre_completed_final_raw_event(event_name: &str) -> bool {
    matches!(
        event_name,
        "response.created"
            | "response.in_progress"
            | "response.output_item.added"
            | "response.output_item.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.delta"
            | "response.output_text.done"
            | "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "error"
    )
}

fn non_final_pre_completed_event_is_suppressible(event_name: &str, data: &Value) -> bool {
    if event_name.starts_with("response.")
        && sse_event_name_has_internal_payload(event_name)
        && !value_contains_tool_or_function_marker(data)
    {
        return true;
    }
    pre_completed_event_is_nonsemantic(event_name, data)
        || response_metadata_event_is_suppressible(event_name, data)
}

fn unknown_pre_completed_event_shape(event_name: &str, data: &Value) -> String {
    let segment_count = event_name
        .split(['.', '_', '-'])
        .filter(|part| !part.is_empty())
        .count();
    let suffix = event_name
        .rsplit(['.', '_', '-'])
        .find(|part| !part.is_empty())
        .filter(|part| {
            matches!(
                *part,
                "added" | "delta" | "done" | "completed" | "created" | "in" | "progress"
            )
        })
        .unwrap_or("other");
    let data_kind = match data {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    let mut data_keys = data
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    data_keys.sort_unstable();
    format!(
        "event_class={};prefix_response={};contains_reasoning={};contains_summary={};contains_commentary={};contains_encrypted={};suffix={suffix};segments={segment_count};data_kind={data_kind};data_keys={}",
        event_name_diagnostic_class(event_name),
        event_name.starts_with("response."),
        event_name.contains("reasoning"),
        event_name.contains("summary"),
        event_name.contains("commentary"),
        event_name.contains("encrypted"),
        data_keys.join(","),
    )
}

fn event_name_diagnostic_class(event_name: &str) -> &'static str {
    if event_name.starts_with("response.") {
        "response_unknown"
    } else {
        match event_name {
            "ping" => "ping",
            "keepalive" => "keepalive",
            "heartbeat" => "heartbeat",
            "message" => "message",
            "unknown" => "unknown",
            "" => "empty",
            _ => "non_response_unknown",
        }
    }
}

fn pre_completed_event_is_nonsemantic(event_name: &str, data: &Value) -> bool {
    matches!(
        event_name,
        "ping" | "keepalive" | "heartbeat" | "message" | "unknown"
    ) && nonsemantic_event_payload_is_safe(event_name, data)
}

fn response_metadata_event_is_suppressible(event_name: &str, data: &Value) -> bool {
    event_name.starts_with("response.")
        && !is_allowed_pre_completed_final_raw_event(event_name)
        && !sse_event_name_has_internal_payload(event_name)
        && !value_contains_tool_or_function_marker(data)
        && !value_contains_internal_payload_key(data)
        && data.as_object().is_some_and(|object| {
            object
                .iter()
                .all(|(key, value)| response_metadata_event_field_is_safe(event_name, key, value))
        })
}

fn response_metadata_event_field_is_safe(event_name: &str, key: &str, value: &Value) -> bool {
    match key {
        "type" => value.as_str().is_some_and(|value| {
            value == event_name && !sse_event_name_has_internal_payload(value)
        }),
        "response_id" => value.as_str().is_some_and(|value| !value.trim().is_empty()),
        "sequence_number" | "sequence" => metadata_sequence_value_is_safe(value),
        "metadata" => metadata_payload_is_nonvisible(value),
        _ => false,
    }
}

fn metadata_sequence_value_is_safe(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(_) | Value::Number(_))
        || value.as_str().is_some_and(|text| {
            !text.trim().is_empty() && text.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn metadata_payload_is_nonvisible(value: &Value) -> bool {
    if value_contains_tool_or_function_marker(value) || value_contains_internal_payload_key(value) {
        return false;
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(items) => items.iter().all(metadata_payload_is_nonvisible),
        Value::Object(object) => object.iter().all(|(key, value)| {
            !reasoning_visible_payload_key(key) && metadata_payload_is_nonvisible(value)
        }),
    }
}

fn nonsemantic_event_payload_is_safe(event_name: &str, data: &Value) -> bool {
    if value_contains_tool_or_function_marker(data) || value_contains_internal_payload_key(data) {
        return false;
    }
    match data {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(_) | Value::Array(_) => false,
        Value::Object(object) => object
            .iter()
            .all(|(key, value)| nonsemantic_event_field_is_safe(event_name, key, value)),
    }
}

fn nonsemantic_event_field_is_safe(event_name: &str, key: &str, value: &Value) -> bool {
    match key {
        "type" => value.as_str().is_some_and(|value| {
            matches!(
                value,
                "ping" | "keepalive" | "heartbeat" | "message" | "unknown"
            ) || value == event_name
        }),
        "sequence_number" | "sequence" | "timestamp" | "created_at" => {
            matches!(value, Value::Null | Value::Bool(_) | Value::Number(_))
                || value.as_str().is_some_and(|text| {
                    !text.trim().is_empty()
                        && text.chars().all(|ch| {
                            ch.is_ascii_digit() || matches!(ch, '-' | ':' | '.' | 'T' | 'Z')
                        })
                })
        }
        _ => false,
    }
}

fn sse_event_name_has_internal_payload(event_name: &str) -> bool {
    event_name
        .split(['.', '_', '-'])
        .any(|part| matches!(part, "reasoning" | "summary" | "commentary"))
        || event_name.contains("encrypted_content")
}

fn value_contains_tool_or_function_marker(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(value_contains_tool_or_function_marker),
        Value::Object(object) => object.iter().any(|(key, value)| {
            key.contains("tool")
                || key.contains("function")
                || (key == "type"
                    && value
                        .as_str()
                        .is_some_and(|text| text.contains("tool") || text.contains("function")))
                || value_contains_tool_or_function_marker(value)
        }),
        Value::String(_) => false,
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn value_contains_internal_payload_key(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(value_contains_internal_payload_key),
        Value::Object(object) => object.iter().any(|(key, value)| {
            sse_payload_key_is_internal(key)
                || sse_payload_value_is_internal_marker(key, value)
                || value_contains_internal_payload_key(value)
        }),
        _ => false,
    }
}

fn sse_payload_key_is_internal(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "encrypted_content"
        || (key.contains("reasoning") && key != "reasoning_tokens")
        || key.contains("summary")
        || key.contains("commentary")
}

fn sse_payload_value_is_internal_marker(key: &str, value: &Value) -> bool {
    if !matches!(key, "type" | "phase" | "channel") {
        return false;
    }
    value
        .as_str()
        .is_some_and(sse_event_name_has_internal_payload)
}

fn validate_final_tool_calls(round: &ContinuationRepairRound) -> Result<(), String> {
    for item in round
        .aggregated
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if is_responses_native_tool_output_item(item) {
            return Err(
                "final tool/function call is not self-contained: tool_output_item".to_string(),
            );
        }
        if is_non_replayable_native_tool_call_item(item) {
            return Err(
                "final tool/function call is not self-contained: native_tool_call_item".to_string(),
            );
        }
        if is_replayable_responses_tool_call_item(item) {
            validate_final_tool_call_item(item).map_err(|reason| {
                format!("final tool/function call is not self-contained: {reason}")
            })?;
        }
    }
    Ok(())
}

fn reject_echoed_prior_reasoning(rounds: &[ContinuationRepairRound]) -> Result<(), String> {
    let Some(final_round) = rounds.last() else {
        return Ok(());
    };
    let prior_reasoning = rounds
        .iter()
        .take(rounds.len().saturating_sub(1))
        .flat_map(reasoning_identity_parts)
        .collect::<Vec<_>>();
    if prior_reasoning.is_empty() {
        return Ok(());
    }
    for value in reasoning_identity_parts(final_round) {
        if prior_reasoning.iter().any(|prior| prior == &value) {
            return Err("final output echoes prior-round reasoning state".to_string());
        }
    }
    Ok(())
}

fn reasoning_identity_parts(round: &ContinuationRepairRound) -> Vec<String> {
    round
        .aggregated
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .flat_map(|item| {
            [
                item.get("id").and_then(Value::as_str),
                item.get("encrypted_content").and_then(Value::as_str),
            ]
            .into_iter()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
        })
        .collect()
}

fn reasoning_item_has_visible_payload(item: &Value) -> bool {
    let Some(object) = item.as_object() else {
        return false;
    };
    object.iter().any(|(key, value)| {
        reasoning_visible_payload_key(key) && reasoning_visible_payload_value(value)
    })
}

fn reasoning_visible_payload_key(key: &str) -> bool {
    if matches!(key, "encrypted_content" | "id" | "type" | "status") {
        return false;
    }
    matches!(
        key,
        "summary" | "content" | "text" | "output_text" | "message" | "messages" | "refusal"
    ) || key.contains("summary")
        || key.contains("text")
        || key.contains("message")
        || key.contains("refusal")
        || (key.contains("content") && key != "encrypted_content")
}

fn reasoning_visible_payload_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(_) | Value::Number(_) => true,
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => {
            !items.is_empty() && items.iter().any(reasoning_visible_payload_value)
        }
        Value::Object(object) => {
            !object.is_empty()
                && object.iter().any(|(key, value)| {
                    reasoning_visible_payload_key(key) || reasoning_visible_payload_value(value)
                })
        }
    }
}

fn validate_final_tool_call_item(item: &Value) -> Result<(), &'static str> {
    if non_empty_item_str(item, "id").is_none() {
        return Err("missing_tool_call_id");
    }
    if non_empty_item_str(item, "call_id").is_none() {
        return Err("missing_tool_call_call_id");
    }
    if non_empty_item_str(item, "name").is_none() {
        return Err("missing_tool_call_name");
    }
    let Some(arguments) = item.get("arguments") else {
        return Err("invalid_tool_call_arguments");
    };
    match arguments {
        Value::String(text) if serde_json::from_str::<Value>(text).is_ok() => Ok(()),
        Value::Object(_) | Value::Array(_) | Value::Null => Ok(()),
        _ => Err("invalid_tool_call_arguments"),
    }
}

fn non_empty_item_str<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn sse_frame_is_nonsemantic_after_completed(frame: &str) -> bool {
    sse_frame_is_comment_or_done(frame)
}

fn sse_frame_is_comment_only(frame: &str) -> bool {
    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        return false;
    }
    true
}

fn sse_frame_is_comment_or_done(frame: &str) -> bool {
    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if rest.trim_start() == "[DONE]" {
                continue;
            }
            return false;
        }
        if line.starts_with("event:") {
            return false;
        }
        return false;
    }
    true
}

fn item_contains_text(item: &Value, needle: &str) -> bool {
    item.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .any(|text| text.contains(needle))
}

fn item_contains_synthetic_continuation_marker(item: &Value) -> bool {
    SYNTHETIC_CONTINUATION_MARKERS
        .iter()
        .any(|marker| item_contains_text(item, marker))
}

fn stable_visible_text_hash(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn build_round_trace(
    rounds: &[ContinuationRepairRound],
    visibility: &[RoundVisibility],
) -> Vec<Value> {
    rounds
        .iter()
        .zip(visibility.iter())
        .enumerate()
        .map(|(index, (round, visible))| {
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
                "hasVisibleClientOutput": visible.has_visible_client_output,
                "visibleTextSource": visible.assistant_message_source,
                "visibleTextLen": visible.visible_text_len,
                "visibleTextHash": visible.visible_text_hash,
                "normalizedVisibleTextLen": visible.normalized_visible_text_len,
                "normalizedVisibleTextHash": visible.normalized_visible_text_hash,
                "hasToolCall": visible.has_tool_call,
                "hasReasoning": visible.has_reasoning,
                "hasCommentaryMarker": visible.has_commentary_marker,
                "nonVisibleReasoningCount": visible.non_visible_reasoning_count,
                "nonVisibleCommentaryCount": visible.non_visible_commentary_count,
                "unsafeReason": visible.unsafe_reason,
            })
        })
        .collect()
}

pub(super) fn reconstruction_round_trace(rounds: &[ContinuationRepairRound]) -> Vec<Value> {
    let visibility = rounds
        .iter()
        .map(classify_round_visibility_for_reconstruction)
        .collect::<Vec<_>>();
    build_round_trace(rounds, &visibility)
}

fn provider_usage_from_rounds(
    rounds: &[ContinuationRepairRound],
) -> Result<crate::usage::UsageExtract, String> {
    let mut total = crate::usage::UsageMetrics::default();
    for (index, round) in rounds.iter().enumerate() {
        let usage =
            crate::usage::parse_usage_from_json_or_sse_bytes("codex", round.raw_sse.as_ref())
                .ok_or_else(|| format!("provider repair usage missing for round {index}"))?;
        add_usage_metrics(&mut total, &usage.metrics);
    }
    Ok(usage_extract_from_metrics(total))
}

fn client_usage_from_rounds(
    first_usage: &crate::usage::UsageExtract,
    final_usage: &crate::usage::UsageExtract,
) -> crate::usage::UsageExtract {
    let mut metrics = final_usage.metrics.clone();
    metrics.input_tokens = first_usage.metrics.input_tokens;
    metrics.cache_read_input_tokens = first_usage.metrics.cache_read_input_tokens;
    metrics.cache_creation_input_tokens = first_usage.metrics.cache_creation_input_tokens;
    metrics.cache_creation_5m_input_tokens = first_usage.metrics.cache_creation_5m_input_tokens;
    metrics.cache_creation_1h_input_tokens = first_usage.metrics.cache_creation_1h_input_tokens;
    metrics.total_tokens = match (metrics.input_tokens, metrics.output_tokens) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        _ => None,
    };
    usage_extract_from_metrics(metrics)
}

fn add_usage_metrics(total: &mut crate::usage::UsageMetrics, next: &crate::usage::UsageMetrics) {
    total.input_tokens = add_optional_i64(total.input_tokens, next.input_tokens);
    total.output_tokens = add_optional_i64(total.output_tokens, next.output_tokens);
    total.total_tokens = add_optional_i64(total.total_tokens, next.total_tokens);
    total.reasoning_tokens = add_optional_i64(total.reasoning_tokens, next.reasoning_tokens);
    total.cache_read_input_tokens =
        add_optional_i64(total.cache_read_input_tokens, next.cache_read_input_tokens);
    total.cache_creation_input_tokens = add_optional_i64(
        total.cache_creation_input_tokens,
        next.cache_creation_input_tokens,
    );
    total.cache_creation_5m_input_tokens = add_optional_i64(
        total.cache_creation_5m_input_tokens,
        next.cache_creation_5m_input_tokens,
    );
    total.cache_creation_1h_input_tokens = add_optional_i64(
        total.cache_creation_1h_input_tokens,
        next.cache_creation_1h_input_tokens,
    );
}

fn add_optional_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn usage_extract_from_metrics(metrics: crate::usage::UsageMetrics) -> crate::usage::UsageExtract {
    let mut obj = Map::new();
    if let Some(value) = metrics.input_tokens {
        obj.insert("input_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.output_tokens {
        obj.insert("output_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.total_tokens {
        obj.insert("total_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.reasoning_tokens {
        obj.insert(
            "output_tokens_details".to_string(),
            json!({"reasoning_tokens": value}),
        );
    }
    if let Some(value) = metrics.cache_read_input_tokens {
        obj.insert("cache_read_input_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.cache_creation_input_tokens {
        obj.insert("cache_creation_input_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.cache_creation_5m_input_tokens {
        obj.insert("cache_creation_5m_input_tokens".to_string(), json!(value));
    }
    if let Some(value) = metrics.cache_creation_1h_input_tokens {
        obj.insert("cache_creation_1h_input_tokens".to_string(), json!(value));
    }
    crate::usage::UsageExtract {
        usage_json: Value::Object(obj).to_string(),
        metrics,
    }
}

fn patch_terminal_completed_usage(
    raw: &Bytes,
    client_usage: &crate::usage::UsageExtract,
    max_output_bytes: usize,
) -> Result<Bytes, String> {
    let usage_value = serde_json::from_str::<Value>(&client_usage.usage_json)
        .map_err(|err| format!("invalid client usage json: {err}"))?;
    let mut cursor = 0usize;
    let mut output = Vec::with_capacity(raw.len().min(max_output_bytes));
    let mut patched_completed = 0usize;

    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let frame_text =
            std::str::from_utf8(frame).map_err(|err| format!("invalid SSE frame: {err}"))?;
        if let Some((event_name, mut data)) =
            crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        {
            if event_name == "response.completed" {
                let response = if data.get("response").is_some_and(Value::is_object) {
                    data.get_mut("response")
                        .expect("response checked as present")
                } else {
                    &mut data
                };
                let response_obj = response
                    .as_object_mut()
                    .ok_or_else(|| "response.completed payload is not an object".to_string())?;
                response_obj.insert("usage".to_string(), usage_value.clone());
                let data = serde_json::to_string(&data)
                    .map_err(|err| format!("failed to encode patched completed event: {err}"))?;
                extend_with_cap(&mut output, b"event: ", max_output_bytes)?;
                extend_with_cap(&mut output, event_name.as_bytes(), max_output_bytes)?;
                extend_with_cap(&mut output, b"\n", max_output_bytes)?;
                extend_with_cap(&mut output, b"data: ", max_output_bytes)?;
                extend_with_cap(&mut output, data.as_bytes(), max_output_bytes)?;
                extend_with_cap(&mut output, b"\n\n", max_output_bytes)?;
                patched_completed = patched_completed.saturating_add(1);
                continue;
            }
        }
        extend_with_cap(&mut output, frame, max_output_bytes)?;
    }
    extend_with_cap(&mut output, &raw[cursor..], max_output_bytes)?;

    if patched_completed != 1 {
        return Err(format!(
            "expected one patched response.completed event, patched {patched_completed}"
        ));
    }
    Ok(Bytes::from(output))
}

fn validate_raw_round_for_reconstruction(
    round: &ContinuationRepairRound,
    visibility: &RoundVisibility,
    is_final: bool,
) -> Result<(), String> {
    if is_final {
        return validate_raw_round_tool_events(round.raw_sse.as_ref()).map_err(str::to_string);
    }

    let mut cursor = 0usize;
    let mut created_count = 0usize;
    let mut completed_count = 0usize;
    let mut created_id: Option<String> = None;
    let mut completed_id: Option<String> = None;
    let mut raw_visible = FinalRawVisibleStream::default();
    while let Some(relative_end) =
        crate::gateway::proxy::sse::find_sse_event_end(&round.raw_sse[cursor..])
    {
        let event_end = cursor + relative_end;
        let frame = &round.raw_sse[cursor..event_end];
        cursor = event_end;
        let frame_text = std::str::from_utf8(frame).map_err(|_| "invalid_sse_utf8")?;
        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        else {
            if sse_frame_is_comment_only(frame_text) {
                continue;
            }
            if completed_count > 0 && sse_frame_is_nonsemantic_after_completed(frame_text) {
                continue;
            }
            return Err("unparseable_sse_frame".to_string());
        };
        if completed_count > 0 {
            return Err("semantic_event_after_completed".to_string());
        }
        if !is_allowed_pre_completed_final_raw_event(&event_name) {
            if non_final_pre_completed_event_is_suppressible(&event_name, &data) {
                continue;
            }
            return Err(format!(
                "unknown_pre_completed_event:{}",
                unknown_pre_completed_event_shape(&event_name, &data)
            ));
        }
        validate_non_final_lifecycle_output_payload(&event_name, &data, round)?;
        if matches!(
            event_name.as_str(),
            "response.failed" | "response.incomplete" | "error"
        ) {
            return Err("terminal_error_event".to_string());
        }
        match event_name.as_str() {
            "response.created" => {
                created_count = created_count.saturating_add(1);
                let id = lifecycle_response_id(&data)
                    .ok_or_else(|| "non-final created response missing id".to_string())?;
                created_id = Some(id.to_string());
            }
            "response.completed" => {
                completed_count = completed_count.saturating_add(1);
                let id = lifecycle_response_id(&data)
                    .ok_or_else(|| "non-final completed response missing id".to_string())?;
                completed_id = Some(id.to_string());
            }
            _ => {}
        }
        if event_name.starts_with("response.function_call_arguments.") {
            return Err("function_call_arguments_event".to_string());
        }
        if matches!(
            event_name.as_str(),
            "response.output_item.added" | "response.output_item.done"
        ) {
            let item = data.get("item").unwrap_or(&data);
            if is_responses_native_tool_context_item(item) {
                return Err("output_item_tool_call_event".to_string());
            }
            if event_name == "response.output_item.added"
                && non_final_raw_output_item_added_is_visible_or_unknown(item)
                && !non_final_raw_output_item_added_is_suppressible_message_metadata(item)
                && !non_final_raw_output_item_added_is_suppressible_reasoning(item)
            {
                return Err(format!(
                    "output_item_added_visible_or_unknown:{}",
                    output_item_added_shape(item)
                ));
            }
        }
        raw_visible.record_event(&event_name, &data, "non-final raw")?;
    }
    if !round.raw_sse[cursor..].iter().all(u8::is_ascii_whitespace) {
        return Err("trailing_partial_sse_data".to_string());
    }
    if created_count > 1 {
        return Err(format!(
            "non-final raw must contain at most one response.created event, found {created_count}"
        ));
    }
    if completed_count != 1 {
        return Err(format!(
            "non-final raw must contain exactly one response.completed event, found {completed_count}"
        ));
    }
    if created_id.is_some() && created_id != completed_id {
        return Err(
            "non-final response.created id does not match response.completed id".to_string(),
        );
    }
    let aggregated_id = round
        .aggregated
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "non-final aggregated response missing id".to_string())?;
    if completed_id.as_deref() != Some(aggregated_id) {
        return Err("non-final raw response id does not match aggregated response id".to_string());
    }
    raw_visible.validate_against_output(round, visibility, "non-final raw")?;
    Ok(())
}

fn validate_raw_round_tool_events(raw: &[u8]) -> Result<(), &'static str> {
    let mut cursor = 0usize;
    while let Some(relative_end) = crate::gateway::proxy::sse::find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let frame_text = std::str::from_utf8(frame).map_err(|_| "invalid_sse_utf8")?;
        let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(frame_text)
        else {
            continue;
        };
        if event_name.starts_with("response.function_call_arguments.") {
            return Err("function_call_arguments_event");
        }
        if matches!(
            event_name.as_str(),
            "response.output_item.added" | "response.output_item.done"
        ) {
            let item = data.get("item").unwrap_or(&data);
            if is_responses_native_tool_context_item(item) {
                return Err("output_item_tool_call_event");
            }
        }
    }
    if !raw[cursor..].iter().all(u8::is_ascii_whitespace) {
        return Err("trailing_partial_sse_data");
    }
    Ok(())
}

fn validate_lifecycle_output_payload(
    event_name: &str,
    data: &Value,
    round: &ContinuationRepairRound,
) -> Result<(), String> {
    if !matches!(
        event_name,
        "response.created" | "response.in_progress" | "response.completed"
    ) {
        return Ok(());
    }
    let response_output = data
        .get("response")
        .and_then(|response| response.get("output"));
    let top_level_output = data.get("output");
    if response_output.is_none() && top_level_output.is_none() {
        return Ok(());
    }

    let response_items = lifecycle_output_items(event_name, "response", response_output)?;
    let top_level_items = lifecycle_output_items(event_name, "top_level", top_level_output)?;
    if let (Some(response_items), Some(top_level_items)) = (response_items, top_level_items) {
        if response_items != top_level_items {
            return Err(format!("{event_name}_duplicate_output_mismatch"));
        }
    }

    let raw_items = response_items
        .filter(|items| !items.is_empty())
        .or_else(|| top_level_items.filter(|items| !items.is_empty()));
    let Some(raw_items) = raw_items else {
        return Ok(());
    };
    let Some(aggregated_items) = round.aggregated.get("output").and_then(Value::as_array) else {
        return Err(format!("{event_name}_output_without_aggregated_output"));
    };
    if raw_items != aggregated_items {
        return Err(format!("{event_name}_output_mismatch"));
    }
    Ok(())
}

fn validate_non_final_lifecycle_output_payload(
    event_name: &str,
    data: &Value,
    round: &ContinuationRepairRound,
) -> Result<(), String> {
    if !matches!(
        event_name,
        "response.created" | "response.in_progress" | "response.completed"
    ) {
        return Ok(());
    }
    let response_output = data
        .get("response")
        .and_then(|response| response.get("output"));
    let top_level_output = data.get("output");
    if response_output.is_none() && top_level_output.is_none() {
        return Ok(());
    }

    let response_items = lifecycle_output_items(event_name, "response", response_output)?;
    let top_level_items = lifecycle_output_items(event_name, "top_level", top_level_output)?;
    if let (Some(response_items), Some(top_level_items)) = (response_items, top_level_items) {
        if response_items != top_level_items {
            return Err(format!("{event_name}_duplicate_output_mismatch"));
        }
    }

    let raw_items = response_items
        .filter(|items| !items.is_empty())
        .or_else(|| top_level_items.filter(|items| !items.is_empty()));
    let Some(raw_items) = raw_items else {
        return Ok(());
    };
    let aggregated_items = round.aggregated.get("output").and_then(Value::as_array);
    if aggregated_items == Some(raw_items) {
        return Ok(());
    }
    if raw_items
        .iter()
        .all(non_final_lifecycle_output_item_is_suppressible)
    {
        return Ok(());
    }
    Err(format!(
        "{event_name}_output_mismatch:{}",
        lifecycle_output_items_shape(raw_items)
    ))
}

fn non_final_lifecycle_output_item_is_suppressible(item: &Value) -> bool {
    if is_responses_native_tool_context_item(item) {
        return false;
    }
    match item.get("type").and_then(Value::as_str) {
        Some("reasoning") => true,
        Some("message") => !item
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|role| role != "assistant"),
        _ => false,
    }
}

fn lifecycle_output_items_shape(items: &[Value]) -> String {
    let shapes = items
        .iter()
        .take(8)
        .map(output_item_shape)
        .collect::<Vec<_>>()
        .join("|");
    format!("len={};items={shapes}", items.len())
}

fn output_item_shape(item: &Value) -> String {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    let mut keys = item
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort_unstable();
    let content_len = item
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let summary_len = item
        .get("summary")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    format!(
        "type={item_type};keys={};content_len={content_len};summary_len={summary_len}",
        keys.join(",")
    )
}

fn lifecycle_output_items<'a>(
    event_name: &str,
    location: &str,
    output: Option<&'a Value>,
) -> Result<Option<&'a Vec<Value>>, String> {
    let Some(output) = output else {
        return Ok(None);
    };
    output
        .as_array()
        .map(Some)
        .ok_or_else(|| format!("{event_name}_{location}_output_not_array"))
}

fn lifecycle_response_id(data: &Value) -> Option<&str> {
    data.get("response")
        .unwrap_or(data)
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn final_raw_output_item_added_is_visible_or_unknown(item: &Value) -> bool {
    !item
        .as_object()
        .is_some_and(raw_output_item_added_is_empty_message_metadata)
}

fn non_final_raw_output_item_added_is_visible_or_unknown(item: &Value) -> bool {
    !item.as_object().is_some_and(|object| {
        raw_output_item_added_is_empty_message_metadata(object)
            || raw_output_item_added_is_empty_reasoning_metadata(object)
    })
}

fn non_final_raw_output_item_added_is_suppressible_message_metadata(item: &Value) -> bool {
    let Some(object) = item.as_object() else {
        return false;
    };
    if object
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|text| text.trim().is_empty())
    {
        return false;
    }
    if object.get("type").and_then(Value::as_str) != Some("message") {
        return false;
    }
    if object.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    if object.get("status").is_some_and(|value| {
        value
            .as_str()
            .is_none_or(|status| !raw_output_item_added_metadata_status_is_allowed(status))
    }) {
        return false;
    }
    if !object
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        return false;
    }
    object.keys().all(|key| {
        matches!(
            key.as_str(),
            "id" | "type"
                | "role"
                | "status"
                | "content"
                | "phase"
                | "metadata"
                | "internal_chat_message_metadata_passthrough"
        )
    })
}

fn non_final_raw_output_item_added_is_suppressible_reasoning(item: &Value) -> bool {
    let Some(object) = item.as_object() else {
        return false;
    };
    if object
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|text| text.trim().is_empty())
    {
        return false;
    }
    if object.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    if object.get("status").is_some_and(|value| {
        value
            .as_str()
            .is_none_or(|status| !raw_output_item_added_metadata_status_is_allowed(status))
    }) {
        return false;
    }
    object.keys().all(|key| {
        matches!(
            key.as_str(),
            "id" | "type"
                | "status"
                | "summary"
                | "content"
                | "encrypted_content"
                | "metadata"
                | "internal_chat_message_metadata_passthrough"
        )
    })
}

fn output_item_added_shape(item: &Value) -> String {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    let mut keys = item
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort_unstable();
    format!("type={item_type};keys={}", keys.join(","))
}

fn raw_output_item_added_is_empty_message_metadata(object: &Map<String, Value>) -> bool {
    if object.len() != 5 {
        return false;
    }
    object
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
        && object.get("type").and_then(Value::as_str) == Some("message")
        && object
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(raw_output_item_added_metadata_status_is_allowed)
        && object.get("role").and_then(Value::as_str) == Some("assistant")
        && object
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

fn raw_output_item_added_is_empty_reasoning_metadata(object: &Map<String, Value>) -> bool {
    if object.len() != 3 && object.len() != 4 {
        return false;
    }
    if object
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(|text| text.trim().is_empty())
    {
        return false;
    }
    if object.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    if object
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|status| !raw_output_item_added_metadata_status_is_allowed(status))
    {
        return false;
    }
    match object.get("summary") {
        Some(summary) => summary.as_array().is_some_and(Vec::is_empty),
        None => object.len() == 3,
    }
}

fn raw_output_item_added_metadata_status_is_allowed(status: &str) -> bool {
    status == "in_progress"
}

fn is_raw_tool_or_function_reason(reason: &str) -> bool {
    reason.contains("tool")
        || reason.contains("function_call")
        || reason.contains("native_tool")
        || reason.contains("arguments")
}

fn extend_with_cap(
    output: &mut Vec<u8>,
    bytes: &[u8],
    max_output_bytes: usize,
) -> Result<(), String> {
    let next_len = output
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| "patched client SSE size overflowed".to_string())?;
    if next_len > max_output_bytes {
        return Err(format!(
            "patched client SSE exceeded reconstruction cap ({next_len} > {max_output_bytes})"
        ));
    }
    output.extend_from_slice(bytes);
    Ok(())
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
    if let Some(item_text) = folded_visible_message_output_text(&item) {
        if let Some((index, relationship)) = output
            .iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                folded_visible_message_output_text(candidate).map(|candidate_text| {
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

fn folded_visible_message_output_text(item: &Value) -> Option<String> {
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

fn visible_message_output_text(item: &Value) -> Result<Option<String>, &'static str> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return Ok(None);
    }
    if item
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role != "assistant")
    {
        return Ok(None);
    }
    if item.get("phase").and_then(Value::as_str) == Some("commentary") {
        return Ok(None);
    }
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return Ok(None);
    };
    if content.is_empty() {
        return Ok(None);
    }
    let mut text = String::new();
    for part in content {
        if part.get("type").and_then(Value::as_str) != Some("output_text") {
            return Err("mixed_message_visible_payload");
        }
        let Some(part_text) = part.get("text").and_then(Value::as_str) else {
            return Err("message_without_output_text");
        };
        text.push_str(part_text);
    }
    Ok((!text.is_empty()).then_some(text))
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
            auto_add_encrypted_reasoning_include: true,
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

    fn sse_round(id: Option<&str>, output: Vec<Value>, usage: Value) -> Bytes {
        let mut raw = String::new();
        if let Some(id) = id {
            push_sse_event(
                &mut raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": id, "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
        }
        for (index, item) in output.iter().enumerate() {
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item,
                }),
            )
            .expect("item");
        }
        let mut response = json!({
            "status": "completed",
            "model": "gpt-cont",
            "usage": usage,
        });
        if let Some(id) = id {
            response["id"] = json!(id);
        }
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": response,
            }),
        )
        .expect("completed");
        Bytes::from(raw)
    }

    fn sse_round_without_usage(id: &str, output: Vec<Value>) -> Bytes {
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": id, "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        for (index, item) in output.iter().enumerate() {
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item,
                }),
            )
            .expect("item");
        }
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": id,
                    "status": "completed",
                    "model": "gpt-cont",
                },
            }),
        )
        .expect("completed");
        Bytes::from(raw)
    }

    fn sse_round_with_delta(id: &str, output: Vec<Value>, delta: &str, usage: Value) -> Bytes {
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": id, "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": delta}),
        )
        .expect("delta");
        for (index, item) in output.iter().enumerate() {
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item,
                }),
            )
            .expect("item");
        }
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": id,
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage,
                },
            }),
        )
        .expect("completed");
        Bytes::from(raw)
    }

    fn repair_round(kind: ContinuationRepairRoundKind, raw: Bytes) -> ContinuationRepairRound {
        let aggregated = crate::gateway::proxy::sse::aggregate_responses_event_stream(raw.as_ref())
            .expect("aggregate round");
        ContinuationRepairRound::new(kind, raw, aggregated, Some(1))
    }

    fn assert_tool_call_fail_closed(err: &str) {
        assert!(
            err.contains("raw tool/function call")
                || err.contains("non-final tool/function call")
                || err.contains("final tool/function call")
                || err.contains("final round unsafe"),
            "unexpected error: {err}"
        );
    }

    fn message(id: &str, text: &str) -> Value {
        json!({
            "id": id,
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": text}]
        })
    }

    fn reasoning(id: &str, encrypted: &str) -> Value {
        json!({"id": id, "type": "reasoning", "encrypted_content": encrypted})
    }

    fn usage(input: i64, output: i64, reasoning: i64) -> Value {
        json!({
            "input_tokens": input,
            "output_tokens": output,
            "total_tokens": input + output,
            "output_tokens_details": {"reasoning_tokens": reasoning},
        })
    }

    fn merge_body_with_path(path: &str, body: Value) -> IncludeMergeOutcome {
        let bytes = serde_json::to_vec(&body).unwrap();
        ensure_encrypted_reasoning_include(IncludeMergeInput {
            repair_enabled: true,
            auto_add_encrypted_reasoning_include: true,
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
    fn include_merge_experimental_mode_is_eligible_without_adding_encrypted_include() {
        let body = json!({"model": "gpt-5.5", "stream": true, "input": "hello"});
        let bytes = serde_json::to_vec(&body).unwrap();

        let outcome = ensure_encrypted_reasoning_include(IncludeMergeInput {
            repair_enabled: true,
            auto_add_encrypted_reasoning_include: false,
            cli_key: "codex",
            upstream_forwarded_path: "/v1/responses",
            body: &bytes,
            active_bridge_type: None,
            oauth_adapter_present: false,
            gemini_oauth_response_mode_present: false,
            use_codex_chatgpt_backend: false,
        });

        assert!(outcome.eligible);
        assert!(!outcome.changed);
        assert_eq!(merged_json(&outcome), body);
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
                auto_add_encrypted_reasoning_include: true,
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
                auto_add_encrypted_reasoning_include: true,
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
                auto_add_encrypted_reasoning_include: true,
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
                auto_add_encrypted_reasoning_include: true,
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
                auto_add_encrypted_reasoning_include: true,
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
                auto_add_encrypted_reasoning_include: true,
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

        let body = build_continuation_payload(
            &base,
            &replay_tail,
            ContinuationPayloadMode::StableEncryptedReplay,
        )
        .expect("payload");
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
    fn experimental_continuation_payload_filters_state_and_encrypted_content() {
        let base = serde_json::to_vec(&json!({
            "model": "gpt-5.5",
            "stream": true,
            "previous_response_id": "resp_old",
            "include": ["foo", ENCRYPTED_REASONING_INCLUDE],
            "input": [
                {"role": "user", "content": "hello", "encrypted_content": "input_secret"},
                {"type": "reasoning", "encrypted_content": "input_reasoning_secret"},
                "plain prompt"
            ],
            "reasoning": {"effort": "high"},
            "tools": [{"type": "function", "name": "lookup"}],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        }))
        .unwrap();
        let replay_tail = vec![
            json!({"id": "rs_hit", "type": "reasoning", "encrypted_content": "hit_secret"}),
            experimental_continuation_instruction_item(0, None),
        ];

        let body = build_continuation_payload(
            &base,
            &replay_tail,
            ContinuationPayloadMode::ExperimentalSafeReplay,
        )
        .expect("payload");
        let value: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(value.get("previous_response_id"), None);
        assert_eq!(value["stream"], json!(true));
        assert_eq!(value["reasoning"], json!({"effort": "high"}));
        assert_eq!(value.get("tools"), None);
        assert_eq!(value.get("tool_choice"), None);
        assert_eq!(value.get("parallel_tool_calls"), None);
        assert_eq!(
            value["include"],
            json!(["foo", ENCRYPTED_REASONING_INCLUDE])
        );
        let input = value["input"].as_array().unwrap();
        assert_eq!(input.len(), 4);
        assert_eq!(input[0], json!({"role": "user", "content": "hello"}));
        assert_eq!(
            input[1],
            json!({"type": "message", "role": "user", "content": "plain prompt"})
        );
        assert_eq!(
            input[2],
            json!({"id": "rs_hit", "type": "reasoning", "encrypted_content": "hit_secret"})
        );
        assert_eq!(input[3]["role"], "assistant");
        assert_eq!(input[3]["phase"], "commentary");
        assert!(item_contains_text(
            &input[3],
            EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT
        ));
        let payload_text = serde_json::to_string(&value).unwrap();
        assert!(!payload_text.contains("input_secret"));
        assert!(!payload_text.contains("input_reasoning_secret"));
        assert!(payload_text.contains("hit_secret"));
    }

    #[test]
    fn experimental_safe_replay_tail_drops_prior_assistant_output() {
        let mut replay_tail = Vec::new();
        let first = json!({
            "output": [
                {"id": "rs_1", "type": "reasoning", "encrypted_content": "enc_1"},
                {
                    "id": "msg_1",
                    "type": "message",
                    "role": "assistant",
                    "encrypted_content": "msg_secret",
                    "content": [{"type": "output_text", "text": "partial "}]
                },
                {
                    "id": "call_1",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_1",
                    "arguments": "{}"
                }
            ]
        });

        let first_tail = ContinuationReplayPolicy::ExperimentalSafeReplay.next_replay_tail(
            &mut replay_tail,
            &first,
            &[],
            0,
        );

        assert_eq!(first_tail.len(), 2);
        assert_eq!(first_tail[0]["type"], "reasoning");
        assert_eq!(
            first_tail[0]
                .get("encrypted_content")
                .and_then(Value::as_str),
            Some("enc_1")
        );
        assert_eq!(first_tail[1]["role"], "assistant");
        assert_eq!(first_tail[1]["phase"], "commentary");
        assert!(item_contains_text(
            &first_tail[1],
            EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT
        ));
        let first_tail_text = serde_json::to_string(&first_tail).unwrap();
        assert!(!first_tail_text.contains("partial "));
        assert!(!first_tail_text.contains("msg_secret"));
        assert!(!first_tail_text.contains("function_call"));

        let second = json!({
            "usage": {"output_tokens_details": {"reasoning_tokens": 516}},
            "output": [
                {"id": "rs_2", "type": "reasoning", "encrypted_content": "enc_2"},
                {
                    "id": "msg_2",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "partial more "}]
                }
            ]
        });
        let second_tail = ContinuationReplayPolicy::ExperimentalSafeReplay.next_replay_tail(
            &mut replay_tail,
            &second,
            &[],
            1,
        );

        assert_eq!(second_tail.len(), 4);
        assert_eq!(second_tail[0]["type"], "reasoning");
        assert_eq!(
            second_tail[0]
                .get("encrypted_content")
                .and_then(Value::as_str),
            Some("enc_1")
        );
        assert_eq!(second_tail[2]["type"], "reasoning");
        assert_eq!(
            second_tail[2]
                .get("encrypted_content")
                .and_then(Value::as_str),
            Some("enc_2")
        );
        assert_eq!(second_tail[3]["role"], "assistant");
        assert_eq!(second_tail[3]["phase"], "commentary");
        let second_instruction = second_tail[3]
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
            .expect("instruction");
        assert!(second_instruction.contains(EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT));
        assert!(second_instruction.contains(EXPERIMENTAL_CONTINUATION_RETRY_PREFIX));
        assert!(second_instruction.contains("516"));
        assert_ne!(
            first_tail[1].get("content").and_then(Value::as_str),
            Some(second_instruction)
        );
        let second_tail_text = serde_json::to_string(&second_tail).unwrap();
        assert!(!second_tail_text.contains("partial "));
        assert!(!second_tail_text.contains("partial more "));
    }

    #[test]
    fn experimental_safe_replay_tail_drops_raw_lifecycle_assistant_output() {
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [{
                        "id": "msg_raw",
                        "type": "message",
                        "role": "assistant",
                        "encrypted_content": "raw-message-secret",
                        "content": [{"type": "output_text", "text": "raw lifecycle answer"}],
                    }],
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let mut replay_tail = Vec::new();
        let tail = ContinuationReplayPolicy::ExperimentalSafeReplay.next_replay_tail(
            &mut replay_tail,
            &json!({"output": []}),
            raw.as_bytes(),
            0,
        );

        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0]["role"], "assistant");
        assert_eq!(tail[0]["phase"], "commentary");
        assert!(item_contains_text(
            &tail[0],
            EXPERIMENTAL_CONTINUATION_INSTRUCTION_TEXT
        ));
        let tail_text = serde_json::to_string(&tail).unwrap();
        assert!(!tail_text.contains("raw lifecycle answer"));
        assert!(!tail_text.contains("encrypted_content"));
        assert!(!tail_text.contains("raw-message-secret"));
    }

    #[test]
    fn empty_prior_final_full_passthrough_preserves_final_visible_text_and_delta_frames() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(100, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round_with_delta(
                "resp_2",
                vec![message("msg_1", "final after continuation")],
                "final after continuation",
                usage(200, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_full_passthrough"
        );
        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
        assert_eq!(reconstructed.canonical_response_id, "resp_2");
        assert_eq!(
            reconstructed.canonical_response_id_continuity,
            BPLUS_RESPONSE_ID_CONTINUITY
        );
        assert!(body.contains("response.output_text.delta"));
        assert!(body.contains("final after continuation"));
        assert!(!body.contains("resp_1"));
        assert_eq!(reconstructed.client_usage.metrics.input_tokens, Some(100));
        assert_eq!(reconstructed.client_usage.metrics.output_tokens, Some(3));
        assert_eq!(reconstructed.client_usage.metrics.total_tokens, Some(103));
        assert_eq!(
            reconstructed.provider_repair_usage.metrics.input_tokens,
            Some(300)
        );
        assert_eq!(
            reconstructed.provider_repair_usage.metrics.output_tokens,
            Some(13)
        );
        assert_eq!(
            reconstructed.provider_repair_usage.metrics.reasoning_tokens,
            Some(518)
        );
        let reparsed = crate::usage::parse_usage_from_json_or_sse_bytes(
            "codex",
            reconstructed.client_raw.as_ref(),
        )
        .expect("client usage");
        assert_eq!(reparsed.usage_json, reconstructed.client_usage.usage_json);
    }

    #[test]
    fn final_reasoning_visible_payload_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(100, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![
                    json!({
                        "id": "rs_final",
                        "type": "reasoning",
                        "encrypted_content": "final-encrypted-reasoning-secret",
                        "summary": [{"type": "summary_text", "text": "visible final reasoning secret"}],
                    }),
                    message("msg_2", "答案是 21。"),
                ],
                usage(200, 30, 20),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
        assert!(body.contains("答案是 21。"));
        assert!(!body.contains("final-encrypted-reasoning-secret"));
        assert!(!body.contains("visible final reasoning secret"));
        assert!(!body.contains("\"type\":\"reasoning\""));
        assert!(!body.contains("\"summary\""));
        assert_eq!(reconstructed.client_usage.metrics.input_tokens, Some(100));
        assert_eq!(reconstructed.client_usage.metrics.output_tokens, Some(30));
        assert_eq!(reconstructed.client_usage.metrics.total_tokens, Some(130));
        let reparsed = crate::usage::parse_usage_from_json_or_sse_bytes(
            "codex",
            reconstructed.client_raw.as_ref(),
        )
        .expect("client usage");
        assert_eq!(reparsed.usage_json, reconstructed.client_usage.usage_json);
    }

    #[test]
    fn final_sanitized_rebuild_uses_final_visibility_message_only() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1"), message("msg_1", "答案是 15。")],
                usage(100, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "delta": "hidden final reasoning",
            }),
        )
        .expect("reasoning delta");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_draft", "答案是 20。"),
            }),
        )
        .expect("draft");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [message("msg_final", "答案是 21。")],
                    "usage": usage(100, 30, 20),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert_eq!(
            reconstructed.visible_assembly_kind,
            "intercepted_prior_replaced"
        );
        assert!(body.contains("答案是 21。"));
        assert!(!body.contains("答案是 15。"));
        assert!(!body.contains("答案是 20。"));
        assert!(!body.contains("hidden final reasoning"));
    }

    #[test]
    fn final_empty_message_added_metadata_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1"), message("msg_1", "ok")],
                usage(100, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "msg_2",
                    "type": "message",
                    "role": "assistant",
                    "status": "in_progress",
                    "phase": "final_answer",
                    "content": [],
                    "metadata": {"trace": "final-added-message-metadata-secret"},
                    "internal_chat_message_metadata_passthrough": {"kind": "message"}
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("done");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(100, 3, 0),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert_eq!(reconstructed.visible_assembly_kind, "exact_duplicate");
        assert!(body.contains("ok"));
        assert!(!body.contains("final-added-message-metadata-secret"));
        assert!(!body.contains("internal_chat_message_metadata_passthrough"));
    }

    #[test]
    fn final_lifecycle_internal_reasoning_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1"), message("msg_1", "ok")],
                usage(100, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_2",
                    "status": "in_progress",
                    "model": "gpt-cont",
                    "output": [{
                        "id": "rs_created",
                        "type": "reasoning",
                        "encrypted_content": "final-created-encrypted-secret",
                        "summary": [{"type": "summary_text", "text": "final-created-summary-secret"}],
                    }]
                }
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("done");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(100, 3, 0),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert_eq!(reconstructed.visible_assembly_kind, "exact_duplicate");
        assert!(body.contains("ok"));
        assert!(!body.contains("final-created-encrypted-secret"));
        assert!(!body.contains("final-created-summary-secret"));
        assert!(!body.contains("\"type\":\"reasoning\""));
    }

    #[test]
    fn final_raw_extra_output_text_delta_before_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "stale visible text"}),
        )
        .expect("delta");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("output_text.delta does not match"));
    }

    #[test]
    fn final_superset_strict_prefix_returns_final_text_only() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_1", "答案是 21。")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "答案是 21。最少取出 21 个糖果。")],
                usage(1, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(reconstructed.visible_assembly_kind, "final_superset");
        assert!(!body.contains("msg_1"));
        assert!(body.contains("msg_2"));
    }

    #[test]
    fn exact_duplicate_returns_one_visible_copy() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_1", "同一答案")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "同一答案")],
                usage(1, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(reconstructed.visible_assembly_kind, "exact_duplicate");
        assert!(!body.contains("msg_1"));
        assert!(body.contains("msg_2"));
    }

    #[test]
    fn intercepted_distinct_visible_messages_can_be_replaced_by_final() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_1", "答案是 15。")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "答案是 21。")],
                usage(1, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");

        assert_eq!(
            reconstructed.visible_assembly_kind,
            "intercepted_prior_replaced"
        );
        assert!(!body.contains("msg_1"));
        assert!(!body.contains("答案是 15。"));
        assert!(body.contains("msg_2"));
        assert!(body.contains("答案是 21。"));
    }

    #[test]
    fn quoted_or_non_prefix_containment_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_1", "答案是 21。")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message(
                    "msg_2",
                    "你前面说“答案是 21。”，这里还要补充最坏情况证明。",
                )],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("non-prefix") || err.contains("distinct"));
    }

    #[test]
    fn prior_refusal_blocks_final_only_branch() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![json!({"id": "ref_1", "type": "refusal", "refusal": "no"})],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("non_message_visible_payload"));
    }

    #[test]
    fn unknown_visible_result_item_blocks_final_only_branch() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![json!({"id": "unknown_1", "type": "future_visible", "text": "maybe"})],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unknown_visible_output_item"));
    }

    #[test]
    fn mixed_assistant_message_payload_is_unsafe_in_non_final_round() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![json!({
                    "id": "msg_mixed",
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "partial answer"},
                        {"type": "refusal", "refusal": "hidden visible branch"}
                    ]
                })],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("mixed_message_visible_payload"));
    }

    #[test]
    fn non_final_reasoning_summary_payload_can_be_suppressed() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![json!({
                    "id": "rs_summary",
                    "type": "reasoning",
                    "encrypted_content": "enc_1",
                    "summary": [{"type": "summary_text", "text": "visible reasoning"}]
                })],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstruction =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe final");
        assert_eq!(reconstruction.visible_assembly_kind, "empty_prior");
        let text = String::from_utf8_lossy(&reconstruction.client_raw);
        assert!(text.contains("ok"));
        assert!(!text.contains("visible reasoning"));
    }

    #[test]
    fn final_round_echoed_prior_reasoning_identity_is_unsafe() {
        for final_reasoning in [
            reasoning("rs_1", "enc_fresh"),
            reasoning("rs_fresh", "enc_1"),
        ] {
            let first = repair_round(
                ContinuationRepairRoundKind::Initial,
                sse_round(
                    Some("resp_1"),
                    vec![reasoning("rs_1", "enc_1")],
                    usage(1, 10, 516),
                ),
            );
            let mut second = repair_round(
                ContinuationRepairRoundKind::Continuation,
                sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
            );
            second
                .aggregated
                .as_object_mut()
                .expect("aggregated object")
                .insert(
                    "output".to_string(),
                    Value::Array(vec![final_reasoning, message("msg_2", "ok")]),
                );

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert!(err.contains("echoes prior-round reasoning state"));
        }
    }

    #[test]
    fn non_final_tool_call_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![json!({
                    "id": "call_1",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_1",
                    "arguments": "{}"
                })],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn non_final_raw_function_call_argument_delta_is_unsafe_even_without_output_item() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("reasoning");
        push_sse_event(
            &mut first_raw,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "call_hidden",
                "delta": "{\"message\":\"Tool call failed\"}",
            }),
        )
        .expect("arguments delta");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw tool/function call"));
        assert!(err.contains("function_call_arguments_event"));
        assert!(!err.contains("Tool call failed"));
    }

    #[test]
    fn non_final_raw_output_item_added_tool_context_is_unsafe_even_without_done_item() {
        for (item, forbidden) in [
            (
                json!({
                    "id": "call_hidden",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_hidden",
                    "arguments": "{\"secret\":\"raw-tool-arg-42\"}",
                }),
                "raw-tool-arg-42",
            ),
            (
                json!({
                    "id": "call_mcp_hidden",
                    "type": "mcp_tool_call",
                    "name": "read",
                    "call_id": "call_mcp_hidden",
                    "arguments": "{\"secret\":\"raw-mcp-arg-84\"}",
                }),
                "raw-mcp-arg-84",
            ),
            (
                json!({
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_hidden",
                    "output": "raw-custom-output-secret-126",
                }),
                "raw-custom-output-secret-126",
            ),
            (
                json!({
                    "type": "tool_search_output",
                    "call_id": "call_search_hidden",
                    "output": "raw-search-output-secret-168",
                }),
                "raw-search-output-secret-168",
            ),
            (
                json!({
                    "id": "call_web_hidden",
                    "type": "web_search_call",
                    "status": "completed",
                    "query": "raw-web-query-secret-210",
                }),
                "raw-web-query-secret-210",
            ),
            (
                json!({
                    "id": "call_image_hidden",
                    "type": "image_generation_call",
                    "status": "completed",
                    "prompt": "raw-image-prompt-secret-252",
                }),
                "raw-image-prompt-secret-252",
            ),
        ] {
            let mut first_raw = String::new();
            push_sse_event(
                &mut first_raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut first_raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "item": reasoning("rs_1", "enc_1"),
                }),
            )
            .expect("reasoning");
            push_sse_event(
                &mut first_raw,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "item": item,
                }),
            )
            .expect("tool added");
            push_sse_event(
                &mut first_raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_1",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 10, 516),
                    },
                }),
            )
            .expect("completed");
            let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
            let second = repair_round(
                ContinuationRepairRoundKind::Continuation,
                sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
            );

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert!(err.contains("raw tool/function call"));
            assert!(err.contains("output_item_tool_call_event"));
            assert!(!err.contains(forbidden));
        }
    }

    #[test]
    fn non_final_raw_visible_delta_matching_aggregated_output_can_reconstruct() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round_with_delta(
                "resp_1",
                vec![message("msg_1", "prefix")],
                "prefix",
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "prefix plus final")],
                usage(1, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");

        assert_eq!(reconstructed.visible_assembly_kind, "final_superset");
    }

    #[test]
    fn non_final_raw_visible_delta_without_aggregated_output_is_unsafe() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "output_index": 0,
                "delta": "raw-lost-visible-secret-294",
            }),
        )
        .expect("delta");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream"));
        assert!(err.contains("output_text.delta exists without visible message"));
        assert!(!err.contains("raw-lost-visible-secret-294"));
    }

    #[test]
    fn non_final_raw_visible_delta_mismatching_aggregated_output_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round_with_delta(
                "resp_1",
                vec![message("msg_1", "aggregated visible")],
                "raw-mismatched-visible-secret-336",
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "aggregated visible plus final")],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream"));
        assert!(err.contains("output_text.delta does not match visible message"));
        assert!(!err.contains("raw-mismatched-visible-secret-336"));
    }

    #[test]
    fn non_final_raw_nonsemantic_ping_can_be_suppressed() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(&mut first_raw, "ping", json!({})).expect("ping");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("reasoning");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");

        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
        assert!(String::from_utf8_lossy(&reconstructed.client_raw).contains("ok"));
    }

    #[test]
    fn non_final_raw_keepalive_metadata_can_be_suppressed() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "keepalive",
            json!({"type": "keepalive", "sequence_number": 1}),
        )
        .expect("keepalive");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("reasoning");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");

        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
        assert!(String::from_utf8_lossy(&reconstructed.client_raw).contains("ok"));
    }

    #[test]
    fn non_final_raw_response_metadata_event_can_be_suppressed() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.metadata",
            json!({
                "type": "response.metadata",
                "response_id": "resp_1",
                "sequence_number": 1,
                "metadata": {"trace": "suppressed-response-metadata-secret"},
            }),
        )
        .expect("metadata");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("reasoning");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
        assert!(body.contains("ok"));
        assert!(!body.contains("suppressed-response-metadata-secret"));
    }

    #[test]
    fn non_final_raw_response_metadata_visible_payload_is_unsafe() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.metadata",
            json!({
                "type": "response.metadata",
                "response_id": "resp_1",
                "sequence_number": 1,
                "metadata": {"text": "raw-response-metadata-visible-secret"},
            }),
        )
        .expect("metadata");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unknown_pre_completed_event"));
        assert!(!err.contains("raw-response-metadata-visible-secret"));
    }

    #[test]
    fn final_raw_nonsemantic_ping_before_completed_can_pass_through() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(&mut raw, "ping", json!({})).expect("ping");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");

        let body = String::from_utf8_lossy(&reconstructed.client_raw);
        assert!(body.contains("event: ping"));
        assert!(body.contains("ok"));
    }

    #[test]
    fn non_final_raw_unknown_semantic_event_is_unsafe_without_payload_leak() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.secret_raw_event_name_abc.delta",
            json!({
                "type": "response.secret_raw_event_name_abc.delta",
                "delta": "raw-unknown-semantic-secret-378",
            }),
        )
        .expect("unknown");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream"));
        assert!(err.contains("unknown_pre_completed_event"));
        assert!(!err.contains("response.secret_raw_event_name_abc.delta"));
        assert!(!err.contains("raw-unknown-semantic-secret-378"));
    }

    #[test]
    fn non_final_raw_output_item_added_visible_message_is_unsafe_without_payload_leak() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": message("msg_hidden", "raw-added-visible-secret-420"),
            }),
        )
        .expect("added");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream"));
        assert!(err.contains("output_item_added_visible_or_unknown"));
        assert!(!err.contains("raw-added-visible-secret-420"));
    }

    #[test]
    fn non_final_raw_output_item_added_message_metadata_can_reconstruct() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "msg_added_live_shape",
                    "type": "message",
                    "role": "assistant",
                    "status": "in_progress",
                    "phase": "final_answer",
                    "content": [],
                    "internal_chat_message_metadata_passthrough": {"kind": "message"},
                    "metadata": {"trace": "suppressed raw message metadata"},
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("reasoning");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
            .expect("metadata-only non-final message added item is safe");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);
        assert!(body.contains("ok"));
        assert!(!body.contains("suppressed raw message metadata"));
    }

    #[test]
    fn non_final_raw_output_item_added_reasoning_metadata_can_reconstruct() {
        for item in [
            json!({
                "id": "rs_added_1",
                "type": "reasoning",
                "status": "in_progress",
            }),
            json!({
                "id": "rs_added_2",
                "type": "reasoning",
                "status": "in_progress",
                "summary": [],
            }),
            json!({
                "id": "rs_added_3",
                "type": "reasoning",
                "status": "in_progress",
                "summary": [{"type": "summary_text", "text": "suppressed raw reasoning summary"}],
            }),
            json!({
                "id": "rs_added_live_shape",
                "type": "reasoning",
                "content": [{"type": "output_text", "text": "suppressed raw reasoning content"}],
                "encrypted_content": "suppressed-raw-reasoning-encrypted",
                "internal_chat_message_metadata_passthrough": {"kind": "reasoning"},
                "metadata": {"trace": "suppressed raw reasoning metadata"},
                "summary": [{"type": "summary_text", "text": "suppressed raw reasoning summary"}],
            }),
        ] {
            let mut first_raw = String::new();
            push_sse_event(
                &mut first_raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut first_raw,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "item": item,
                }),
            )
            .expect("added");
            push_sse_event(
                &mut first_raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "item": reasoning("rs_1", "enc_1"),
                }),
            )
            .expect("reasoning");
            push_sse_event(
                &mut first_raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_1",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 10, 516),
                    },
                }),
            )
            .expect("completed");
            let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
            let second = repair_round(
                ContinuationRepairRoundKind::Continuation,
                sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
            );

            let reconstructed = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect("metadata-only reasoning added item is safe");
            let body = String::from_utf8_lossy(&reconstructed.client_raw);
            assert!(body.contains("ok"));
            assert!(!body.contains("suppressed raw reasoning content"));
            assert!(!body.contains("suppressed-raw-reasoning-encrypted"));
            assert!(!body.contains("suppressed raw reasoning metadata"));
            assert!(!body.contains("suppressed raw reasoning summary"));
        }
    }

    #[test]
    fn non_final_raw_output_item_added_malformed_reasoning_metadata_is_unsafe() {
        for (item, forbidden) in [
            (
                json!({
                    "type": "reasoning",
                    "status": "in_progress",
                }),
                None,
            ),
            (
                json!({
                    "id": "   ",
                    "type": "reasoning",
                    "status": "in_progress",
                }),
                None,
            ),
            (
                json!({
                    "id": "rs_bad_status",
                    "type": "reasoning",
                    "status": "completed",
                }),
                None,
            ),
            (
                json!({
                    "id": "rs_bad_unknown",
                    "type": "reasoning",
                    "status": "in_progress",
                    "unknown_payload": "raw-reasoning-extra-secret-554",
                }),
                Some("raw-reasoning-extra-secret-554"),
            ),
        ] {
            let mut first_raw = String::new();
            push_sse_event(
                &mut first_raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut first_raw,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "item": item,
                }),
            )
            .expect("added");
            push_sse_event(
                &mut first_raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_1",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 10, 516),
                    },
                }),
            )
            .expect("completed");
            let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
            let second = repair_round(
                ContinuationRepairRoundKind::Continuation,
                sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
            );

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert!(err.contains("output_item_added_visible_or_unknown"));
            if let Some(forbidden) = forbidden {
                assert!(!err.contains(forbidden));
            }
        }
    }

    #[test]
    fn non_final_lifecycle_output_visible_payload_can_be_replaced_without_payload_leak() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [message("msg_hidden", "raw-completed-visible-secret-462")],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(
            reconstructed.visible_assembly_kind,
            "intercepted_prior_replaced"
        );
        assert!(body.contains("ok"));
        assert!(!body.contains("raw-completed-visible-secret-462"));
    }

    #[test]
    fn non_final_lifecycle_output_duplicate_visible_message_can_reconstruct() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [message("msg_hidden", "ok")],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
            .expect("duplicate lifecycle visible message is safe");
        assert_eq!(reconstructed.visible_assembly_kind, "exact_duplicate");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);
        assert!(body.contains("ok"));
    }

    #[test]
    fn non_final_lifecycle_output_reasoning_payload_can_be_suppressed() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [{
                        "id": "rs_completed_live_shape",
                        "type": "reasoning",
                        "content": [{"type": "summary_text", "text": "suppressed completed reasoning content"}],
                        "encrypted_content": "suppressed-completed-reasoning-encrypted",
                        "metadata": {"trace": "suppressed completed reasoning metadata"},
                    }],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let reconstructed = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
            .expect("non-final completed reasoning output is suppressible");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);
        assert!(body.contains("ok"));
        assert!(!body.contains("suppressed completed reasoning content"));
        assert!(!body.contains("suppressed-completed-reasoning-encrypted"));
        assert!(!body.contains("suppressed completed reasoning metadata"));
    }

    #[test]
    fn non_final_lifecycle_output_tool_payload_is_unsafe_without_payload_leak() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [{
                        "type": "tool_search_output",
                        "call_id": "call_search_hidden",
                        "output": "raw-completed-tool-secret-504",
                    }],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream") || err.contains("raw tool/function call"));
        assert!(err.contains("response.completed_output_mismatch"));
        assert!(!err.contains("raw-completed-tool-secret-504"));
    }

    #[test]
    fn non_final_lifecycle_output_tool_payload_matching_aggregated_still_fails_closed() {
        let tool_item = json!({
            "id": "call_hidden",
            "type": "function_call",
            "name": "lookup",
            "call_id": "call_hidden",
            "arguments": "{\"secret\":\"raw-completed-tool-secret-505\"}",
        });
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [tool_item.clone()],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let mut first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        first.aggregated["output"] = json!([tool_item]);
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("non-final tool/function call"));
        assert!(!err.contains("raw-completed-tool-secret-505"));
    }

    #[test]
    fn non_final_raw_created_completed_id_mismatch_is_unsafe() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_created",
                    "status": "in_progress",
                    "model": "gpt-cont",
                },
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": reasoning("rs_1", "enc_1"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_completed",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("non-final response.created id does not match response.completed id"));
    }

    #[test]
    fn non_final_raw_response_id_must_match_aggregated_response_id() {
        let mut first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        first.aggregated["id"] = json!("resp_tampered");
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("non-final raw response id does not match aggregated response id"));
    }

    #[test]
    fn non_final_lifecycle_top_level_output_is_audited_without_payload_leak() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_1", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "output": [message("msg_hidden", "raw-top-level-visible-secret-588")],
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [],
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("raw event-stream"));
        assert!(err.contains("response.completed_duplicate_output_mismatch"));
        assert!(!err.contains("raw-top-level-visible-secret-588"));
    }

    #[test]
    fn non_final_empty_lifecycle_output_does_not_override_output_item_done() {
        let mut first_raw = String::new();
        push_sse_event(
            &mut first_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_1",
                    "status": "in_progress",
                    "model": "gpt-cont",
                    "output": [],
                }
            }),
        )
        .expect("created");
        push_sse_event(
            &mut first_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_1", "prefix"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut first_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 10, 516),
                },
            }),
        )
        .expect("completed");
        let first = repair_round(ContinuationRepairRoundKind::Initial, Bytes::from(first_raw));
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", "prefix plus final")],
                usage(1, 3, 2),
            ),
        );

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("safe");
        assert_eq!(reconstructed.visible_assembly_kind, "final_superset");
    }

    #[test]
    fn final_lifecycle_output_must_match_output_item_done_without_payload_leak() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut second_raw = String::new();
        push_sse_event(
            &mut second_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut second_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_2", "safe final"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut second_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [message("msg_hidden", "raw-final-completed-secret-546")],
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            Bytes::from(second_raw),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("response.completed_output_mismatch"));
        assert!(!err.contains("raw-final-completed-secret-546"));
    }

    #[test]
    fn final_lifecycle_top_level_output_is_audited_without_payload_leak() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut second_raw = String::new();
        push_sse_event(
            &mut second_raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut second_raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_2", "safe final"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut second_raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "output": [message("msg_hidden", "raw-final-top-level-secret-630")],
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "output": [message("msg_2", "safe final")],
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            Bytes::from(second_raw),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("response.completed_duplicate_output_mismatch"));
        assert!(!err.contains("raw-final-top-level-secret-630"));
    }

    #[test]
    fn final_tool_call_with_empty_prior_is_unsafe_even_with_valid_arguments() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![json!({
                    "id": "call_2",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_2",
                    "arguments": "{\"query\":\"ok\"}"
                })],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn final_tool_call_argument_delta_is_unsafe_even_when_matching() {
        for delta in ["{\"query\":\"ok\"}", "{\"query\":\"stale\"}"] {
            let first = repair_round(
                ContinuationRepairRoundKind::Initial,
                sse_round(
                    Some("resp_1"),
                    vec![reasoning("rs_1", "enc_1")],
                    usage(1, 10, 516),
                ),
            );
            let mut raw = String::new();
            push_sse_event(
                &mut raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut raw,
                "response.function_call_arguments.delta",
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": "call_2",
                    "delta": delta,
                }),
            )
            .expect("arguments delta");
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "id": "call_2",
                        "type": "function_call",
                        "name": "lookup",
                        "call_id": "call_2",
                        "arguments": "{\"query\":\"ok\"}",
                    },
                }),
            )
            .expect("item");
            push_sse_event(
                &mut raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_2",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 3, 2),
                    },
                }),
            )
            .expect("completed");
            let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));
            let result = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024);

            let err = result.expect_err("final tool/function call should fail closed");
            assert_tool_call_fail_closed(&err);
        }
    }

    #[test]
    fn final_tool_call_does_not_bypass_non_final_visible_output() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_early", "early visible answer")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![json!({
                    "id": "call_2",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_2",
                    "arguments": "{}"
                })],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn final_tool_call_with_replaced_visible_text_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![message("msg_early", "答案是 15。")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![
                    message("msg_2", "答案是 21。"),
                    json!({
                        "id": "call_2",
                        "type": "function_call",
                        "name": "lookup",
                        "call_id": "call_2",
                        "arguments": "{}"
                    }),
                ],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn final_tool_call_is_unsafe_before_argument_shape_is_trusted() {
        for item in [
            json!({
                "id": "call_invalid",
                "type": "function_call",
                "name": "lookup",
                "call_id": "call_invalid",
                "arguments": "{\"query\":"
            }),
            json!({
                "id": "call_missing",
                "type": "function_call",
                "name": "lookup",
                "call_id": "call_missing"
            }),
            json!({
                "type": "function_call",
                "name": "lookup",
                "call_id": "call_missing_id",
                "arguments": "{}"
            }),
            json!({
                "id": "call_missing_call_id",
                "type": "function_call",
                "name": "lookup",
                "arguments": "{}"
            }),
            json!({
                "id": "call_missing_name",
                "type": "function_call",
                "call_id": "call_missing_name",
                "arguments": "{}"
            }),
        ] {
            let first = repair_round(
                ContinuationRepairRoundKind::Initial,
                sse_round(
                    Some("resp_1"),
                    vec![reasoning("rs_1", "enc_1")],
                    usage(1, 10, 516),
                ),
            );
            let second = repair_round(
                ContinuationRepairRoundKind::Continuation,
                sse_round(Some("resp_2"), vec![item], usage(1, 3, 2)),
            );

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert_tool_call_fail_closed(&err);
        }
    }

    #[test]
    fn missing_final_response_id_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(None, vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("missing final response id"));
    }

    #[test]
    fn final_raw_response_id_must_match_aggregated_response_id() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );
        second.aggregated["id"] = json!("resp_tampered");

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("final raw response id does not match aggregated response id"));
    }

    #[test]
    fn final_raw_with_internal_marker_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(
                Some("resp_2"),
                vec![message("msg_2", CONTINUATION_MARKER_TEXT)],
                usage(1, 3, 2),
            ),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("synthetic continuation marker"));
    }

    #[test]
    fn final_raw_missing_created_event_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("response.created"));
    }

    #[test]
    fn final_raw_created_completed_id_mismatch_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_created", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_completed",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("does not match"));
    }

    #[test]
    fn final_raw_event_after_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        push_sse_event(
            &mut raw,
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "late"}),
        )
        .expect("late delta");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("after response.completed"));
    }

    #[test]
    fn final_raw_reasoning_summary_delta_before_completed_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "delta": "hidden chain summary",
            }),
        )
        .expect("reasoning delta");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert!(body.contains("ok"));
        assert!(!body.contains("response.reasoning_summary_text.delta"));
        assert!(!body.contains("hidden chain summary"));
    }

    #[test]
    fn final_raw_output_item_added_visible_message_is_unsafe_without_payload_leak() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": message("msg_2", "raw-final-added-visible-secret-587"),
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("final raw output_item.added contains visible or unknown payload"));
        assert!(!err.contains("raw-final-added-visible-secret-587"));
    }

    #[test]
    fn final_raw_output_item_added_empty_message_metadata_can_passthrough() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "msg_2",
                    "type": "message",
                    "role": "assistant",
                    "status": "in_progress",
                    "content": [],
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
            .expect("metadata-only added item is safe");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);
        assert!(body.contains("response.output_item.added"));
        assert!(body.contains("ok"));
    }

    #[test]
    fn final_raw_output_item_added_reasoning_metadata_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "rs_2",
                    "type": "reasoning",
                    "status": "in_progress",
                    "summary": [],
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert!(body.contains("ok"));
        assert!(!body.contains("\"type\":\"reasoning\""));
        assert!(!body.contains("\"summary\""));
    }

    #[test]
    fn final_raw_output_item_added_reasoning_payload_rebuilds_sanitized_client_sse() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "rs_2",
                    "type": "reasoning",
                    "status": "in_progress",
                    "content": [{"type": "summary_text", "text": "raw-final-reasoning-secret-811"}],
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = String::from_utf8_lossy(&reconstructed.client_raw);

        assert_eq!(
            reconstructed.reconstruction_status,
            "final_sanitized_rebuild"
        );
        assert!(body.contains("ok"));
        assert!(!body.contains("raw-final-reasoning-secret-811"));
        assert!(!body.contains("\"type\":\"reasoning\""));
    }

    #[test]
    fn final_raw_output_item_added_reasoning_unknown_payload_remains_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "item": {
                    "id": "rs_2",
                    "type": "reasoning",
                    "status": "in_progress",
                    "unknown_payload": "raw-final-reasoning-unknown-secret-817",
                },
            }),
        )
        .expect("added");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("final raw has unsafe pre-completed event"));
        assert!(!err.contains("raw-final-reasoning-unknown-secret-817"));
    }

    #[test]
    fn final_raw_output_item_added_input_payloads_are_unsafe_without_payload_leak() {
        for (item, forbidden) in [
            (
                json!({
                    "id": "input_text_1",
                    "type": "input_text",
                    "text": "raw-final-input-text-secret-631",
                }),
                "raw-final-input-text-secret-631",
            ),
            (
                json!({
                    "id": "input_image_1",
                    "type": "input_image",
                    "image_url": "https://example.invalid/raw-final-input-image-secret-672.png",
                }),
                "raw-final-input-image-secret-672",
            ),
        ] {
            let first = repair_round(
                ContinuationRepairRoundKind::Initial,
                sse_round(
                    Some("resp_1"),
                    vec![reasoning("rs_1", "enc_1")],
                    usage(1, 10, 516),
                ),
            );
            let mut raw = String::new();
            push_sse_event(
                &mut raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut raw,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "item": item,
                }),
            )
            .expect("added");
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "item": message("msg_2", "ok"),
                }),
            )
            .expect("item");
            push_sse_event(
                &mut raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_2",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 3, 2),
                    },
                }),
            )
            .expect("completed");
            let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert!(err.contains("final raw output_item.added contains visible or unknown payload"));
            assert!(!err.contains(forbidden));
        }
    }

    #[test]
    fn final_raw_output_item_added_malformed_metadata_is_unsafe() {
        for item in [
            json!({
                "type": "message",
                "role": "assistant",
                "status": "in_progress",
                "content": [],
            }),
            json!({
                "id": "msg_missing_status",
                "type": "message",
                "role": "assistant",
                "content": [],
            }),
            json!({
                "id": "msg_missing_role",
                "type": "message",
                "status": "in_progress",
                "content": [],
            }),
            json!({
                "id": "msg_missing_content",
                "type": "message",
                "role": "assistant",
                "status": "in_progress",
            }),
            json!({
                "id": "msg_wrong_role",
                "type": "message",
                "role": "user",
                "status": "in_progress",
                "content": [],
            }),
            json!({
                "id": "msg_unknown_status",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [],
            }),
            json!({
                "id": "msg_extra",
                "type": "message",
                "role": "assistant",
                "status": "in_progress",
                "content": [],
                "metadata": {"trace": "raw-final-added-extra-secret-713"},
            }),
        ] {
            let first = repair_round(
                ContinuationRepairRoundKind::Initial,
                sse_round(
                    Some("resp_1"),
                    vec![reasoning("rs_1", "enc_1")],
                    usage(1, 10, 516),
                ),
            );
            let mut raw = String::new();
            push_sse_event(
                &mut raw,
                "response.created",
                json!({
                    "type": "response.created",
                    "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
                }),
            )
            .expect("created");
            push_sse_event(
                &mut raw,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "item": item,
                }),
            )
            .expect("added");
            push_sse_event(
                &mut raw,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "item": message("msg_2", "ok"),
                }),
            )
            .expect("item");
            push_sse_event(
                &mut raw,
                "response.completed",
                json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp_2",
                        "status": "completed",
                        "model": "gpt-cont",
                        "usage": usage(1, 3, 2),
                    },
                }),
            )
            .expect("completed");
            let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

            let err = reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024)
                .expect_err("unsafe");
            assert!(err.contains("final raw output_item.added contains visible or unknown payload"));
            assert!(!err.contains("raw-final-added-extra-secret-713"));
        }
    }

    #[test]
    fn final_raw_done_before_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        raw.push_str("data: [DONE]\n\n");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unparseable SSE frame before response.completed"));
    }

    #[test]
    fn final_raw_multi_part_done_texts_match_joined_final_visible_text() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let final_message = json!({
            "id": "msg_2",
            "type": "message",
            "content": [
                {"type": "output_text", "text": "hello "},
                {"type": "output_text", "text": "world"}
            ],
        });
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "hello world"}),
        )
        .expect("delta");
        for (index, text) in ["hello ", "world"].into_iter().enumerate() {
            push_sse_event(
                &mut raw,
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "output_index": 0,
                    "content_index": index,
                    "part": {"type": "output_text", "text": text},
                }),
            )
            .expect("content part done");
        }
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": final_message,
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        assert_eq!(reconstructed.visible_assembly_kind, "empty_prior");
    }

    #[test]
    fn final_raw_stale_content_part_done_text_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "stale "},
            }),
        )
        .expect("content part done");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": message("msg_2", "fresh"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("content_part.done does not match final visible message"));
    }

    #[test]
    fn final_raw_unknown_event_before_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.secret_final_raw_event_name_abc.delta",
            json!({
                "type": "response.secret_final_raw_event_name_abc.delta",
                "delta": "raw-final-unknown-secret-612",
            }),
        )
        .expect("future event");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unknown pre-completed event"));
        assert!(!err.contains("response.secret_final_raw_event_name_abc.delta"));
        assert!(!err.contains("raw-final-unknown-secret-612"));
    }

    #[test]
    fn final_raw_unparseable_data_before_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        raw.push_str("event: response.output_text.delta\ndata: {not-json}\n\n");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unparseable SSE frame before response.completed"));
    }

    #[test]
    fn final_raw_unparseable_data_after_completed_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        raw.push_str("data: {not-json}\n\n");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("unparseable SSE frame after response.completed"));
    }

    #[test]
    fn final_raw_done_after_completed_remains_nonsemantic() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": message("msg_2", "ok"),
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        raw.push_str(": keepalive\n\n");
        raw.push_str("data: [DONE]\n\n");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let reconstructed =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect("reconstruct");
        let body = std::str::from_utf8(reconstructed.client_raw.as_ref()).expect("utf8");
        assert!(body.contains("data: [DONE]"));
    }

    #[test]
    fn final_raw_matching_function_call_argument_delta_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "call_2",
                "delta": "{\"query\":\"ok\"}",
            }),
        )
        .expect("arguments delta");
        push_sse_event(
            &mut raw,
            "response.function_call_arguments.done",
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "call_2",
                "arguments": "{\"query\":\"ok\"}",
            }),
        )
        .expect("arguments done");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": {
                    "id": "call_2",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_2",
                    "arguments": "{\"query\":\"ok\"}",
                },
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn final_raw_mismatched_function_call_argument_delta_is_unsafe() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let mut raw = String::new();
        push_sse_event(
            &mut raw,
            "response.created",
            json!({
                "type": "response.created",
                "response": {"id": "resp_2", "status": "in_progress", "model": "gpt-cont"}
            }),
        )
        .expect("created");
        push_sse_event(
            &mut raw,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "call_2",
                "delta": "{\"query\":\"stale\"}",
            }),
        )
        .expect("arguments delta");
        push_sse_event(
            &mut raw,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "item": {
                    "id": "call_2",
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call_2",
                    "arguments": "{\"query\":\"ok\"}",
                },
            }),
        )
        .expect("item");
        push_sse_event(
            &mut raw,
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_2",
                    "status": "completed",
                    "model": "gpt-cont",
                    "usage": usage(1, 3, 2),
                },
            }),
        )
        .expect("completed");
        let second = repair_round(ContinuationRepairRoundKind::Continuation, Bytes::from(raw));

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert_tool_call_fail_closed(&err);
    }

    #[test]
    fn provider_repair_usage_requires_every_round_usage() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round_without_usage("resp_1", vec![reasoning("rs_1", "enc_1")]),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("provider repair usage missing for round 0"));
    }

    #[test]
    fn provider_repair_usage_requires_final_round_usage() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round_without_usage("resp_2", vec![message("msg_2", "ok")]),
        );

        let err =
            reconstruct_bplus_client_sse(&[first, second], 20 * 1024 * 1024).expect_err("unsafe");
        assert!(err.contains("provider repair usage missing for round 1"));
    }

    #[test]
    fn aggregate_retained_bytes_cap_is_enforced() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );

        let err = reconstruct_bplus_client_sse(&[first, second], 16).expect_err("cap");
        assert!(err.contains("aggregate repair raw bytes exceeded cap"));
    }

    #[test]
    fn aggregate_retained_plus_reconstructed_client_bytes_cap_is_enforced() {
        let first = repair_round(
            ContinuationRepairRoundKind::Initial,
            sse_round(
                Some("resp_1"),
                vec![reasoning("rs_1", "enc_1")],
                usage(1, 10, 516),
            ),
        );
        let second = repair_round(
            ContinuationRepairRoundKind::Continuation,
            sse_round(Some("resp_2"), vec![message("msg_2", "ok")], usage(1, 3, 2)),
        );
        let cap = first
            .raw_sse
            .len()
            .saturating_add(second.raw_sse.len())
            .saturating_add(8);

        let err = reconstruct_bplus_client_sse(&[first, second], cap).expect_err("cap");
        assert!(err.contains("patched client SSE exceeded reconstruction cap"));
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
    fn folded_sse_preserves_quoted_non_prefix_visible_message_text_across_rounds() {
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
    fn folded_sse_preserves_distinct_multi_segment_visible_messages() {
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

    #[test]
    fn folded_sse_preserves_mixed_message_and_refusal_items_without_bplus_fail_closed() {
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
                ]}
            ],
            "usage": {"output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 2}}
        });

        let output = folded_output_items(&[first, second]);

        assert!(output.iter().any(|item| item["id"] == "msg_mixed"));
        assert!(output.iter().any(|item| item["id"] == "refusal_item"));
        assert!(output.iter().any(|item| item["id"] == "msg_final"));
    }
}
