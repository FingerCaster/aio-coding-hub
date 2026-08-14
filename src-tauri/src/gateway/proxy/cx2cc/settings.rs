//! CX2CC bridge runtime settings, read from AppSettings.

use crate::infra::settings;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Cx2ccSettings {
    pub fallback_model_opus: String,
    pub fallback_model_sonnet: String,
    pub fallback_model_haiku: String,
    pub fallback_model_main: String,
    pub model_reasoning_effort: Option<String>,
    pub reasoning_effort_mappings: Vec<settings::Cx2ccReasoningEffortMapping>,
    pub service_tier: Option<String>,
    pub disable_response_storage: bool,
    pub enable_reasoning_to_thinking: bool,
    pub drop_stop_sequences: bool,
    pub clean_schema: bool,
    pub filter_batch_tool: bool,
}

impl Cx2ccSettings {
    pub fn from_app_settings(s: &settings::AppSettings) -> Self {
        Self {
            fallback_model_opus: s.cx2cc_fallback_model_opus.clone(),
            fallback_model_sonnet: s.cx2cc_fallback_model_sonnet.clone(),
            fallback_model_haiku: s.cx2cc_fallback_model_haiku.clone(),
            fallback_model_main: s.cx2cc_fallback_model_main.clone(),
            model_reasoning_effort: non_empty(&s.cx2cc_model_reasoning_effort),
            reasoning_effort_mappings: s.cx2cc_reasoning_effort_mappings.clone(),
            service_tier: non_empty(&s.cx2cc_service_tier),
            disable_response_storage: s.cx2cc_disable_response_storage,
            enable_reasoning_to_thinking: s.cx2cc_enable_reasoning_to_thinking,
            drop_stop_sequences: s.cx2cc_drop_stop_sequences,
            clean_schema: s.cx2cc_clean_schema,
            filter_batch_tool: s.cx2cc_filter_batch_tool,
        }
    }

    pub fn map_reasoning_effort(&self, effort: &str) -> String {
        let source = effort.trim();
        self.reasoning_effort_mappings
            .iter()
            .find(|mapping| mapping.source == source)
            .map(|mapping| mapping.target.clone())
            .unwrap_or_else(|| effort.to_string())
    }
}

impl Default for Cx2ccSettings {
    fn default() -> Self {
        Self {
            fallback_model_opus: settings::DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            fallback_model_sonnet: settings::DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            fallback_model_haiku: settings::DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            fallback_model_main: settings::DEFAULT_CX2CC_FALLBACK_MODEL.to_string(),
            model_reasoning_effort: None,
            reasoning_effort_mappings: settings::default_cx2cc_reasoning_effort_mappings(),
            service_tier: None,
            disable_response_storage: true,
            enable_reasoning_to_thinking: true,
            drop_stop_sequences: true,
            clean_schema: true,
            filter_batch_tool: true,
        }
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Cx2ccSettings;
    use crate::infra::settings::{
        AppSettings, Cx2ccReasoningEffortMapping, DEFAULT_CX2CC_FALLBACK_MODEL,
    };

    #[test]
    fn default_uses_expected_values() {
        let cfg = Cx2ccSettings::default();

        assert_eq!(cfg.fallback_model_opus, DEFAULT_CX2CC_FALLBACK_MODEL);
        assert_eq!(cfg.fallback_model_sonnet, DEFAULT_CX2CC_FALLBACK_MODEL);
        assert_eq!(cfg.fallback_model_haiku, DEFAULT_CX2CC_FALLBACK_MODEL);
        assert_eq!(cfg.fallback_model_main, DEFAULT_CX2CC_FALLBACK_MODEL);
        assert_eq!(cfg.model_reasoning_effort, None);
        assert_eq!(cfg.map_reasoning_effort("max"), "max");
        assert_eq!(cfg.map_reasoning_effort("ultra"), "max");
        assert_eq!(cfg.service_tier, None);
        assert!(cfg.disable_response_storage);
        assert!(cfg.enable_reasoning_to_thinking);
        assert!(cfg.drop_stop_sequences);
        assert!(cfg.clean_schema);
        assert!(cfg.filter_batch_tool);
    }

    #[test]
    fn from_app_settings_trims_optional_strings() {
        let app = AppSettings {
            cx2cc_fallback_model_opus: "o3".to_string(),
            cx2cc_fallback_model_sonnet: "gpt-4.1".to_string(),
            cx2cc_fallback_model_haiku: "gpt-4.1-mini".to_string(),
            cx2cc_fallback_model_main: "gpt-5.4".to_string(),
            cx2cc_model_reasoning_effort: " medium ".to_string(),
            cx2cc_reasoning_effort_mappings: vec![Cx2ccReasoningEffortMapping {
                source: "ultra".to_string(),
                target: "max".to_string(),
            }],
            cx2cc_service_tier: "  flex ".to_string(),
            cx2cc_disable_response_storage: false,
            cx2cc_enable_reasoning_to_thinking: false,
            cx2cc_drop_stop_sequences: false,
            cx2cc_clean_schema: false,
            cx2cc_filter_batch_tool: false,
            ..Default::default()
        };

        let cfg = Cx2ccSettings::from_app_settings(&app);

        assert_eq!(cfg.fallback_model_opus, "o3");
        assert_eq!(cfg.fallback_model_sonnet, "gpt-4.1");
        assert_eq!(cfg.fallback_model_haiku, "gpt-4.1-mini");
        assert_eq!(cfg.fallback_model_main, "gpt-5.4");
        assert_eq!(cfg.model_reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(cfg.map_reasoning_effort(" ultra "), "max");
        assert_eq!(cfg.map_reasoning_effort(" future "), " future ");
        assert_eq!(cfg.service_tier.as_deref(), Some("flex"));
        assert!(!cfg.disable_response_storage);
        assert!(!cfg.enable_reasoning_to_thinking);
        assert!(!cfg.drop_stop_sequences);
        assert!(!cfg.clean_schema);
        assert!(!cfg.filter_batch_tool);
    }

    #[test]
    fn reasoning_effort_mapping_is_exact_and_applied_once() {
        let cfg = Cx2ccSettings {
            reasoning_effort_mappings: vec![
                Cx2ccReasoningEffortMapping {
                    source: "ultra".to_string(),
                    target: "max".to_string(),
                },
                Cx2ccReasoningEffortMapping {
                    source: "max".to_string(),
                    target: "low".to_string(),
                },
            ],
            ..Default::default()
        };

        assert_eq!(cfg.map_reasoning_effort("ultra"), "max");
        assert_eq!(cfg.map_reasoning_effort("Ultra"), "Ultra");

        let empty = Cx2ccSettings {
            reasoning_effort_mappings: Vec::new(),
            ..Default::default()
        };
        assert_eq!(empty.map_reasoning_effort("ultra"), "ultra");
    }
}
