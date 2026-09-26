//! Versioned Pi/OMP catalog defaults for exact source/protocol/model identities.
//! This only enriches existing candidates. It never advertises the whole catalog.
use super::provider_model_discovery::ModelCapabilitySuggestion;
use crate::domain::native_gateway::ThinkingSpec;
use crate::shared::gateway_protocol::GatewayProtocol;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Catalog {
    rows: Vec<Row>,
}
#[derive(Deserialize)]
struct Row {
    client: String,
    provider: String,
    protocol: GatewayProtocol,
    suggestion: ModelCapabilitySuggestion,
}
fn rows() -> &'static [Row] {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    &CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("native_model_defaults.json"))
                .expect("validated bundled native model catalog")
        })
        .rows
}

/// Suggestions only: presence in this catalog is not an authentication check.
pub(crate) fn omp_settings_models() -> Vec<crate::domain::omp_settings::OmpModelOption> {
    rows()
        .iter()
        .filter(|row| row.client == "omp")
        .map(|row| crate::domain::omp_settings::OmpModelOption {
            selector: format!("{}/{}", row.provider, row.suggestion.model_id),
            label: format!(
                "{} · {}",
                row.suggestion
                    .display_name
                    .as_deref()
                    .unwrap_or(&row.suggestion.model_id),
                row.provider
            ),
            source: "内置目录 · OMP 18.3.2（未检查登录）".into(),
            thinking_levels: row.suggestion.reasoning_efforts.clone().unwrap_or_default(),
        })
        .collect()
}

pub(super) fn fill_catalog_defaults(
    models: &mut [ModelCapabilitySuggestion],
    client: &str,
    source: &str,
    oauth: bool,
    protocol: GatewayProtocol,
) {
    let provider = match (source, oauth) {
        ("codex", true) => "openai-codex",
        ("codex", false) => "openai",
        ("claude", _) => "anthropic",
        ("gemini", false) => "google",
        ("grok", true) => "xai-oauth",
        ("grok", false) => "xai",
        _ => return,
    };
    for model in models {
        if model
            .sources
            .iter()
            .any(|s| matches!(s.as_str(), "upstream_conflict" | "routing_confirmation"))
        {
            continue;
        }
        let Some(row) = rows().iter().find(|row| {
            row.client == client
                && row.provider == provider
                && row.protocol == protocol
                && row.suggestion.model_id == model.model_id
        }) else {
            continue;
        };
        let defaults = &row.suggestion;
        let before = model.clone();
        if model.display_name.is_none() {
            model.display_name = defaults.display_name.clone();
        }
        if model.input.is_none() {
            model.input = defaults.input.clone();
        }
        if model.context_window.is_none() {
            model.context_window = defaults.context_window;
        }
        if model.max_tokens.is_none() {
            model.max_tokens = defaults
                .max_tokens
                .map(|v| model.context_window.map_or(v, |c| v.min(c)));
        }
        if model.supports_tools.is_none() {
            model.supports_tools = defaults.supports_tools;
        }
        if model.reasoning.is_none() {
            model.reasoning = defaults.reasoning;
        }
        if model.reasoning == Some(true) {
            if model.reasoning_efforts.is_none() {
                model.reasoning_efforts = defaults.reasoning_efforts.clone();
            }
            if model.thinking_mode.is_none() {
                model.thinking_mode = defaults.thinking_mode.clone();
            }
            if model.supports_display.is_none() {
                model.supports_display = defaults.supports_display;
            }
            if model.requires_effort.is_none() {
                model.requires_effort = defaults.requires_effort;
            }
            // Retain native non-identity mappings, but never widen explicit upstream efforts.
            let mut thinking = defaults.native_thinking.clone();
            if model.thinking_mode != defaults.thinking_mode {
                thinking = None;
            }
            if let Some(supported) = &model.reasoning_efforts {
                match thinking.as_mut() {
                    Some(ThinkingSpec::Pi { level_map }) => {
                        for target in level_map.values_mut() {
                            if target
                                .as_ref()
                                .is_some_and(|value| !supported.contains(value))
                            {
                                *target = None;
                            }
                        }
                        if !level_map
                            .iter()
                            .any(|(level, value)| level != "off" && value.is_some())
                        {
                            thinking = None;
                        }
                    }
                    Some(ThinkingSpec::Omp {
                        efforts,
                        effort_map,
                        default_level,
                        ..
                    }) => {
                        efforts.retain(|level| {
                            supported.contains(effort_map.get(level).unwrap_or(level))
                        });
                        effort_map.retain(|level, _| efforts.contains(level));
                        *default_level = model
                            .default_reasoning_effort
                            .as_ref()
                            .and_then(|wanted| {
                                efforts
                                    .iter()
                                    .find(|level| effort_map.get(*level).unwrap_or(level) == wanted)
                            })
                            .cloned()
                            .or_else(|| {
                                default_level
                                    .clone()
                                    .filter(|level| efforts.contains(level))
                            })
                            .or_else(|| {
                                ["medium", "low"].iter().find_map(|level| {
                                    efforts.iter().find(|v| v.as_str() == *level).cloned()
                                })
                            })
                            .or_else(|| efforts.first().cloned());
                        if efforts.is_empty() {
                            thinking = None;
                        }
                    }
                    None => {}
                }
            }
            model.native_thinking = thinking;
            if model.default_reasoning_effort.is_none() {
                model.default_reasoning_effort =
                    defaults.default_reasoning_effort.clone().filter(|v| {
                        model
                            .reasoning_efforts
                            .as_ref()
                            .is_some_and(|levels| levels.contains(v))
                    });
            }
        }
        if *model != before {
            model.sources.extend(defaults.sources.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty(id: &str) -> ModelCapabilitySuggestion {
        ModelCapabilitySuggestion {
            model_id: id.into(),
            ..Default::default()
        }
    }
    #[test]
    fn enriches_exact_models_with_client_specific_capacities_and_thinking() {
        let mut models = vec![empty("gpt-5.4"), empty("custom-gpt-5.4")];
        fill_catalog_defaults(
            &mut models,
            "omp",
            "codex",
            false,
            GatewayProtocol::OpenaiResponses,
        );
        assert!(models[0].context_window.is_some());
        assert!(models[0].max_tokens.is_some());
        assert_eq!(models[0].supports_tools, Some(true));
        assert!(
            matches!(&models[0].native_thinking, Some(ThinkingSpec::Omp { default_level: Some(v), .. }) if v == "medium")
        );
        assert_eq!(models[1], empty("custom-gpt-5.4"));
        let mut pi = vec![empty("gpt-5.4")];
        fill_catalog_defaults(
            &mut pi,
            "pi",
            "codex",
            false,
            GatewayProtocol::OpenaiResponses,
        );
        assert!(
            matches!(&pi[0].native_thinking, Some(ThinkingSpec::Pi { level_map }) if level_map["off"].as_deref() == Some("none"))
        );
    }
    #[test]
    fn preserves_explicit_constraints_and_never_bypasses_identity_or_conflicts() {
        let mut models = vec![ModelCapabilitySuggestion {
            context_window: Some(32000),
            supports_tools: Some(false),
            reasoning_efforts: Some(vec!["high".into()]),
            ..empty("gpt-5.4")
        }];
        fill_catalog_defaults(
            &mut models,
            "omp",
            "codex",
            false,
            GatewayProtocol::OpenaiResponses,
        );
        assert_eq!(models[0].context_window, Some(32000));
        assert_eq!(models[0].max_tokens, Some(32000));
        assert_eq!(models[0].supports_tools, Some(false));
        assert!(
            matches!(&models[0].native_thinking, Some(ThinkingSpec::Omp {efforts, default_level:Some(level), ..}) if efforts == &["high"] && level == "high")
        );
        for source in ["upstream_conflict", "routing_confirmation"] {
            let mut models = vec![ModelCapabilitySuggestion {
                sources: vec![source.into()],
                ..empty("gpt-5.4")
            }];
            let before = models.clone();
            fill_catalog_defaults(
                &mut models,
                "omp",
                "codex",
                false,
                GatewayProtocol::OpenaiResponses,
            );
            assert_eq!(models, before);
        }
        for (source, protocol) in [
            ("claude", GatewayProtocol::OpenaiResponses),
            ("codex", GatewayProtocol::AnthropicMessages),
        ] {
            let mut models = vec![empty("gpt-5.4")];
            fill_catalog_defaults(&mut models, "omp", source, false, protocol);
            assert_eq!(models[0], empty("gpt-5.4"));
        }
    }
    #[test]
    fn bundled_complete_models_validate_for_the_native_client() {
        use crate::domain::native_gateway::{ModelInput, NativeModelSpec};
        let mut count = 0;
        for row in rows() {
            let s = &row.suggestion;
            if s.supports_tools != Some(true)
                || (s.reasoning == Some(true) && s.native_thinking.is_none())
            {
                continue;
            }
            NativeModelSpec {
                request_model_id: s.model_id.clone(),
                display_name: s.display_name.clone().unwrap(),
                context_window: s.context_window.unwrap(),
                max_tokens: s.max_tokens.unwrap(),
                input: s
                    .input
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|v| {
                        if v == "image" {
                            ModelInput::Image
                        } else {
                            ModelInput::Text
                        }
                    })
                    .collect(),
                supports_tools: s.supports_tools,
                reasoning: s.reasoning.unwrap(),
                thinking: s.native_thinking.clone(),
            }
            .validate(&row.client)
            .unwrap_or_else(|e| panic!("{} {}: {e}", row.client, s.model_id));
            count += 1;
        }
        assert!(count > 150);
    }
}
