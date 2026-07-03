//! Outbound adapter for OpenAI Chat Completions upstreams.

use crate::gateway::proxy::protocol_bridge::ir::*;
use crate::gateway::proxy::protocol_bridge::traits::*;
use serde_json::{json, Value};

pub(crate) struct OpenAIChatCompletionsOutbound;

impl Outbound for OpenAIChatCompletionsOutbound {
    fn protocol(&self) -> &'static str {
        "openai_chat_completions"
    }

    fn target_path(&self) -> &str {
        "/v1/chat/completions"
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
    for msg in &ir.messages {
        for block in &msg.content {
            match block {
                IRContentBlock::Image { .. } | IRContentBlock::Thinking { .. } => {
                    return Err(BridgeError::UnsupportedFeature(
                        "image/thinking content cannot be bridged to Chat Completions".into(),
                    ));
                }
                IRContentBlock::ResponsesNativeInputItem { raw } => {
                    let item_type = raw.get("type").and_then(Value::as_str).unwrap_or("unknown");
                    return Err(BridgeError::UnsupportedFeature(format!(
                        "Responses input item type '{item_type}' cannot be bridged to Chat Completions"
                    )));
                }
                _ => {}
            }
        }
    }
    if let Some(native_tool_type) = ir.tools.iter().find_map(|tool| match tool {
        IRToolDefinition::ResponsesNative { tool_type, .. } => Some(tool_type.as_str()),
        _ => None,
    }) {
        return Err(BridgeError::UnsupportedFeature(format!(
            "Responses tool type '{native_tool_type}' cannot be bridged to Chat Completions"
        )));
    }

    let mut messages = Vec::new();
    if let Some(system) = ir.system.as_deref().filter(|value| !value.is_empty()) {
        messages.push(json!({"role": "system", "content": system}));
    }
    for msg in &ir.messages {
        messages.extend(chat_messages_from_ir_message(msg)?);
    }

    let mut result = json!({
        "model": ir.model,
        "messages": messages,
        "stream": ir.stream
    });
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
        result["stop"] = json!(ir.stop_sequences);
    }
    let function_tools: Vec<Value> = ir
        .tools
        .iter()
        .filter_map(|tool| {
            tool.as_function().map(|(name, description, parameters)| {
                json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters
                }
                })
            })
        })
        .collect();
    let has_function_tools = !function_tools.is_empty();
    if has_function_tools {
        result["tools"] = json!(function_tools);
    }
    if let Some(tool_choice) = &ir.tool_choice {
        let converted = match tool_choice {
            IRToolChoice::Auto => Some(json!("auto")),
            IRToolChoice::Required => {
                if ir.tools.is_empty() || has_function_tools {
                    Some(json!("required"))
                } else {
                    None
                }
            }
            IRToolChoice::None => Some(json!("none")),
            IRToolChoice::Specific { name } => {
                if ir.tools.is_empty()
                    || ir
                        .tools
                        .iter()
                        .any(|tool| tool.function_name() == Some(name.as_str()))
                {
                    Some(json!({"type": "function", "function": {"name": name}}))
                } else {
                    None
                }
            }
            IRToolChoice::ResponsesNative { raw } => {
                let tool_type = raw.get("type").and_then(Value::as_str).unwrap_or("unknown");
                return Err(BridgeError::UnsupportedFeature(format!(
                    "Responses tool_choice type '{tool_type}' cannot be bridged to Chat Completions"
                )));
            }
        };
        if let Some(converted) = converted {
            result["tool_choice"] = converted;
        }
    }
    Ok(result)
}

fn chat_messages_from_ir_message(msg: &IRMessage) -> Result<Vec<Value>, BridgeError> {
    let mut result = Vec::new();
    let role = match msg.role {
        IRRole::User => "user",
        IRRole::Assistant => "assistant",
    };
    let text = msg
        .content
        .iter()
        .filter_map(|block| match block {
            IRContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tool_calls = msg
        .content
        .iter()
        .filter_map(|block| match block {
            IRContentBlock::ToolUse { id, name, input } => Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
                }
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !text.is_empty() || !tool_calls.is_empty() {
        let mut message = json!({"role": role, "content": text});
        if !tool_calls.is_empty() {
            message["tool_calls"] = json!(tool_calls);
        }
        result.push(message);
    }
    for block in &msg.content {
        if let IRContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } = block
        {
            result.push(json!({
                "role": "tool",
                "tool_call_id": tool_use_id,
                "content": content
            }));
        }
    }
    Ok(result)
}

fn response_to_ir(body: Value) -> Result<InternalResponse, BridgeError> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| BridgeError::TransformFailed("Chat response missing choices".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| BridgeError::TransformFailed("Chat response missing message".into()))?;
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(IRContentBlock::Text {
                text: text.to_string(),
            });
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            let function = call.get("function").unwrap_or(&Value::Null);
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            content.push(IRContentBlock::ToolUse {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                input: serde_json::from_str(arguments).map_err(|err| {
                    BridgeError::TransformFailed(format!(
                        "Chat response tool call arguments are not valid JSON: {err}"
                    ))
                })?,
            });
        }
    }
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
        content,
        stop_reason: finish_reason_to_stop(choice.get("finish_reason").and_then(Value::as_str)),
        usage: parse_usage(body.get("usage")),
    })
}

fn finish_reason_to_stop(reason: Option<&str>) -> IRStopReason {
    match reason {
        Some("tool_calls") | Some("function_call") => IRStopReason::ToolUse,
        Some("length") => IRStopReason::MaxTokens,
        Some("stop") => IRStopReason::EndTurn,
        Some(other) => IRStopReason::Unknown(other.to_string()),
        None => IRStopReason::Unknown("missing_finish_reason".into()),
    }
}

fn parse_usage(value: Option<&Value>) -> IRUsage {
    let Some(value) = value else {
        return IRUsage::default();
    };
    IRUsage {
        input_tokens: value
            .get("prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: value
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_creation_input_tokens: None,
        cache_creation_5m_input_tokens: None,
        cache_creation_1h_input_tokens: None,
        cache_read_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
    }
}

fn sse_event_to_ir(
    event_type: &str,
    data: &Value,
    state: &mut StreamState,
) -> Result<Vec<IRStreamChunk>, BridgeError> {
    if event_type == "done" || data.as_str() == Some("[DONE]") {
        return Ok(vec![IRStreamChunk::MessageStop]);
    }
    let choice = data
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let Some(choice) = choice else {
        return Ok(Vec::new());
    };
    let mut chunks = Vec::new();
    if !state.extra.contains_key("started") {
        state.extra.insert("started".into(), Value::Bool(true));
        chunks.push(IRStreamChunk::MessageStart {
            id: data
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            model: data
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            initial_usage: None,
        });
    }
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    if delta.get("tool_calls").is_some() {
        return Err(BridgeError::UnsupportedFeature(
            "streamed Chat Completions tool_calls cannot be bridged yet".into(),
        ));
    }
    if let Some(text) = delta.get("content").and_then(Value::as_str) {
        if !state.block_open {
            let index = state.block_index;
            state.block_index += 1;
            state.block_open = true;
            chunks.push(IRStreamChunk::ContentBlockStart {
                index,
                block_type: IRBlockType::Text,
            });
        }
        chunks.push(IRStreamChunk::ContentBlockDelta {
            index: state.block_index.saturating_sub(1),
            delta: IRDelta::TextDelta {
                text: text.to_string(),
            },
        });
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        if state.block_open {
            chunks.push(IRStreamChunk::ContentBlockStop {
                index: state.block_index.saturating_sub(1),
            });
            state.block_open = false;
        }
        chunks.push(IRStreamChunk::MessageDelta {
            stop_reason: finish_reason_to_stop(Some(reason)),
            usage: parse_usage(data.get("usage")),
        });
    }
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_chat_completions_request() {
        let req = ir_to_request(&InternalRequest {
            model: "gpt-4.1".into(),
            messages: vec![IRMessage {
                role: IRRole::User,
                content: vec![IRContentBlock::Text { text: "Hi".into() }],
            }],
            system: Some("Be brief".into()),
            tools: vec![IRToolDefinition::function(
                "lookup",
                None,
                json!({"type": "object"}),
            )],
            tool_choice: Some(IRToolChoice::Auto),
            max_tokens: Some(100),
            temperature: None,
            top_p: None,
            stop_sequences: Vec::new(),
            stream: true,
            metadata: IRMetadata::default(),
        })
        .unwrap();

        assert_eq!(req["model"], "gpt-4.1");
        assert_eq!(req["messages"][0]["role"], "system");
        assert_eq!(req["messages"][1]["content"], "Hi");
        assert_eq!(req["tools"][0]["type"], "function");
        assert_eq!(req["stream"], true);
    }

    #[test]
    fn rejects_responses_native_tools_for_chat_completions() {
        let err = ir_to_request(&InternalRequest {
            model: "gpt-4.1".into(),
            messages: vec![IRMessage {
                role: IRRole::User,
                content: vec![IRContentBlock::Text { text: "Hi".into() }],
            }],
            system: None,
            tools: vec![IRToolDefinition::responses_native(
                "tool_search",
                json!({"type": "tool_search"}),
            )],
            tool_choice: Some(IRToolChoice::ResponsesNative {
                raw: json!({"type": "tool_search"}),
            }),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop_sequences: Vec::new(),
            stream: false,
            metadata: IRMetadata::default(),
        })
        .unwrap_err();

        assert!(err.to_string().contains("tool_search"));
        assert!(err.to_string().contains("Chat Completions"));
    }

    #[test]
    fn rejects_responses_native_input_items_for_chat_completions() {
        let err = ir_to_request(&InternalRequest {
            model: "gpt-4.1".into(),
            messages: vec![IRMessage {
                role: IRRole::Assistant,
                content: vec![IRContentBlock::ResponsesNativeInputItem {
                    raw: json!({"type": "reasoning", "summary": []}),
                }],
            }],
            system: None,
            tools: Vec::new(),
            tool_choice: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop_sequences: Vec::new(),
            stream: false,
            metadata: IRMetadata::default(),
        })
        .unwrap_err();

        assert!(err.to_string().contains("reasoning"));
        assert!(err.to_string().contains("Chat Completions"));
    }

    #[test]
    fn rejects_invalid_tool_call_arguments() {
        let err = response_to_ir(json!({
            "id": "chatcmpl_1",
            "model": "gpt-4.1",
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "lookup", "arguments": "{invalid"}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }))
        .unwrap_err();

        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn rejects_streamed_tool_call_deltas_instead_of_dropping_them() {
        let mut state = StreamState::default();
        let err = sse_event_to_ir(
            "",
            &json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "lookup", "arguments": "{\"q\""}
                        }]
                    }
                }]
            }),
            &mut state,
        )
        .unwrap_err();

        assert!(err.to_string().contains("tool_calls"));
    }
}
