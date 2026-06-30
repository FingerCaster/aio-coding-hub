//! Outbound adapter for Anthropic Messages upstreams.

use crate::gateway::proxy::protocol_bridge::ir::*;
use crate::gateway::proxy::protocol_bridge::traits::*;
use serde_json::{json, Value};

pub(crate) struct AnthropicMessagesOutbound;

impl Outbound for AnthropicMessagesOutbound {
    fn protocol(&self) -> &'static str {
        "anthropic_messages"
    }

    fn target_path(&self) -> &str {
        "/v1/messages"
    }

    fn ir_to_request(
        &self,
        ir: &InternalRequest,
        _ctx: &BridgeContext,
    ) -> Result<Value, BridgeError> {
        ir_to_request(ir)
    }

    fn response_to_ir(
        &self,
        body: Value,
        _ctx: &BridgeContext,
    ) -> Result<InternalResponse, BridgeError> {
        response_to_ir(body)
    }

    fn sse_event_to_ir(
        &self,
        event_type: &str,
        data: &Value,
        state: &mut StreamState,
    ) -> Result<Vec<IRStreamChunk>, BridgeError> {
        sse_event_to_ir(event_type, data, state)
    }
}

fn ir_to_request(ir: &InternalRequest) -> Result<Value, BridgeError> {
    let mut messages = Vec::new();
    for msg in &ir.messages {
        let role = match msg.role {
            IRRole::User => "user",
            IRRole::Assistant => "assistant",
        };
        let mut content = Vec::new();
        for block in &msg.content {
            content.push(match block {
                IRContentBlock::Text { text } => json!({"type": "text", "text": text}),
                IRContentBlock::Image { media_type, data } => json!({
                    "type": "image",
                    "source": {"type": "base64", "media_type": media_type, "data": data}
                }),
                IRContentBlock::ToolUse { id, name, input } => {
                    json!({"type": "tool_use", "id": id, "name": name, "input": input})
                }
                IRContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error
                }),
                IRContentBlock::Thinking { thinking } => {
                    json!({"type": "thinking", "thinking": thinking})
                }
            });
        }
        messages.push(json!({"role": role, "content": content}));
    }

    let mut result = json!({
        "model": ir.model,
        "messages": messages,
        "stream": ir.stream
    });
    if let Some(system) = ir.system.as_deref().filter(|value| !value.is_empty()) {
        result["system"] = json!(system);
    }
    if let Some(max_tokens) = ir.max_tokens {
        result["max_tokens"] = json!(max_tokens);
    }
    if let Some(temperature) = ir.temperature {
        result["temperature"] = json!(temperature);
    }
    if let Some(top_p) = ir.top_p {
        result["top_p"] = json!(top_p);
    }
    if !ir.stop_sequences.is_empty() {
        result["stop_sequences"] = json!(ir.stop_sequences);
    }
    if !ir.tools.is_empty() {
        result["tools"] = json!(ir
            .tools
            .iter()
            .map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters
            }))
            .collect::<Vec<_>>());
    }
    if let Some(choice) = &ir.tool_choice {
        result["tool_choice"] = match choice {
            IRToolChoice::Auto => json!({"type": "auto"}),
            IRToolChoice::Required => json!({"type": "any"}),
            IRToolChoice::None => json!({"type": "none"}),
            IRToolChoice::Specific { name } => json!({"type": "tool", "name": name}),
        };
    }
    Ok(result)
}

fn response_to_ir(body: Value) -> Result<InternalResponse, BridgeError> {
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| parse_content_blocks(blocks))
        .transpose()?
        .unwrap_or_default();
    Ok(InternalResponse {
        id: body
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        stop_reason: stop_reason_from_anthropic(body.get("stop_reason").and_then(Value::as_str)),
        usage: parse_usage(body.get("usage")),
        content,
    })
}

fn parse_content_blocks(blocks: &[Value]) -> Result<Vec<IRContentBlock>, BridgeError> {
    blocks.iter().map(parse_content_block).collect()
}

fn parse_content_block(block: &Value) -> Result<IRContentBlock, BridgeError> {
    match block.get("type").and_then(Value::as_str).unwrap_or("") {
        "text" => Ok(IRContentBlock::Text {
            text: block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        "tool_use" => Ok(IRContentBlock::ToolUse {
            id: block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            input: block.get("input").cloned().unwrap_or_else(|| json!({})),
        }),
        "thinking" => Ok(IRContentBlock::Thinking {
            thinking: block
                .get("thinking")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        other => Err(BridgeError::UnsupportedFeature(format!(
            "Anthropic Messages response content block type is not supported: {other}"
        ))),
    }
}

fn stop_reason_from_anthropic(reason: Option<&str>) -> IRStopReason {
    match reason {
        Some("end_turn") => IRStopReason::EndTurn,
        Some("tool_use") => IRStopReason::ToolUse,
        Some("max_tokens") => IRStopReason::MaxTokens,
        Some("stop_sequence") => IRStopReason::StopSequence,
        Some(other) => IRStopReason::Unknown(other.to_string()),
        None => IRStopReason::Unknown("missing_stop_reason".into()),
    }
}

fn parse_usage(value: Option<&Value>) -> IRUsage {
    let Some(value) = value else {
        return IRUsage::default();
    };
    IRUsage {
        input_tokens: value
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_creation_input_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
        cache_creation_5m_input_tokens: value
            .get("cache_creation_5m_input_tokens")
            .and_then(Value::as_u64),
        cache_creation_1h_input_tokens: value
            .get("cache_creation_1h_input_tokens")
            .and_then(Value::as_u64),
        cache_read_input_tokens: value.get("cache_read_input_tokens").and_then(Value::as_u64),
    }
}

fn sse_event_to_ir(
    event_type: &str,
    data: &Value,
    state: &mut StreamState,
) -> Result<Vec<IRStreamChunk>, BridgeError> {
    let chunks = match event_type {
        "message_start" => vec![IRStreamChunk::MessageStart {
            id: data
                .pointer("/message/id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            model: data
                .pointer("/message/model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            initial_usage: Some(parse_usage(data.pointer("/message/usage"))),
        }],
        "content_block_start" => {
            let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
            state.block_index = index.saturating_add(1);
            let block = data.get("content_block").unwrap_or(&Value::Null);
            let block_type = match block.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => IRBlockType::Text,
                "tool_use" => IRBlockType::ToolUse {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "thinking" => IRBlockType::Thinking,
                other => {
                    return Err(BridgeError::UnsupportedFeature(format!(
                        "Anthropic Messages stream content block type is not supported: {other}"
                    )));
                }
            };
            vec![IRStreamChunk::ContentBlockStart { index, block_type }]
        }
        "content_block_delta" => {
            let index = data.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
            let delta = data.get("delta").unwrap_or(&Value::Null);
            let delta = match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                "text_delta" => IRDelta::TextDelta {
                    text: delta
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "input_json_delta" => IRDelta::InputJsonDelta {
                    partial_json: delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "thinking_delta" => IRDelta::ThinkingDelta {
                    thinking: delta
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                other => {
                    return Err(BridgeError::UnsupportedFeature(format!(
                        "Anthropic Messages stream delta type is not supported: {other}"
                    )));
                }
            };
            vec![IRStreamChunk::ContentBlockDelta { index, delta }]
        }
        "content_block_stop" => vec![IRStreamChunk::ContentBlockStop {
            index: data.get("index").and_then(Value::as_u64).unwrap_or(0) as u32,
        }],
        "message_delta" => vec![IRStreamChunk::MessageDelta {
            stop_reason: stop_reason_from_anthropic(
                data.pointer("/delta/stop_reason").and_then(Value::as_str),
            ),
            usage: parse_usage(data.get("usage")),
        }],
        "message_stop" => vec![IRStreamChunk::MessageStop],
        "ping" => vec![IRStreamChunk::Ping],
        other => {
            return Err(BridgeError::UnsupportedFeature(format!(
                "Anthropic Messages stream event is not supported: {other}"
            )));
        }
    };
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_anthropic_messages_request() {
        let req = ir_to_request(&InternalRequest {
            model: "claude-3-5-sonnet".into(),
            messages: vec![IRMessage {
                role: IRRole::User,
                content: vec![IRContentBlock::Text { text: "Hi".into() }],
            }],
            system: Some("Be brief".into()),
            tools: vec![IRToolDefinition {
                name: "lookup".into(),
                description: None,
                parameters: json!({"type": "object"}),
            }],
            tool_choice: Some(IRToolChoice::Required),
            max_tokens: Some(100),
            temperature: None,
            top_p: None,
            stop_sequences: Vec::new(),
            stream: true,
            metadata: IRMetadata::default(),
        })
        .unwrap();

        assert_eq!(req["model"], "claude-3-5-sonnet");
        assert_eq!(req["system"], "Be brief");
        assert_eq!(req["messages"][0]["content"][0]["type"], "text");
        assert_eq!(req["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(req["tool_choice"]["type"], "any");
        assert_eq!(req["stream"], true);
    }

    #[test]
    fn rejects_unknown_anthropic_response_content_block_type() {
        let err = response_to_ir(json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet",
            "content": [{"type": "server_tool_use", "name": "web_search"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
        .unwrap_err();

        assert!(err.to_string().contains("server_tool_use"));
    }

    #[test]
    fn rejects_unknown_anthropic_stream_event_type() {
        let mut state = StreamState::default();
        let err = sse_event_to_ir("message_thinking_redacted", &json!({}), &mut state).unwrap_err();

        assert!(err.to_string().contains("message_thinking_redacted"));
    }

    #[test]
    fn rejects_unknown_anthropic_stream_delta_type() {
        let mut state = StreamState::default();
        let err = sse_event_to_ir(
            "content_block_delta",
            &json!({
                "index": 0,
                "delta": {"type": "citations_delta", "text": "source"}
            }),
            &mut state,
        )
        .unwrap_err();

        assert!(err.to_string().contains("citations_delta"));
    }
}
