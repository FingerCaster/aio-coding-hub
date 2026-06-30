//! Usage: Host privacy redaction service for Extension Host plugins.

use super::privacy_filter::{PrivacyFilter, PrivacyFilterError, PrivacyFilterOptions};
use super::runtime_cache::{runtime_cache_key, RuntimeCacheKeyInput};
use super::runtime_lifecycle::PluginRuntimeCache;
use crate::plugins::PluginDetail;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex};

pub(crate) const MAX_PRIVACY_FILTER_RULE_FILE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrivacyRedactionOutput {
    pub(crate) hit: bool,
    pub(crate) count: usize,
    pub(crate) redacted: String,
}

#[derive(Default)]
pub(crate) struct PrivacyRedactionService {
    cache: Mutex<HashMap<String, Arc<PrivacyFilter>>>,
}

impl PrivacyRedactionService {
    pub(crate) fn redact_text(
        &self,
        plugin: &PluginDetail,
        text: &str,
        options: &Value,
    ) -> Result<PrivacyRedactionOutput, PrivacyFilterError> {
        let filter = self.get_or_load_privacy_filter(plugin)?;
        let options = privacy_filter_options_from_config(options);
        let redacted = filter.redact_with_options(text, &options);
        Ok(PrivacyRedactionOutput {
            hit: redacted.hit,
            count: redacted.count,
            redacted: redacted.redacted,
        })
    }

    pub(crate) fn redact_request_body(
        &self,
        plugin: &PluginDetail,
        body: &str,
        options: &Value,
    ) -> Result<PrivacyRedactionOutput, PrivacyFilterError> {
        let filter = self.get_or_load_privacy_filter(plugin)?;
        let filter_options = privacy_filter_options_from_config(options);
        let scopes = privacy_filter_redaction_scopes_from_config(options);
        let redacted = redact_request_body_strings(&filter, body, &filter_options, &scopes)?;
        Ok(redacted.unwrap_or_else(|| PrivacyRedactionOutput {
            hit: false,
            count: 0,
            redacted: body.to_string(),
        }))
    }

    #[allow(dead_code)]
    pub(crate) fn clear_runtime_caches(&self) {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn get_or_load_privacy_filter(
        &self,
        plugin: &PluginDetail,
    ) -> Result<Arc<PrivacyFilter>, PrivacyFilterError> {
        let cache_key = privacy_filter_cache_key(plugin);
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(filter) = cache.get(&cache_key) {
            return Ok(Arc::clone(filter));
        }

        let filter = Arc::new(load_privacy_filter(plugin)?);
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

impl PluginRuntimeCache for PrivacyRedactionService {
    fn retain_for_plugins(&self, plugins: &[PluginDetail]) {
        let privacy_plugins = plugins
            .iter()
            .filter(|plugin| has_privacy_redact_capability(plugin))
            .collect::<Vec<_>>();
        let privacy_keys = privacy_plugins
            .iter()
            .map(|plugin| privacy_filter_cache_key(plugin))
            .collect::<HashSet<_>>();

        for plugin in privacy_plugins {
            if let Err(err) = self.get_or_load_privacy_filter(plugin) {
                tracing::warn!(
                    plugin_id = %plugin.summary.plugin_id,
                    error = %err,
                    "failed to prewarm privacy redaction service"
                );
            }
        }

        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| privacy_keys.contains(key));
    }

    fn clear_all(&self) {
        self.clear_runtime_caches();
    }
}

fn has_privacy_redact_capability(plugin: &PluginDetail) -> bool {
    plugin
        .manifest
        .capabilities
        .iter()
        .any(|capability| capability == "privacy.redact")
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
        runtime_key: "privacy.redact",
    })
}

fn load_privacy_filter(plugin: &PluginDetail) -> Result<PrivacyFilter, PrivacyFilterError> {
    let root_dir = plugin.installed_dir.as_deref().ok_or_else(|| {
        PrivacyFilterError::new(format!(
            "plugin {} has no installed_dir for privacy-filter rule loading",
            plugin.summary.plugin_id
        ))
    })?;
    let rules_path = std::path::Path::new(root_dir).join("rules/gitleaks.toml");
    let metadata = fs::metadata(&rules_path).map_err(|err| {
        PrivacyFilterError::new(format!(
            "failed to read privacy-filter gitleaks rules metadata for plugin {}: {err}",
            plugin.summary.plugin_id
        ))
    })?;
    if metadata.len() > MAX_PRIVACY_FILTER_RULE_FILE_BYTES as u64 {
        return Err(PrivacyFilterError::new(format!(
            "privacy filter rule file exceeds {MAX_PRIVACY_FILTER_RULE_FILE_BYTES} bytes"
        )));
    }
    let raw = fs::read_to_string(&rules_path).map_err(|err| {
        PrivacyFilterError::new(format!(
            "failed to read privacy-filter gitleaks rules for plugin {}: {err}",
            plugin.summary.plugin_id
        ))
    })?;
    PrivacyFilter::from_gitleaks_toml(&raw)
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
) -> Result<Option<PrivacyRedactionOutput>, PrivacyFilterError> {
    let Ok(mut root) = serde_json::from_str::<Value>(body) else {
        if !scopes.legacy_prompt {
            return Ok(None);
        }
        let redacted = filter.redact_with_options(body, options);
        return Ok(redacted.hit.then_some(PrivacyRedactionOutput {
            hit: true,
            count: redacted.count,
            redacted: redacted.redacted,
        }));
    };
    let mut matched = false;
    redact_request_json_allowlist(&mut root, filter, options, scopes, &mut matched);
    if !matched {
        return Ok(None);
    }
    let redacted = serde_json::to_string(&root).map_err(|err| {
        PrivacyFilterError::new(format!("failed to serialize redacted JSON: {err}"))
    })?;
    Ok(Some(PrivacyRedactionOutput {
        hit: true,
        count: 1,
        redacted,
    }))
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
        Some("text") if scopes.message_content_enabled(role) => {
            if let Some(text) = map.get_mut("text") {
                redact_text_value(text, filter, options, matched);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::plugins::runtime_lifecycle::PluginRuntimeCache;
    use crate::plugins::{
        PluginDetail, PluginInstallSource, PluginPermissionRisk, PluginStatus, PluginSummary,
    };
    use serde_json::json;

    fn privacy_filter_detail(config: serde_json::Value) -> PluginDetail {
        let fixture = crate::app::plugins::official::official_plugin("official.privacy-filter")
            .expect("official privacy filter fixture");
        PluginDetail {
            summary: PluginSummary {
                id: 1,
                plugin_id: fixture.manifest.id.clone(),
                name: fixture.manifest.name.clone(),
                current_version: Some(fixture.manifest.version.clone()),
                status: PluginStatus::Enabled,
                runtime: "extensionHost".to_string(),
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
            granted_permissions: vec![],
            pending_permissions: vec![],
            audit_logs: vec![],
            runtime_failures: vec![],
            rollback_versions: vec![],
        }
    }

    fn privacy_filter_plugin_detail_with_dir(
        installed_dir: String,
        config: serde_json::Value,
    ) -> PluginDetail {
        let mut plugin = privacy_filter_detail(config);
        plugin.installed_dir = Some(installed_dir);
        plugin
    }

    fn execute_privacy_filter_request(
        config: serde_json::Value,
        body: impl Into<String>,
    ) -> serde_json::Value {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));
        let output = service
            .redact_request_body(&plugin, &body.into(), &config)
            .expect("privacy filter request redaction");
        assert!(output.hit, "request body should be redacted");
        serde_json::from_str(&output.redacted).unwrap_or_else(|err| {
            panic!(
                "redacted request body should remain valid JSON: {err}; body={}",
                output.redacted
            )
        })
    }

    fn default_privacy_filter_config() -> serde_json::Value {
        json!({
            "redactBeforeUpstream": true,
            "redactLogs": true
        })
    }

    #[test]
    fn privacy_redaction_service_rejects_rule_file_over_byte_limit() {
        let dir = tempfile::tempdir().expect("temp plugin dir");
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).expect("rules dir");
        std::fs::write(
            rules_dir.join("gitleaks.toml"),
            format!(
                "title = \"rules\"\n{}",
                " ".repeat(MAX_PRIVACY_FILTER_RULE_FILE_BYTES + 1)
            ),
        )
        .expect("rules file");

        let plugin = privacy_filter_plugin_detail_with_dir(
            dir.path().to_string_lossy().to_string(),
            serde_json::json!({ "redactLogs": true }),
        );

        let err = load_privacy_filter(&plugin).expect_err("oversized rules should fail");

        assert!(err.to_string().contains("privacy filter rule file exceeds"));
    }

    #[test]
    fn privacy_redaction_service_retain_prewarms_and_prunes_privacy_redact_plugins() {
        let dir = tempfile::tempdir().expect("temp plugin dir");
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).expect("rules dir");
        std::fs::write(rules_dir.join("gitleaks.toml"), "").expect("rules file");
        let plugin = privacy_filter_plugin_detail_with_dir(
            dir.path().to_string_lossy().to_string(),
            json!({}),
        );
        let service = PrivacyRedactionService::default();

        service.retain_for_plugins(&[plugin]);

        assert_eq!(service.cache_size_for_tests(), 1);

        service.retain_for_plugins(&[]);

        assert_eq!(service.cache_size_for_tests(), 0);
    }

    #[test]
    fn privacy_redaction_service_redacts_phone_numbers_in_provider_request_shapes() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));
        let config = json!({});

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
            let output = service
                .redact_request_body(&plugin, body, &config)
                .unwrap_or_else(|err| panic!("{name} privacy filter failed: {err}"))
                .redacted;
            assert!(
                !output.contains("13344441520"),
                "{name} leaked phone number: {output}"
            );
        }
    }

    #[test]
    fn privacy_redaction_service_redacts_before_send_request_bodies() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let output = service
            .redact_request_body(
                &plugin,
                r#"{"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"phone 13344441520"}]}]}"#,
                &json!({}),
            )
            .expect("privacy filter request body redaction")
            .redacted;
        assert!(output.contains("[电话]"));
        assert!(!output.contains("13344441520"));
    }

    #[test]
    fn privacy_redaction_service_redacts_only_claude_allowlisted_fields() {
        let tool_use_id = "toolu_123";
        let output = execute_privacy_filter_request(
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
    fn privacy_redaction_service_respects_disabled_tool_result_scope() {
        let output = execute_privacy_filter_request(
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
    fn privacy_redaction_service_redacts_only_openai_responses_allowlisted_fields() {
        let output = execute_privacy_filter_request(
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
    fn privacy_redaction_service_redacts_codex_responses_payload_shape() {
        let output = execute_privacy_filter_request(
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
    fn privacy_redaction_service_redacts_only_chat_allowlisted_fields() {
        let output = execute_privacy_filter_request(
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
    fn privacy_redaction_service_respects_legacy_prompt_scope_for_raw_text() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let result = service
            .redact_request_body(
                &plugin,
                "raw email raw@example.com",
                &json!({
                    "redactionScopes": ["system_instructions", "user_prompts", "tool_results"]
                }),
            )
            .expect("privacy filter request redaction");

        assert!(!result.hit);
        assert_eq!(result.redacted, "raw email raw@example.com");
    }

    #[test]
    fn privacy_redaction_service_log_redaction_ignores_request_redaction_scopes() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let result = service
            .redact_text(
                &plugin,
                "trace email log@example.com",
                &json!({ "redactionScopes": [] }),
            )
            .expect("privacy filter log redaction");

        assert!(result.redacted.contains("[邮箱]"));
        assert!(!result.redacted.contains("log@example.com"));
    }

    #[test]
    fn privacy_redaction_service_preserves_claude_tool_use_protocol_ids() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));
        let tool_use_id = "ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ";
        let tool_use_input_phone = "13344441520";
        let tool_result_phone = "13344441521";
        let body =
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
            .to_string();

        let output = service
            .redact_request_body(&plugin, &body, &json!({}))
            .expect("privacy filter request redaction")
            .redacted;
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
    fn privacy_redaction_service_redacts_log_messages_after_request_redaction() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let result = service
            .redact_text(&plugin, "trace log 13344441520", &json!({}))
            .expect("privacy filter log redaction");

        assert!(!result.redacted.contains("13344441520"));
    }

    #[test]
    fn privacy_redaction_service_respects_sensitive_types_config() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let output = service
            .redact_request_body(
                &plugin,
                r#"{"messages":[{"role":"user","content":"email test@example.com phone 13344441520"}]}"#,
                &json!({ "sensitiveTypes": ["email"] }),
            )
            .expect("privacy filter request redaction")
            .redacted;
        assert!(output.contains("[邮箱]"));
        assert!(!output.contains("test@example.com"));
        assert!(
            output.contains("13344441520"),
            "cn_phone should remain visible when sensitiveTypes omits it: {output}"
        );
    }

    #[test]
    fn privacy_redaction_service_allows_disabling_all_sensitive_types() {
        let service = PrivacyRedactionService::default();
        let plugin = privacy_filter_detail(json!({}));

        let result = service
            .redact_request_body(
                &plugin,
                r#"{"messages":[{"role":"user","content":"email test@example.com phone 13344441520"}]}"#,
                &json!({ "sensitiveTypes": [] }),
            )
            .expect("privacy filter request redaction");

        assert!(!result.hit);
        assert_eq!(
            result.redacted,
            r#"{"messages":[{"role":"user","content":"email test@example.com phone 13344441520"}]}"#
        );
    }
}
