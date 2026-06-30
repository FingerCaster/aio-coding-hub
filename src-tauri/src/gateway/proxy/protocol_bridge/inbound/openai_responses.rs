//! Inbound adapter for OpenAI Responses API client traffic.

use super::super::ir::*;
use super::super::traits::*;
use axum::body::Bytes;
use serde_json::{json, Value};

pub(crate) struct OpenAIResponsesInbound;

impl Inbound for OpenAIResponsesInbound {
    fn protocol(&self) -> &'static str {
        "openai_responses"
    }

    fn request_to_ir(
        &self,
        body: Value,
        _ctx: &BridgeContext,
    ) -> Result<InternalRequest, BridgeError> {
        parse_request(body)
    }

    fn ir_to_response(
        &self,
        ir: &InternalResponse,
        ctx: &BridgeContext,
    ) -> Result<Value, BridgeError> {
        build_response(ir, ctx)
    }

    fn ir_chunk_to_sse(
        &self,
        chunk: &IRStreamChunk,
        ctx: &BridgeContext,
    ) -> Result<Vec<Bytes>, BridgeError> {
        render_chunk(chunk, ctx)
    }
}

fn parse_request(body: Value) -> Result<InternalRequest, BridgeError> {
    reject_present(&body, "previous_response_id")?;
    reject_present(&body, "background")?;
    reject_present(&body, "prompt")?;
    reject_present(&body, "computer_use")?;
    reject_present(&body, "truncation")?;

    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BridgeError::InvalidInput("Responses request missing model".into()))?
        .to_string();

    let system = body
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let messages = parse_input(body.get("input"))?;
    let tools = parse_tools(body.get("tools"))?;
    let tool_choice = parse_tool_choice(body.get("tool_choice"))?;

    let max_tokens = body
        .get("max_output_tokens")
        .or_else(|| body.get("max_tokens"))
        .and_then(Value::as_u64);
    let temperature = body.get("temperature").and_then(Value::as_f64);
    let top_p = body.get("top_p").and_then(Value::as_f64);
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let stop_sequences = parse_stop(body.get("stop"))?;

    Ok(InternalRequest {
        model,
        messages,
        system,
        tools,
        tool_choice,
        max_tokens,
        temperature,
        top_p,
        stop_sequences,
        stream,
        metadata: IRMetadata::default(),
    })
}

fn reject_present(body: &Value, key: &str) -> Result<(), BridgeError> {
    if body.get(key).is_some_and(|value| !value.is_null()) {
        return Err(BridgeError::UnsupportedFeature(format!(
            "Responses field '{key}' cannot be bridged"
        )));
    }
    Ok(())
}

fn parse_input(input: Option<&Value>) -> Result<Vec<IRMessage>, BridgeError> {
    match input {
        Some(Value::String(text)) => Ok(vec![IRMessage {
            role: IRRole::User,
            content: vec![IRContentBlock::Text { text: text.clone() }],
        }]),
        Some(Value::Array(items)) => parse_input_items(items),
        Some(_) => Err(BridgeError::InvalidInput(
            "Responses input must be a string or array".into(),
        )),
        None => Ok(Vec::new()),
    }
}

fn parse_input_items(items: &[Value]) -> Result<Vec<IRMessage>, BridgeError> {
    let mut messages = Vec::new();
    for item in items {
        if let Some(role) = item.get("role").and_then(Value::as_str) {
            let role = parse_role(role);
            let content = parse_message_content(item.get("content"), role)?;
            messages.push(IRMessage { role, content });
            continue;
        }

        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => {
                let role = parse_role(item.get("role").and_then(Value::as_str).unwrap_or("user"));
                let content = parse_message_content(item.get("content"), role)?;
                messages.push(IRMessage { role, content });
            }
            "function_call" => messages.push(IRMessage {
                role: IRRole::Assistant,
                content: vec![parse_function_call(item)?],
            }),
            "function_call_output" => messages.push(IRMessage {
                role: IRRole::User,
                content: vec![IRContentBlock::ToolResult {
                    tool_use_id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    content: item
                        .get("output")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    is_error: false,
                }],
            }),
            "reasoning" => {
                return Err(BridgeError::UnsupportedFeature(
                    "Responses reasoning input items cannot be bridged".into(),
                ));
            }
            other => {
                return Err(BridgeError::UnsupportedFeature(format!(
                    "Responses input item type '{other}' cannot be bridged"
                )));
            }
        }
    }
    Ok(messages)
}

fn parse_role(role: &str) -> IRRole {
    match role {
        "assistant" => IRRole::Assistant,
        _ => IRRole::User,
    }
}

fn parse_message_content(
    content: Option<&Value>,
    role: IRRole,
) -> Result<Vec<IRContentBlock>, BridgeError> {
    match content {
        Some(Value::String(text)) => Ok(vec![IRContentBlock::Text { text: text.clone() }]),
        Some(Value::Array(blocks)) => {
            let mut result = Vec::new();
            for block in blocks {
                result.push(parse_content_block(block, role)?);
            }
            Ok(result)
        }
        Some(_) => Err(BridgeError::InvalidInput(
            "Responses message content must be string or array".into(),
        )),
        None => Ok(Vec::new()),
    }
}

fn parse_content_block(block: &Value, role: IRRole) -> Result<IRContentBlock, BridgeError> {
    match block.get("type").and_then(Value::as_str).unwrap_or("") {
        "input_text" | "output_text" => Ok(IRContentBlock::Text {
            text: block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        "input_image" if role == IRRole::User => parse_input_image(block),
        "refusal" => Ok(IRContentBlock::Text {
            text: block
                .get("refusal")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        other => Err(BridgeError::UnsupportedFeature(format!(
            "Responses content block type '{other}' cannot be bridged"
        ))),
    }
}

fn parse_input_image(block: &Value) -> Result<IRContentBlock, BridgeError> {
    let image_url = block
        .get("image_url")
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::InvalidInput("input_image missing image_url".into()))?;
    let Some(rest) = image_url.strip_prefix("data:") else {
        return Err(BridgeError::UnsupportedFeature(
            "Only data: image URLs can be bridged".into(),
        ));
    };
    let Some((media_type, data)) = rest.split_once(";base64,") else {
        return Err(BridgeError::UnsupportedFeature(
            "Only base64 data image URLs can be bridged".into(),
        ));
    };
    Ok(IRContentBlock::Image {
        media_type: media_type.to_string(),
        data: data.to_string(),
    })
}

fn parse_function_call(item: &Value) -> Result<IRContentBlock, BridgeError> {
    let args = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let input = serde_json::from_str(args).map_err(|err| {
        BridgeError::InvalidInput(format!("invalid function_call arguments: {err}"))
    })?;
    Ok(IRContentBlock::ToolUse {
        id: item
            .get("call_id")
            .and_then(Value::as_str)
            .or_else(|| item.get("id").and_then(Value::as_str))
            .unwrap_or("")
            .to_string(),
        name: item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        input,
    })
}

fn parse_tools(tools: Option<&Value>) -> Result<Vec<IRToolDefinition>, BridgeError> {
    let Some(Value::Array(tools)) = tools else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for tool in tools {
        let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
        if tool_type != "function" {
            return Err(BridgeError::UnsupportedFeature(format!(
                "Responses tool type '{tool_type}' cannot be bridged"
            )));
        }
        result.push(IRToolDefinition {
            name: tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            parameters: tool.get("parameters").cloned().unwrap_or_else(|| json!({})),
        });
    }
    Ok(result)
}

fn parse_tool_choice(value: Option<&Value>) -> Result<Option<IRToolChoice>, BridgeError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::String(value) => Ok(match value.as_str() {
            "auto" => Some(IRToolChoice::Auto),
            "required" => Some(IRToolChoice::Required),
            "none" => Some(IRToolChoice::None),
            other => {
                return Err(BridgeError::UnsupportedFeature(format!(
                    "Responses tool_choice '{other}' cannot be bridged"
                )))
            }
        }),
        Value::Object(obj) if obj.get("type").and_then(Value::as_str) == Some("function") => {
            Ok(Some(IRToolChoice::Specific {
                name: obj
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }))
        }
        _ => Err(BridgeError::UnsupportedFeature(
            "Responses tool_choice shape cannot be bridged".into(),
        )),
    }
}

fn parse_stop(value: Option<&Value>) -> Result<Vec<String>, BridgeError> {
    match value {
        Some(Value::String(value)) => Ok(vec![value.clone()]),
        Some(Value::Array(values)) => Ok(values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()),
        Some(_) => Err(BridgeError::InvalidInput(
            "Responses stop must be string or string array".into(),
        )),
        None => Ok(Vec::new()),
    }
}

fn build_response(ir: &InternalResponse, ctx: &BridgeContext) -> Result<Value, BridgeError> {
    let model = ctx
        .requested_model
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&ir.model);
    let mut output = Vec::new();
    let mut message_content = Vec::new();

    for block in &ir.content {
        match block {
            IRContentBlock::Text { text } => {
                message_content.push(json!({"type": "output_text", "text": text}));
            }
            IRContentBlock::ToolUse { id, name, input } => {
                if !message_content.is_empty() {
                    output.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": std::mem::take(&mut message_content)
                    }));
                }
                output.push(json!({
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string())
                }));
            }
            IRContentBlock::Thinking { thinking } => {
                output.push(json!({
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": thinking}]
                }));
            }
            IRContentBlock::Image { .. } | IRContentBlock::ToolResult { .. } => {}
        }
    }
    if !message_content.is_empty() || output.is_empty() {
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": message_content
        }));
    }

    Ok(json!({
        "id": ir.id,
        "object": "response",
        "status": status_from_stop_reason(&ir.stop_reason),
        "model": model,
        "output": output,
        "usage": {
            "input_tokens": ir.usage.input_tokens,
            "output_tokens": ir.usage.output_tokens
        }
    }))
}

fn status_from_stop_reason(stop_reason: &IRStopReason) -> &'static str {
    match stop_reason {
        IRStopReason::MaxTokens => "incomplete",
        _ => "completed",
    }
}

fn sse_frame(event_type: &str, payload: Value) -> Bytes {
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_type}\ndata: {data}\n\n"))
}

fn render_chunk(chunk: &IRStreamChunk, ctx: &BridgeContext) -> Result<Vec<Bytes>, BridgeError> {
    let frames = match chunk {
        IRStreamChunk::MessageStart {
            id,
            model,
            initial_usage,
        } => vec![sse_frame(
            "response.created",
            json!({
                "type": "response.created",
                "response": {
                    "id": id,
                    "object": "response",
                    "status": "in_progress",
                    "model": ctx.requested_model.as_deref().unwrap_or(model),
                    "usage": initial_usage.as_ref().map(|usage| json!({
                        "input_tokens": usage.input_tokens,
                        "output_tokens": usage.output_tokens
                    }))
                }
            }),
        )],
        IRStreamChunk::ContentBlockStart { index, block_type } => {
            let item = match block_type {
                IRBlockType::Text => json!({
                    "id": format!("msg_{index}"),
                    "type": "message",
                    "role": "assistant",
                    "content": []
                }),
                IRBlockType::ToolUse { id, name } => json!({
                    "id": id,
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": ""
                }),
                IRBlockType::Thinking => json!({
                    "id": format!("reasoning_{index}"),
                    "type": "reasoning",
                    "summary": []
                }),
            };
            vec![sse_frame(
                "response.output_item.added",
                json!({"type": "response.output_item.added", "output_index": index, "item": item}),
            )]
        }
        IRStreamChunk::ContentBlockDelta { index, delta } => match delta {
            IRDelta::TextDelta { text } => vec![sse_frame(
                "response.output_text.delta",
                json!({"type": "response.output_text.delta", "output_index": index, "delta": text}),
            )],
            IRDelta::InputJsonDelta { partial_json } => vec![sse_frame(
                "response.function_call_arguments.delta",
                json!({"type": "response.function_call_arguments.delta", "output_index": index, "delta": partial_json}),
            )],
            IRDelta::ThinkingDelta { thinking } => vec![sse_frame(
                "response.reasoning_summary_text.delta",
                json!({"type": "response.reasoning_summary_text.delta", "output_index": index, "delta": thinking}),
            )],
        },
        IRStreamChunk::ContentBlockStop { .. } => Vec::new(),
        IRStreamChunk::MessageDelta { stop_reason, usage } => vec![sse_frame(
            "response.completed",
            json!({
                "type": "response.completed",
                "response": {
                    "status": status_from_stop_reason(stop_reason),
                    "usage": {
                        "input_tokens": usage.input_tokens,
                        "output_tokens": usage.output_tokens
                    }
                }
            }),
        )],
        IRStreamChunk::MessageStop => vec![Bytes::from_static(b"data: [DONE]\n\n")],
        IRStreamChunk::Ping => Vec::new(),
    };
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_input_and_function_tool() {
        let ir = parse_request(json!({
            "model": "gpt-4.1",
            "instructions": "Be brief",
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "tools": [{"type": "function", "name": "lookup", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "lookup"},
            "stream": true
        }))
        .unwrap();

        assert_eq!(ir.model, "gpt-4.1");
        assert_eq!(ir.system.as_deref(), Some("Be brief"));
        assert_eq!(ir.messages.len(), 1);
        assert_eq!(ir.tools[0].name, "lookup");
        assert!(
            matches!(ir.tool_choice, Some(IRToolChoice::Specific { ref name }) if name == "lookup")
        );
        assert!(ir.stream);
    }

    #[test]
    fn rejects_stateful_responses_fields() {
        let err = parse_request(json!({
            "model": "gpt-4.1",
            "previous_response_id": "resp_1",
            "input": "Hi"
        }))
        .unwrap_err();

        assert!(err.to_string().contains("previous_response_id"));
    }
}
