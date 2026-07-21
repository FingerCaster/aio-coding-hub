//! Build compact audit settings for requested-vs-returned model routing.

use serde_json::{json, Value};

const KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS: &[(&str, &str)] = &[
    ("gpt-5.5", "medium"),
    ("gpt-5.5-pro", "high"),
    ("gpt-5.4", "none"),
    ("gpt-5.4-mini", "none"),
    ("gpt-5.4-nano", "none"),
    ("gpt-5.4-pro", "medium"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway) struct ModelRouteSettingInput<'a> {
    pub(in crate::gateway) cli_key: &'a str,
    pub(in crate::gateway) requested_model: Option<&'a str>,
    pub(in crate::gateway) actual_model: Option<&'a str>,
    pub(in crate::gateway) actual_reasoning_effort: Option<&'a str>,
    pub(in crate::gateway) special_settings: &'a [Value],
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: &'a str,
}

pub(in crate::gateway) struct SharedModelRouteSettingInput<'a> {
    pub(in crate::gateway) cli_key: &'a str,
    pub(in crate::gateway) requested_model: Option<&'a str>,
    pub(in crate::gateway) actual_model: Option<&'a str>,
    pub(in crate::gateway) conflicting_actual_model: Option<&'a str>,
    pub(in crate::gateway) actual_reasoning_effort: Option<&'a str>,
    pub(in crate::gateway) special_settings: &'a std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: &'a str,
}

pub(in crate::gateway) struct ModelRouteBytesInput<'a> {
    pub(in crate::gateway) cli_key: &'a str,
    pub(in crate::gateway) requested_model: Option<&'a str>,
    pub(in crate::gateway) response_bytes: &'a [u8],
    pub(in crate::gateway) special_settings: &'a std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: &'a str,
}

pub(in crate::gateway) fn observe_model_route_from_bytes(
    input: ModelRouteBytesInput<'_>,
) -> crate::usage::ModelRouteEvidence {
    let evidence = crate::usage::parse_model_route_evidence_from_json_or_sse_bytes(
        input.cli_key,
        input.response_bytes,
    );
    if let Some(setting) =
        build_model_route_mapping_setting_from_shared(SharedModelRouteSettingInput {
            cli_key: input.cli_key,
            requested_model: input.requested_model,
            actual_model: evidence.first_model.as_deref(),
            conflicting_actual_model: evidence.first_conflicting_model.as_deref(),
            actual_reasoning_effort: evidence.reasoning_effort.as_deref(),
            special_settings: input.special_settings,
            provider_id: input.provider_id,
            provider_name: input.provider_name,
        })
    {
        crate::gateway::response_fixer::push_model_route_mapping_special_setting(
            input.special_settings,
            setting,
        );
    }
    evidence
}

pub(in crate::gateway) fn build_model_route_mapping_setting(
    input: ModelRouteSettingInput<'_>,
) -> Option<Value> {
    if input.cli_key != "codex" {
        return None;
    }

    let requested_model = normalize_text(input.requested_model)?;
    let actual_model = normalize_text(input.actual_model)?;
    let requested_effort =
        resolve_requested_effort(input.cli_key, &requested_model, input.special_settings);
    let actual_effort = resolve_actual_effort(
        &actual_model,
        input.actual_reasoning_effort,
        requested_effort.source == "model_default",
    );
    let model_mismatch = !same_route_part(&requested_model, &actual_model);
    let effort_mismatch = match (&requested_effort.effort, &actual_effort.effort) {
        (Some(requested), Some(actual)) => !same_route_part(requested, actual),
        _ => false,
    };

    if !model_mismatch && !effort_mismatch {
        return None;
    }

    Some(json!({
        "type": "model_route_mapping",
        "cliKey": input.cli_key,
        "requestedModel": truncate_display_text(&requested_model),
        "requestedReasoningEffort": requested_effort.effort,
        "requestedReasoningEffortSource": requested_effort.source,
        "actualModel": truncate_display_text(&actual_model),
        "actualReasoningEffort": actual_effort.effort,
        "actualReasoningEffortSource": actual_effort.source,
        "modelMismatch": model_mismatch,
        "effortMismatch": effort_mismatch,
        "mismatch": true,
        "providerId": input.provider_id,
        "providerName": normalize_text(Some(input.provider_name))
            .map(|name| truncate_display_text(&name)),
    }))
}

pub(in crate::gateway) fn build_model_route_mapping_setting_from_shared(
    input: SharedModelRouteSettingInput<'_>,
) -> Option<Value> {
    let observation = classify_model_observation(
        input.requested_model,
        input.actual_model,
        input.conflicting_actual_model,
    );
    crate::gateway::managed_model_route::update_observation(
        input.special_settings,
        input.provider_id,
        observation.kind,
    );
    let settings = input.special_settings.lock().ok()?.clone();
    let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
        cli_key: input.cli_key,
        requested_model: input.requested_model,
        actual_model: observation.actual_model.as_deref(),
        actual_reasoning_effort: input.actual_reasoning_effort,
        special_settings: settings.as_slice(),
        provider_id: input.provider_id,
        provider_name: input.provider_name,
    });

    // A request can observe more than one upstream attempt. Once a later
    // attempt is matched or unobserved, an earlier attempt's mismatch must
    // not survive into the terminal log and create a false warning.
    if setting.is_none() && input.cli_key == "codex" {
        crate::gateway::response_fixer::clear_model_route_mapping_special_setting(
            input.special_settings,
        );
    }

    setting
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelObservation {
    kind: &'static str,
    actual_model: Option<String>,
}

fn classify_model_observation(
    requested_model: Option<&str>,
    actual_model: Option<&str>,
    conflicting_actual_model: Option<&str>,
) -> ModelObservation {
    let Some(actual_model) = normalize_text(actual_model) else {
        return ModelObservation {
            kind: "unobserved",
            actual_model: None,
        };
    };
    let conflicting_actual_model = normalize_text(conflicting_actual_model)
        .filter(|conflict| !same_route_part(&actual_model, conflict));
    let requested_model = normalize_text(requested_model);

    if let Some(conflict) = conflicting_actual_model {
        let actual_for_comparison = requested_model
            .as_deref()
            .filter(|requested| !same_route_part(requested, &actual_model))
            .map(|_| actual_model)
            .unwrap_or(conflict);
        return ModelObservation {
            kind: "conflict",
            actual_model: Some(actual_for_comparison),
        };
    }

    let kind = match requested_model {
        Some(requested) if same_route_part(&requested, &actual_model) => "matched",
        Some(_) => "mismatch",
        None => "unobserved",
    };
    ModelObservation {
        kind,
        actual_model: Some(actual_model),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffortResolution {
    effort: Option<String>,
    source: &'static str,
}

fn resolve_requested_effort(
    cli_key: &str,
    requested_model: &str,
    special_settings: &[Value],
) -> EffortResolution {
    if cli_key != "codex" {
        return EffortResolution {
            effort: None,
            source: "unknown",
        };
    }

    let explicit = special_settings.iter().rev().find(|setting| {
        setting.get("type").and_then(Value::as_str) == Some("codex_reasoning_effort")
    });
    if let Some(setting) = explicit {
        return EffortResolution {
            effort: setting
                .get("effort")
                .and_then(Value::as_str)
                .and_then(normalize_effort)
                .or_else(|| {
                    setting
                        .get("rawEffort")
                        .and_then(Value::as_str)
                        .and_then(normalize_effort)
                }),
            source: "request",
        };
    }

    EffortResolution {
        effort: default_codex_reasoning_effort(requested_model).map(str::to_string),
        source: "model_default",
    }
}

fn resolve_actual_effort(
    actual_model: &str,
    explicit_effort: Option<&str>,
    infer_model_default: bool,
) -> EffortResolution {
    if let Some(explicit_effort) = explicit_effort {
        return EffortResolution {
            effort: normalize_effort(explicit_effort),
            source: "response",
        };
    }

    if infer_model_default {
        return EffortResolution {
            effort: default_codex_reasoning_effort(actual_model).map(str::to_string),
            source: "model_default",
        };
    }

    EffortResolution {
        effort: None,
        source: "unknown",
    }
}

fn default_codex_reasoning_effort(model: &str) -> Option<&'static str> {
    let normalized = model.trim().to_ascii_lowercase();
    KNOWN_CODEX_MODEL_DEFAULT_REASONING_EFFORTS
        .iter()
        .find_map(|(known_model, effort)| (*known_model == normalized).then_some(*effort))
}

fn normalize_effort(value: &str) -> Option<String> {
    let effort = value.trim().to_ascii_lowercase();
    matches!(
        effort.as_str(),
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
    )
    .then_some(effort)
}

fn normalize_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

fn truncate_display_text(value: &str) -> String {
    if value.chars().count() > 200 {
        value.chars().take(200).collect()
    } else {
        value.to_string()
    }
}

fn same_route_part(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_codex_model_mismatch_with_effort_sources() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some(" gpt-5.5 "),
            actual_model: Some("gpt-5.4-mini"),
            actual_reasoning_effort: Some("high"),
            special_settings: &[json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            })],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("setting");

        assert_eq!(
            setting.get("type").and_then(Value::as_str),
            Some("model_route_mapping")
        );
        assert_eq!(
            setting.get("requestedModel").and_then(Value::as_str),
            Some("gpt-5.5")
        );
        assert_eq!(
            setting
                .get("requestedReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            setting.get("actualModel").and_then(Value::as_str),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            setting.get("actualReasoningEffort").and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            setting
                .get("actualReasoningEffortSource")
                .and_then(Value::as_str),
            Some("response")
        );
        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn skips_identity_route_even_when_case_differs() {
        assert!(build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("GPT-5.5"),
            actual_model: Some("gpt-5.5"),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .is_none());
    }

    #[test]
    fn compares_model_defaults_when_neither_side_has_explicit_effort() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some("gpt-5.4-mini"),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("model and default effort mismatch");

        assert_eq!(
            setting.get("actualReasoningEffort").and_then(Value::as_str),
            Some("none")
        );
        assert_eq!(
            setting
                .get("actualReasoningEffortSource")
                .and_then(Value::as_str),
            Some("model_default")
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn builds_same_model_effort_mismatch_from_explicit_response() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some("gpt-5.5"),
            actual_reasoning_effort: Some("medium"),
            special_settings: &[json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            })],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("effort mismatch");

        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting
                .get("requestedReasoningEffort")
                .and_then(Value::as_str),
            Some("high")
        );
        assert_eq!(
            setting.get("actualReasoningEffort").and_then(Value::as_str),
            Some("medium")
        );
        assert_eq!(
            setting
                .get("actualReasoningEffortSource")
                .and_then(Value::as_str),
            Some("response")
        );
    }

    #[test]
    fn skips_same_model_when_explicit_request_effort_has_no_response_effort() {
        assert!(build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some("gpt-5.5"),
            actual_reasoning_effort: None,
            special_settings: &[json!({
                "type": "codex_reasoning_effort",
                "effort": "high"
            })],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .is_none());
    }

    #[test]
    fn skips_non_codex_routes() {
        assert!(build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "claude",
            requested_model: Some("claude-sonnet"),
            actual_model: Some("claude-sonnet-4-20250514"),
            actual_reasoning_effort: Some("high"),
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .is_none());
    }

    #[test]
    fn does_not_treat_unknown_effort_as_effort_mismatch() {
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-future"),
            actual_model: Some("gpt-other"),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("model mismatch");

        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting.get("effortMismatch").and_then(Value::as_bool),
            Some(false)
        );
        assert!(setting
            .get("actualReasoningEffort")
            .is_some_and(Value::is_null));
    }

    #[test]
    fn accepts_main_extended_and_raw_requested_efforts() {
        for setting in [
            json!({
                "type": "codex_reasoning_effort",
                "effort": "max"
            }),
            json!({
                "type": "codex_reasoning_effort",
                "effort": null,
                "rawEffort": "Ultra"
            }),
        ] {
            let mapping = build_model_route_mapping_setting(ModelRouteSettingInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                actual_model: Some("gpt-5.5"),
                actual_reasoning_effort: Some("medium"),
                special_settings: &[setting],
                provider_id: 7,
                provider_name: "Provider A",
            })
            .expect("extended effort mismatch");

            assert!(mapping
                .get("requestedReasoningEffort")
                .and_then(Value::as_str)
                .is_some_and(|effort| effort == "max" || effort == "ultra"));
        }
    }

    #[test]
    fn truncates_route_text_on_utf8_char_boundaries() {
        let long_unicode_model = "模型".repeat(201);
        let long_provider_name = "供应商".repeat(201);
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("gpt-5.5"),
            actual_model: Some(&long_unicode_model),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: &long_provider_name,
        })
        .expect("model mismatch");

        let actual = setting
            .get("actualModel")
            .and_then(Value::as_str)
            .expect("actual model");
        assert_eq!(actual.chars().count(), 200);
        assert!(actual.is_char_boundary(actual.len()));

        let provider_name = setting
            .get("providerName")
            .and_then(Value::as_str)
            .expect("provider name");
        assert_eq!(provider_name.chars().count(), 200);
        assert!(provider_name.is_char_boundary(provider_name.len()));
    }

    #[test]
    fn compares_model_suffixes_beyond_the_display_limit() {
        let shared_prefix = "x".repeat(200);
        let requested = format!("{shared_prefix}a");
        let actual = format!("{shared_prefix}b");
        let setting = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some(&requested),
            actual_model: Some(&actual),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("suffix mismatch must not be hidden by display truncation");

        assert_eq!(
            setting.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            setting
                .get("requestedModel")
                .and_then(Value::as_str)
                .map(str::len),
            Some(200)
        );
        assert_eq!(
            classify_model_observation(Some(&requested), Some(&requested), Some(&actual)),
            ModelObservation {
                kind: "conflict",
                actual_model: Some(actual),
            }
        );
    }

    #[test]
    fn response_evidence_detects_suffix_mismatch_beyond_200_bytes() {
        let shared_prefix = "x".repeat(200);
        let requested = format!("{shared_prefix}{}", "a".repeat(56));
        let actual = format!("{shared_prefix}{}", "b".repeat(56));
        assert_eq!(requested.len(), 256);
        assert_eq!(actual.len(), 256);
        let response = serde_json::json!({ "model": actual }).to_string();
        let special_settings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let evidence = observe_model_route_from_bytes(ModelRouteBytesInput {
            cli_key: "codex",
            requested_model: Some(&requested),
            response_bytes: response.as_bytes(),
            special_settings: &special_settings,
            provider_id: 7,
            provider_name: "Provider A",
        });

        assert_eq!(evidence.first_model.as_deref(), Some(actual.as_str()));
        let settings = special_settings.lock().expect("route settings");
        let mapping = settings
            .iter()
            .find(|setting| {
                setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
            })
            .expect("suffix mismatch must produce a route warning");
        assert_eq!(
            mapping.get("modelMismatch").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn conflict_keeps_the_first_model_that_differs_from_wire() {
        assert_eq!(
            classify_model_observation(Some("wire"), Some("wire"), Some("other")),
            ModelObservation {
                kind: "conflict",
                actual_model: Some("other".to_string()),
            }
        );
        assert_eq!(
            classify_model_observation(Some("wire"), Some("other"), Some("wire")),
            ModelObservation {
                kind: "conflict",
                actual_model: Some("other".to_string()),
            }
        );
    }

    #[test]
    fn missing_model_is_unobserved() {
        assert_eq!(
            classify_model_observation(Some("wire"), None, None),
            ModelObservation {
                kind: "unobserved",
                actual_model: None,
            }
        );
    }

    #[test]
    fn later_matched_or_unobserved_observation_clears_stale_mismatch() {
        let special_settings = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mismatch = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("wire"),
            actual_model: Some("other"),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("mismatch setting");
        crate::gateway::response_fixer::push_model_route_mapping_special_setting(
            &special_settings,
            mismatch,
        );

        let matched = build_model_route_mapping_setting_from_shared(SharedModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("wire"),
            actual_model: Some("wire"),
            conflicting_actual_model: None,
            actual_reasoning_effort: None,
            special_settings: &special_settings,
            provider_id: 7,
            provider_name: "Provider A",
        });
        assert!(matched.is_none());
        assert!(!special_settings.lock().unwrap().iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));

        let mismatch = build_model_route_mapping_setting(ModelRouteSettingInput {
            cli_key: "codex",
            requested_model: Some("wire"),
            actual_model: Some("other"),
            actual_reasoning_effort: None,
            special_settings: &[],
            provider_id: 7,
            provider_name: "Provider A",
        })
        .expect("second mismatch setting");
        crate::gateway::response_fixer::push_model_route_mapping_special_setting(
            &special_settings,
            mismatch,
        );
        let unobserved =
            build_model_route_mapping_setting_from_shared(SharedModelRouteSettingInput {
                cli_key: "codex",
                requested_model: Some("wire"),
                actual_model: None,
                conflicting_actual_model: None,
                actual_reasoning_effort: None,
                special_settings: &special_settings,
                provider_id: 7,
                provider_name: "Provider A",
            });
        assert!(unobserved.is_none());
        assert!(!special_settings.lock().unwrap().iter().any(|setting| {
            setting.get("type").and_then(Value::as_str) == Some("model_route_mapping")
        }));
    }
}
