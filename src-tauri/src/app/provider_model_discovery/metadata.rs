//! Allowlisted, partial capabilities from the same bounded, read-only catalog response.
use super::{catalog_items, grok_oauth_model_id, ModelCatalogFormat};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelCapabilitySuggestion {
    pub model_id: String,
    pub display_name: Option<String>,
    pub input: Option<Vec<String>>,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
    pub supports_tools: Option<bool>,
    pub reasoning: Option<bool>,
    pub reasoning_efforts: Option<Vec<String>>,
    pub default_reasoning_effort: Option<String>,
    pub thinking_mode: Option<String>,
    pub supports_display: Option<bool>,
    pub requires_effort: Option<bool>,
    pub native_thinking: Option<crate::domain::native_gateway::ThinkingSpec>,
    pub sources: Vec<String>,
}

fn clean_text(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty() && text.len() <= 256 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}
fn number(item: &Value, names: &[&str]) -> Option<u32> {
    names.iter().find_map(|name| {
        item.get(name)?
            .as_u64()
            .filter(|v| (1..=10_000_000).contains(v))
            .map(|v| v as u32)
    })
}
fn boolean(item: &Value, names: &[&str]) -> Option<bool> {
    names.iter().find_map(|name| item.get(name)?.as_bool())
}
fn string(item: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| item.get(name).and_then(clean_text))
}

pub(super) fn parse_capabilities(
    format: ModelCatalogFormat,
    body: &str,
) -> Vec<ModelCapabilitySuggestion> {
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(items) = catalog_items(&root, format) else {
        return Vec::new();
    };
    let mut by_id: BTreeMap<String, ModelCapabilitySuggestion> = BTreeMap::new();
    let mut conflicting = std::collections::BTreeSet::new();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let id = match format {
            ModelCatalogFormat::DataIds => string(item, &["id"]),
            ModelCatalogFormat::CodexOAuthSlugs => string(item, &["slug"]),
            ModelCatalogFormat::GeminiNames => string(item, &["name"]),
            ModelCatalogFormat::GrokOAuth => grok_oauth_model_id(object),
        };
        let Some(id) = id else { continue };
        let id = if matches!(
            format,
            ModelCatalogFormat::GeminiNames | ModelCatalogFormat::GrokOAuth
        ) {
            id.strip_prefix("models/").unwrap_or(&id).trim().to_owned()
        } else {
            id.trim().to_owned()
        };
        let mut info = ModelCapabilitySuggestion {
            model_id: id.clone(),
            display_name: string(item, &["display_name", "displayName"]),
            context_window: number(
                item,
                &["context_window", "contextWindow", "inputTokenLimit"],
            ),
            max_tokens: number(
                item,
                &[
                    "max_output_tokens",
                    "max_tokens",
                    "maxTokens",
                    "outputTokenLimit",
                ],
            ),
            // Parallel-tool support says nothing about ordinary tool support.
            supports_tools: boolean(item, &["supports_tools", "supportsTools"]),
            reasoning: boolean(item, &["reasoning", "supports_reasoning", "thinking"]),
            sources: vec!["upstream".into()],
            ..Default::default()
        };
        if let Some(modalities) = item
            .get("input_modalities")
            .or_else(|| item.get("input"))
            .and_then(Value::as_array)
        {
            let all: Option<Vec<&str>> = modalities.iter().map(Value::as_str).collect();
            if let Some(all) = all.filter(|v| v.contains(&"text")) {
                info.input = Some(if all.contains(&"image") {
                    vec!["text".into(), "image".into()]
                } else {
                    vec!["text".into()]
                });
            }
        }
        if let Some(efforts) = item
            .get("supported_reasoning_levels")
            .or_else(|| item.get("supported_reasoning_efforts"))
            .and_then(Value::as_array)
        {
            let values: Option<Vec<String>> = efforts
                .iter()
                .map(|v| clean_text(v.get("effort").unwrap_or(v)).filter(|s| s.len() <= 64))
                .collect();
            if let Some(mut values) = values.filter(|v| v.len() <= 16) {
                values.sort();
                values.dedup();
                if info.reasoning != Some(false)
                    && values.iter().any(|v| !matches!(v.as_str(), "off" | "none"))
                {
                    info.reasoning = Some(true);
                }
                info.reasoning_efforts = Some(values);
            }
        }
        info.default_reasoning_effort = string(
            item,
            &["default_reasoning_level", "default_reasoning_effort"],
        )
        .filter(|v| {
            info.reasoning_efforts
                .as_ref()
                .is_some_and(|levels| levels.contains(v))
        });
        info.thinking_mode = string(item, &["thinking_mode"]).filter(|v| {
            matches!(
                v.as_str(),
                "effort"
                    | "budget"
                    | "google-level"
                    | "anthropic-adaptive"
                    | "anthropic-budget-effort"
            )
        });
        info.supports_display =
            boolean(item, &["supports_reasoning_summaries", "supports_display"]);
        info.requires_effort = boolean(item, &["requires_effort"]);
        if info.reasoning == Some(false) {
            info.reasoning_efforts = None;
            info.default_reasoning_effort = None;
            info.thinking_mode = None;
        }
        if info
            .context_window
            .zip(info.max_tokens)
            .is_some_and(|(context, max)| max > context)
        {
            info.max_tokens = None;
        }
        // Duplicate conflicting rows cannot silently widen a model's capabilities.
        if by_id.get(&id).is_some_and(|old| old != &info) {
            conflicting.insert(id.clone());
        }
        by_id.insert(id, info);
    }
    for id in conflicting {
        by_id.insert(
            id.clone(),
            ModelCapabilitySuggestion {
                model_id: id,
                sources: vec!["upstream_conflict".into()],
                ..Default::default()
            },
        );
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_preserves_explicit_fields_without_guessing_tools_or_output() {
        let values = parse_capabilities(
            ModelCatalogFormat::CodexOAuthSlugs,
            r#"{"models":[{"slug":"future","context_window":272000,"supports_parallel_tool_calls":false,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"}]}"#,
        );
        assert_eq!(values[0].context_window, Some(272000));
        assert_eq!(values[0].supports_tools, None);
        assert_eq!(values[0].max_tokens, None);
        assert_eq!(values[0].reasoning, Some(true));
        assert_eq!(values[0].default_reasoning_effort.as_deref(), Some("high"));
    }
    #[test]
    fn gemini_preserves_limits_and_unknowns() {
        let values = parse_capabilities(
            ModelCatalogFormat::GeminiNames,
            r#"{"models":[{"name":"models/new","inputTokenLimit":1000000,"outputTokenLimit":65536,"thinking":true}]}"#,
        );
        assert_eq!(values[0].model_id, "new");
        assert_eq!(values[0].max_tokens, Some(65536));
        assert_eq!(values[0].reasoning_efforts, None);
        assert_eq!(values[0].supports_tools, None);
    }
    #[test]
    fn rejects_invalid_and_conflicting_capabilities_without_losing_ids() {
        let values = parse_capabilities(
            ModelCatalogFormat::DataIds,
            r#"{"data":[{"id":"a","context_window":100},{"id":"a","context_window":200},{"id":"b","max_tokens":-1,"supports_tools":"true","reasoning":false,"supported_reasoning_efforts":["high"]}]}"#,
        );
        assert_eq!(values[0].context_window, None);
        assert_eq!(values[0].sources, ["upstream_conflict"]);
        assert_eq!(values[1].max_tokens, None);
        assert_eq!(values[1].supports_tools, None);
        assert_eq!(values[1].reasoning_efforts, None);
    }
}
