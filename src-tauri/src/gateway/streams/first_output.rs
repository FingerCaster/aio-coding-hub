//! Per-attempt deadline until a native Codex Responses stream produces useful output.

use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

use crate::domain::usage;
use crate::gateway::proxy::sse::parse_sse_frame;

const MAX_PENDING_FRAME_BYTES: usize = 1024 * 1024;

/// Created once on receipt of SSE headers and transferred unchanged across commit.
#[derive(Debug, Clone, Copy)]
pub(in crate::gateway) struct FirstOutputDeadline {
    pub(in crate::gateway) deadline: Instant,
    pub(in crate::gateway) budget: Duration,
    pub(in crate::gateway) source: &'static str,
}

impl FirstOutputDeadline {
    pub(in crate::gateway) fn new(budget: Duration, source: &'static str) -> Self {
        Self {
            deadline: Instant::now() + budget,
            budget,
            source,
        }
    }

    pub(in crate::gateway) fn expired(self) -> bool {
        Instant::now() >= self.deadline
    }
}

fn nonempty(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
}

fn custom_tool_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("custom_tool_call")
        && (nonempty(item.get("input"))
            || (nonempty(item.get("name")) && nonempty(item.get("call_id"))))
}

/// Local lifecycle policy only; billing and empty-response classification stay independent.
pub(in crate::gateway) fn has_codex_first_output(data: &Value) -> bool {
    usage::has_codex_meaningful_output(data)
        || match data.get("type").and_then(Value::as_str) {
            Some("response.custom_tool_call_input.delta") => nonempty(data.get("delta")),
            Some("response.custom_tool_call_input.done") => nonempty(data.get("input")),
            _ => false,
        }
        || data.get("item").is_some_and(custom_tool_item)
        || [
            data.get("output"),
            data.get("response").and_then(|v| v.get("output")),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .any(|items| items.iter().any(custom_tool_item))
}

fn terminal(event: &str, data: &Value) -> bool {
    matches!(
        event,
        "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.error"
            | "error"
    ) || matches!(
        data.get("type").and_then(Value::as_str),
        Some(
            "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "response.error"
                | "error"
        )
    )
}

/// Bounded parser for the early-release relay. Replay starts at a frame boundary; no prefix
/// parser tail is copied, and large/unknown frames never extend the absolute deadline.
pub(in crate::gateway) struct FirstOutputWait {
    deadline: Option<FirstOutputDeadline>,
    pending: Vec<u8>,
    oversized: bool,
}

impl FirstOutputWait {
    pub(in crate::gateway) fn new(deadline: FirstOutputDeadline) -> Self {
        Self {
            deadline: Some(deadline),
            pending: Vec::new(),
            oversized: false,
        }
    }

    pub(in crate::gateway) fn deadline(&self) -> Option<FirstOutputDeadline> {
        self.deadline
    }

    pub(in crate::gateway) fn ingest_chunk(&mut self, chunk: &[u8]) {
        if self.deadline.is_none_or(FirstOutputDeadline::expired) {
            return;
        }
        for &byte in chunk {
            if self.pending.len() == MAX_PENDING_FRAME_BYTES {
                self.pending.drain(..self.pending.len() - 3);
                self.oversized = true;
            }
            self.pending.push(byte);
            if self.pending.ends_with(b"\n\n") || self.pending.ends_with(b"\r\n\r\n") {
                if !self.oversized {
                    if let Some((event, data)) = std::str::from_utf8(&self.pending)
                        .ok()
                        .and_then(parse_sse_frame)
                    {
                        if self.deadline.is_some_and(FirstOutputDeadline::expired) {
                            return;
                        }
                        if terminal(&event, &data) || has_codex_first_output(&data) {
                            // Terminal frames are handled by the existing tracker/firewall.
                            self.deadline = None;
                            self.pending = Vec::new();
                            return;
                        }
                    }
                }
                self.pending.clear();
                self.oversized = false;
            } else if self.oversized && self.pending.len() > 4 {
                self.pending.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn first_output_recognizes_only_supported_progress() {
        for data in [
            json!({"type":"response.output_text.delta","delta":"hello"}),
            json!({"type":"response.refusal.delta","delta":"no"}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"thinking"}),
            json!({"type":"response.function_call_arguments.delta","delta":"{"}),
            json!({"type":"response.custom_tool_call_input.delta","delta":"patch"}),
            json!({"type":"response.custom_tool_call_input.done","input":"patch"}),
            json!({"item":{"type":"custom_tool_call","name":"apply_patch","call_id":"call_1"}}),
            json!({"response":{"output":[{"type":"custom_tool_call","input":"patch"}]}}),
        ] {
            assert!(has_codex_first_output(&data), "{data}");
        }
        for data in [
            json!({"type":"response.created","response":{"output":[]}}),
            json!({"type":"response.in_progress","response":{"output":[]}}),
            json!({"type":"response.output_text.delta","delta":""}),
            json!({"type":"response.custom_tool_call_input.delta","delta":""}),
            json!({"type":"unknown","delta":"text"}),
            json!({"usage":{"output_tokens":100}}),
            json!({"item":{"type":"custom_tool_call","name":"apply_patch"}}),
        ] {
            assert!(!has_codex_first_output(&data), "{data}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn first_output_heartbeats_never_extend_deadline() {
        let deadline = FirstOutputDeadline::new(Duration::from_secs(300), "global");
        let mut wait = FirstOutputWait::new(deadline);
        for _ in 0..20 {
            wait.ingest_chunk(b"data: {\"type\":\"response.in_progress\",\"response\":{\"output\":[]}}\n\n: keepalive\n\n");
            tokio::time::advance(Duration::from_secs(15)).await;
        }
        assert_eq!(wait.deadline().unwrap().deadline, deadline.deadline);
        assert!(wait.deadline().unwrap().expired());
        wait.ingest_chunk(
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"too late\"}\n\n",
        );
        assert!(wait.deadline().is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn first_output_split_crlf_disarms_permanently_before_guard() {
        let mut wait =
            FirstOutputWait::new(FirstOutputDeadline::new(Duration::from_secs(1), "provider"));
        tokio::time::advance(Duration::from_millis(999)).await;
        for byte in b"event: response.custom_tool_call_input.delta\r\ndata: {\"type\":\"response.custom_tool_call_input.delta\",\"delta\":\"patch\"}\r\n\r\n" {
            wait.ingest_chunk(&[*byte]);
        }
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert!(wait.deadline().is_none());
        assert!(wait.pending.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn first_output_oversized_and_malformed_frames_remain_bounded_and_recover() {
        let mut wait =
            FirstOutputWait::new(FirstOutputDeadline::new(Duration::from_secs(1), "global"));
        wait.ingest_chunk(&vec![b'x'; MAX_PENDING_FRAME_BYTES * 2]);
        assert!(wait.pending.len() <= 4);
        wait.ingest_chunk(b"\n\ndata: invalid\n\n: keepalive\n\n");
        assert!(wait.deadline().is_some());
        wait.ingest_chunk(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n");
        assert!(wait.deadline().is_none());
    }
}
