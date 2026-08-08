//! Provider-aware model and reasoning-effort rewrites for final wire requests.

use axum::body::Bytes;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway) struct ConfiguredModelRoute {
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: String,
    pub(in crate::gateway) policy_source: &'static str,
    pub(in crate::gateway) source_model: String,
    pub(in crate::gateway) target_model: Option<String>,
    pub(in crate::gateway) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway) struct ConfiguredModelRouteOutcome {
    pub(in crate::gateway) path: String,
    pub(in crate::gateway) query: Option<String>,
    pub(in crate::gateway) body: Bytes,
    pub(in crate::gateway) effective_model: Option<String>,
    pub(in crate::gateway) model_applied: bool,
    pub(in crate::gateway) reasoning_effort_applied: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gateway) struct ConfiguredModelRouteApplyError {
    pub(in crate::gateway) reason_code: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WireProtocol {
    ClaudeMessages,
    Responses,
    ChatCompletions,
    GeminiGenerateContent,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::gateway) fn resolve(
    cli_key: &str,
    method: &str,
    path: &str,
    requested_model: Option<&str>,
    managed_model_route: bool,
    global_policy: &crate::settings::ModelRoutingPolicy,
    provider_policy: Option<&crate::settings::ModelRoutingPolicy>,
    provider_id: i64,
    provider_name: &str,
) -> Option<ConfiguredModelRoute> {
    if managed_model_route || !is_supported_inference_request(cli_key, method, path) {
        return None;
    }

    let requested_model = requested_model.filter(|value| !value.is_empty())?;
    if requested_model.starts_with("aio/") {
        return None;
    }

    let (policy, policy_source) = provider_policy
        .map(|policy| (policy, "provider"))
        .unwrap_or((global_policy, "global"));
    if !policy.enabled {
        return None;
    }

    let rule = policy
        .rules
        .iter()
        .find(|rule| rule.source_model == requested_model)?;
    if rule.target_model.is_none() && rule.reasoning_effort.is_none() {
        return None;
    }

    Some(ConfiguredModelRoute {
        provider_id,
        provider_name: provider_name.to_string(),
        policy_source,
        source_model: requested_model.to_string(),
        target_model: rule.target_model.clone(),
        reasoning_effort: rule.reasoning_effort.clone(),
    })
}

fn is_supported_inference_request(cli_key: &str, method: &str, path: &str) -> bool {
    if !method.eq_ignore_ascii_case("POST") {
        return false;
    }
    let path = normalized_path(path);
    match cli_key {
        "claude" => path.ends_with("/messages"),
        "codex" => is_responses_path(&path),
        "grok" => is_responses_path(&path) || path.ends_with("/chat/completions"),
        "gemini" => is_gemini_generate_path(&path),
        _ => false,
    }
}

fn normalized_path(path: &str) -> String {
    path.split('?')
        .next()
        .unwrap_or(path)
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn is_responses_path(path: &str) -> bool {
    path.ends_with("/responses") || path.ends_with("/responses/compact")
}

fn is_gemini_generate_path(path: &str) -> bool {
    path.ends_with(":generatecontent") || path.ends_with(":streamgeneratecontent")
}

fn classify_wire_protocol(path: &str) -> Option<WireProtocol> {
    let path = normalized_path(path);
    if path.ends_with("/messages") {
        Some(WireProtocol::ClaudeMessages)
    } else if is_responses_path(&path) {
        Some(WireProtocol::Responses)
    } else if path.ends_with("/chat/completions") {
        Some(WireProtocol::ChatCompletions)
    } else if is_gemini_generate_path(&path) {
        Some(WireProtocol::GeminiGenerateContent)
    } else {
        None
    }
}

pub(in crate::gateway) fn apply(
    route: &ConfiguredModelRoute,
    path: &str,
    query: Option<&str>,
    body: &Bytes,
) -> Result<ConfiguredModelRouteOutcome, ConfiguredModelRouteApplyError> {
    let protocol = classify_wire_protocol(path).ok_or(ConfiguredModelRouteApplyError {
        reason_code: "unsupported_final_protocol",
    })?;
    let mut next_path = path.to_string();
    let next_query = query.map(str::to_string);
    let needs_body = route.reasoning_effort.is_some()
        || (route.target_model.is_some() && protocol != WireProtocol::GeminiGenerateContent);
    let mut body_json = if needs_body {
        Some(serde_json::from_slice::<Value>(body.as_ref()).map_err(|_| {
            ConfiguredModelRouteApplyError {
                reason_code: "invalid_json_body",
            }
        })?)
    } else {
        None
    };
    if body_json.as_ref().is_some_and(|root| !root.is_object()) {
        return Err(ConfiguredModelRouteApplyError {
            reason_code: "invalid_json_object",
        });
    }

    if let Some(target_model) = route.target_model.as_deref() {
        match protocol {
            WireProtocol::GeminiGenerateContent => {
                next_path = crate::gateway::proxy::replace_model_in_path(&next_path, target_model)
                    .ok_or(ConfiguredModelRouteApplyError {
                        reason_code: "model_write_failed",
                    })?;
            }
            WireProtocol::ClaudeMessages
            | WireProtocol::Responses
            | WireProtocol::ChatCompletions => {
                let root = body_json.as_mut().ok_or(ConfiguredModelRouteApplyError {
                    reason_code: "invalid_json_body",
                })?;
                if !crate::gateway::proxy::replace_model_in_body_json(root, target_model) {
                    return Err(ConfiguredModelRouteApplyError {
                        reason_code: "model_write_failed",
                    });
                }
            }
        }
    }

    if let Some(effort) = route.reasoning_effort.as_deref() {
        let root = body_json.as_mut().ok_or(ConfiguredModelRouteApplyError {
            reason_code: "invalid_json_body",
        })?;
        write_reasoning_effort(protocol, root, effort)?;
    }

    verify_requested_outputs(route, protocol, &next_path, body_json.as_ref())?;

    let next_body =
        match body_json.as_ref() {
            Some(root) => Bytes::from(serde_json::to_vec(root).map_err(|_| {
                ConfiguredModelRouteApplyError {
                    reason_code: "body_serialize_failed",
                }
            })?),
            None => body.clone(),
        };
    let effective_model = effective_model(protocol, &next_path, body_json.as_ref());

    Ok(ConfiguredModelRouteOutcome {
        path: next_path,
        query: next_query,
        body: next_body,
        effective_model,
        model_applied: route.target_model.is_some(),
        reasoning_effort_applied: route.reasoning_effort.is_some(),
    })
}

fn write_reasoning_effort(
    protocol: WireProtocol,
    root: &mut Value,
    effort: &str,
) -> Result<(), ConfiguredModelRouteApplyError> {
    let object = root.as_object_mut().ok_or(ConfiguredModelRouteApplyError {
        reason_code: "invalid_json_object",
    })?;
    match protocol {
        WireProtocol::ClaudeMessages => {
            object_slot(object, "output_config").insert("effort".to_string(), json!(effort));
        }
        WireProtocol::Responses => {
            object_slot(object, "reasoning").insert("effort".to_string(), json!(effort));
        }
        WireProtocol::ChatCompletions => {
            object.insert("reasoning_effort".to_string(), json!(effort));
        }
        WireProtocol::GeminiGenerateContent => {
            let generation_config = object_slot(object, "generationConfig");
            let thinking_config = object_slot(generation_config, "thinkingConfig");
            if let Ok(budget) = effort.parse::<i64>() {
                thinking_config.insert("thinkingBudget".to_string(), json!(budget));
                thinking_config.remove("thinkingLevel");
            } else {
                thinking_config.insert("thinkingLevel".to_string(), json!(effort));
                thinking_config.remove("thinkingBudget");
            }
        }
    }
    Ok(())
}

fn object_slot<'a>(
    object: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("object slot")
}

fn verify_requested_outputs(
    route: &ConfiguredModelRoute,
    protocol: WireProtocol,
    path: &str,
    body_json: Option<&Value>,
) -> Result<(), ConfiguredModelRouteApplyError> {
    if let Some(target_model) = route.target_model.as_deref() {
        if effective_model(protocol, path, body_json).as_deref() != Some(target_model) {
            return Err(ConfiguredModelRouteApplyError {
                reason_code: "model_verification_failed",
            });
        }
    }
    if let Some(effort) = route.reasoning_effort.as_deref() {
        if !reasoning_effort_matches(protocol, body_json, effort) {
            return Err(ConfiguredModelRouteApplyError {
                reason_code: "effort_verification_failed",
            });
        }
    }
    Ok(())
}

fn effective_model(
    protocol: WireProtocol,
    path: &str,
    body_json: Option<&Value>,
) -> Option<String> {
    if protocol == WireProtocol::GeminiGenerateContent {
        let needle = "/models/";
        let start = path.find(needle)? + needle.len();
        let rest = &path[start..];
        let end = rest.find(['/', ':', '?']).unwrap_or(rest.len());
        return Some(crate::gateway::util::url_decode_component(&rest[..end]));
    }
    crate::gateway::util::infer_requested_model_info(path, None, body_json).model
}

fn reasoning_effort_matches(
    protocol: WireProtocol,
    body_json: Option<&Value>,
    effort: &str,
) -> bool {
    let Some(root) = body_json else {
        return false;
    };
    match protocol {
        WireProtocol::ClaudeMessages => {
            root.pointer("/output_config/effort")
                .and_then(Value::as_str)
                == Some(effort)
        }
        WireProtocol::Responses => {
            root.pointer("/reasoning/effort").and_then(Value::as_str) == Some(effort)
        }
        WireProtocol::ChatCompletions => {
            root.get("reasoning_effort").and_then(Value::as_str) == Some(effort)
        }
        WireProtocol::GeminiGenerateContent => {
            let thinking = root.pointer("/generationConfig/thinkingConfig");
            match effort.parse::<i64>() {
                Ok(budget) => {
                    thinking
                        .and_then(|value| value.get("thinkingBudget"))
                        .and_then(Value::as_i64)
                        == Some(budget)
                        && thinking
                            .and_then(|value| value.get("thinkingLevel"))
                            .is_none()
                }
                Err(_) => {
                    thinking
                        .and_then(|value| value.get("thinkingLevel"))
                        .and_then(Value::as_str)
                        == Some(effort)
                        && thinking
                            .and_then(|value| value.get("thinkingBudget"))
                            .is_none()
                }
            }
        }
    }
}

pub(in crate::gateway) fn mark_applied(
    special_settings: &Arc<Mutex<Vec<Value>>>,
    route: &ConfiguredModelRoute,
    priced_cli_key: &str,
    outcome: &ConfiguredModelRouteOutcome,
) {
    crate::gateway::response_fixer::upsert_configured_model_route(
        special_settings,
        json!({
            "type": "configured_model_route",
            "scope": "request",
            "providerId": route.provider_id,
            "providerName": route.provider_name,
            "policySource": route.policy_source,
            "sourceModel": route.source_model,
            "targetModel": route.target_model,
            "reasoningEffort": route.reasoning_effort,
            "effectiveModel": outcome.effective_model,
            "pricedCliKey": priced_cli_key,
            "pricedModel": outcome.effective_model,
            "applied": true,
            "modelApplied": outcome.model_applied,
            "reasoningEffortApplied": outcome.reasoning_effort_applied,
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(target_model: Option<&str>, effort: Option<&str>) -> ConfiguredModelRoute {
        ConfiguredModelRoute {
            provider_id: 7,
            provider_name: "backup".to_string(),
            policy_source: "provider",
            source_model: "source".to_string(),
            target_model: target_model.map(str::to_string),
            reasoning_effort: effort.map(str::to_string),
        }
    }

    #[test]
    fn provider_policy_replaces_global_and_matching_is_exact() {
        let global = crate::settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![crate::settings::ModelRoutingRule {
                source_model: "source".to_string(),
                target_model: Some("global-target".to_string()),
                reasoning_effort: None,
            }],
        };
        let disabled = crate::settings::ModelRoutingPolicy::default();
        assert!(resolve(
            "claude",
            "POST",
            "/v1/messages",
            Some("source"),
            false,
            &global,
            Some(&disabled),
            7,
            "backup"
        )
        .is_none());
        assert!(resolve(
            "claude",
            "POST",
            "/v1/messages",
            Some("Source"),
            false,
            &global,
            None,
            7,
            "backup"
        )
        .is_none());
    }

    #[test]
    fn routing_is_single_pass_against_the_original_model() {
        let policy = crate::settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![
                crate::settings::ModelRoutingRule {
                    source_model: "source".to_string(),
                    target_model: Some("intermediate".to_string()),
                    reasoning_effort: None,
                },
                crate::settings::ModelRoutingRule {
                    source_model: "intermediate".to_string(),
                    target_model: Some("final".to_string()),
                    reasoning_effort: None,
                },
            ],
        };

        let resolved = resolve(
            "codex",
            "POST",
            "/v1/responses",
            Some("source"),
            false,
            &policy,
            None,
            7,
            "backup",
        )
        .expect("first exact rule should match");

        assert_eq!(resolved.source_model, "source");
        assert_eq!(resolved.target_model.as_deref(), Some("intermediate"));
    }

    #[test]
    fn excludes_managed_aliases_and_auxiliary_requests() {
        let policy = crate::settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![crate::settings::ModelRoutingRule {
                source_model: "aio/model".to_string(),
                target_model: Some("target".to_string()),
                reasoning_effort: None,
            }],
        };
        for (method, path) in [
            ("GET", "/v1/messages"),
            ("POST", "/v1/messages/count_tokens"),
            ("POST", "/v1/models"),
        ] {
            assert!(resolve(
                "claude",
                method,
                path,
                Some("aio/model"),
                false,
                &policy,
                None,
                7,
                "backup"
            )
            .is_none());
        }
    }

    #[test]
    fn rewrites_all_supported_protocol_shapes() {
        let cases = [
            (
                "/v1/messages",
                br#"{"model":"source"}"#.as_slice(),
                "/output_config/effort",
            ),
            (
                "/v1/responses/compact",
                br#"{"model":"source"}"#.as_slice(),
                "/reasoning/effort",
            ),
            (
                "/v1/chat/completions",
                br#"{"model":"source"}"#.as_slice(),
                "/reasoning_effort",
            ),
        ];
        for (path, body, effort_pointer) in cases {
            let outcome = apply(
                &route(Some("target"), Some("high")),
                path,
                None,
                &Bytes::copy_from_slice(body),
            )
            .expect("apply route");
            let root: Value = serde_json::from_slice(&outcome.body).expect("json body");
            assert_eq!(root["model"], "target");
            assert_eq!(
                root.pointer(effort_pointer).and_then(Value::as_str),
                Some("high")
            );
        }
    }

    #[test]
    fn effort_only_route_preserves_an_earlier_model_rewrite() {
        let outcome = apply(
            &route(None, Some("low")),
            "/v1/responses",
            None,
            &Bytes::from_static(br#"{"model":"bridge-target"}"#),
        )
        .expect("apply effort-only route");

        assert_eq!(outcome.effective_model.as_deref(), Some("bridge-target"));
        assert!(!outcome.model_applied);
        assert!(outcome.reasoning_effort_applied);
        let root: Value = serde_json::from_slice(&outcome.body).expect("json body");
        assert_eq!(root["model"], "bridge-target");
        assert_eq!(
            root.pointer("/reasoning/effort").and_then(Value::as_str),
            Some("low")
        );
    }

    #[test]
    fn cx2cc_final_responses_shape_is_rewritten() {
        let outcome = apply(
            &route(Some("gpt-5.4"), Some("medium")),
            "/v1/responses",
            None,
            &Bytes::from_static(br#"{"model":"claude-sonnet"}"#),
        )
        .expect("apply route after CX2CC conversion");

        let root: Value = serde_json::from_slice(&outcome.body).expect("json body");
        assert_eq!(root["model"], "gpt-5.4");
        assert_eq!(
            root.pointer("/reasoning/effort").and_then(Value::as_str),
            Some("medium")
        );
    }

    #[test]
    fn model_route_succeeds_when_target_already_matches_final_wire_value() {
        let outcome = apply(
            &route(Some("target"), None),
            "/v1/responses",
            None,
            &Bytes::from_static(br#"{"model":"target"}"#),
        )
        .expect("same-value route should still verify");

        assert_eq!(outcome.effective_model.as_deref(), Some("target"));
        assert!(outcome.model_applied);
    }

    #[test]
    fn gemini_rewrites_path_and_keeps_effort_siblings_exclusive() {
        let outcome = apply(
            &route(Some("publisher/gemini-flash"), Some("1024")),
            "/v1beta/models/source:streamGenerateContent",
            None,
            &Bytes::from_static(
                br#"{"generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH"}}}"#,
            ),
        )
        .expect("apply Gemini route");
        let root: Value = serde_json::from_slice(&outcome.body).expect("json body");
        assert!(outcome
            .path
            .contains("/models/publisher%2Fgemini-flash:streamGenerateContent"));
        assert_eq!(
            root.pointer("/generationConfig/thinkingConfig/thinkingBudget")
                .and_then(Value::as_i64),
            Some(1024)
        );
        assert!(root
            .pointer("/generationConfig/thinkingConfig/thinkingLevel")
            .is_none());
    }

    #[test]
    fn failed_multi_field_route_keeps_input_state_unmodified() {
        let path = "/v1/responses";
        let body = Bytes::from_static(b"not-json");
        assert_eq!(
            apply(&route(Some("target"), Some("high")), path, None, &body)
                .expect_err("invalid JSON must fail")
                .reason_code,
            "invalid_json_body"
        );
        assert_eq!(path, "/v1/responses");
        assert_eq!(body, Bytes::from_static(b"not-json"));
    }
}
