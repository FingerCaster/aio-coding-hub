//! Inbound adapter for OpenAI Responses API client traffic.

use super::super::ir::*;
use super::super::response_cache;
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
    let metadata = responses_metadata(&body);

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
        metadata,
    })
}

pub(crate) fn prepare_responses_bridge_body(
    mut body: Value,
    cache_namespace: &str,
) -> Result<(Value, Vec<Value>), BridgeError> {
    normalize_responses_structured_output_format(&mut body)?;
    normalize_responses_input(&mut body);
    expand_previous_response(&mut body, cache_namespace)?;
    normalize_responses_input_item_ids(&mut body);
    let expanded_input = body
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok((body, expanded_input))
}

fn responses_metadata(body: &Value) -> IRMetadata {
    let mut metadata = IRMetadata::default();
    for key in [
        "reasoning",
        "service_tier",
        "store",
        "text",
        "response_format",
        "include",
        "parallel_tool_calls",
    ] {
        if let Some(value) = body.get(key).filter(|value| !value.is_null()) {
            metadata.extra.insert(key.to_string(), value.clone());
        }
    }
    metadata
}

fn expand_previous_response(body: &mut Value, cache_namespace: &str) -> Result<(), BridgeError> {
    let Some(prev_id) = body
        .get("previous_response_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return Ok(());
    };
    if let Some(obj) = body.as_object_mut() {
        obj.remove("previous_response_id");
    }

    let input_items = body
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if input_items
        .iter()
        .any(response_cache::is_tool_call_context_item)
    {
        return Ok(());
    }

    let Some(key) = response_cache::ResponsesCacheKey::new(cache_namespace, prev_id.clone()) else {
        return Ok(());
    };
    if let Some(mut cached_items) = response_cache::get(&key) {
        cached_items.extend(input_items);
        body["input"] = Value::Array(cached_items);
        return Ok(());
    }

    if input_items.iter().any(response_cache::is_tool_output_item) {
        return Err(BridgeError::UnsupportedFeature(format!(
            "Responses previous_response_id '{prev_id}' is not in local bridge cache and input contains tool outputs that require prior tool-call context"
        )));
    }
    Ok(())
}

fn normalize_responses_input(body: &mut Value) {
    let Some(input) = body.get_mut("input") else {
        return;
    };
    match input {
        Value::Array(items) => {
            let mut normalized = Vec::with_capacity(items.len());
            for mut item in std::mem::take(items) {
                normalize_responses_input_item(&mut item);
                if let Some(compaction) = compaction_to_developer_message(&item) {
                    normalized.push(compaction);
                } else if is_empty_compaction(&item) {
                    continue;
                } else {
                    normalized.push(item);
                }
            }
            *items = normalized;
        }
        Value::String(_) => {}
        _ => {}
    }
}

fn normalize_responses_input_item(item: &mut Value) {
    let Some(obj) = item.as_object_mut() else {
        return;
    };
    normalize_responses_content_item_type(obj, None);
    let role = obj.get("role").and_then(Value::as_str).map(str::to_string);
    if let Some(Value::Array(content)) = obj.get_mut("content") {
        for content_item in content {
            if let Some(content_obj) = content_item.as_object_mut() {
                normalize_responses_content_item_type(content_obj, role.as_deref());
            }
        }
    }
}

fn normalize_responses_input_item_ids(body: &mut Value) {
    let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        if let Some(obj) = item.as_object_mut() {
            obj.remove("id");
        }
    }
}

fn normalize_responses_content_item_type(
    obj: &mut serde_json::Map<String, Value>,
    role: Option<&str>,
) {
    let item_type = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    match item_type.as_str() {
        "file" => {
            obj.insert("type".to_string(), json!("input_file"));
            normalize_input_file_fields(obj);
        }
        "image" | "image_url" => {
            obj.insert("type".to_string(), json!("input_image"));
            normalize_image_url_field(obj);
        }
        "text" => {
            let next_type = if role == Some("assistant") {
                "output_text"
            } else {
                "input_text"
            };
            obj.insert("type".to_string(), json!(next_type));
        }
        "input_text" if role == Some("assistant") => {
            obj.insert("type".to_string(), json!("output_text"));
        }
        "output_text" if role != Some("assistant") => {
            obj.insert("type".to_string(), json!("input_text"));
        }
        "input_file" => normalize_input_file_fields(obj),
        "input_image" | "computer_screenshot" => normalize_image_url_field(obj),
        _ => {}
    }
}

fn normalize_input_file_fields(obj: &mut serde_json::Map<String, Value>) {
    let Some(raw_file) = obj.remove("file") else {
        return;
    };
    match raw_file {
        Value::Object(file_obj) => {
            for key in ["file_id", "file_data", "file_url", "filename"] {
                if !obj.contains_key(key) {
                    if let Some(value) = file_obj.get(key) {
                        obj.insert(key.to_string(), value.clone());
                    }
                }
            }
        }
        Value::String(file_id) if !file_id.trim().is_empty() => {
            obj.entry("file_id".to_string()).or_insert(json!(file_id));
        }
        _ => {}
    }
}

fn normalize_image_url_field(obj: &mut serde_json::Map<String, Value>) {
    let Some(Value::Object(image_url)) = obj.get("image_url") else {
        return;
    };
    if let Some(url) = image_url
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        obj.insert("image_url".to_string(), json!(url));
    }
}

fn compaction_to_developer_message(item: &Value) -> Option<Value> {
    if item.get("type").and_then(Value::as_str) != Some("compaction") {
        return None;
    }
    let summary = compaction_summary_text(item.get("summary"))
        .or_else(|| compaction_summary_text(item.get("text")))?;
    Some(json!({
        "type": "message",
        "role": "developer",
        "content": [{
            "type": "input_text",
            "text": format!("[Conversation summary from earlier turns]\n{summary}")
        }]
    }))
}

fn is_empty_compaction(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("compaction")
}

fn compaction_summary_text(raw: Option<&Value>) -> Option<String> {
    match raw? {
        Value::String(value) => {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        }
        value => {
            let text = serde_json::to_string(value).ok()?;
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
    }
}

fn normalize_responses_structured_output_format(body: &mut Value) -> Result<(), BridgeError> {
    let response_format = body.get("response_format").cloned();
    if let Some(Value::Object(format_obj)) = response_format {
        let Some(text_format) = responses_text_format_from_response_format(&format_obj) else {
            return Err(BridgeError::UnsupportedFeature(
                "Responses response_format cannot be bridged".into(),
            ));
        };
        let obj = body.as_object_mut().ok_or_else(|| {
            BridgeError::InvalidInput("Responses request must be an object".into())
        })?;
        let text = obj
            .entry("text")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| BridgeError::InvalidInput("Responses text must be an object".into()))?;
        text.entry("format".to_string())
            .or_insert(Value::Object(text_format));
        obj.remove("response_format");
    } else if response_format.is_some() && !body.get("response_format").is_some_and(Value::is_null)
    {
        return Err(BridgeError::UnsupportedFeature(
            "Responses response_format cannot be bridged".into(),
        ));
    }
    Ok(())
}

fn responses_text_format_from_response_format(
    response_format: &serde_json::Map<String, Value>,
) -> Option<serde_json::Map<String, Value>> {
    let format_type = response_format
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    match format_type {
        "json_schema" => {
            if let Some(Value::Object(json_schema)) = response_format.get("json_schema") {
                let mut out = json_schema.clone();
                out.insert("type".to_string(), json!("json_schema"));
                Some(out)
            } else {
                let mut out = response_format.clone();
                out.remove("json_schema");
                out.insert("type".to_string(), json!("json_schema"));
                Some(out)
            }
        }
        "json_object" | "text" => {
            let mut out = serde_json::Map::new();
            out.insert("type".to_string(), json!(format_type));
            Some(out)
        }
        _ => None,
    }
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
            item_type if is_responses_native_input_item_type(item_type) => {
                messages.push(IRMessage {
                    role: IRRole::Assistant,
                    content: vec![IRContentBlock::ResponsesNativeInputItem { raw: item.clone() }],
                });
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

fn is_responses_native_input_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "reasoning"
            | "tool_call"
            | "local_shell_call"
            | "local_shell_call_output"
            | "tool_search_call"
            | "tool_search_output"
            | "tool_search_call_output"
            | "custom_tool_call"
            | "custom_tool_call_output"
            | "mcp_tool_call"
            | "mcp_tool_call_output"
            | "item_reference"
            | "image_generation_call"
            | "web_search_call"
            | "compaction"
            | "compaction_summary"
            | "input_text"
            | "input_image"
            | "output_text"
            | "refusal"
            | "input_file"
            | "computer_screenshot"
            | "summary_text"
    )
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
        parse_tool_definition(tool, None, &mut result)?;
    }
    Ok(result)
}

fn parse_tool_definition(
    tool: &Value,
    namespace: Option<&str>,
    result: &mut Vec<IRToolDefinition>,
) -> Result<(), BridgeError> {
    let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
    match tool_type {
        "" | "function" => {
            result.push(IRToolDefinition::function(
                qualified_tool_name(namespace, tool_name(tool)),
                tool_description(tool).map(str::to_string),
                tool_parameters(tool).cloned().unwrap_or_else(|| json!({})),
            ));
            Ok(())
        }
        "namespace" => {
            let namespace_name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(Value::Array(children)) = tool.get("tools") else {
                return Ok(());
            };
            for child in children {
                parse_tool_definition(child, Some(namespace_name), result)?;
            }
            Ok(())
        }
        "custom" if is_apply_patch_custom_tool(tool) => Ok(()),
        other if is_responses_native_tool_type(other) => {
            result.push(IRToolDefinition::responses_native(
                other,
                normalize_native_tool(tool),
            ));
            Ok(())
        }
        other => Err(BridgeError::UnsupportedFeature(format!(
            "Responses tool type '{other}' cannot be bridged"
        ))),
    }
}

fn tool_name(tool: &Value) -> &str {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            tool.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("")
}

fn tool_description(tool: &Value) -> Option<&str> {
    tool.get("description").and_then(Value::as_str).or_else(|| {
        tool.get("function")
            .and_then(|function| function.get("description"))
            .and_then(Value::as_str)
    })
}

fn tool_parameters(tool: &Value) -> Option<&Value> {
    for path in [
        ["parameters"].as_slice(),
        ["parametersJsonSchema"].as_slice(),
        ["input_schema"].as_slice(),
        ["function", "parameters"].as_slice(),
        ["function", "parametersJsonSchema"].as_slice(),
    ] {
        let mut current = tool;
        let mut found = true;
        for key in path {
            match current.get(*key) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found {
            return Some(current);
        }
    }
    None
}

fn qualified_tool_name(namespace: Option<&str>, child_name: &str) -> String {
    let child_name = child_name.trim();
    let namespace = namespace.unwrap_or("").trim();
    if child_name.is_empty()
        || namespace.is_empty()
        || child_name.starts_with("mcp__")
        || child_name.starts_with(namespace)
    {
        return child_name.to_string();
    }
    if namespace.ends_with("__") {
        format!("{namespace}{child_name}")
    } else {
        format!("{namespace}__{child_name}")
    }
}

fn is_apply_patch_custom_tool(tool: &Value) -> bool {
    tool.get("type").and_then(Value::as_str) == Some("custom")
        && tool_name(tool).trim() == "apply_patch"
}

fn is_responses_native_tool_type(tool_type: &str) -> bool {
    matches!(
        tool_type,
        "tool_search"
            | "web_search"
            | "web_search_preview"
            | "web_search_preview_2025_03_11"
            | "web_search_2025_08_26"
            | "image_generation"
            | "file_search"
            | "code_interpreter"
            | "computer_use_preview"
            | "mcp"
    )
}

fn normalize_native_tool(tool: &Value) -> Value {
    let mut raw = tool.clone();
    if raw.get("type").and_then(Value::as_str) == Some("tool_search")
        && raw.get("description").is_none()
    {
        raw["description"] =
            json!("Search through available tools to find the most relevant one for the task.");
    }
    raw
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
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    .or_else(|| obj.get("name").and_then(Value::as_str))
                    .unwrap_or("")
                    .to_string(),
            }))
        }
        Value::Object(obj) => {
            let tool_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
            if is_responses_native_tool_type(tool_type) {
                Ok(Some(IRToolChoice::ResponsesNative { raw: value.clone() }))
            } else {
                Err(BridgeError::UnsupportedFeature(format!(
                    "Responses tool_choice type '{tool_type}' cannot be bridged"
                )))
            }
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
            IRContentBlock::Image { .. }
            | IRContentBlock::ToolResult { .. }
            | IRContentBlock::ResponsesNativeInputItem { .. } => {}
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
        assert_eq!(ir.tools[0].function_name(), Some("lookup"));
        assert!(
            matches!(ir.tool_choice, Some(IRToolChoice::Specific { ref name }) if name == "lookup")
        );
        assert!(ir.stream);
    }

    #[test]
    fn skips_apply_patch_custom_tool() {
        let ir = parse_request(json!({
            "model": "gpt-4.1",
            "input": "Hi",
            "tools": [
                {
                    "type": "custom",
                    "name": "apply_patch",
                    "description": "Use apply_patch to edit files.",
                    "format": {"type": "grammar", "syntax": "lark", "definition": "start: patch"}
                },
                {"type": "function", "name": "shell", "parameters": {"type": "object"}}
            ]
        }))
        .unwrap();

        assert_eq!(ir.tools.len(), 1);
        assert_eq!(ir.tools[0].function_name(), Some("shell"));
    }

    #[test]
    fn rejects_unknown_custom_tool_type_explicitly() {
        let err = parse_request(json!({
            "model": "gpt-4.1",
            "input": "Hi",
            "tools": [{"type": "custom", "name": "shell"}]
        }))
        .unwrap_err();

        assert!(err.to_string().contains("Responses tool type 'custom'"));
    }

    #[test]
    fn parses_namespace_tool_as_qualified_function() {
        let ir = parse_request(json!({
            "model": "gpt-4.1",
            "input": "Hi",
            "tools": [{
                "type": "namespace",
                "name": "mcp__test",
                "tools": [{
                    "type": "function",
                    "name": "lookup",
                    "description": "Lookup value",
                    "parametersJsonSchema": {"type": "object", "required": ["query"]}
                }]
            }]
        }))
        .unwrap();

        assert_eq!(ir.tools.len(), 1);
        let Some((name, description, parameters)) = ir.tools[0].as_function() else {
            panic!("expected function tool");
        };
        assert_eq!(name, "mcp__test__lookup");
        assert_eq!(description, Some("Lookup value"));
        assert_eq!(parameters["required"][0], "query");
    }

    #[test]
    fn parses_responses_native_tools_and_tool_choice() {
        let ir = parse_request(json!({
            "model": "gpt-4.1",
            "input": "Hi",
            "tools": [
                {"type": "tool_search"},
                {"type": "web_search_preview", "search_context_size": "low"},
                {"type": "image_generation", "size": "1024x1024"},
                {"type": "file_search", "vector_store_ids": ["vs_1"]},
                {"type": "code_interpreter", "container": {"type": "auto"}},
                {"type": "mcp", "server_label": "docs", "server_url": "https://example.com/mcp"}
            ],
            "tool_choice": {"type": "tool_search"}
        }))
        .unwrap();

        assert_eq!(ir.tools.len(), 6);
        assert!(matches!(
            &ir.tools[0],
            IRToolDefinition::ResponsesNative { tool_type, raw }
                if tool_type == "tool_search"
                    && raw["description"]
                        == "Search through available tools to find the most relevant one for the task."
        ));
        assert!(matches!(
            &ir.tool_choice,
            Some(IRToolChoice::ResponsesNative { raw }) if raw["type"] == "tool_search"
        ));
    }

    #[test]
    fn parses_responses_native_input_items() {
        let ir = parse_request(json!({
            "model": "gpt-4.1",
            "input": [
                {"type": "reasoning", "id": "rs_1", "summary": []},
                {"type": "tool_search_call", "id": "ts_1", "call_id": "call_search", "status": "completed"},
                {"type": "tool_search_output", "call_id": "call_search", "output": "ok"},
                {"type": "mcp_tool_call", "call_id": "call_mcp", "name": "read", "arguments": "{}"},
                {"type": "custom_tool_call_output", "call_id": "call_custom", "output": "ok"},
                {"type": "item_reference", "id": "item_1"},
                {"type": "compaction", "summary": "previous context"},
                {"type": "input_file", "file_id": "file_abc"},
                {"type": "computer_screenshot", "image_url": "https://example.com/screen.png"},
                {"type": "summary_text", "text": "summary"},
                {"role": "user", "content": [{"type": "input_text", "text": "continue"}]}
            ]
        }))
        .unwrap();

        assert!(matches!(
            &ir.messages[0].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "reasoning"
        ));
        assert!(matches!(
            &ir.messages[1].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "tool_search_call"
        ));
        assert!(matches!(
            &ir.messages[2].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "tool_search_output"
        ));
        assert!(matches!(
            &ir.messages[3].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "mcp_tool_call"
        ));
        assert!(matches!(
            &ir.messages[4].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw }
                if raw["type"] == "custom_tool_call_output"
        ));
        assert!(matches!(
            &ir.messages[6].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "compaction"
        ));
        assert!(matches!(
            &ir.messages[9].content[0],
            IRContentBlock::ResponsesNativeInputItem { raw } if raw["type"] == "summary_text"
        ));
        assert!(matches!(
            &ir.messages[10].content[0],
            IRContentBlock::Text { text } if text == "continue"
        ));
    }

    #[test]
    fn rejects_stateful_responses_fields() {
        for field in [
            "previous_response_id",
            "background",
            "prompt",
            "computer_use",
            "truncation",
        ] {
            let mut body = json!({
                "model": "gpt-4.1",
                "input": "Hi"
            });
            body[field] = json!("unsupported");
            let err = parse_request(body).unwrap_err();

            assert!(err.to_string().contains(field), "unexpected error: {err}");
        }
    }

    #[test]
    fn preserves_supported_responses_metadata() {
        let ir = parse_request(json!({
            "model": "gpt-5",
            "input": "Hi",
            "reasoning": {"effort": "high"},
            "service_tier": "flex",
            "store": false
        }))
        .unwrap();

        assert_eq!(ir.metadata.extra["reasoning"], json!({"effort": "high"}));
        assert_eq!(ir.metadata.extra["service_tier"], json!("flex"));
        assert_eq!(ir.metadata.extra["store"], json!(false));
    }

    #[test]
    fn prepare_responses_bridge_body_normalizes_codex_responses_extensions() {
        let _guard = response_cache::test_guard();
        response_cache::clear_for_tests();
        let (prepared, expanded_input) = prepare_responses_bridge_body(
            json!({
                "model": "gpt-5",
                "input": [
                    {
                        "id": "msg_1",
                        "type": "message",
                        "role": "user",
                        "content": [
                            {"type": "text", "text": "hello"},
                            {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                            {"type": "file", "file": {"file_id": "file_1"}}
                        ]
                    },
                    {
                        "id": "msg_2",
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "text", "text": "done"}]
                    },
                    {"id": "cmp_1", "type": "compaction", "summary": "earlier context"}
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "answer",
                        "schema": {"type": "object"}
                    }
                }
            }),
            "bridge:source=1:session=a",
        )
        .unwrap();

        assert!(prepared.get("response_format").is_none());
        assert_eq!(prepared["text"]["format"]["type"], "json_schema");
        assert_eq!(prepared["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(prepared["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            prepared["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,abc"
        );
        assert_eq!(prepared["input"][0]["content"][2]["type"], "input_file");
        assert_eq!(prepared["input"][0]["content"][2]["file_id"], "file_1");
        assert_eq!(prepared["input"][1]["content"][0]["type"], "output_text");
        assert_eq!(prepared["input"][2]["role"], "developer");
        assert!(prepared["input"][0].get("id").is_none());
        assert_eq!(
            expanded_input,
            prepared["input"].as_array().unwrap().clone()
        );
    }

    #[test]
    fn prepare_responses_bridge_body_expands_cached_previous_response() {
        let _guard = response_cache::test_guard();
        response_cache::clear_for_tests();
        let key =
            response_cache::ResponsesCacheKey::new("bridge:source=1:session=a", "resp_1").unwrap();
        response_cache::set(
            key,
            vec![json!({
                "type": "tool_search_call",
                "call_id": "call_search",
                "status": "completed"
            })],
        );

        let (prepared, expanded_input) = prepare_responses_bridge_body(
            json!({
                "model": "gpt-5",
                "previous_response_id": "resp_1",
                "input": [
                    {
                        "type": "tool_search_output",
                        "call_id": "call_search",
                        "output": "found"
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "text", "text": "continue"}]
                    }
                ]
            }),
            "bridge:source=1:session=a",
        )
        .unwrap();

        assert!(prepared.get("previous_response_id").is_none());
        assert_eq!(prepared["input"][0]["type"], "tool_search_call");
        assert_eq!(prepared["input"][1]["type"], "tool_search_output");
        assert_eq!(prepared["input"][2]["content"][0]["type"], "input_text");
        assert_eq!(
            expanded_input,
            prepared["input"].as_array().unwrap().clone()
        );
    }

    #[test]
    fn prepare_responses_bridge_body_rejects_uncached_tool_output_continuation() {
        let _guard = response_cache::test_guard();
        response_cache::clear_for_tests();
        let err = prepare_responses_bridge_body(
            json!({
                "model": "gpt-5",
                "previous_response_id": "missing",
                "input": [{
                    "type": "tool_search_output",
                    "call_id": "call_search",
                    "output": "found"
                }]
            }),
            "bridge:source=1:session=a",
        )
        .unwrap_err();

        assert!(err.to_string().contains("previous_response_id"));
        assert!(err.to_string().contains("local bridge cache"));
    }
}
