//! Shared SSE frame helpers for gateway proxy paths.

use serde_json::Value;

/// Find the byte offset immediately after the first complete SSE event,
/// terminated by `\n\n` or `\r\n\r\n`.
pub(in crate::gateway::proxy) fn find_sse_event_end(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < buffer.len() {
        if buffer[i] == b'\n' {
            if i + 1 < buffer.len() && buffer[i + 1] == b'\n' {
                return Some(i + 2);
            }
        } else if buffer[i] == b'\r'
            && i + 3 < buffer.len()
            && buffer[i + 1] == b'\n'
            && buffer[i + 2] == b'\r'
            && buffer[i + 3] == b'\n'
        {
            return Some(i + 4);
        }
        i += 1;
    }
    None
}

/// Parse a single SSE frame string into (event_type, data_json).
///
/// Supports both `event: xxx\ndata: {...}\n\n` and `data: {...}\n\n` formats.
/// In the latter case, the event type is inferred from the `type` field of the
/// JSON data.
pub(in crate::gateway::proxy) fn parse_sse_frame(frame: &str) -> Option<(String, Value)> {
    let mut event_type = None;
    let mut data_parts: Vec<&str> = Vec::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            let payload = rest.trim_start();
            if payload == "[DONE]" {
                return None;
            }
            data_parts.push(payload);
        }
    }

    if data_parts.is_empty() {
        return None;
    }
    let data_str = data_parts.join("\n");
    let data: Value = serde_json::from_str(&data_str).ok()?;

    let event_type = event_type.unwrap_or_else(|| {
        data.get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_string()
    });

    Some((event_type, data))
}

/// Aggregate an OpenAI Responses SSE stream into a single JSON response.
pub(in crate::gateway::proxy) fn aggregate_responses_event_stream(
    raw: &[u8],
) -> Result<Value, String> {
    let mut response: Option<Value> = None;
    let mut output: Vec<Value> = Vec::new();
    let mut cursor = 0usize;
    let mut saw_completed = false;
    let mut saw_done = false;

    while let Some(relative_end) = find_sse_event_end(&raw[cursor..]) {
        let event_end = cursor + relative_end;
        let frame = &raw[cursor..event_end];
        cursor = event_end;
        let text =
            std::str::from_utf8(frame).map_err(|e| format!("invalid utf-8 in SSE frame: {e}"))?;
        if sse_frame_has_done_data(text) {
            saw_done = true;
        }
        let Some((event_name, data)) = parse_sse_frame(text) else {
            continue;
        };

        match event_name.as_str() {
            "response.created" => {
                let created = data.get("response").cloned().unwrap_or(data);
                response = Some(created);
            }
            "response.output_item.done" => {
                let item = data
                    .get("item")
                    .cloned()
                    .ok_or_else(|| "missing item in response.output_item.done".to_string())?;
                upsert_output_item(&mut output, item);
            }
            "response.completed" => {
                saw_completed = true;
                let completed = data.get("response").cloned().unwrap_or(data);
                if let Some(existing) = response.as_mut() {
                    merge_response_object(existing, &completed);
                } else {
                    response = Some(completed);
                }
            }
            "response.failed" => {
                return Err(sse_error_detail(&data, "response.failed"));
            }
            "response.incomplete" => {
                return Err(sse_error_detail(&data, "response.incomplete"));
            }
            "error" => {
                return Err(sse_error_detail(&data, "unknown SSE error"));
            }
            _ => {}
        }
    }

    let mut response =
        response.ok_or_else(|| "missing response.created/response.completed".to_string())?;
    if !saw_completed {
        return if saw_done {
            Err("responses stream ended with [DONE] before response.completed".to_string())
        } else {
            Err("missing response.completed".to_string())
        };
    }
    let obj = response
        .as_object_mut()
        .ok_or_else(|| "aggregated response is not an object".to_string())?;
    obj.insert("output".to_string(), Value::Array(output));
    Ok(response)
}

fn merge_response_object(base: &mut Value, update: &Value) {
    let (Some(base_obj), Some(update_obj)) = (base.as_object_mut(), update.as_object()) else {
        *base = update.clone();
        return;
    };

    for (key, value) in update_obj {
        if key == "output" {
            continue;
        }
        base_obj.insert(key.clone(), value.clone());
    }
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
    output.push(item);
}

fn sse_frame_has_done_data(frame: &str) -> bool {
    frame.lines().any(|line| {
        line.trim_end_matches('\r')
            .strip_prefix("data:")
            .is_some_and(|payload| payload.trim_start() == "[DONE]")
    })
}

fn sse_error_detail(data: &Value, fallback: &str) -> String {
    data.get("detail")
        .and_then(Value::as_str)
        .or_else(|| data.get("message").and_then(Value::as_str))
        .or_else(|| data.pointer("/error/message").and_then(Value::as_str))
        .or_else(|| {
            data.pointer("/response/error/message")
                .and_then(Value::as_str)
        })
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_sse_event_end_basic() {
        assert_eq!(find_sse_event_end(b"abc\n\ndef"), Some(5));
        assert_eq!(find_sse_event_end(b"abc\ndef"), None);
        assert_eq!(find_sse_event_end(b"\n\n"), Some(2));
        assert_eq!(find_sse_event_end(b"abc\r\n\r\ndef"), Some(7));
    }

    #[test]
    fn parse_sse_frame_with_event() {
        let frame = "event: response.created\ndata: {\"id\":\"r1\"}\n\n";
        let (evt, data) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "response.created");
        assert_eq!(data.get("id").unwrap().as_str().unwrap(), "r1");
    }

    #[test]
    fn parse_sse_frame_data_only_infers_type() {
        let frame = "data: {\"type\":\"response.completed\",\"id\":\"r2\"}\n\n";
        let (evt, data) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "response.completed");
        assert_eq!(data.get("id").unwrap().as_str().unwrap(), "r2");
    }

    #[test]
    fn parse_sse_frame_done_returns_none() {
        let frame = "data: [DONE]\n\n";
        assert!(parse_sse_frame(frame).is_none());
    }

    #[test]
    fn parse_sse_frame_comment_lines_ignored() {
        let frame = ": keepalive\ndata: {\"type\":\"ping\"}\n\n";
        let (evt, _) = parse_sse_frame(frame).unwrap();
        assert_eq!(evt, "ping");
    }

    #[test]
    fn aggregate_responses_event_stream_handles_many_frames_with_trailing_partial() {
        let mut raw = String::from(
            "event: response.created\n\
             data: {\"response\":{\"id\":\"resp_many\",\"status\":\"in_progress\"}}\n\n",
        );
        for index in 0..128 {
            raw.push_str(&format!(
                "event: response.output_item.done\n\
                 data: {{\"item\":{{\"id\":\"msg_{index}\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}}}\n\n"
            ));
        }
        raw.push_str(
            "event: response.completed\n\
             data: {\"response\":{\"id\":\"resp_many\",\"status\":\"completed\"}}\n\n\
             event: response.output_item.done\n\
             data: {\"item\":{\"id\":\"partial\"",
        );

        let aggregated = aggregate_responses_event_stream(raw.as_bytes()).expect("aggregate");

        assert_eq!(aggregated["id"], "resp_many");
        assert_eq!(aggregated["status"], "completed");
        assert_eq!(
            aggregated
                .get("output")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(128)
        );
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_response_failed_terminal() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_failed\",\"status\":\"in_progress\"}}\n\n",
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed\",\"error\":{\"message\":\"upstream failed\"}}}\n\n",
            ": keepalive\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(err, "upstream failed");
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_response_incomplete_terminal() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_incomplete\",\"status\":\"in_progress\"}}\n\n",
            "event: response.incomplete\n",
            "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_incomplete\",\"status\":\"incomplete\"}}\n\n",
            ": keepalive\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(err, "response.incomplete");
    }

    #[test]
    fn aggregate_responses_event_stream_rejects_done_without_completed() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"response\":{\"id\":\"resp_done\",\"status\":\"in_progress\"}}\n\n",
            "data: [DONE]\n\n"
        );

        let err = aggregate_responses_event_stream(raw.as_bytes()).expect_err("aggregate failure");

        assert_eq!(
            err,
            "responses stream ended with [DONE] before response.completed"
        );
    }
}
