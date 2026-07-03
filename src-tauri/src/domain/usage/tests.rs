use super::*;

#[test]
fn parse_openai_chatcompletions_usage() {
    let body =
        br#"{"id":"x","usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;
    let extract = parse_usage_from_json_bytes(body).expect("should parse usage");
    assert_eq!(extract.metrics.input_tokens, Some(10));
    assert_eq!(extract.metrics.output_tokens, Some(5));
    assert_eq!(extract.metrics.total_tokens, Some(15));
    assert_eq!(extract.metrics.cache_read_input_tokens, None);
}

#[test]
fn parse_openai_responses_usage_with_cached_tokens() {
    let body = br#"{"usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":5}}}"#;
    let extract = parse_usage_from_json_bytes(body).expect("should parse usage");
    assert_eq!(extract.metrics.input_tokens, Some(11));
    assert_eq!(extract.metrics.output_tokens, Some(7));
    assert_eq!(extract.metrics.total_tokens, Some(18));
    assert_eq!(extract.metrics.reasoning_tokens, Some(5));
    assert_eq!(extract.metrics.cache_read_input_tokens, Some(3));
    let usage_json: serde_json::Value =
        serde_json::from_str(&extract.usage_json).expect("normalized usage json");
    assert_eq!(
        usage_json
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(serde_json::Value::as_i64),
        Some(5)
    );
}

#[test]
fn parse_reasoning_tokens_from_camel_case_usage_shapes() {
    let body = br#"{"usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"outputTokensDetails":{"reasoningTokens":6}}}"#;
    let extract = parse_usage_from_json_bytes(body).expect("should parse usage");
    assert_eq!(extract.metrics.reasoning_tokens, Some(6));

    let body = br#"{"usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"reasoningTokenCount":9}}"#;
    let extract = parse_usage_from_json_bytes(body).expect("should parse usage");
    assert_eq!(extract.metrics.reasoning_tokens, Some(9));
}

#[test]
fn parse_gemini_usage_metadata() {
    let body = br#"{"usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":9,"thoughtsTokenCount":2,"totalTokenCount":19,"cachedContentTokenCount":4}}"#;
    let extract = parse_usage_from_json_bytes(body).expect("should parse usage");
    assert_eq!(extract.metrics.input_tokens, Some(8));
    assert_eq!(extract.metrics.output_tokens, Some(11));
    assert_eq!(extract.metrics.total_tokens, Some(19));
    assert_eq!(extract.metrics.reasoning_tokens, Some(2));
    assert_eq!(extract.metrics.cache_read_input_tokens, Some(4));
}

#[test]
fn parse_claude_sse_merge_message_start_and_delta() {
    let sse = b"event: message_start\n\
            data: {\"message\":{\"model\":\"claude-haiku-4-5-20251001\",\"usage\":{\"cache_creation\":{\"ephemeral_5m_input_tokens\":20,\"ephemeral_1h_input_tokens\":5},\"cache_read_input_tokens\":4}}}\n\
            \n\
            event: message_delta\n\
            data: {\"delta\":{\"usage\":{\"input_tokens\":30,\"output_tokens\":10,\"total_tokens\":40}}}\n\
            \n";

    let mut tracker = SseUsageTracker::new("claude");
    tracker.ingest_chunk(&sse[..20]);
    tracker.ingest_chunk(&sse[20..]);
    let extract = tracker.finalize().expect("should parse usage");

    assert_eq!(
        tracker.best_effort_model().as_deref(),
        Some("claude-haiku-4-5-20251001")
    );
    assert_eq!(extract.metrics.input_tokens, Some(30));
    assert_eq!(extract.metrics.output_tokens, Some(10));
    assert_eq!(extract.metrics.total_tokens, Some(40));
    assert_eq!(extract.metrics.cache_read_input_tokens, Some(4));
    assert_eq!(extract.metrics.cache_creation_5m_input_tokens, Some(20));
    assert_eq!(extract.metrics.cache_creation_1h_input_tokens, Some(5));
    assert_eq!(extract.metrics.cache_creation_input_tokens, Some(25));
}

#[test]
fn parse_model_top_level() {
    let body = br#"{"model":"claude-opus-4-5-20251101"}"#;
    assert_eq!(
        parse_model_from_json_bytes(body).as_deref(),
        Some("claude-opus-4-5-20251101")
    );
}

#[test]
fn parse_model_nested_message() {
    let body = br#"{"message":{"model":"claude-haiku-4-5-20251001"}}"#;
    assert_eq!(
        parse_model_from_json_bytes(body).as_deref(),
        Some("claude-haiku-4-5-20251001")
    );
}

#[test]
fn parse_generic_sse_usage_without_event_name() {
    let sse =
        b"data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let extract = tracker.finalize().expect("should parse usage");
    assert_eq!(extract.metrics.input_tokens, Some(1));
    assert_eq!(extract.metrics.output_tokens, Some(2));
    assert_eq!(extract.metrics.total_tokens, Some(3));
}

#[test]
fn parse_sse_done_marker_marks_completion_seen() {
    let sse = b"data: [DONE]\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    assert!(tracker.completion_seen());
    assert!(tracker.finalize().is_none());
}

#[test]
fn parse_claude_message_stop_type_marks_completion_seen() {
    let sse = b"data: {\"type\":\"message_stop\"}\n\n";
    let mut tracker = SseUsageTracker::new("claude");
    tracker.ingest_chunk(sse);
    let _ = tracker.finalize();
    assert!(tracker.completion_seen());
}

#[test]
fn parse_codex_response_completed_marks_completion_seen() {
    let sse = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let extract = tracker.finalize().expect("should parse usage");
    assert!(tracker.completion_seen());
    assert_eq!(extract.metrics.input_tokens, Some(1));
    assert_eq!(extract.metrics.output_tokens, Some(2));
    assert_eq!(extract.metrics.total_tokens, Some(3));
}

#[test]
fn parse_openai_chat_finish_reason_marks_completion_seen() {
    let sse = b"data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let _ = tracker.finalize();
    assert!(tracker.completion_seen());
}

#[test]
fn parse_gemini_finish_reason_marks_completion_seen() {
    let sse = b"data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n\n";
    let mut tracker = SseUsageTracker::new("gemini");
    tracker.ingest_chunk(sse);
    let _ = tracker.finalize();
    assert!(tracker.completion_seen());
}

#[test]
fn codex_output_text_delta_marks_meaningful_output() {
    let sse = b"event: response.output_text.delta\n\
                data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n\
                event: response.completed\n\
                data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let usage = tracker.finalize();

    assert!(tracker.meaningful_output_seen());
    assert!(!tracker.is_empty_success("/v1/responses", 200, usage.as_ref()));
}

#[test]
fn codex_completed_output_text_marks_meaningful_output() {
    let sse = b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let usage = tracker.finalize();

    assert!(tracker.meaningful_output_seen());
    assert!(!tracker.is_empty_success("/responses", 200, usage.as_ref()));
}

#[test]
fn codex_function_call_output_marks_meaningful_output() {
    let sse = b"event: response.output_item.done\n\
                data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"call_1\",\"type\":\"function_call\",\"name\":\"lookup\",\"arguments\":\"{}\"}}\n\n\
                event: response.completed\n\
                data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let usage = tracker.finalize();

    assert!(tracker.meaningful_output_seen());
    assert!(!tracker.is_empty_success("/v1/responses", 200, usage.as_ref()));
}

#[test]
fn codex_reasoning_delta_does_not_mask_empty_success() {
    let sse = b"event: response.reasoning_text.delta\n\
                data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"internal reasoning\"}\n\n\
                event: response.completed\n\
                data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let usage = tracker.finalize();

    assert!(tracker.completion_seen());
    assert!(!tracker.meaningful_output_seen());
    assert!(tracker.is_empty_success("/v1/responses", 200, usage.as_ref()));
    assert!(tracker.is_empty_success("/v1/codex/responses", 200, usage.as_ref()));
}

#[test]
fn codex_empty_success_requires_zero_output_tokens() {
    let sse = b"event: response.completed\n\
                data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let usage = tracker.finalize();

    assert!(tracker.completion_seen());
    assert!(!tracker.meaningful_output_seen());
    assert!(tracker.is_empty_success("/v1/responses", 200, usage.as_ref()));
}

#[test]
fn empty_success_is_scoped_to_codex_responses_success_without_errors() {
    let sse = b"event: response.completed\n\
                data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}}\n\n";

    let mut codex_tracker = SseUsageTracker::new("codex");
    codex_tracker.ingest_chunk(sse);
    let codex_usage = codex_tracker.finalize();
    assert!(!codex_tracker.is_empty_success("/v1/chat/completions", 200, codex_usage.as_ref()));
    assert!(!codex_tracker.is_empty_success("/v1/responses", 500, codex_usage.as_ref()));

    let mut claude_tracker = SseUsageTracker::new("claude");
    claude_tracker.ingest_chunk(sse);
    let claude_usage = claude_tracker.finalize();
    assert!(!claude_tracker.is_empty_success("/v1/responses", 200, claude_usage.as_ref()));

    let error_sse = b"event: response.error\n\
                    data: {\"type\":\"response.error\",\"error\":{\"message\":\"broken\"},\"usage\":{\"input_tokens\":1,\"output_tokens\":0,\"total_tokens\":1}}\n\n";
    let mut error_tracker = SseUsageTracker::new("codex");
    error_tracker.ingest_chunk(error_sse);
    let error_usage = error_tracker.finalize();
    assert!(!error_tracker.is_empty_success("/v1/responses", 200, error_usage.as_ref()));
}

#[test]
fn parse_codex_sse_keeps_positive_reasoning_when_later_usage_reports_zero() {
    let sse = b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"total_tokens\":30,\"output_tokens_details\":{\"reasoning_tokens\":304}}}}\n\n\
                data: {\"type\":\"response.done\",\"response\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"total_tokens\":30,\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    let extract = tracker.finalize().expect("should parse usage");
    assert_eq!(extract.metrics.reasoning_tokens, Some(304));
    let usage_json: serde_json::Value =
        serde_json::from_str(&extract.usage_json).expect("normalized usage json");
    assert_eq!(
        usage_json
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(serde_json::Value::as_i64),
        Some(304)
    );
}

#[test]
fn parse_sse_error_event_marks_terminal_error_seen() {
    let sse = b"event: error\ndata: {\"error\":{\"message\":\"upstream failed\"}}\n\n";
    let mut tracker = SseUsageTracker::new("claude");
    tracker.ingest_chunk(sse);
    assert!(tracker.terminal_error_seen());
}

#[test]
fn parse_response_error_type_marks_terminal_error_seen() {
    let sse = b"data: {\"type\":\"response.error\",\"error\":{\"message\":\"broken\"}}\n\n";
    let mut tracker = SseUsageTracker::new("codex");
    tracker.ingest_chunk(sse);
    assert!(tracker.terminal_error_seen());
}

#[test]
fn sse_usage_tracker_drops_oversized_pending_line() {
    let mut tracker = SseUsageTracker::new("codex");
    let oversized = vec![b'a'; MAX_SSE_USAGE_TRACKER_PENDING_BYTES + 1];

    tracker.ingest_chunk(&oversized);

    assert!(tracker.buffer.is_empty());
    assert!(tracker.current_event.is_empty());
    assert!(tracker.current_data.is_empty());
    assert!(tracker.finalize().is_none());
}

#[test]
fn sse_usage_tracker_recovers_after_oversized_pending_line() {
    let mut tracker = SseUsageTracker::new("codex");
    let oversized = vec![b'a'; MAX_SSE_USAGE_TRACKER_PENDING_BYTES + 1];

    tracker.ingest_chunk(&oversized);
    tracker.ingest_chunk(b"\n\n");
    tracker.ingest_chunk(
        b"data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
    );
    let extract = tracker.finalize().expect("should parse later valid usage");

    assert_eq!(extract.metrics.input_tokens, Some(1));
    assert_eq!(extract.metrics.output_tokens, Some(2));
    assert_eq!(extract.metrics.total_tokens, Some(3));
}

#[test]
fn sse_usage_tracker_drops_oversized_event_data() {
    let mut tracker = SseUsageTracker::new("codex");
    let half = vec![b'a'; (MAX_SSE_USAGE_TRACKER_PENDING_BYTES / 2) + 1];

    tracker.ingest_chunk(b"data: ");
    tracker.ingest_chunk(&half);
    tracker.ingest_chunk(b"\n");
    tracker.ingest_chunk(b"data: ");
    tracker.ingest_chunk(&half);
    tracker.ingest_chunk(b"\n\n");

    assert!(tracker.buffer.is_empty());
    assert!(tracker.current_event.is_empty());
    assert!(tracker.current_data.is_empty());
    assert!(tracker.finalize().is_none());
}
