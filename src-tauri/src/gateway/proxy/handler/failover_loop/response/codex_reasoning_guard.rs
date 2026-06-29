//! Usage: Codex degraded-reasoning detection helpers.

use crate::gateway::events::{decision_chain as dc, FailoverAttempt};
use crate::gateway::proxy::ErrorCategory;
use crate::gateway::response_fixer;
use crate::settings::{CodexReasoningGuardCompareMode, CodexReasoningGuardModelRule};
use axum::http::StatusCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) const CODEX_REASONING_GUARD_ERROR_CODE: &str = "GW_CODEX_REASONING_GUARD";
pub(super) const CODEX_REASONING_GUARD_REASON_CODE: &str = "codex_reasoning_guard";
const CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT: &str = "global_default";
const CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE: &str = "model_rule";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardMatch {
    pub(super) reasoning_tokens: i64,
    pub(super) pointer: &'static str,
    pub(super) compare_mode: CodexReasoningGuardCompareMode,
    pub(super) matched_rule_value: i64,
    pub(super) requested_model: Option<String>,
    pub(super) rule_source: &'static str,
    pub(super) rule_model: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedCodexReasoningGuardRule<'a> {
    compare_mode: CodexReasoningGuardCompareMode,
    configured_values: &'a [i64],
    rule_source: &'static str,
    rule_model: Option<&'a str>,
}

const REASONING_POINTERS: &[&str] = &[
    "/usage/output_tokens_details/reasoning_tokens",
    "/usage/completion_tokens_details/reasoning_tokens",
    "/response/usage/output_tokens_details/reasoning_tokens",
    "/response/usage/completion_tokens_details/reasoning_tokens",
];

pub(super) fn detect_from_json(
    cli_key: &str,
    requested_model: Option<&str>,
    value: &serde_json::Value,
    fallback_compare_mode: CodexReasoningGuardCompareMode,
    fallback_values: &[i64],
    model_rules: &[CodexReasoningGuardModelRule],
) -> Option<CodexReasoningGuardMatch> {
    if cli_key != "codex" {
        return None;
    }
    let resolved_rule = resolve_guard_rule(
        requested_model,
        fallback_compare_mode,
        fallback_values,
        model_rules,
    )?;

    for pointer in REASONING_POINTERS {
        let Some(raw) = value.pointer(pointer) else {
            continue;
        };
        let reasoning_tokens = match raw {
            serde_json::Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|v| i64::try_from(v).ok())),
            _ => None,
        };
        let Some(reasoning_tokens) = reasoning_tokens else {
            continue;
        };
        if let Some(matched_rule_value) = find_matched_rule_value(
            resolved_rule.compare_mode,
            reasoning_tokens,
            resolved_rule.configured_values,
        ) {
            return Some(CodexReasoningGuardMatch {
                reasoning_tokens,
                pointer,
                compare_mode: resolved_rule.compare_mode,
                matched_rule_value,
                requested_model: requested_model
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(ToOwned::to_owned),
                rule_source: resolved_rule.rule_source,
                rule_model: resolved_rule.rule_model.map(ToOwned::to_owned),
            });
        }
    }

    None
}

fn resolve_guard_rule<'a>(
    requested_model: Option<&str>,
    fallback_compare_mode: CodexReasoningGuardCompareMode,
    fallback_values: &'a [i64],
    model_rules: &'a [CodexReasoningGuardModelRule],
) -> Option<ResolvedCodexReasoningGuardRule<'a>> {
    let requested_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(requested_model) = requested_model {
        if let Some(rule) = model_rules
            .iter()
            .find(|rule| rule.requested_model == requested_model)
        {
            if !rule.reasoning_equals.is_empty() {
                return Some(ResolvedCodexReasoningGuardRule {
                    compare_mode: rule.compare_mode,
                    configured_values: &rule.reasoning_equals,
                    rule_source: CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE,
                    rule_model: Some(rule.requested_model.as_str()),
                });
            }
        }
    }
    if fallback_values.is_empty() {
        return None;
    }
    Some(ResolvedCodexReasoningGuardRule {
        compare_mode: fallback_compare_mode,
        configured_values: fallback_values,
        rule_source: CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT,
        rule_model: None,
    })
}

fn find_matched_rule_value(
    compare_mode: CodexReasoningGuardCompareMode,
    reasoning_tokens: i64,
    configured_values: &[i64],
) -> Option<i64> {
    match compare_mode {
        CodexReasoningGuardCompareMode::Equals => configured_values
            .iter()
            .copied()
            .find(|value| *value == reasoning_tokens),
        CodexReasoningGuardCompareMode::LessThanOrEqual => configured_values
            .iter()
            .copied()
            .filter(|value| reasoning_tokens <= *value)
            .min(),
    }
}

fn compare_mode_symbol(compare_mode: CodexReasoningGuardCompareMode) -> &'static str {
    match compare_mode {
        CodexReasoningGuardCompareMode::Equals => "==",
        CodexReasoningGuardCompareMode::LessThanOrEqual => "<=",
    }
}

pub(super) fn push_special_setting(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name: &str,
    retry_index: u32,
    matched: &CodexReasoningGuardMatch,
    backoff: CodexReasoningGuardBackoffDecision,
) {
    response_fixer::push_special_setting(
        special_settings,
        serde_json::json!({
            "type": "codex_reasoning_guard",
            "scope": "attempt",
            "hit": true,
            "providerId": provider_id,
            "providerName": provider_name,
            "reasoningTokens": matched.reasoning_tokens,
            "compareMode": matched.compare_mode,
            "compareModeSymbol": compare_mode_symbol(matched.compare_mode),
            "matchedRuleValue": matched.matched_rule_value,
            "pointer": matched.pointer,
            "requestedModel": matched.requested_model,
            "ruleSource": matched.rule_source,
            "ruleModel": matched.rule_model,
            "retryAttemptNumber": retry_index,
            "retryAttemptNumberNext": retry_index.saturating_add(1),
            "displayStatus": StatusCode::BAD_GATEWAY.as_u16(),
            "action": "retry_same_provider_no_circuit",
            "backoffApplied": backoff.applied,
            "backoffAfterHits": backoff.after_hits,
            "backoffMs": backoff.ms,
            "guardHitNumber": backoff.hit_number,
        }),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardBackoffDecision {
    pub(super) applied: bool,
    pub(super) after_hits: u32,
    pub(super) ms: u32,
    pub(super) hit_number: u32,
}

impl CodexReasoningGuardBackoffDecision {
    fn disabled(hit_number: u32, after_hits: u32) -> Self {
        Self {
            applied: false,
            after_hits,
            ms: 0,
            hit_number,
        }
    }
}

pub(super) fn backoff_decision(
    current_hits: u32,
    backoff_after_hits: u32,
    backoff_ms: u32,
) -> CodexReasoningGuardBackoffDecision {
    let hit_number = current_hits.saturating_add(1);
    if backoff_after_hits == 0 || backoff_ms == 0 || hit_number < backoff_after_hits {
        return CodexReasoningGuardBackoffDecision::disabled(hit_number, backoff_after_hits);
    }

    CodexReasoningGuardBackoffDecision {
        applied: true,
        after_hits: backoff_after_hits,
        ms: backoff_ms,
        hit_number,
    }
}

pub(super) async fn apply_backoff_if_needed(decision: CodexReasoningGuardBackoffDecision) {
    if !decision.applied {
        return;
    }
    tokio::time::sleep(Duration::from_millis(decision.ms as u64)).await;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record_guard_retry_attempt(
    attempts: &mut Vec<FailoverAttempt>,
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    provider_index: u32,
    retry_index: u32,
    session_reuse: Option<bool>,
    attempt_started_ms: u128,
    attempt_duration_ms: u128,
    circuit_state_before: &'static str,
    circuit_failure_count: u32,
    circuit_failure_threshold: u32,
    matched: &CodexReasoningGuardMatch,
) {
    attempts.push(FailoverAttempt {
        provider_id,
        provider_name: provider_name.to_string(),
        base_url: base_url.to_string(),
        outcome: "codex_reasoning_guard_retry".to_string(),
        status: Some(StatusCode::BAD_GATEWAY.as_u16()),
        provider_index: Some(provider_index),
        retry_index: Some(retry_index),
        session_reuse,
        error_category: Some(ErrorCategory::SystemError.as_str()),
        error_code: Some(CODEX_REASONING_GUARD_ERROR_CODE),
        decision: Some("retry_same_provider"),
        reason: Some(format!(
            "codex reasoning guard matched reasoning_tokens={} {} {} via {} ({})",
            matched.reasoning_tokens,
            compare_mode_symbol(matched.compare_mode),
            matched.matched_rule_value,
            matched.pointer,
            matched.rule_source
        )),
        selection_method: dc::selection_method(provider_index, retry_index, session_reuse),
        reason_code: Some(CODEX_REASONING_GUARD_REASON_CODE),
        attempt_started_ms: Some(attempt_started_ms),
        attempt_duration_ms: Some(attempt_duration_ms),
        circuit_state_before: Some(circuit_state_before),
        circuit_state_after: Some(circuit_state_before),
        circuit_failure_count: Some(circuit_failure_count),
        circuit_failure_threshold: Some(circuit_failure_threshold),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_from_json_matches_equals_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516, 1024],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.reasoning_tokens, 516);
        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(matched.compare_mode, CodexReasoningGuardCompareMode::Equals);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
    }

    #[test]
    fn backoff_decision_applies_at_threshold_and_afterward() {
        assert_eq!(
            backoff_decision(3, 5, 1_000),
            CodexReasoningGuardBackoffDecision {
                applied: false,
                after_hits: 5,
                ms: 0,
                hit_number: 4,
            }
        );
        assert_eq!(
            backoff_decision(4, 5, 1_000),
            CodexReasoningGuardBackoffDecision {
                applied: true,
                after_hits: 5,
                ms: 1_000,
                hit_number: 5,
            }
        );
        assert_eq!(
            backoff_decision(5, 5, 1_000),
            CodexReasoningGuardBackoffDecision {
                applied: true,
                after_hits: 5,
                ms: 1_000,
                hit_number: 6,
            }
        );
    }

    #[test]
    fn backoff_decision_supports_zero_as_disabled() {
        assert!(!backoff_decision(9, 0, 1_000).applied);
        assert!(!backoff_decision(9, 5, 0).applied);
    }

    #[test]
    fn detect_from_json_does_not_match_equals_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[],
        );

        assert!(matched.is_none());
    }

    #[test]
    fn detect_from_json_matches_less_than_or_equal_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[516],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.reasoning_tokens, 300);
        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(
            matched.compare_mode,
            CodexReasoningGuardCompareMode::LessThanOrEqual
        );
    }

    #[test]
    fn detect_from_json_uses_smallest_matching_less_than_or_equal_threshold() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[1024, 516, 2048],
            &[],
        )
        .expect("should match");

        assert_eq!(matched.matched_rule_value, 516);
    }

    #[test]
    fn detect_from_json_does_not_match_less_than_or_equal_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 800 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::LessThanOrEqual,
            &[516],
            &[],
        );

        assert!(matched.is_none());
    }

    #[test]
    fn detect_from_json_prefers_exact_model_rule() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 600 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[CodexReasoningGuardModelRule {
                requested_model: "gpt-5-codex".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                reasoning_equals: vec![700],
            }],
        )
        .expect("should match model rule");

        assert_eq!(matched.matched_rule_value, 700);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE
        );
        assert_eq!(matched.rule_model.as_deref(), Some("gpt-5-codex"));
    }

    #[test]
    fn detect_from_json_falls_back_to_global_rule_when_model_rule_missing() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let matched = detect_from_json(
            "codex",
            Some("gpt-5-mini-codex"),
            &value,
            CodexReasoningGuardCompareMode::Equals,
            &[516],
            &[CodexReasoningGuardModelRule {
                requested_model: "gpt-5-codex".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                reasoning_equals: vec![700],
            }],
        )
        .expect("should fall back to global rule");

        assert_eq!(matched.matched_rule_value, 516);
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
        assert!(matched.rule_model.is_none());
    }
}
