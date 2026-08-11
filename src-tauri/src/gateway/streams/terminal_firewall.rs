//! Bounded frame-level firewall for the final Codex Responses SSE wire format.

use crate::usage::{self, StreamInternalErrorEvidence};
use axum::body::Bytes;

pub(super) const MAX_TERMINAL_FIREWALL_FRAME_BYTES: usize = 1024 * 1024;

/// Validates a fully buffered final-wire Codex Responses SSE payload.
///
/// This deliberately reuses the live firewall state machine, so duplicate
/// completions and response-id drift fail closed in both paths. Exact-byte
/// equality additionally rejects any bytes after `[DONE]`, which the live
/// relay safely suppresses after commit.
/// Responses SSE may end at a complete frame without a `[DONE]` sentinel;
/// when present, `[DONE]` must still follow the one valid completion.
pub(crate) fn validate_complete_codex_sse(
    bytes: &[u8],
    passthrough_keywords: &[String],
) -> Result<(), &'static str> {
    let mut firewall = CodexTerminalFirewall::new_strict_complete(passthrough_keywords);
    let output = firewall.ingest(bytes);
    if output.terminal_error {
        return Err(output.fail_closed_reason.unwrap_or("terminal_error"));
    }
    if !output.stop {
        let eof = firewall.finish();
        if eof.terminal_error {
            return Err(eof.fail_closed_reason.unwrap_or("terminal_error"));
        }
        if !output.completion_seen {
            return Err("missing_completed");
        }
        if output.bytes.as_ref() != bytes {
            return Err("trailing_or_duplicate_terminal");
        }
        return Ok(());
    }
    if !output.completion_seen {
        return Err("missing_completed");
    }
    if output.bytes.as_ref() != bytes {
        return Err("trailing_or_duplicate_terminal");
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct TerminalFirewallOutput {
    pub(super) bytes: Bytes,
    pub(super) stop: bool,
    pub(super) terminal_error: bool,
    pub(super) completion_seen: bool,
    pub(super) evidence: Option<StreamInternalErrorEvidence>,
    pub(super) fail_closed_reason: Option<&'static str>,
}

impl TerminalFirewallOutput {
    fn open(bytes: Vec<u8>, completion_seen: bool) -> Self {
        Self {
            bytes: Bytes::from(bytes),
            stop: false,
            terminal_error: false,
            completion_seen,
            evidence: None,
            fail_closed_reason: None,
        }
    }

    fn stopped(
        bytes: Vec<u8>,
        terminal_error: bool,
        completion_seen: bool,
        evidence: Option<StreamInternalErrorEvidence>,
        fail_closed_reason: Option<&'static str>,
    ) -> Self {
        Self {
            bytes: Bytes::from(bytes),
            stop: true,
            terminal_error,
            completion_seen,
            evidence,
            fail_closed_reason,
        }
    }
}

pub(super) struct CodexTerminalFirewall {
    passthrough_keywords: Vec<String>,
    pending: Vec<u8>,
    stopped: bool,
    completion_seen: bool,
    strict_complete: bool,
    response_id: Option<String>,
}

impl CodexTerminalFirewall {
    pub(super) fn new(passthrough_keywords: &[String]) -> Self {
        Self {
            passthrough_keywords: passthrough_keywords.to_vec(),
            pending: Vec::new(),
            stopped: false,
            completion_seen: false,
            strict_complete: false,
            response_id: None,
        }
    }

    fn new_strict_complete(passthrough_keywords: &[String]) -> Self {
        Self {
            strict_complete: true,
            ..Self::new(passthrough_keywords)
        }
    }

    pub(super) fn ingest(&mut self, mut input: &[u8]) -> TerminalFirewallOutput {
        if self.stopped {
            return TerminalFirewallOutput::stopped(
                Vec::new(),
                false,
                self.completion_seen,
                None,
                None,
            );
        }

        let mut forwarded = Vec::with_capacity(input.len().min(64 * 1024));

        while !input.is_empty() {
            let room = MAX_TERMINAL_FIREWALL_FRAME_BYTES.saturating_sub(self.pending.len());
            if room == 0 {
                self.stopped = true;
                self.pending.clear();
                return TerminalFirewallOutput::stopped(
                    forwarded,
                    true,
                    self.completion_seen,
                    None,
                    Some("frame_too_large"),
                );
            }

            let take = room.min(input.len());
            self.pending.extend_from_slice(&input[..take]);
            input = &input[take..];

            while let Some(frame_end) =
                crate::gateway::proxy::sse::find_sse_event_end(self.pending.as_slice())
            {
                let frame = self.pending[..frame_end].to_vec();
                self.pending.drain(..frame_end);
                match self.inspect_frame(frame.as_slice()) {
                    FrameDecision::Forward { completion } => {
                        forwarded.extend_from_slice(frame.as_slice());
                        if completion {
                            self.completion_seen = true;
                        }
                    }
                    FrameDecision::Done => {
                        forwarded.extend_from_slice(frame.as_slice());
                        self.stopped = true;
                        self.pending.clear();
                        return TerminalFirewallOutput::stopped(
                            forwarded,
                            false,
                            self.completion_seen,
                            None,
                            None,
                        );
                    }
                    FrameDecision::DropTerminal { evidence } => {
                        self.stopped = true;
                        self.pending.clear();
                        return TerminalFirewallOutput::stopped(
                            forwarded,
                            true,
                            self.completion_seen,
                            Some(evidence),
                            None,
                        );
                    }
                    FrameDecision::ForwardTerminal { evidence } => {
                        forwarded.extend_from_slice(frame.as_slice());
                        self.stopped = true;
                        self.pending.clear();
                        return TerminalFirewallOutput::stopped(
                            forwarded,
                            true,
                            self.completion_seen,
                            Some(evidence),
                            None,
                        );
                    }
                    FrameDecision::FailClosed(reason) => {
                        self.stopped = true;
                        self.pending.clear();
                        return TerminalFirewallOutput::stopped(
                            forwarded,
                            true,
                            self.completion_seen,
                            None,
                            Some(reason),
                        );
                    }
                }
            }

            if self.pending.len() == MAX_TERMINAL_FIREWALL_FRAME_BYTES {
                self.stopped = true;
                self.pending.clear();
                return TerminalFirewallOutput::stopped(
                    forwarded,
                    true,
                    self.completion_seen,
                    None,
                    Some("frame_too_large"),
                );
            }
        }

        TerminalFirewallOutput::open(forwarded, self.completion_seen)
    }

    pub(super) fn finish(&mut self) -> TerminalFirewallOutput {
        if self.stopped || self.pending.is_empty() {
            return TerminalFirewallOutput::open(Vec::new(), self.completion_seen);
        }
        self.stopped = true;
        self.pending.clear();
        TerminalFirewallOutput::stopped(
            Vec::new(),
            true,
            self.completion_seen,
            None,
            Some("partial_frame_at_eof"),
        )
    }

    fn inspect_frame(&mut self, frame: &[u8]) -> FrameDecision {
        let Ok(text) = std::str::from_utf8(frame) else {
            return FrameDecision::FailClosed("invalid_utf8");
        };

        if let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(text) {
            let completion = is_completion_frame(&event_name, &data);
            if completion && self.completion_seen {
                return FrameDecision::FailClosed("duplicate_completed");
            }
            if self.strict_complete {
                if completion && !is_valid_completion_frame(&data) {
                    return FrameDecision::FailClosed("invalid_completed");
                }
                if self.completion_seen {
                    return FrameDecision::FailClosed("semantic_frame_after_completed");
                }
                if !has_consistent_codex_event_type(&event_name, &data) {
                    return FrameDecision::FailClosed("unknown_semantic_frame");
                }
                if !is_safe_strict_replay_frame(&event_name, &data) {
                    return FrameDecision::FailClosed("unsafe_replay_frame");
                }
            }

            let response_id = canonical_response_id(&event_name, &data).map(str::to_owned);
            match (self.response_id.as_deref(), response_id.as_deref()) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return FrameDecision::FailClosed("response_id_mismatch");
                }
                (Some(_), None) if completion => {
                    return FrameDecision::FailClosed("response_id_mismatch");
                }
                (None, Some(response_id)) => {
                    self.response_id = Some(response_id.to_string());
                }
                _ => {}
            }

            if let Some(evidence) = usage::classify_codex_stream_internal_error(
                &event_name,
                &data,
                true,
                &self.passthrough_keywords,
                &[],
                false,
                "dropped_after_commit",
            ) {
                return if evidence.is_passthrough_exception() {
                    FrameDecision::ForwardTerminal { evidence }
                } else {
                    FrameDecision::DropTerminal { evidence }
                };
            }

            return FrameDecision::Forward { completion };
        }

        match classify_non_json_frame(text) {
            NonJsonFrame::Keepalive => FrameDecision::Forward { completion: false },
            NonJsonFrame::Done => FrameDecision::Done,
            NonJsonFrame::Malformed => FrameDecision::FailClosed("malformed_frame"),
        }
    }
}

enum FrameDecision {
    Forward {
        completion: bool,
    },
    Done,
    DropTerminal {
        evidence: StreamInternalErrorEvidence,
    },
    ForwardTerminal {
        evidence: StreamInternalErrorEvidence,
    },
    FailClosed(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NonJsonFrame {
    Keepalive,
    Done,
    Malformed,
}

fn classify_non_json_frame(frame: &str) -> NonJsonFrame {
    let mut saw_done = false;
    let mut saw_data = false;
    let mut saw_non_comment_field = false;
    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(payload) = line.strip_prefix("data:") {
            saw_data = true;
            if payload.trim_start() == "[DONE]" {
                saw_done = true;
            } else {
                return NonJsonFrame::Malformed;
            }
        } else if line.starts_with("event:") && !saw_data {
            saw_non_comment_field = true;
        } else {
            return NonJsonFrame::Malformed;
        }
    }

    if saw_done && !saw_non_comment_field {
        NonJsonFrame::Done
    } else if saw_done {
        NonJsonFrame::Malformed
    } else if !saw_data && !saw_non_comment_field {
        NonJsonFrame::Keepalive
    } else {
        NonJsonFrame::Malformed
    }
}

fn is_completion_frame(event_name: &str, data: &serde_json::Value) -> bool {
    event_name == "response.completed"
        || data.get("type").and_then(serde_json::Value::as_str) == Some("response.completed")
}

fn is_valid_completion_frame(data: &serde_json::Value) -> bool {
    data.get("response")
        .and_then(serde_json::Value::as_object)
        .and_then(|response| response.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("completed")
}

fn has_consistent_codex_event_type(event_name: &str, data: &serde_json::Value) -> bool {
    let Some(data) = data.as_object() else {
        return false;
    };
    data.get("type").and_then(serde_json::Value::as_str) == Some(event_name)
}

fn is_sensitive_replay_entry(key: &str, value: &serde_json::Value) -> bool {
    let key = key.to_ascii_lowercase();
    let safe_reasoning_metric =
        matches!(key.as_str(), "reasoning_tokens" | "reasoningtokens") && value.as_u64().is_some();
    let named_sensitive_payload = (key == "encrypted_content"
        || key == "commentary"
        || key == "refusal"
        || key == "summary"
        || (key.starts_with("reasoning") && !safe_reasoning_metric)
        || key.ends_with("_summary"))
        && !value.is_null();
    let sensitive_discriminator = matches!(key.as_str(), "type" | "phase" | "channel")
        && value.as_str().is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value == "commentary"
                || value == "refusal"
                || value == "summary"
                || value.starts_with("reasoning")
        });
    named_sensitive_payload || sensitive_discriminator
}

fn has_sensitive_replay_payload(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(has_sensitive_replay_payload),
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            is_sensitive_replay_entry(key, value) || has_sensitive_replay_payload(value)
        }),
        _ => false,
    }
}

fn has_only_keys(value: &serde_json::Map<String, serde_json::Value>, allowed: &[&str]) -> bool {
    value.keys().all(|key| allowed.contains(&key.as_str()))
}

fn is_missing_or_empty_array(value: Option<&serde_json::Value>) -> bool {
    value.is_none_or(|value| value.is_null() || value.as_array().is_some_and(Vec::is_empty))
}

fn is_missing_or_null(value: Option<&serde_json::Value>) -> bool {
    value.is_none_or(serde_json::Value::is_null)
}

fn is_missing_or_empty_object(value: Option<&serde_json::Value>) -> bool {
    value.is_none_or(|value| {
        value.is_null() || value.as_object().is_some_and(serde_json::Map::is_empty)
    })
}

const SAFE_RESPONSE_SNAPSHOT_KEYS: &[&str] = &[
    "background",
    "billing",
    "completed_at",
    "conversation",
    "created_at",
    "error",
    "id",
    "incomplete_details",
    "input",
    "instructions",
    "max_output_tokens",
    "max_tool_calls",
    "metadata",
    "model",
    "object",
    "output",
    "parallel_tool_calls",
    "previous_response_id",
    "prompt",
    "prompt_cache_key",
    "prompt_cache_retention",
    "reasoning",
    "safety_identifier",
    "service_tier",
    "status",
    "store",
    "temperature",
    "text",
    "tool_choice",
    "tools",
    "top_logprobs",
    "top_p",
    "truncation",
    "usage",
    "user",
];

fn optional_field_matches(
    response: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    predicate: fn(&serde_json::Value) -> bool,
) -> bool {
    response
        .get(key)
        .is_none_or(|value| value.is_null() || predicate(value))
}

fn is_nonempty_string(value: &serde_json::Value) -> bool {
    value.as_str().is_some_and(|value| !value.is_empty())
}

fn is_nonnegative_integer(value: &serde_json::Value) -> bool {
    value.as_u64().is_some()
}

fn is_safe_text_configuration(value: &serde_json::Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(text) = value.as_object() else {
        return false;
    };
    if !has_only_keys(text, &["format", "verbosity"])
        || text
            .get("verbosity")
            .is_some_and(|verbosity| !matches!(verbosity.as_str(), Some("low" | "medium" | "high")))
    {
        return false;
    }
    text.get("format").is_none_or(|format| {
        format.as_object().is_some_and(|format| {
            has_only_keys(format, &["type"])
                && format.get("type").and_then(serde_json::Value::as_str) == Some("text")
        })
    })
}

fn is_safe_token_details(value: Option<&serde_json::Value>, allowed: &[&str]) -> bool {
    value.is_none_or(|value| {
        value.is_null()
            || value.as_object().is_some_and(|details| {
                has_only_keys(details, allowed)
                    && details.values().all(|value| value.as_u64().is_some())
            })
    })
}

fn is_safe_usage(value: &serde_json::Value) -> bool {
    if value.is_null() {
        return true;
    }
    let Some(usage) = value.as_object() else {
        return false;
    };
    has_only_keys(
        usage,
        &[
            "input_tokens",
            "input_tokens_details",
            "output_tokens",
            "output_tokens_details",
            "total_tokens",
        ],
    ) && ["input_tokens", "output_tokens", "total_tokens"]
        .iter()
        .all(|key| optional_field_matches(usage, key, is_nonnegative_integer))
        && is_safe_token_details(usage.get("input_tokens_details"), &["cached_tokens"])
        && is_safe_token_details(usage.get("output_tokens_details"), &["reasoning_tokens"])
}

fn has_safe_response_auxiliary_fields(
    response: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let string_fields = [
        "id",
        "model",
        "object",
        "previous_response_id",
        "prompt_cache_key",
        "prompt_cache_retention",
        "safety_identifier",
        "service_tier",
        "status",
        "truncation",
        "user",
    ];
    let boolean_fields = ["background", "parallel_tool_calls", "store"];
    let integer_fields = [
        "completed_at",
        "created_at",
        "max_output_tokens",
        "max_tool_calls",
        "top_logprobs",
    ];
    string_fields
        .iter()
        .all(|key| optional_field_matches(response, key, is_nonempty_string))
        && boolean_fields
            .iter()
            .all(|key| optional_field_matches(response, key, serde_json::Value::is_boolean))
        && integer_fields
            .iter()
            .all(|key| optional_field_matches(response, key, is_nonnegative_integer))
        && ["temperature", "top_p"]
            .iter()
            .all(|key| optional_field_matches(response, key, serde_json::Value::is_number))
        && [
            "billing",
            "conversation",
            "error",
            "incomplete_details",
            "instructions",
            "prompt",
        ]
        .iter()
        .all(|key| is_missing_or_null(response.get(*key)))
        && is_missing_or_empty_array(response.get("input"))
        && is_missing_or_empty_array(response.get("tools"))
        && is_missing_or_empty_object(response.get("metadata"))
        && response.get("text").is_none_or(is_safe_text_configuration)
        && response.get("tool_choice").is_none_or(|tool_choice| {
            matches!(tool_choice.as_str(), Some("auto" | "none" | "required"))
        })
        && response.get("usage").is_none_or(is_safe_usage)
}

fn is_safe_output_text_part(value: &serde_json::Value) -> bool {
    let Some(part) = value.as_object() else {
        return false;
    };
    has_only_keys(part, &["type", "text", "annotations", "logprobs"])
        && part.get("type").and_then(serde_json::Value::as_str) == Some("output_text")
        && part.get("text").is_some_and(serde_json::Value::is_string)
        && is_missing_or_empty_array(part.get("annotations"))
        && is_missing_or_empty_array(part.get("logprobs"))
        && !has_sensitive_replay_payload(value)
}

fn is_safe_output_text_item(value: &serde_json::Value) -> bool {
    let Some(item) = value.as_object() else {
        return false;
    };
    if !has_only_keys(item, &["id", "type", "status", "role", "content", "phase"])
        || item.get("type").and_then(serde_json::Value::as_str) != Some("message")
        || item
            .get("role")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|role| role != "assistant")
        || item
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|phase| phase != "final")
    {
        return false;
    }

    item.get("content")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|content| content.iter().all(is_safe_output_text_part))
        && !has_sensitive_replay_payload(value)
}

fn is_safe_output_text_item_with_status(value: &serde_json::Value, status: &str) -> bool {
    is_safe_output_text_item(value)
        && value
            .get("status")
            .is_none_or(|value| value.as_str() == Some(status))
}

fn is_safe_lifecycle_response_snapshot(event_name: &str, value: &serde_json::Value) -> bool {
    let Some(response) = value.as_object() else {
        return false;
    };
    has_only_keys(response, SAFE_RESPONSE_SNAPSHOT_KEYS)
        && has_safe_response_auxiliary_fields(response)
        && is_missing_or_empty_array(response.get("output"))
        && response.get("status").is_none_or(|status| {
            status.as_str()
                == Some(match event_name {
                    "response.queued" => "queued",
                    _ => "in_progress",
                })
        })
        && !has_sensitive_replay_payload(value)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalVisiblePayloadKind {
    OutputText,
    Refusal,
    FunctionCall,
}

fn classify_final_message_item(
    value: &serde_json::Value,
) -> Option<Option<FinalVisiblePayloadKind>> {
    let item = value.as_object()?;
    if !has_only_keys(item, &["id", "type", "status", "role", "content", "phase"])
        || item.get("type").and_then(serde_json::Value::as_str) != Some("message")
        || item
            .get("role")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|role| role != "assistant")
        || item
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|phase| phase != "final")
        || item
            .get("status")
            .is_some_and(|status| status.as_str() != Some("completed"))
    {
        return None;
    }

    let content = item.get("content")?.as_array()?;
    let mut visible_kind = None;
    for part in content {
        let part = part.as_object()?;
        let part_kind = match part.get("type").and_then(serde_json::Value::as_str) {
            Some("output_text")
                if is_safe_output_text_part(&serde_json::Value::Object(part.clone())) =>
            {
                FinalVisiblePayloadKind::OutputText
            }
            Some("refusal")
                if has_only_keys(part, &["type", "refusal"])
                    && part
                        .get("refusal")
                        .is_some_and(serde_json::Value::is_string) =>
            {
                FinalVisiblePayloadKind::Refusal
            }
            _ => return None,
        };
        if visible_kind.is_some_and(|kind| kind != part_kind) {
            return None;
        }
        visible_kind = Some(part_kind);
    }
    Some(visible_kind)
}

fn classify_final_output_item(
    value: &serde_json::Value,
) -> Option<Option<FinalVisiblePayloadKind>> {
    if value.get("type").and_then(serde_json::Value::as_str) == Some("message") {
        return classify_final_message_item(value);
    }

    let item = value.as_object()?;
    if !has_only_keys(
        item,
        &[
            "id",
            "type",
            "status",
            "call_id",
            "name",
            "arguments",
            "phase",
        ],
    ) || item.get("type").and_then(serde_json::Value::as_str) != Some("function_call")
        || item
            .get("call_id")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        || item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        || !item
            .get("arguments")
            .is_some_and(serde_json::Value::is_string)
        || item
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|phase| phase != "final")
        || item
            .get("status")
            .is_some_and(|status| status.as_str() != Some("completed"))
        || item
            .get("id")
            .is_some_and(|id| id.as_str().is_none_or(str::is_empty))
        || has_sensitive_replay_payload(value)
    {
        return None;
    }
    Some(Some(FinalVisiblePayloadKind::FunctionCall))
}

fn is_safe_completed_response_snapshot(value: &serde_json::Value) -> bool {
    let Some(response) = value.as_object() else {
        return false;
    };
    if !has_only_keys(response, SAFE_RESPONSE_SNAPSHOT_KEYS)
        || !has_safe_response_auxiliary_fields(response)
        || response.iter().any(|(key, value)| {
            key != "output"
                && (is_sensitive_replay_entry(key, value) || has_sensitive_replay_payload(value))
        })
    {
        return false;
    }

    let Some(output) = response.get("output") else {
        return true;
    };
    let Some(items) = output.as_array() else {
        return false;
    };
    let mut visible_kind = None;
    for item in items {
        let Some(item_kind) = classify_final_output_item(item) else {
            return false;
        };
        let Some(item_kind) = item_kind else {
            continue;
        };
        if visible_kind.is_some_and(|kind| kind != item_kind) {
            return false;
        }
        visible_kind = Some(item_kind);
    }
    true
}

fn has_safe_event_keys(data: &serde_json::Value, allowed: &[&str]) -> bool {
    data.as_object()
        .is_some_and(|data| has_only_keys(data, allowed))
}

fn is_safe_strict_replay_frame(event_name: &str, data: &serde_json::Value) -> bool {
    match event_name {
        "response.created" | "response.queued" | "response.in_progress" => {
            has_safe_event_keys(data, &["type", "response", "sequence_number", "id"])
                && !has_sensitive_replay_payload(data)
                && data.get("response").is_some_and(|response| {
                    is_safe_lifecycle_response_snapshot(event_name, response)
                })
        }
        "response.completed" => {
            has_safe_event_keys(data, &["type", "response", "sequence_number", "id"])
                && data
                    .get("response")
                    .is_some_and(is_safe_completed_response_snapshot)
        }
        "response.output_item.added" | "response.output_item.done" => {
            has_safe_event_keys(
                data,
                &[
                    "type",
                    "response_id",
                    "output_index",
                    "item",
                    "sequence_number",
                ],
            ) && !has_sensitive_replay_payload(data)
                && data.get("item").is_some_and(|item| {
                    is_safe_output_text_item_with_status(
                        item,
                        if event_name == "response.output_item.added" {
                            "in_progress"
                        } else {
                            "completed"
                        },
                    )
                })
        }
        "response.content_part.added" | "response.content_part.done" => {
            has_safe_event_keys(
                data,
                &[
                    "type",
                    "response_id",
                    "item_id",
                    "output_index",
                    "content_index",
                    "part",
                    "sequence_number",
                ],
            ) && !has_sensitive_replay_payload(data)
                && data.get("part").is_some_and(is_safe_output_text_part)
        }
        "response.output_text.delta" => {
            has_safe_event_keys(
                data,
                &[
                    "type",
                    "response_id",
                    "item_id",
                    "output_index",
                    "content_index",
                    "delta",
                    "logprobs",
                    "obfuscation",
                    "sequence_number",
                ],
            ) && !has_sensitive_replay_payload(data)
                && data.get("delta").is_some_and(serde_json::Value::is_string)
                && is_missing_or_empty_array(data.get("logprobs"))
                && data
                    .get("obfuscation")
                    .is_none_or(serde_json::Value::is_string)
        }
        "response.output_text.done" => {
            has_safe_event_keys(
                data,
                &[
                    "type",
                    "response_id",
                    "item_id",
                    "output_index",
                    "content_index",
                    "text",
                    "logprobs",
                    "sequence_number",
                ],
            ) && !has_sensitive_replay_payload(data)
                && data.get("text").is_some_and(serde_json::Value::is_string)
                && is_missing_or_empty_array(data.get("logprobs"))
        }
        _ => false,
    }
}

fn canonical_response_id<'a>(event_name: &str, data: &'a serde_json::Value) -> Option<&'a str> {
    data.pointer("/response/id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| data.get("response_id").and_then(serde_json::Value::as_str))
        .or_else(|| {
            (matches!(event_name, "response.created" | "response.completed")
                || matches!(
                    data.get("type").and_then(serde_json::Value::as_str),
                    Some("response.created" | "response.completed")
                ))
            .then(|| data.get("id").and_then(serde_json::Value::as_str))
            .flatten()
        })
        .filter(|response_id| !response_id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(event: &str, data: &str, newline: &str) -> String {
        format!("event: {event}{newline}data: {data}{newline}{newline}")
    }

    #[test]
    fn complete_validator_accepts_clean_eof_or_done_after_one_completed() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"r","status":"completed"}}"#,
            "\n",
        );
        let valid_with_done = format!("{completed}data: [DONE]\n\n");
        assert_eq!(
            validate_complete_codex_sse(completed.as_bytes(), &[]),
            Ok(())
        );
        assert_eq!(
            validate_complete_codex_sse(valid_with_done.as_bytes(), &[]),
            Ok(())
        );
        let completed_without_id = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(completed_without_id.as_bytes(), &[]),
            Ok(())
        );
        assert_eq!(
            validate_complete_codex_sse(format!("{completed}{completed}").as_bytes(), &[]),
            Err("duplicate_completed")
        );
        assert_eq!(
            validate_complete_codex_sse(format!("{valid_with_done}: trailing\n\n").as_bytes(), &[],),
            Err("trailing_or_duplicate_terminal")
        );
        assert_eq!(
            validate_complete_codex_sse(b"data: [DONE]\n\n", &[]),
            Err("missing_completed")
        );
        let invalid_completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"incomplete"}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(
                format!("{invalid_completed}data: [DONE]\n\n").as_bytes(),
                &[],
            ),
            Err("invalid_completed")
        );
        let semantic_after_completed = format!(
            "{completed}{}data: [DONE]\n\n",
            frame(
                "response.output_text.delta",
                r#"{"type":"response.output_text.delta","delta":"late"}"#,
                "\n",
            )
        );
        assert_eq!(
            validate_complete_codex_sse(semantic_after_completed.as_bytes(), &[]),
            Err("semantic_frame_after_completed")
        );
        let terminal_after_completed = format!(
            "{completed}{}data: [DONE]\n\n",
            frame(
                "response.failed",
                r#"{"type":"response.failed","response":{"error":{"code":"server_error","message":"late"}}}"#,
                "\n",
            )
        );
        assert_eq!(
            validate_complete_codex_sse(terminal_after_completed.as_bytes(), &[]),
            Err("semantic_frame_after_completed")
        );
    }

    #[test]
    fn complete_validator_accepts_real_codex_responses_shape_without_done() {
        let success = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-x\",\"status\":\"in_progress\",\"model\":\"gpt-5\",\"output\":[]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-x\",\"status\":\"completed\",\"model\":\"gpt-5\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}],\"usage\":{\"input_tokens\":11,\"output_tokens\":1,\"total_tokens\":12}}}\n\n"
        );

        assert_eq!(validate_complete_codex_sse(success.as_bytes(), &[]), Ok(()));
    }

    #[test]
    fn strict_validator_rejects_unknown_semantic_frame_without_changing_live_passthrough() {
        let unknown = frame(
            "response.reasoning.raw",
            r#"{"type":"response.reasoning.raw","encrypted_content":"opaque"}"#,
            "\n",
        );
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );

        assert_eq!(
            validate_complete_codex_sse(format!("{unknown}{completed}").as_bytes(), &[]),
            Err("unsafe_replay_frame")
        );
        let mismatched = frame(
            "response.output_text.delta",
            r#"{"type":"response.reasoning.raw","encrypted_content":"opaque"}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(format!("{mismatched}{completed}").as_bytes(), &[]),
            Err("unknown_semantic_frame")
        );
        let scalar = frame("response.output_text.delta", r#""opaque""#, "\n");
        assert_eq!(
            validate_complete_codex_sse(format!("{scalar}{completed}").as_bytes(), &[]),
            Err("unknown_semantic_frame")
        );

        let mut live = CodexTerminalFirewall::new(&[]);
        let output = live.ingest(unknown.as_bytes());
        assert_eq!(output.bytes.as_ref(), unknown.as_bytes());
        assert!(!output.stop);
        assert!(!output.terminal_error);
    }

    #[test]
    fn strict_validator_rejects_non_final_or_mixed_sensitive_payloads() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let rejected = [
            (
                "known output-text event carrying encrypted content",
                frame(
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","delta":"ok","encrypted_content":"opaque"}"#,
                    "\n",
                ),
            ),
            (
                "reasoning summary",
                frame(
                    "response.reasoning_summary_text.delta",
                    r#"{"type":"response.reasoning_summary_text.delta","delta":"hidden"}"#,
                    "\n",
                ),
            ),
            (
                "non-final refusal",
                frame(
                    "response.refusal.delta",
                    r#"{"type":"response.refusal.delta","delta":"blocked"}"#,
                    "\n",
                ),
            ),
            (
                "reasoning output item",
                frame(
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","item":{"id":"reason_1","type":"reasoning","summary":[{"type":"summary_text","text":"hidden"}]}}"#,
                    "\n",
                ),
            ),
            (
                "function call output item",
                frame(
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","item":{"id":"call_1","type":"function_call","call_id":"call_1","name":"shell","arguments":"{}"}}"#,
                    "\n",
                ),
            ),
            (
                "mixed visible commentary",
                frame(
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","item":{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"},{"type":"commentary","text":"hidden"}]}}"#,
                    "\n",
                ),
            ),
            (
                "unknown field nested in lifecycle response",
                frame(
                    "response.created",
                    r#"{"type":"response.created","response":{"status":"in_progress","output":[],"vendor_visible":"hidden"}}"#,
                    "\n",
                ),
            ),
            (
                "visible output nested in a lifecycle response",
                frame(
                    "response.in_progress",
                    r#"{"type":"response.in_progress","response":{"status":"in_progress","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"must-not-replay"}]}]}}"#,
                    "\n",
                ),
            ),
            (
                "instructions echoed by a lifecycle response",
                frame(
                    "response.created",
                    r#"{"type":"response.created","response":{"status":"in_progress","output":[],"instructions":"must-not-replay"}}"#,
                    "\n",
                ),
            ),
            (
                "unknown metadata nested in a lifecycle response",
                frame(
                    "response.created",
                    r#"{"type":"response.created","response":{"status":"in_progress","output":[],"metadata":{"vendor_visible":"must-not-replay"}}}"#,
                    "\n",
                ),
            ),
            (
                "unknown field mixed into output text",
                frame(
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","delta":"ok","vendor_visible":"hidden"}"#,
                    "\n",
                ),
            ),
            (
                "unknown annotation mixed into output text",
                frame(
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok","annotations":[{"type":"vendor_visible","text":"hidden"}]}]}}"#,
                    "\n",
                ),
            ),
        ];

        for (case, rejected_frame) in &rejected {
            assert_eq!(
                validate_complete_codex_sse(format!("{rejected_frame}{completed}").as_bytes(), &[],),
                Err("unsafe_replay_frame"),
                "{case} must fail closed",
            );
        }

        let mut live = CodexTerminalFirewall::new(&[]);
        let output = live.ingest(rejected[0].1.as_bytes());
        assert_eq!(output.bytes.as_ref(), rejected[0].1.as_bytes());
        assert!(!output.stop);
        assert!(!output.terminal_error);
    }

    #[test]
    fn strict_validator_accepts_plain_output_text_and_classified_final_payloads() {
        let output_text = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-text\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"in_progress\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "event: response.content_part.added\n",
            "data: {\"type\":\"response.content_part.added\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\",\"annotations\":[]}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"hello\",\"logprobs\":[]}\n\n",
            "event: response.output_text.done\n",
            "data: {\"type\":\"response.output_text.done\",\"output_index\":0,\"content_index\":0,\"text\":\"hello\",\"logprobs\":[]}\n\n",
            "event: response.content_part.done\n",
            "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-text\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"status\":\"completed\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\",\"annotations\":[]}]}]}}\n\n"
        );
        assert_eq!(
            validate_complete_codex_sse(output_text.as_bytes(), &[]),
            Ok(())
        );

        let final_refusal = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-refusal","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"refusal","refusal":"blocked"}]}]}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(final_refusal.as_bytes(), &[]),
            Ok(())
        );

        let final_with_safe_usage = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-usage","object":"response","status":"completed","instructions":null,"input":[],"metadata":{},"tools":[],"text":{"format":{"type":"text"}},"tool_choice":"auto","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok","annotations":[]}]}],"usage":{"input_tokens":11,"input_tokens_details":{"cached_tokens":3},"output_tokens":7,"output_tokens_details":{"reasoning_tokens":5},"total_tokens":18}}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(final_with_safe_usage.as_bytes(), &[]),
            Ok(())
        );

        let final_function_call = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-call","status":"completed","output":[{"type":"function_call","status":"completed","call_id":"call_1","name":"shell","arguments":"{}"}]}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(final_function_call.as_bytes(), &[]),
            Ok(())
        );

        let incomplete_final_function_call = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-call","status":"completed","output":[{"type":"function_call","status":"in_progress","call_id":"call_1","name":"shell","arguments":"{}"}]}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(incomplete_final_function_call.as_bytes(), &[]),
            Err("unsafe_replay_frame")
        );

        let mixed_final_payload = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-mixed","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]},{"type":"function_call","status":"completed","call_id":"call_1","name":"shell","arguments":"{}"}]}}"#,
            "\n",
        );
        assert_eq!(
            validate_complete_codex_sse(mixed_final_payload.as_bytes(), &[]),
            Err("unsafe_replay_frame")
        );
    }

    #[test]
    fn complete_validator_rejects_mismatched_response_ids() {
        let mixed = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-a\",\"status\":\"in_progress\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-b\",\"status\":\"completed\"}}\n\n"
        );

        assert_eq!(
            validate_complete_codex_sse(mixed.as_bytes(), &[]),
            Err("response_id_mismatch")
        );

        let missing_completion_id = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-a\",\"status\":\"in_progress\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        assert_eq!(
            validate_complete_codex_sse(missing_completion_id.as_bytes(), &[]),
            Err("response_id_mismatch")
        );

        let data_type_only = concat!(
            "data: {\"type\":\"response.created\",\"id\":\"resp-a\",\"response\":{\"status\":\"in_progress\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"id\":\"resp-a\",\"response\":{\"status\":\"completed\"}}\n\n"
        );
        assert_eq!(
            validate_complete_codex_sse(data_type_only.as_bytes(), &[]),
            Ok(())
        );
    }

    #[test]
    fn live_firewall_rejects_mismatched_or_missing_completion_response_id() {
        let created = frame(
            "response.created",
            r#"{"type":"response.created","response":{"id":"resp-a","status":"in_progress"}}"#,
            "\n",
        );
        let mismatched = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"resp-b","status":"completed"}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&[]);
        assert_eq!(
            firewall.ingest(created.as_bytes()).bytes.as_ref(),
            created.as_bytes()
        );

        let output = firewall.ingest(mismatched.as_bytes());
        assert!(output.bytes.is_empty());
        assert!(output.stop);
        assert!(output.terminal_error);
        assert_eq!(output.fail_closed_reason, Some("response_id_mismatch"));

        let missing_id = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let _ = firewall.ingest(created.as_bytes());
        let output = firewall.ingest(missing_id.as_bytes());
        assert!(output.bytes.is_empty());
        assert!(output.stop);
        assert!(output.terminal_error);
        assert_eq!(output.fail_closed_reason, Some("response_id_mismatch"));
    }

    #[test]
    fn buffers_split_frames_and_preserves_exact_bytes() {
        let raw = frame(
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"ok"}"#,
            "\r\n",
        );
        let split = raw.len() / 2;
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let first = firewall.ingest(&raw.as_bytes()[..split]);
        assert!(first.bytes.is_empty());
        assert!(!first.stop);
        let second = firewall.ingest(&raw.as_bytes()[split..]);
        assert_eq!(second.bytes.as_ref(), raw.as_bytes());
        assert!(!second.stop);
    }

    #[test]
    fn keeps_prior_frames_and_drops_terminal_and_following_frames() {
        let visible = frame(
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"hello"}"#,
            "\n",
        );
        let terminal = frame(
            "response.failed",
            r#"{"type":"response.failed","response":{"error":{"code":"server_error","message":"secret"}}}"#,
            "\n",
        );
        let trailing = frame(
            "response.output_text.delta",
            r#"{"type":"response.output_text.delta","delta":"must-not-leak"}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let output = firewall.ingest(format!("{visible}{terminal}{trailing}").as_bytes());
        assert_eq!(output.bytes.as_ref(), visible.as_bytes());
        assert!(output.stop);
        assert!(output.terminal_error);
        assert_eq!(
            output
                .evidence
                .as_ref()
                .map(|value| value.classification.as_str()),
            Some("transient_provider")
        );
    }

    #[test]
    fn passthrough_keeps_complete_matching_frame_but_never_capacity() {
        let policy = vec!["ticket-123".to_string(), "capacity".to_string()];
        let matched = frame(
            "response.failed",
            r#"{"type":"response.failed","response":{"error":{"code":"odd","message":"ticket-123"}}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&policy);
        let output = firewall.ingest(matched.as_bytes());
        assert_eq!(output.bytes.as_ref(), matched.as_bytes());
        assert!(output.stop);
        assert!(output
            .evidence
            .as_ref()
            .is_some_and(StreamInternalErrorEvidence::is_passthrough_exception));

        let capacity = frame(
            "response.failed",
            r#"{"type":"response.failed","response":{"error":{"code":"server_is_overloaded","message":"capacity ticket-123"}}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&policy);
        let output = firewall.ingest(capacity.as_bytes());
        assert!(output.bytes.is_empty());
        assert!(output.stop);
        assert!(output
            .evidence
            .as_ref()
            .is_some_and(|value| value.classification == "transient_capacity"));

        let policy_frame = frame(
            "response.failed",
            r#"{"type":"response.failed","error":{"code":"content_policy_violation","message":"ticket-123"}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&policy);
        let output = firewall.ingest(policy_frame.as_bytes());
        assert_eq!(output.bytes.as_ref(), policy_frame.as_bytes());
        assert!(output.stop);
        assert!(output
            .evidence
            .as_ref()
            .is_some_and(|value| value.classification == "policy"
                && value.disposition == "passthrough_exception"));

        let auth = frame(
            "response.failed",
            r#"{"type":"response.failed","error":{"code":"invalid_api_key","message":"ticket-123"}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&policy);
        let output = firewall.ingest(auth.as_bytes());
        assert!(output.bytes.is_empty());
        assert!(output.stop);
        assert!(output
            .evidence
            .as_ref()
            .is_some_and(|value| value.classification == "auth"));
    }

    #[test]
    fn default_policy_passes_through_high_risk_cyber_terminal_frame() {
        let policy = crate::infra::settings::UpstreamStreamInternalErrorPolicy::default();
        let terminal = frame(
            "response.failed",
            r#"{"type":"response.failed","response":{"error":{"code":"content_policy_violation","message":"high-risk cyber request"}}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&policy.passthrough_keywords);

        let output = firewall.ingest(terminal.as_bytes());

        assert_eq!(output.bytes.as_ref(), terminal.as_bytes());
        assert!(output.stop);
        assert!(output
            .evidence
            .as_ref()
            .is_some_and(|value| value.classification == "policy"
                && value.disposition == "passthrough_exception"));
    }

    #[test]
    fn normalized_bridge_codex_terminal_frame_uses_the_same_firewall() {
        let normalized = frame(
            "response.error",
            r#"{"type":"response.error","error":{"type":"server_error","message":"normalized bridge failure"}}"#,
            "\n",
        );
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let output = firewall.ingest(normalized.as_bytes());

        assert!(output.bytes.is_empty());
        assert!(output.stop);
        assert!(output.terminal_error);
        assert!(output.evidence.as_ref().is_some_and(|value| {
            value.classification == "transient_provider"
                && value.disposition == "dropped_after_commit"
        }));
    }

    #[test]
    fn malformed_partial_and_oversized_frames_fail_closed() {
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let malformed = firewall.ingest(b"data: not-json\n\n");
        assert!(malformed.stop);
        assert_eq!(malformed.fail_closed_reason, Some("malformed_frame"));

        let mut firewall = CodexTerminalFirewall::new(&[]);
        let disguised_terminal = firewall.ingest(b"event: response.failed\ndata: [DONE]\n\n");
        assert!(disguised_terminal.stop);
        assert_eq!(
            disguised_terminal.fail_closed_reason,
            Some("malformed_frame")
        );

        let mut firewall = CodexTerminalFirewall::new(&[]);
        let _ = firewall.ingest(b"event: response.created\ndata: {");
        let partial = firewall.finish();
        assert!(partial.stop);
        assert_eq!(partial.fail_closed_reason, Some("partial_frame_at_eof"));

        let mut firewall = CodexTerminalFirewall::new(&[]);
        let oversized = firewall.ingest(&vec![b'x'; MAX_TERMINAL_FIREWALL_FRAME_BYTES]);
        assert!(oversized.stop);
        assert_eq!(oversized.fail_closed_reason, Some("frame_too_large"));
    }

    #[test]
    fn done_and_completion_order_is_preserved() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let done = "data: [DONE]\n\n";
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let output = firewall.ingest(format!("{completed}{done}").as_bytes());
        assert_eq!(
            output.bytes.as_ref(),
            format!("{completed}{done}").as_bytes()
        );
        assert!(output.completion_seen);
        assert!(output.stop);
        assert!(!output.terminal_error);
    }

    #[test]
    fn duplicate_completion_fails_closed_before_duplicate_or_done() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let done = "data: [DONE]\n\n";
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let output = firewall.ingest(format!("{completed}{completed}{done}").as_bytes());

        assert_eq!(output.bytes.as_ref(), completed.as_bytes());
        assert!(output.completion_seen);
        assert!(output.stop);
        assert!(output.terminal_error);
        assert_eq!(output.fail_closed_reason, Some("duplicate_completed"));
    }

    #[test]
    fn completion_seen_stays_monotonic_across_keepalive_and_finish() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let keepalive = ": keepalive\n\n";
        let mut firewall = CodexTerminalFirewall::new(&[]);

        let completion = firewall.ingest(completed.as_bytes());
        assert!(completion.completion_seen);
        assert!(!completion.stop);

        let heartbeat = firewall.ingest(keepalive.as_bytes());
        assert_eq!(heartbeat.bytes.as_ref(), keepalive.as_bytes());
        assert!(heartbeat.completion_seen);
        assert!(!heartbeat.stop);

        let eof = firewall.finish();
        assert!(eof.bytes.is_empty());
        assert!(eof.completion_seen);
        assert!(!eof.stop);
        assert!(!eof.terminal_error);
    }
}
