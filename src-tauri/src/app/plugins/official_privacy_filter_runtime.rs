//! Usage: Official native privacy filter plugin runtime.

use super::privacy_filter::{PrivacyFilter, PrivacyFilterError, PrivacyFilterOptions};
use super::runtime_cache::{runtime_cache_key, RuntimeCacheKeyInput};
use crate::gateway::plugins::context::{GatewayHookResult, GatewayVisibleHookContext};
use crate::gateway::plugins::permissions::GatewayPluginError;
use crate::plugins::PluginDetail;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct OfficialPrivacyFilterRuntime {
    cache: Mutex<HashMap<String, Arc<PrivacyFilter>>>,
}

impl OfficialPrivacyFilterRuntime {
    pub(crate) fn execute_plugin(
        &self,
        plugin: &PluginDetail,
        context: GatewayVisibleHookContext,
    ) -> Result<GatewayHookResult, GatewayPluginError> {
        let filter = self.get_or_load_privacy_filter(plugin)?;
        execute_official_privacy_filter_hook(&filter, &context, &plugin.config)
            .map_err(to_privacy_filter_error)
    }

    pub(crate) fn retain_runtime_caches_for_plugins(&self, plugins: &[PluginDetail]) {
        let privacy_keys: HashSet<String> = plugins
            .iter()
            .filter(|plugin| plugin.summary.plugin_id == "official.privacy-filter")
            .map(privacy_filter_cache_key)
            .collect();
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| privacy_keys.contains(key));
    }

    fn get_or_load_privacy_filter(
        &self,
        plugin: &PluginDetail,
    ) -> Result<Arc<PrivacyFilter>, GatewayPluginError> {
        let cache_key = privacy_filter_cache_key(plugin);
        {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(filter) = cache.get(&cache_key) {
                return Ok(Arc::clone(filter));
            }
        }

        let filter =
            Arc::new(load_official_privacy_filter(plugin).map_err(to_privacy_filter_error)?);
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(Arc::clone(
            cache
                .entry(cache_key)
                .or_insert_with(|| Arc::clone(&filter)),
        ))
    }

    #[cfg(test)]
    pub(crate) fn cache_size_for_tests(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

fn privacy_filter_cache_key(plugin: &PluginDetail) -> String {
    let version = plugin
        .summary
        .current_version
        .as_deref()
        .unwrap_or(plugin.manifest.version.as_str());
    let installed_dir = plugin.installed_dir.as_deref().unwrap_or("");
    let updated_at = plugin.summary.updated_at;
    runtime_cache_key(RuntimeCacheKeyInput {
        plugin_id: plugin.summary.plugin_id.as_str(),
        version,
        installed_dir,
        updated_at,
        runtime_key: "native:privacyFilter",
    })
}

fn load_official_privacy_filter(
    plugin: &PluginDetail,
) -> Result<PrivacyFilter, PrivacyFilterError> {
    let root_dir = plugin.installed_dir.as_deref().ok_or_else(|| {
        PrivacyFilterError::new(format!(
            "plugin {} has no installed_dir for privacy-filter rule loading",
            plugin.summary.plugin_id
        ))
    })?;
    let rules_path = std::path::Path::new(root_dir).join("rules/gitleaks.toml");
    let raw = fs::read_to_string(&rules_path).map_err(|err| {
        PrivacyFilterError::new(format!(
            "failed to read privacy-filter gitleaks rules for plugin {}: {err}",
            plugin.summary.plugin_id
        ))
    })?;
    PrivacyFilter::from_gitleaks_toml(&raw)
}

fn execute_official_privacy_filter_hook(
    filter: &PrivacyFilter,
    context: &GatewayVisibleHookContext,
    config: &Value,
) -> Result<GatewayHookResult, PrivacyFilterError> {
    let mut result = GatewayHookResult::continue_unchanged();
    let options = privacy_filter_options_from_config(config);
    let scopes = privacy_filter_redaction_scopes_from_config(config);
    match context.hook_name.as_str() {
        "gateway.request.afterBodyRead" | "gateway.request.beforeSend" => {
            if config.get("redactBeforeUpstream") != Some(&Value::Bool(true)) {
                return Ok(result);
            }
            let Some(body) = context.request.body.as_deref() else {
                return Ok(result);
            };
            if let Some(next_body) = redact_request_body_strings(filter, body, &options, &scopes)? {
                result.request_body = Some(next_body);
            }
        }
        "log.beforePersist" => {
            if config.get("redactLogs") != Some(&Value::Bool(true)) {
                return Ok(result);
            }
            let Some(message) = context.log.message.as_deref() else {
                return Ok(result);
            };
            let redacted = filter.redact_with_options(message, &options);
            if redacted.hit {
                result.log_message = Some(redacted.redacted);
            }
        }
        _ => {}
    }
    Ok(result)
}

fn privacy_filter_options_from_config(config: &Value) -> PrivacyFilterOptions {
    let sensitive_types = config
        .get("sensitiveTypes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        });
    PrivacyFilterOptions::from_sensitive_types(sensitive_types.as_deref())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrivacyFilterRedactionScopes {
    system_instructions: bool,
    user_prompts: bool,
    assistant_context: bool,
    tool_results: bool,
    legacy_prompt: bool,
}

impl PrivacyFilterRedactionScopes {
    fn default_enabled() -> Self {
        Self {
            system_instructions: true,
            user_prompts: true,
            assistant_context: false,
            tool_results: true,
            legacy_prompt: true,
        }
    }

    fn empty() -> Self {
        Self {
            system_instructions: false,
            user_prompts: false,
            assistant_context: false,
            tool_results: false,
            legacy_prompt: false,
        }
    }

    fn enable(&mut self, scope: &str) {
        match scope {
            "system_instructions" => self.system_instructions = true,
            "user_prompts" => self.user_prompts = true,
            "assistant_context" => self.assistant_context = true,
            "tool_results" => self.tool_results = true,
            "legacy_prompt" => self.legacy_prompt = true,
            _ => {}
        }
    }

    fn message_content_enabled(&self, role: Option<&str>) -> bool {
        match role.unwrap_or("user") {
            "system" | "developer" => self.system_instructions,
            "user" => self.user_prompts,
            "assistant" => self.assistant_context,
            "tool" => self.tool_results,
            _ => false,
        }
    }
}

fn privacy_filter_redaction_scopes_from_config(config: &Value) -> PrivacyFilterRedactionScopes {
    let Some(items) = config.get("redactionScopes").and_then(Value::as_array) else {
        return PrivacyFilterRedactionScopes::default_enabled();
    };
    let mut scopes = PrivacyFilterRedactionScopes::empty();
    for item in items {
        if let Some(scope) = item.as_str() {
            scopes.enable(scope);
        }
    }
    scopes
}

fn redact_request_body_strings(
    filter: &PrivacyFilter,
    body: &str,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
) -> Result<Option<String>, PrivacyFilterError> {
    let Ok(mut root) = serde_json::from_str::<Value>(body) else {
        if !scopes.legacy_prompt {
            return Ok(None);
        }
        let redacted = filter.redact_with_options(body, options);
        return Ok(redacted.hit.then_some(redacted.redacted));
    };
    let mut matched = false;
    redact_request_json_allowlist(&mut root, filter, options, scopes, &mut matched);
    if !matched {
        return Ok(None);
    }
    serde_json::to_string(&root)
        .map(Some)
        .map_err(|err| PrivacyFilterError::new(format!("failed to serialize redacted JSON: {err}")))
}

fn redact_request_json_allowlist(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    let Some(map) = value.as_object_mut() else {
        return;
    };

    if scopes.system_instructions {
        if let Some(system) = map.get_mut("system") {
            redact_text_or_text_blocks(system, filter, options, matched, &["text"]);
        }
        if let Some(instructions) = map.get_mut("instructions") {
            redact_text_value(instructions, filter, options, matched);
        }
    }

    if let Some(input) = map.get_mut("input") {
        redact_responses_input(input, filter, options, scopes, matched);
    }

    if let Some(messages) = map.get_mut("messages") {
        redact_messages(messages, filter, options, scopes, matched);
    }

    if scopes.legacy_prompt {
        if let Some(prompt) = map.get_mut("prompt") {
            redact_text_value(prompt, filter, options, matched);
        }
    }
}

fn redact_messages(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    let Some(messages) = value.as_array_mut() else {
        return;
    };
    for message in messages {
        redact_message(message, filter, options, scopes, matched);
    }
}

fn redact_message(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let role = map.get("role").and_then(Value::as_str).map(str::to_string);
    if let Some(content) = map.get_mut("content") {
        redact_message_content(content, role.as_deref(), filter, options, scopes, matched);
    }
}

fn redact_message_content(
    value: &mut Value,
    role: Option<&str>,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    match value {
        Value::String(_) => {
            if scopes.message_content_enabled(role) {
                redact_text_value(value, filter, options, matched);
            }
        }
        Value::Array(parts) => {
            for part in parts {
                redact_message_content_part(part, role, filter, options, scopes, matched);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

fn redact_message_content_part(
    value: &mut Value,
    role: Option<&str>,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    match map.get("type").and_then(Value::as_str) {
        Some("tool_result") => {
            if scopes.tool_results {
                redact_tool_result_content(map.get_mut("content"), filter, options, matched);
            }
        }
        Some("text") => {
            if scopes.message_content_enabled(role) {
                if let Some(text) = map.get_mut("text") {
                    redact_text_value(text, filter, options, matched);
                }
            }
        }
        _ => {}
    }
}

fn redact_responses_input(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    match value {
        Value::String(_) => {
            if scopes.user_prompts {
                redact_text_value(value, filter, options, matched);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_responses_input_item(item, filter, options, scopes, matched);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

fn redact_responses_input_item(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    match map.get("type").and_then(Value::as_str) {
        Some("function_call_output") => {
            if scopes.tool_results {
                redact_tool_result_content(map.get_mut("output"), filter, options, matched);
            }
        }
        Some("message") | None => {
            let role = map.get("role").and_then(Value::as_str).map(str::to_string);
            if let Some(content) = map.get_mut("content") {
                redact_responses_content(
                    content,
                    role.as_deref(),
                    filter,
                    options,
                    scopes,
                    matched,
                );
            }
        }
        _ => {}
    }
}

fn redact_responses_content(
    value: &mut Value,
    role: Option<&str>,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    scopes: &PrivacyFilterRedactionScopes,
    matched: &mut bool,
) {
    match value {
        Value::String(_) => {
            if scopes.message_content_enabled(role) {
                redact_text_value(value, filter, options, matched);
            }
        }
        Value::Array(parts) => {
            if !scopes.message_content_enabled(role) {
                return;
            }
            for part in parts {
                redact_typed_text_part(
                    part,
                    filter,
                    options,
                    matched,
                    &["input_text", "output_text"],
                );
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

fn redact_tool_result_content(
    value: Option<&mut Value>,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    matched: &mut bool,
) {
    let Some(value) = value else {
        return;
    };
    match value {
        Value::String(_) => redact_text_value(value, filter, options, matched),
        Value::Array(parts) => {
            for part in parts {
                redact_typed_text_part(
                    part,
                    filter,
                    options,
                    matched,
                    &["text", "input_text", "output_text"],
                );
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

fn redact_text_or_text_blocks(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    matched: &mut bool,
    allowed_types: &[&str],
) {
    match value {
        Value::String(_) => redact_text_value(value, filter, options, matched),
        Value::Array(parts) => {
            for part in parts {
                redact_typed_text_part(part, filter, options, matched, allowed_types);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

fn redact_typed_text_part(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    matched: &mut bool,
    allowed_types: &[&str],
) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let Some(kind) = map.get("type").and_then(Value::as_str) else {
        return;
    };
    if !allowed_types.contains(&kind) {
        return;
    }
    if let Some(text) = map.get_mut("text") {
        redact_text_value(text, filter, options, matched);
    }
}

fn redact_text_value(
    value: &mut Value,
    filter: &PrivacyFilter,
    options: &PrivacyFilterOptions,
    matched: &mut bool,
) {
    let Value::String(text) = value else {
        return;
    };
    let redacted = filter.redact_with_options(text, options);
    if redacted.hit {
        *text = redacted.redacted;
        *matched = true;
    }
}

fn to_privacy_filter_error(err: PrivacyFilterError) -> GatewayPluginError {
    GatewayPluginError::new("PLUGIN_PRIVACY_FILTER_FAILED", err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::plugins::context::{
        GatewayVisibleHookContext, GatewayVisibleLogContext, GatewayVisibleRequestContext,
        GatewayVisibleResponseContext, GatewayVisibleStreamContext,
    };
    use crate::plugins::{
        PluginDetail, PluginInstallSource, PluginPermissionRisk, PluginStatus, PluginSummary,
    };
    use serde_json::json;

    fn context_for_request_body_text(body: impl Into<String>) -> GatewayVisibleHookContext {
        GatewayVisibleHookContext {
            hook_name: "gateway.request.afterBodyRead".to_string(),
            trace_id: "trace-privacy-filter-test".to_string(),
            request: GatewayVisibleRequestContext {
                cli_key: Some("codex".to_string()),
                method: Some("POST".to_string()),
                path: Some("/v1/chat/completions".to_string()),
                query: None,
                headers: None,
                body: Some(body.into()),
                requested_model: Some("gpt-test".to_string()),
                ..GatewayVisibleRequestContext::default()
            },
            response: GatewayVisibleResponseContext::default(),
            stream: GatewayVisibleStreamContext::default(),
            log: GatewayVisibleLogContext::default(),
        }
    }

    fn context_for_log_message(message: &str) -> GatewayVisibleHookContext {
        GatewayVisibleHookContext {
            hook_name: "log.beforePersist".to_string(),
            trace_id: "trace-privacy-filter-test".to_string(),
            request: GatewayVisibleRequestContext::default(),
            response: GatewayVisibleResponseContext::default(),
            stream: GatewayVisibleStreamContext::default(),
            log: GatewayVisibleLogContext {
                message: Some(message.to_string()),
            },
        }
    }

    fn official_privacy_filter_detail(config: serde_json::Value) -> PluginDetail {
        let fixture = crate::app::plugins::official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        let permissions = fixture.manifest.permissions.clone();
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "native:privacyFilter".to_string(),
                permission_risk: PluginPermissionRisk::High,
                update_available: false,
                last_error: None,
                created_at: 1,
                updated_at: 1,
            },
            manifest: fixture.manifest,
            install_source: PluginInstallSource::Official,
            installed_dir: Some(fixture.root_dir.to_string_lossy().to_string()),
            config,
            granted_permissions: permissions,
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
        }
    }

    fn execute_official_privacy_filter_request(
        config: serde_json::Value,
        body: impl Into<String>,
    ) -> serde_json::Value {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(config);
        let result = executor
            .execute_plugin(&plugin, context_for_request_body_text(body))
            .expect("privacy filter request hook");
        let output = result
            .request_body
            .expect("request body should be redacted");
        serde_json::from_str(&output).unwrap_or_else(|err| {
            panic!("redacted request body should remain valid JSON: {err}; body={output}")
        })
    }

    fn default_privacy_filter_config() -> serde_json::Value {
        json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        })
    }

    #[test]
    fn official_privacy_filter_redacts_phone_numbers_in_provider_request_shapes() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        }));

        for (name, body) in [
            (
                "claude",
                r#"{"messages":[{"role":"user","content":[{"type":"text","text":"phone 13344441520"}]}]}"#,
            ),
            (
                "openai_chat",
                r#"{"messages":[{"role":"user","content":"phone 13344441520"}]}"#,
            ),
            (
                "codex_responses",
                r#"{"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"phone 13344441520"}]}]}"#,
            ),
            ("raw_text", "phone 13344441520"),
        ] {
            let context = context_for_request_body_text(body);
            let result = executor
                .execute_plugin(&plugin, context)
                .unwrap_or_else(|err| panic!("{name} privacy filter failed: {err}"));
            let output = result
                .request_body
                .expect("request body should be redacted");
            assert!(
                !output.contains("13344441520"),
                "{name} leaked phone number: {output}"
            );
        }
    }

    #[test]
    fn official_privacy_filter_redacts_before_send_request_bodies() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        }));

        let mut context = context_for_request_body_text(
            r#"{"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"phone 13344441520"}]}]}"#,
        );
        context.hook_name = "gateway.request.beforeSend".to_string();

        let result = executor
            .execute_plugin(&plugin, context)
            .expect("privacy filter beforeSend hook");

        let output = result
            .request_body
            .expect("request body should be redacted");
        assert!(output.contains("[电话]"));
        assert!(!output.contains("13344441520"));
    }

    #[test]
    fn official_privacy_filter_redacts_only_claude_allowlisted_fields() {
        let tool_use_id = "toolu_123";
        let output = execute_official_privacy_filter_request(
            default_privacy_filter_config(),
            json!({
                "system": [
                    { "type": "text", "text": "系统邮箱 sys@example.com" }
                ],
                "tools": [
                    {
                        "name": "send_email",
                        "description": "Send to admin@example.com",
                        "input_schema": {
                            "type": "object",
                            "properties": {
                                "recipient": {
                                    "type": "string",
                                    "description": "Email like schema@example.com"
                                }
                            }
                        }
                    }
                ],
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "我的邮箱 user@example.com" }
                        ]
                    },
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": tool_use_id,
                                "name": "send_email",
                                "input": { "recipient": "tool-input@example.com" }
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": "工具读取到 result@example.com"
                            }
                        ]
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(
            output["tools"][0]["description"],
            "Send to admin@example.com"
        );
        assert_eq!(
            output["tools"][0]["input_schema"]["properties"]["recipient"]["description"],
            "Email like schema@example.com"
        );
        assert_eq!(
            output["messages"][1]["content"][0]["input"]["recipient"],
            "tool-input@example.com"
        );
        assert_eq!(output["messages"][1]["content"][0]["id"], tool_use_id);
        assert!(output["system"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["messages"][0]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["messages"][2]["content"][0]["content"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
    }

    #[test]
    fn official_privacy_filter_respects_disabled_tool_result_scope() {
        let output = execute_official_privacy_filter_request(
            json!({
                "redactBeforeUpstream": true,
                "redactLogs": true,
                "redactionScopes": ["system_instructions", "user_prompts", "legacy_prompt"]
            }),
            json!({
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            { "type": "text", "text": "用户邮箱 user@example.com" }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            { "type": "tool_result", "tool_use_id": "toolu_123", "content": "工具结果 result@example.com" }
                        ]
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(
            output["messages"][1]["content"][0]["content"],
            "工具结果 result@example.com"
        );
        assert!(output["messages"][0]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
    }

    #[test]
    fn official_privacy_filter_redacts_only_openai_responses_allowlisted_fields() {
        let output = execute_official_privacy_filter_request(
            default_privacy_filter_config(),
            json!({
                "instructions": "系统邮箱 sys@example.com",
                "tools": [
                    {
                        "type": "function",
                        "name": "lookup",
                        "description": "Lookup admin@example.com",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "query": {
                                    "type": "string",
                                    "description": "Query schema@example.com"
                                }
                            }
                        }
                    }
                ],
                "input": [
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            { "type": "input_text", "text": "用户邮箱 user@example.com" }
                        ]
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_123",
                        "name": "lookup",
                        "arguments": "{\"email\":\"args@example.com\"}"
                    },
                    {
                        "type": "function_call_output",
                        "call_id": "call_123",
                        "output": "工具输出 result@example.com"
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(
            output["tools"][0]["description"],
            "Lookup admin@example.com"
        );
        assert_eq!(
            output["tools"][0]["parameters"]["properties"]["query"]["description"],
            "Query schema@example.com"
        );
        assert_eq!(
            output["input"][1]["arguments"],
            "{\"email\":\"args@example.com\"}"
        );
        assert!(output["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["input"][0]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["input"][2]["output"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
    }

    #[test]
    fn official_privacy_filter_redacts_codex_responses_payload_shape() {
        let output = execute_official_privacy_filter_request(
            default_privacy_filter_config(),
            json!({
                "model": "gpt-5.5",
                "instructions": "developer prompt with sys@example.com",
                "input": [
                    {
                        "type": "message",
                        "role": "developer",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "developer-visible phone 13344441521"
                            }
                        ]
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "你知道 13344441520 是哪里的手机号嘛"
                            }
                        ]
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_123",
                        "name": "lookup_phone",
                        "arguments": "{\"phone\":\"13344441522\"}"
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "name": "lookup_phone",
                        "description": "Lookup 13344441523",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "phone": {
                                    "type": "string",
                                    "description": "Phone like 13344441524"
                                }
                            }
                        }
                    }
                ],
                "tool_choice": "auto",
                "reasoning": { "effort": "xhigh" },
                "client_metadata": {
                    "x-codex-window-id": "13344441525"
                }
            })
            .to_string(),
        );

        assert!(output["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["input"][0]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[电话]")));
        assert!(output["input"][1]["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("[电话]")));
        assert_eq!(
            output["input"][2]["arguments"],
            "{\"phone\":\"13344441522\"}"
        );
        assert_eq!(output["tools"][0]["description"], "Lookup 13344441523");
        assert_eq!(
            output["tools"][0]["parameters"]["properties"]["phone"]["description"],
            "Phone like 13344441524"
        );
        assert_eq!(
            output["client_metadata"]["x-codex-window-id"],
            "13344441525"
        );
    }

    #[test]
    fn official_privacy_filter_redacts_only_chat_allowlisted_fields() {
        let output = execute_official_privacy_filter_request(
            default_privacy_filter_config(),
            json!({
                "messages": [
                    { "role": "system", "content": "系统邮箱 sys@example.com" },
                    { "role": "user", "content": "用户邮箱 user@example.com" },
                    {
                        "role": "assistant",
                        "tool_calls": [
                            {
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "lookup",
                                    "arguments": "{\"email\":\"args@example.com\"}"
                                }
                            }
                        ]
                    },
                    {
                        "role": "tool",
                        "tool_call_id": "call_123",
                        "content": "工具输出 result@example.com"
                    }
                ],
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "lookup",
                            "description": "Lookup admin@example.com",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "email": { "type": "string", "description": "Schema schema@example.com" }
                                }
                            }
                        }
                    }
                ]
            })
            .to_string(),
        );

        assert_eq!(
            output["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{\"email\":\"args@example.com\"}"
        );
        assert_eq!(
            output["tools"][0]["function"]["description"],
            "Lookup admin@example.com"
        );
        assert_eq!(
            output["tools"][0]["function"]["parameters"]["properties"]["email"]["description"],
            "Schema schema@example.com"
        );
        assert!(output["messages"][0]["content"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["messages"][1]["content"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
        assert!(output["messages"][3]["content"]
            .as_str()
            .is_some_and(|text| text.contains("[邮箱]")));
    }

    #[test]
    fn official_privacy_filter_respects_legacy_prompt_scope_for_raw_text() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true,
            "redactionScopes": ["system_instructions", "user_prompts", "tool_results"]
        }));

        let result = executor
            .execute_plugin(
                &plugin,
                context_for_request_body_text("raw email raw@example.com"),
            )
            .expect("privacy filter request hook");

        assert!(
            result.request_body.is_none(),
            "raw text should not be redacted when legacy_prompt scope is disabled"
        );
    }

    #[test]
    fn official_privacy_filter_log_redaction_ignores_request_redaction_scopes() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true,
            "redactionScopes": []
        }));
        let context = context_for_log_message("trace email log@example.com");

        let result = executor
            .execute_plugin(&plugin, context)
            .expect("privacy filter log hook");

        let message = result.log_message.expect("log message should be redacted");
        assert!(message.contains("[邮箱]"));
        assert!(!message.contains("log@example.com"));
    }

    #[test]
    fn official_privacy_filter_preserves_claude_tool_use_protocol_ids() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        }));
        let tool_use_id = "ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ";
        let tool_use_input_phone = "13344441520";
        let tool_result_phone = "13344441521";
        let context = context_for_request_body_text(
            json!({
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": tool_use_id,
                                "name": "lookup_phone",
                                "input": { "query": format!("你知道 {tool_use_input_phone} 是哪里的手机号嘛") }
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": format!("手机号 {tool_result_phone} 查询完成")
                            }
                        ]
                    }
                ]
            })
            .to_string(),
        );

        let result = executor
            .execute_plugin(&plugin, context)
            .expect("privacy filter request hook");

        let output = result
            .request_body
            .expect("request body should be redacted");
        assert!(
            output.contains(tool_use_id),
            "tool id was changed: {output}"
        );
        assert!(
            output.contains(tool_use_input_phone),
            "tool input should remain unchanged: {output}"
        );
        assert!(output.contains("[电话]"));
        assert!(!output.contains(tool_result_phone));
    }

    #[test]
    fn official_privacy_filter_redacts_log_messages_after_request_redaction() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        }));
        let context = context_for_log_message("trace log 13344441520");

        let result = executor
            .execute_plugin(&plugin, context)
            .expect("privacy filter log hook");

        let message = result.log_message.expect("log message should be redacted");
        assert!(!message.contains("13344441520"));
    }

    #[test]
    fn official_privacy_filter_respects_sensitive_types_config() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true,
            "sensitiveTypes": ["email"]
        }));

        let result = executor
            .execute_plugin(
                &plugin,
                context_for_request_body_text(
                    r#"{"messages":[{"role":"user","content":"email test@example.com phone 13344441520"}]}"#,
                ),
            )
            .expect("privacy filter request hook");

        let output = result
            .request_body
            .expect("request body should be redacted");
        assert!(output.contains("[邮箱]"));
        assert!(!output.contains("test@example.com"));
        assert!(
            output.contains("13344441520"),
            "cn_phone should remain visible when sensitiveTypes omits it: {output}"
        );
    }

    #[test]
    fn official_privacy_filter_allows_disabling_all_sensitive_types() {
        let executor = OfficialPrivacyFilterRuntime::default();
        let plugin = official_privacy_filter_detail(json!({
            "redactBeforeUpstream": true,
            "redactLogs": true,
            "sensitiveTypes": []
        }));

        let result = executor
            .execute_plugin(
                &plugin,
                context_for_request_body_text(
                    r#"{"messages":[{"role":"user","content":"email test@example.com phone 13344441520"}]}"#,
                ),
            )
            .expect("privacy filter request hook");

        assert!(
            result.request_body.is_none(),
            "empty sensitiveTypes should disable every configured strategy"
        );
    }
}
