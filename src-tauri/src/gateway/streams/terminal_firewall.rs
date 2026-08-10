//! Bounded frame-level firewall for the final Codex Responses SSE wire format.

use crate::usage::{self, StreamInternalErrorEvidence};
use axum::body::Bytes;

pub(super) const MAX_TERMINAL_FIREWALL_FRAME_BYTES: usize = 1024 * 1024;

/// Validates a fully buffered final-wire Codex Responses SSE payload.
///
/// This deliberately reuses the live firewall state machine. Exact-byte
/// equality additionally rejects duplicate terminals and any bytes after
/// `[DONE]`, both of which the live relay safely suppresses after commit.
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
            return Err(eof.fail_closed_reason.unwrap_or("missing_done"));
        }
        return Err("missing_done");
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
    completion_forwarded: bool,
    strict_complete: bool,
}

impl CodexTerminalFirewall {
    pub(super) fn new(passthrough_keywords: &[String]) -> Self {
        Self {
            passthrough_keywords: passthrough_keywords.to_vec(),
            pending: Vec::new(),
            stopped: false,
            completion_forwarded: false,
            strict_complete: false,
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
            return TerminalFirewallOutput::stopped(Vec::new(), false, false, None, None);
        }

        let mut forwarded = Vec::with_capacity(input.len().min(64 * 1024));
        let mut completion_seen = false;

        while !input.is_empty() {
            let room = MAX_TERMINAL_FIREWALL_FRAME_BYTES.saturating_sub(self.pending.len());
            if room == 0 {
                self.stopped = true;
                self.pending.clear();
                return TerminalFirewallOutput::stopped(
                    forwarded,
                    true,
                    completion_seen,
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
                        if !completion || !self.completion_forwarded {
                            forwarded.extend_from_slice(frame.as_slice());
                        }
                        if completion {
                            self.completion_forwarded = true;
                            completion_seen = true;
                        }
                    }
                    FrameDecision::Done => {
                        forwarded.extend_from_slice(frame.as_slice());
                        self.stopped = true;
                        self.pending.clear();
                        return TerminalFirewallOutput::stopped(
                            forwarded,
                            false,
                            completion_seen,
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
                            completion_seen,
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
                            completion_seen,
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
                            completion_seen,
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
                    completion_seen,
                    None,
                    Some("frame_too_large"),
                );
            }
        }

        TerminalFirewallOutput::open(forwarded, completion_seen)
    }

    pub(super) fn finish(&mut self) -> TerminalFirewallOutput {
        if self.stopped || self.pending.is_empty() {
            return TerminalFirewallOutput::open(Vec::new(), false);
        }
        self.stopped = true;
        self.pending.clear();
        TerminalFirewallOutput::stopped(Vec::new(), true, false, None, Some("partial_frame_at_eof"))
    }

    fn inspect_frame(&self, frame: &[u8]) -> FrameDecision {
        let Ok(text) = std::str::from_utf8(frame) else {
            return FrameDecision::FailClosed("invalid_utf8");
        };

        if let Some((event_name, data)) = crate::gateway::proxy::sse::parse_sse_frame(text) {
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

            let completion = is_completion_frame(&event_name, &data);
            if self.strict_complete {
                if completion && !is_valid_completion_frame(&data) {
                    return FrameDecision::FailClosed("invalid_completed");
                }
                if self.completion_forwarded {
                    return FrameDecision::FailClosed("semantic_frame_after_completed");
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(event: &str, data: &str, newline: &str) -> String {
        format!("event: {event}{newline}data: {data}{newline}{newline}")
    }

    #[test]
    fn complete_validator_requires_one_completed_then_done_and_exact_eof() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"id":"r","status":"completed"}}"#,
            "\n",
        );
        let valid = format!("{completed}data: [DONE]\n\n");
        assert_eq!(validate_complete_codex_sse(valid.as_bytes(), &[]), Ok(()));
        assert_eq!(
            validate_complete_codex_sse(completed.as_bytes(), &[]),
            Err("missing_done")
        );
        assert_eq!(
            validate_complete_codex_sse(format!("{valid}: trailing\n\n").as_bytes(), &[]),
            Err("trailing_or_duplicate_terminal")
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
    fn duplicate_completion_is_forwarded_once_before_done() {
        let completed = frame(
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed"}}"#,
            "\n",
        );
        let done = "data: [DONE]\n\n";
        let mut firewall = CodexTerminalFirewall::new(&[]);
        let output = firewall.ingest(format!("{completed}{completed}{done}").as_bytes());

        assert_eq!(
            output.bytes.as_ref(),
            format!("{completed}{done}").as_bytes()
        );
        assert!(output.completion_seen);
        assert!(output.stop);
    }
}
