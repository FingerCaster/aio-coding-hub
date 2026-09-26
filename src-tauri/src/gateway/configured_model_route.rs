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
    if requested_model.starts_with("aio/")
        && !crate::gateway::proxy::protocol::is_native_client(cli_key)
    {
        return None;
    }

    let (policy, policy_source) = provider_policy
        .map(|policy| (policy, "provider"))
        .unwrap_or((global_policy, "global"));
    if !policy.enabled {
        return None;
    }

    let (rule, capture) = matching_rule(policy, requested_model)?;
    if rule.target_model.is_none() && rule.reasoning_effort.is_none() {
        return None;
    }

    Some(ConfiguredModelRoute {
        provider_id,
        provider_name: provider_name.to_string(),
        policy_source,
        source_model: requested_model.to_string(),
        target_model: rule.target_model.as_ref().map(|target| {
            if supports_wildcard_expansion(rule) {
                target.replace('*', capture)
            } else {
                target.clone()
            }
        }),
        reasoning_effort: rule.reasoning_effort.clone(),
    })
}

/// A catalog describes the upstream ID. A routed request may use a different
/// model/effort; do not silently attach the original ID's capabilities to it.
pub(crate) fn discovery_requires_model_confirmation(
    model: &str,
    global: &crate::settings::ModelRoutingPolicy,
    provider: Option<&crate::settings::ModelRoutingPolicy>,
) -> bool {
    let Some((rule, capture)) = matching_rule(provider.unwrap_or(global), model) else {
        return false;
    };
    if rule.reasoning_effort.is_some() {
        return true;
    }
    rule.target_model.as_ref().is_some_and(|target| {
        let target = if supports_wildcard_expansion(rule) {
            target.replace('*', capture)
        } else {
            target.clone()
        };
        target != model
    })
}

// Older exact-only settings could contain literal asterisks. Retain those rows
// and their original meaning if they cannot represent a single-wildcard mapping.
fn supports_wildcard_expansion(rule: &crate::settings::ModelRoutingRule) -> bool {
    rule.source_model.matches('*').count() <= 1
        && !rule.target_model.as_ref().is_some_and(|target| {
            target.matches('*').count() > 1
                || (target.contains('*') && !rule.source_model.contains('*'))
        })
}

// Match upstream exact/specific/wildcard order without reordering persisted rules.
fn matching_rule<'a>(
    policy: &'a crate::settings::ModelRoutingPolicy,
    model: &'a str,
) -> Option<(&'a crate::settings::ModelRoutingRule, &'a str)> {
    if !policy.enabled {
        return None;
    }
    policy
        .rules
        .iter()
        .filter_map(|rule| {
            if rule.target_model.is_none() && rule.reasoning_effort.is_none() {
                return None;
            }
            if supports_wildcard_expansion(rule) {
                match_pattern(&rule.source_model, model).map(|capture| (rule, capture))
            } else {
                (rule.source_model == model).then_some((rule, ""))
            }
        })
        .min_by(|(left, _), (right, _)| {
            let left_wildcard =
                supports_wildcard_expansion(left) && left.source_model.contains('*');
            let right_wildcard =
                supports_wildcard_expansion(right) && right.source_model.contains('*');
            left_wildcard
                .cmp(&right_wildcard)
                .then_with(|| compare_patterns(&left.source_model, &right.source_model))
        })
}

fn compare_patterns(left: &str, right: &str) -> std::cmp::Ordering {
    left.contains('*')
        .cmp(&right.contains('*'))
        .then_with(|| {
            right
                .chars()
                .filter(|c| *c != '*')
                .count()
                .cmp(&left.chars().filter(|c| *c != '*').count())
        })
        .then_with(|| left.cmp(right))
}

fn match_pattern<'a>(pattern: &str, model: &'a str) -> Option<&'a str> {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return (pattern == model).then_some("");
    };
    if suffix.contains('*') {
        return None;
    }
    model.strip_prefix(prefix)?.strip_suffix(suffix)
}

/// A matched effective policy declares an explicit candidate. Preserve route order;
/// absent/disabled/unmatched policies remain fallback candidates when none match.
/// Forced providers retain their own policy and bypass sibling preference narrowing.
#[allow(clippy::too_many_arguments)]
pub(in crate::gateway) fn filter_providers(
    providers: &mut Vec<crate::providers::ProviderForGateway>,
    cli_key: &str,
    method: &str,
    path: &str,
    requested_model: Option<&str>,
    bypass: bool,
    global_policy: &crate::settings::ModelRoutingPolicy,
    forced_provider_id: Option<i64>,
) {
    if bypass
        || forced_provider_id.is_some()
        || !is_supported_inference_request(cli_key, method, path)
    {
        return;
    }
    let Some(model) = requested_model.filter(|model| {
        !model.is_empty()
            && (!model.starts_with("aio/")
                || crate::gateway::proxy::protocol::is_native_client(cli_key))
    }) else {
        return;
    };
    let is_explicit = |provider: &crate::providers::ProviderForGateway| {
        !crate::providers::has_bridged_input_semantics(
            provider.source_provider_id,
            provider.bridge_type.as_deref(),
        ) && matching_rule(
            provider
                .model_routing_policy_override
                .as_ref()
                .unwrap_or(global_policy),
            model,
        )
        .is_some()
    };
    if providers.iter().any(is_explicit) {
        providers.retain(is_explicit);
    }
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
        "pi" | "omp" => classify_wire_protocol(&path).is_some(),
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

    #[test]
    fn upstream_pattern_precedence_captures_unicode_without_cascading() {
        let policy = crate::settings::ModelRoutingPolicy {
            enabled: true,
            rules: vec![
                crate::settings::ModelRoutingRule {
                    source_model: "*".into(),
                    target_model: Some("fallback-*".into()),
                    reasoning_effort: None,
                },
                crate::settings::ModelRoutingRule {
                    source_model: "gpt-*".into(),
                    target_model: Some("remote-*".into()),
                    reasoning_effort: Some("high".into()),
                },
                crate::settings::ModelRoutingRule {
                    source_model: "gpt-exact".into(),
                    target_model: Some("exact-target".into()),
                    reasoning_effort: None,
                },
                crate::settings::ModelRoutingRule {
                    source_model: "gpt-*-mini".into(),
                    target_model: Some("small-*".into()),
                    reasoning_effort: None,
                },
            ],
        };
        for (model, target) in [
            ("gpt-exact", "exact-target"),
            ("gpt-多语言-mini", "small-多语言"),
            ("gpt-large", "remote-large"),
            ("other", "fallback-other"),
        ] {
            let route = resolve(
                "codex",
                "POST",
                "/v1/responses",
                Some(model),
                false,
                &policy,
                None,
                1,
                "provider",
            )
            .unwrap();
            assert_eq!(route.target_model.as_deref(), Some(target));
            assert_eq!(route.source_model, model);
        }
        assert!(resolve(
            "codex",
            "POST",
            "/v1/responses",
            Some("aio/profile"),
            false,
            &policy,
            None,
            1,
            "provider"
        )
        .is_none());
        assert!(resolve(
            "codex",
            "POST",
            "/v1/responses",
            Some("gpt-exact"),
            true,
            &policy,
            None,
            1,
            "provider"
        )
        .is_none());
        assert!(match_pattern("a*a", "a").is_none());
    }

    #[test]
    fn legacy_asterisk_rules_remain_literal_when_not_valid_wildcard_mappings() {
        for (source, target) in [("gpt**", "remote**"), ("exact", "remote-*")] {
            let mut policy = crate::settings::ModelRoutingPolicy {
                enabled: true,
                rules: vec![crate::settings::ModelRoutingRule {
                    source_model: source.into(),
                    target_model: Some(target.into()),
                    reasoning_effort: None,
                }],
            };
            let route = resolve(
                "codex",
                "POST",
                "/v1/responses",
                Some(source),
                false,
                &policy,
                None,
                1,
                "provider",
            )
            .unwrap();
            assert_eq!(route.target_model.as_deref(), Some(target));
            assert!(resolve(
                "codex",
                "POST",
                "/v1/responses",
                Some("gpt-large"),
                false,
                &policy,
                None,
                1,
                "provider"
            )
            .is_none());
            policy.rules.insert(
                0,
                crate::settings::ModelRoutingRule {
                    source_model: "*".into(),
                    target_model: Some("fallback-*".into()),
                    reasoning_effort: None,
                },
            );
            let route = resolve(
                "codex",
                "POST",
                "/v1/responses",
                Some(source),
                false,
                &policy,
                None,
                1,
                "provider",
            )
            .unwrap();
            assert_eq!(
                route.target_model.as_deref(),
                Some(target),
                "legacy literal rules retain exact precedence"
            );
        }
    }

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
    fn ordinary_responses_route_rewrites_final_wire_shape() {
        let outcome = apply(
            &route(Some("gpt-5.4"), Some("medium")),
            "/v1/responses",
            None,
            &Bytes::from_static(br#"{"model":"source"}"#),
        )
        .expect("apply ordinary Responses route");

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
