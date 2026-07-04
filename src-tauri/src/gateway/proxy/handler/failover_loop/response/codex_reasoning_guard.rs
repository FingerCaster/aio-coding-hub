//! Usage: Codex degraded-reasoning detection helpers.

use super::codex_reasoning_features::CodexReasoningFeatureSample;
use crate::gateway::events::{decision_chain as dc, FailoverAttempt};
use crate::gateway::proxy::ErrorCategory;
use crate::gateway::response_fixer;
use crate::settings::{
    CodexReasoningGuardCompareMode, CodexReasoningGuardExhaustedAction,
    CodexReasoningGuardModelRule, CodexReasoningGuardPostMatchStrategy,
    CodexReasoningGuardRetryPolicy, CodexReasoningGuardRuleMode, CodexReasoningGuardRuleTemplate,
    CodexReasoningGuardTemplateFilter, CodexReasoningGuardTemplateFilterField,
    CodexReasoningGuardTemplateFilterOperator, CodexReasoningGuardTemplateRule,
    CodexReasoningGuardTemplateRuleAction, CodexReasoningGuardTemplateRuleFormula,
    CodexReasoningGuardTemplateRuleLogic,
};
use axum::http::StatusCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(super) const CODEX_REASONING_GUARD_ERROR_CODE: &str = "GW_CODEX_REASONING_GUARD";
pub(super) const CODEX_REASONING_GUARD_REASON_CODE: &str = "codex_reasoning_guard";
const CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT: &str = "global_default";
const CODEX_REASONING_GUARD_RULE_SOURCE_MODEL_RULE: &str = "model_rule";
const CODEX_REASONING_GUARD_RULE_SOURCE_FEATURE: &str = "feature";
const CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_BUILTIN: &str = "template_builtin";
const CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_CUSTOM: &str = "template_custom";
const CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_NAME: &str = "Legacy reasoning tokens";
const CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_NAME: &str =
    "Reasoning tokens 518*N-2";
const CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_NAME: &str =
    "Final-answer-only high/xhigh";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CodexReasoningGuardHitSource {
    ReasoningTokens,
    FinalAnswerOnlyHighXhigh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardMatch {
    pub(super) rule_mode: CodexReasoningGuardRuleMode,
    pub(super) hit_source: CodexReasoningGuardHitSource,
    pub(super) reasoning_tokens: Option<i64>,
    pub(super) pointer: Option<&'static str>,
    pub(super) compare_mode: Option<CodexReasoningGuardCompareMode>,
    pub(super) matched_rule_value: Option<i64>,
    pub(super) requested_model: Option<String>,
    pub(super) rule_source: &'static str,
    pub(super) rule_model: Option<String>,
    pub(super) request_reasoning_effort: Option<String>,
    pub(super) final_answer_only: Option<bool>,
    pub(super) commentary_observed: Option<bool>,
    pub(super) has_tool_call: Option<bool>,
    pub(super) has_reasoning_item: Option<bool>,
    pub(super) rule_action: CodexReasoningGuardTemplateRuleAction,
    pub(super) template_id: Option<String>,
    pub(super) template_name: Option<String>,
    pub(super) rule_id: Option<String>,
    pub(super) rule_name: Option<String>,
    pub(super) rule_token: Option<i64>,
    pub(super) rule_formula: Option<CodexReasoningGuardTemplateRuleFormula>,
    pub(super) matched_condition: Option<String>,
    pub(super) matched_filter_ids: Vec<String>,
}

impl CodexReasoningGuardMatch {
    fn reasoning_token_condition_summary(&self) -> String {
        if let Some(condition) = self.matched_condition.as_deref() {
            return condition.to_string();
        }

        format!(
            "{} {}",
            self.compare_mode.map(compare_mode_symbol).unwrap_or("?"),
            self.matched_rule_value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    }

    fn reason_summary(&self, budget: CodexReasoningGuardBudgetDecision) -> String {
        let template = self
            .template_id
            .as_deref()
            .map(|id| format!(" template={id}"))
            .unwrap_or_default();
        let rule = self
            .rule_id
            .as_deref()
            .map(|id| format!(" rule={id}"))
            .unwrap_or_default();
        match self.hit_source {
            CodexReasoningGuardHitSource::ReasoningTokens => format!(
                "codex reasoning guard matched reasoning_tokens={} {} via {} ({}){}{} rule_action={:?} hit={} phase={} action={}",
                self.reasoning_tokens.unwrap_or_default(),
                self.reasoning_token_condition_summary(),
                self.pointer.unwrap_or(""),
                self.rule_source,
                template,
                rule,
                self.rule_action,
                budget.hit_number,
                budget.phase,
                budget.action_taken
            ),
            CodexReasoningGuardHitSource::FinalAnswerOnlyHighXhigh => format!(
                "codex reasoning guard matched final_answer_only_high_xhigh effort={} final_answer_only={} commentary_observed={} has_tool_call={} has_reasoning_item={} via {}{}{} rule_action={:?} hit={} phase={} action={}",
                self.request_reasoning_effort.as_deref().unwrap_or("unknown"),
                self.final_answer_only.unwrap_or(false),
                self.commentary_observed.unwrap_or(false),
                self.has_tool_call.unwrap_or(false),
                self.has_reasoning_item.unwrap_or(false),
                self.rule_source,
                template,
                rule,
                self.rule_action,
                budget.hit_number,
                budget.phase,
                budget.action_taken
            ),
        }
    }

    fn decision_reason_summary(&self) -> String {
        let template = self
            .template_id
            .as_deref()
            .map(|id| format!(" template={id}"))
            .unwrap_or_default();
        let rule = self
            .rule_id
            .as_deref()
            .map(|id| format!(" rule={id}"))
            .unwrap_or_default();
        match self.hit_source {
            CodexReasoningGuardHitSource::ReasoningTokens => format!(
                "codex reasoning guard decision matched reasoning_tokens={} {} via {} ({}){}{} rule_action={:?} action=allow",
                self.reasoning_tokens.unwrap_or_default(),
                self.reasoning_token_condition_summary(),
                self.pointer.unwrap_or(""),
                self.rule_source,
                template,
                rule,
                self.rule_action,
            ),
            CodexReasoningGuardHitSource::FinalAnswerOnlyHighXhigh => format!(
                "codex reasoning guard decision matched final_answer_only_high_xhigh effort={} final_answer_only={} commentary_observed={} has_tool_call={} has_reasoning_item={} via {}{}{} rule_action={:?} action=allow",
                self.request_reasoning_effort.as_deref().unwrap_or("unknown"),
                self.final_answer_only.unwrap_or(false),
                self.commentary_observed.unwrap_or(false),
                self.has_tool_call.unwrap_or(false),
                self.has_reasoning_item.unwrap_or(false),
                self.rule_source,
                template,
                rule,
                self.rule_action,
            ),
        }
    }
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

fn detect_from_json(
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
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                hit_source: CodexReasoningGuardHitSource::ReasoningTokens,
                reasoning_tokens: Some(reasoning_tokens),
                pointer: Some(pointer),
                compare_mode: Some(resolved_rule.compare_mode),
                matched_rule_value: Some(matched_rule_value),
                requested_model: requested_model
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(ToOwned::to_owned),
                rule_source: resolved_rule.rule_source,
                rule_model: resolved_rule.rule_model.map(ToOwned::to_owned),
                request_reasoning_effort: None,
                final_answer_only: None,
                commentary_observed: None,
                has_tool_call: None,
                has_reasoning_item: None,
                rule_action: CodexReasoningGuardTemplateRuleAction::Intercept,
                template_id: None,
                template_name: None,
                rule_id: None,
                rule_name: None,
                rule_token: Some(matched_rule_value),
                rule_formula: None,
                matched_condition: None,
                matched_filter_ids: Vec::new(),
            });
        }
    }

    None
}

pub(super) struct CodexReasoningGuardEvaluationInput<'a> {
    pub(super) cli_key: &'a str,
    pub(super) requested_model: Option<&'a str>,
    pub(super) value: &'a serde_json::Value,
    pub(super) rule_mode: CodexReasoningGuardRuleMode,
    pub(super) feature_sample: Option<&'a CodexReasoningFeatureSample>,
}

struct CodexReasoningGuardLegacyEvaluationInput<'a> {
    base: CodexReasoningGuardEvaluationInput<'a>,
    fallback_compare_mode: CodexReasoningGuardCompareMode,
    fallback_values: &'a [i64],
    model_rules: &'a [CodexReasoningGuardModelRule],
}

pub(super) struct CodexReasoningGuardDecisionEvaluationInput<'a> {
    pub(super) base: CodexReasoningGuardEvaluationInput<'a>,
    pub(super) active_template_id: &'a str,
    pub(super) custom_templates: &'a [CodexReasoningGuardRuleTemplate],
    pub(super) duration_ms: Option<u128>,
    pub(super) ttfb_ms: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardEvaluationDecision {
    pub(super) action: CodexReasoningGuardTemplateRuleAction,
    pub(super) matched: CodexReasoningGuardMatch,
}

#[allow(dead_code)]
fn evaluate(
    input: CodexReasoningGuardLegacyEvaluationInput<'_>,
) -> Option<CodexReasoningGuardMatch> {
    evaluate_legacy(input)
}

fn evaluate_legacy(
    input: CodexReasoningGuardLegacyEvaluationInput<'_>,
) -> Option<CodexReasoningGuardMatch> {
    if input.base.cli_key != "codex" {
        return None;
    }
    if input
        .base
        .feature_sample
        .and_then(|sample| sample.intercept_exempt_reason)
        .is_some()
    {
        return None;
    }

    match input.base.rule_mode {
        CodexReasoningGuardRuleMode::ReasoningTokens => detect_from_json(
            input.base.cli_key,
            input.base.requested_model,
            input.base.value,
            input.fallback_compare_mode,
            input.fallback_values,
            input.model_rules,
        ),
        CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh => {
            detect_final_answer_only_high_xhigh(input.base)
        }
    }
}

pub(super) fn evaluate_decision(
    input: CodexReasoningGuardDecisionEvaluationInput<'_>,
) -> Option<CodexReasoningGuardEvaluationDecision> {
    if input.base.cli_key != "codex" {
        return None;
    }
    if input
        .base
        .feature_sample
        .and_then(|sample| sample.intercept_exempt_reason)
        .is_some()
    {
        return None;
    }

    let template = resolve_template(input.active_template_id, input.custom_templates);
    if template.id
        == crate::settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
        && input
            .base
            .feature_sample
            .is_some_and(|sample| sample.reasoning_tokens == Some(0))
    {
        return None;
    }

    evaluate_template(input, template)
}

#[derive(Debug, Clone)]
struct ResolvedGuardTemplate {
    id: String,
    name: String,
    source: &'static str,
    rules: Vec<CodexReasoningGuardTemplateRule>,
}

fn resolve_template(
    active_template_id: &str,
    custom_templates: &[CodexReasoningGuardRuleTemplate],
) -> ResolvedGuardTemplate {
    let active_template_id = active_template_id.trim();
    if active_template_id
        == crate::settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
    {
        return ResolvedGuardTemplate {
            id: crate::settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
                .to_string(),
            name: CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_NAME.to_string(),
            source: CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_BUILTIN,
            rules: Vec::new(),
        };
    }
    if active_template_id
        == crate::settings::CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
    {
        return reasoning_tokens_518n_minus_2_template();
    }

    if let Some(template) = custom_templates
        .iter()
        .find(|template| template.id.trim() == active_template_id)
    {
        return ResolvedGuardTemplate {
            id: template.id.trim().to_string(),
            name: template.name.trim().to_string(),
            source: CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_CUSTOM,
            rules: template.rules.clone(),
        };
    }

    legacy_reasoning_tokens_template()
}

fn reasoning_tokens_518n_minus_2_template() -> ResolvedGuardTemplate {
    ResolvedGuardTemplate {
        id: crate::settings::CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
            .to_string(),
        name: CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_NAME.to_string(),
        source: CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_BUILTIN,
        rules: vec![CodexReasoningGuardTemplateRule {
            id: "reasoning-tokens-518n-minus-2".to_string(),
            name: "reasoning_tokens == 518*N-2".to_string(),
            reasoning_tokens: None,
            reasoning_tokens_formula: Some(
                CodexReasoningGuardTemplateRuleFormula::ReasoningTokens518NMinus2,
            ),
            action: CodexReasoningGuardTemplateRuleAction::Intercept,
            logic: CodexReasoningGuardTemplateRuleLogic::And,
            filters: Vec::new(),
        }],
    }
}

fn legacy_reasoning_tokens_template() -> ResolvedGuardTemplate {
    ResolvedGuardTemplate {
        id: crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string(),
        name: CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_NAME.to_string(),
        source: CODEX_REASONING_GUARD_RULE_SOURCE_TEMPLATE_BUILTIN,
        rules: crate::settings::DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
            .iter()
            .copied()
            .enumerate()
            .map(|(index, value)| CodexReasoningGuardTemplateRule {
                id: format!("builtin-token-{value}-{index}"),
                name: format!("reasoning_tokens == {value}"),
                reasoning_tokens: Some(value),
                reasoning_tokens_formula: None,
                action: CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: CodexReasoningGuardTemplateRuleLogic::And,
                filters: Vec::new(),
            })
            .collect(),
    }
}

fn evaluate_template(
    input: CodexReasoningGuardDecisionEvaluationInput<'_>,
    template: ResolvedGuardTemplate,
) -> Option<CodexReasoningGuardEvaluationDecision> {
    if template.id
        == crate::settings::CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
    {
        let mut matched = detect_final_answer_only_high_xhigh(input.base)?;
        apply_template_evidence(
            &mut matched,
            &template,
            TemplateEvidence {
                rule_id: "builtin-final-answer-only-high-xhigh",
                rule_name: CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_NAME,
                rule_token: None,
                rule_formula: None,
                matched_filter_ids: Vec::new(),
                action: CodexReasoningGuardTemplateRuleAction::Intercept,
            },
        );
        return Some(CodexReasoningGuardEvaluationDecision {
            action: CodexReasoningGuardTemplateRuleAction::Intercept,
            matched,
        });
    }

    let fields = NormalizedTemplateFields::from_input(&input);
    let concrete_rules = fields.reasoning_tokens.and_then(|_| {
        template
            .rules
            .iter()
            .filter(|rule| template_rule_condition_matches(rule, &fields))
            .find_map(|rule| evaluate_template_rule(rule, &fields).map(|ids| (rule, ids)))
    });
    let wildcard_rule = || {
        template
            .rules
            .iter()
            .filter(|rule| {
                rule.reasoning_tokens.is_none() && rule.reasoning_tokens_formula.is_none()
            })
            .find_map(|rule| evaluate_template_rule(rule, &fields).map(|ids| (rule, ids)))
    };
    let (rule, matched_filter_ids) = concrete_rules.or_else(wildcard_rule)?;
    let (compare_mode, matched_rule_value) =
        matched_template_rule_value(rule, &matched_filter_ids, fields.reasoning_tokens);

    let mut matched = CodexReasoningGuardMatch {
        rule_mode: input.base.rule_mode,
        hit_source: CodexReasoningGuardHitSource::ReasoningTokens,
        reasoning_tokens: fields.reasoning_tokens,
        pointer: fields.reasoning_tokens_pointer,
        compare_mode,
        matched_rule_value,
        requested_model: fields.requested_model.clone(),
        rule_source: template.source,
        rule_model: None,
        request_reasoning_effort: fields.request_reasoning_effort.clone(),
        final_answer_only: fields.final_answer_only,
        commentary_observed: fields.commentary_observed,
        has_tool_call: fields.has_tool_call,
        has_reasoning_item: fields.has_reasoning_item,
        rule_action: rule.action,
        template_id: None,
        template_name: None,
        rule_id: None,
        rule_name: None,
        rule_token: rule.reasoning_tokens,
        rule_formula: rule.reasoning_tokens_formula,
        matched_condition: rule_condition_label(rule, fields.reasoning_tokens),
        matched_filter_ids: Vec::new(),
    };
    let rule_id = rule.id.trim();
    let rule_name = rule.name.trim();
    apply_template_evidence(
        &mut matched,
        &template,
        TemplateEvidence {
            rule_id: if rule_id.is_empty() {
                "unnamed-rule"
            } else {
                rule_id
            },
            rule_name: if rule_name.is_empty() {
                rule_id
            } else {
                rule_name
            },
            rule_token: rule.reasoning_tokens,
            rule_formula: rule.reasoning_tokens_formula,
            matched_filter_ids,
            action: rule.action,
        },
    );

    Some(CodexReasoningGuardEvaluationDecision {
        action: rule.action,
        matched,
    })
}

fn template_rule_condition_matches(
    rule: &CodexReasoningGuardTemplateRule,
    fields: &NormalizedTemplateFields,
) -> bool {
    if let Some(token) = rule.reasoning_tokens {
        return fields.reasoning_tokens == Some(token);
    }
    if let Some(formula) = rule.reasoning_tokens_formula {
        return match formula {
            CodexReasoningGuardTemplateRuleFormula::ReasoningTokens518NMinus2 => {
                super::codex_reasoning_continuation::is_truncation_continuation_pattern(
                    fields.reasoning_tokens,
                )
            }
        };
    }
    true
}

struct TemplateEvidence<'a> {
    rule_id: &'a str,
    rule_name: &'a str,
    rule_token: Option<i64>,
    rule_formula: Option<CodexReasoningGuardTemplateRuleFormula>,
    matched_filter_ids: Vec<String>,
    action: CodexReasoningGuardTemplateRuleAction,
}

fn apply_template_evidence(
    matched: &mut CodexReasoningGuardMatch,
    template: &ResolvedGuardTemplate,
    evidence: TemplateEvidence<'_>,
) {
    matched.rule_source = template.source;
    matched.template_id = Some(template.id.clone());
    matched.template_name = Some(template.name.clone());
    matched.rule_id = Some(evidence.rule_id.to_string());
    matched.rule_name = Some(evidence.rule_name.to_string());
    matched.rule_token = evidence.rule_token;
    matched.rule_formula = evidence.rule_formula;
    matched.matched_filter_ids = evidence.matched_filter_ids;
    matched.rule_action = evidence.action;
}

fn matched_template_rule_value(
    rule: &CodexReasoningGuardTemplateRule,
    matched_filter_ids: &[String],
    reasoning_tokens: Option<i64>,
) -> (Option<CodexReasoningGuardCompareMode>, Option<i64>) {
    if let Some(value) = rule.reasoning_tokens {
        return (Some(CodexReasoningGuardCompareMode::Equals), Some(value));
    }
    if let Some(CodexReasoningGuardTemplateRuleFormula::ReasoningTokens518NMinus2) =
        rule.reasoning_tokens_formula
    {
        if super::codex_reasoning_continuation::is_truncation_continuation_pattern(reasoning_tokens)
        {
            return (
                Some(CodexReasoningGuardCompareMode::Equals),
                reasoning_tokens,
            );
        }
    }

    let Some(reasoning_tokens) = reasoning_tokens else {
        return (None, None);
    };
    let mut less_than_or_equal_value: Option<i64> = None;
    for filter in &rule.filters {
        if filter.field != CodexReasoningGuardTemplateFilterField::ReasoningTokens {
            continue;
        }
        if !matched_filter_ids.iter().any(|id| id == &filter.id) {
            continue;
        }
        let Some(value) = filter.number_value.and_then(number_filter_value_to_i64) else {
            continue;
        };
        match filter.operator {
            CodexReasoningGuardTemplateFilterOperator::Equals if value == reasoning_tokens => {
                return (Some(CodexReasoningGuardCompareMode::Equals), Some(value));
            }
            CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
                if reasoning_tokens <= value =>
            {
                less_than_or_equal_value =
                    Some(less_than_or_equal_value.map_or(value, |current| current.min(value)));
            }
            _ => {}
        }
    }

    if let Some(value) = less_than_or_equal_value {
        return (
            Some(CodexReasoningGuardCompareMode::LessThanOrEqual),
            Some(value),
        );
    }
    (None, None)
}

fn rule_condition_label(
    rule: &CodexReasoningGuardTemplateRule,
    reasoning_tokens: Option<i64>,
) -> Option<String> {
    if let Some(token) = rule.reasoning_tokens {
        return Some(format!("reasoning_tokens == {token}"));
    }
    if let Some(CodexReasoningGuardTemplateRuleFormula::ReasoningTokens518NMinus2) =
        rule.reasoning_tokens_formula
    {
        if super::codex_reasoning_continuation::is_truncation_continuation_pattern(reasoning_tokens)
        {
            return Some("reasoning_tokens == 518*N-2".to_string());
        }
    }
    None
}

fn number_filter_value_to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    Some(value as i64)
}

fn evaluate_template_rule(
    rule: &CodexReasoningGuardTemplateRule,
    fields: &NormalizedTemplateFields,
) -> Option<Vec<String>> {
    if rule.filters.is_empty() {
        return Some(Vec::new());
    }

    let mut matched_filter_ids = Vec::new();
    match rule.logic {
        CodexReasoningGuardTemplateRuleLogic::And => {
            for filter in &rule.filters {
                if !evaluate_template_filter(filter, fields) {
                    return None;
                }
                matched_filter_ids.push(filter.id.clone());
            }
            Some(matched_filter_ids)
        }
        CodexReasoningGuardTemplateRuleLogic::Or => {
            for filter in &rule.filters {
                if evaluate_template_filter(filter, fields) {
                    matched_filter_ids.push(filter.id.clone());
                }
            }
            (!matched_filter_ids.is_empty()).then_some(matched_filter_ids)
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedTemplateFields {
    duration_ms: Option<f64>,
    tps: Option<f64>,
    output_tokens: Option<f64>,
    input_tokens: Option<f64>,
    total_tokens: Option<f64>,
    reasoning_tokens: Option<i64>,
    reasoning_tokens_pointer: Option<&'static str>,
    final_answer_only: Option<bool>,
    has_tool_call: Option<bool>,
    has_reasoning_item: Option<bool>,
    commentary_observed: Option<bool>,
    request_reasoning_effort: Option<String>,
    requested_model: Option<String>,
}

impl NormalizedTemplateFields {
    fn from_input(input: &CodexReasoningGuardDecisionEvaluationInput<'_>) -> Self {
        let (reasoning_tokens, reasoning_tokens_pointer) =
            extract_reasoning_tokens(input.base.value).unwrap_or((None, None));
        let output_tokens = extract_token_count(
            input.base.value,
            &[
                "/usage/output_tokens",
                "/usage/completion_tokens",
                "/response/usage/output_tokens",
                "/response/usage/completion_tokens",
            ],
        );
        let input_tokens = extract_token_count(
            input.base.value,
            &[
                "/usage/input_tokens",
                "/usage/prompt_tokens",
                "/response/usage/input_tokens",
                "/response/usage/prompt_tokens",
            ],
        );
        let total_tokens = extract_token_count(
            input.base.value,
            &["/usage/total_tokens", "/response/usage/total_tokens"],
        )
        .or(match (input_tokens, output_tokens) {
            (Some(input_tokens), Some(output_tokens)) => Some(input_tokens + output_tokens),
            _ => None,
        });
        let duration_ms = input.duration_ms.map(|value| value as f64);
        let ttfb_ms = input.ttfb_ms.map(|value| value as f64);
        let tps = output_tokens.and_then(|tokens| {
            let duration_ms = duration_ms?;
            let generation_ms = ttfb_ms
                .and_then(|ttfb| (duration_ms > ttfb).then_some(duration_ms - ttfb))
                .unwrap_or(duration_ms);
            (generation_ms > 0.0).then_some(tokens / (generation_ms / 1000.0))
        });
        let sample = input.base.feature_sample;

        Self {
            duration_ms,
            tps,
            output_tokens,
            input_tokens,
            total_tokens,
            reasoning_tokens,
            reasoning_tokens_pointer,
            final_answer_only: sample.and_then(|sample| sample.final_answer_only),
            has_tool_call: sample.and_then(|sample| sample.has_tool_call),
            has_reasoning_item: sample.and_then(|sample| sample.has_reasoning_item),
            commentary_observed: sample.and_then(|sample| sample.commentary_observed),
            request_reasoning_effort: sample.and_then(|sample| {
                sample
                    .request_reasoning_effort
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            }),
            requested_model: input
                .base
                .requested_model
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(ToOwned::to_owned),
        }
    }
}

fn extract_reasoning_tokens(
    value: &serde_json::Value,
) -> Option<(Option<i64>, Option<&'static str>)> {
    for pointer in REASONING_POINTERS {
        let Some(raw) = value.pointer(pointer) else {
            continue;
        };
        if let Some(tokens) = json_number_to_i64(raw) {
            return Some((Some(tokens), Some(pointer)));
        }
    }
    None
}

fn extract_token_count(value: &serde_json::Value, pointers: &[&str]) -> Option<f64> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(json_number_to_f64))
}

fn json_number_to_i64(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        _ => None,
    }
}

fn json_number_to_f64(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        _ => None,
    }
}

fn evaluate_template_filter(
    filter: &CodexReasoningGuardTemplateFilter,
    fields: &NormalizedTemplateFields,
) -> bool {
    match filter.field {
        CodexReasoningGuardTemplateFilterField::DurationMs => {
            evaluate_number_filter(fields.duration_ms, filter)
        }
        CodexReasoningGuardTemplateFilterField::Tps => evaluate_number_filter(fields.tps, filter),
        CodexReasoningGuardTemplateFilterField::OutputTokens => {
            evaluate_number_filter(fields.output_tokens, filter)
        }
        CodexReasoningGuardTemplateFilterField::InputTokens => {
            evaluate_number_filter(fields.input_tokens, filter)
        }
        CodexReasoningGuardTemplateFilterField::TotalTokens => {
            evaluate_number_filter(fields.total_tokens, filter)
        }
        CodexReasoningGuardTemplateFilterField::ReasoningTokens => {
            evaluate_number_filter(fields.reasoning_tokens.map(|value| value as f64), filter)
        }
        CodexReasoningGuardTemplateFilterField::FinalAnswerOnly => {
            evaluate_bool_filter(fields.final_answer_only, filter)
        }
        CodexReasoningGuardTemplateFilterField::HasToolCall => {
            evaluate_bool_filter(fields.has_tool_call, filter)
        }
        CodexReasoningGuardTemplateFilterField::HasReasoningItem => {
            evaluate_bool_filter(fields.has_reasoning_item, filter)
        }
        CodexReasoningGuardTemplateFilterField::CommentaryObserved => {
            evaluate_bool_filter(fields.commentary_observed, filter)
        }
        CodexReasoningGuardTemplateFilterField::RequestReasoningEffort => {
            evaluate_string_filter(fields.request_reasoning_effort.as_deref(), filter)
        }
        CodexReasoningGuardTemplateFilterField::RequestedModel => {
            evaluate_string_filter(fields.requested_model.as_deref(), filter)
        }
    }
}

fn evaluate_number_filter(value: Option<f64>, filter: &CodexReasoningGuardTemplateFilter) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(expected) = filter.number_value else {
        return false;
    };
    match filter.operator {
        CodexReasoningGuardTemplateFilterOperator::Equals => value == expected,
        CodexReasoningGuardTemplateFilterOperator::NotEquals => value != expected,
        CodexReasoningGuardTemplateFilterOperator::LessThan => value < expected,
        CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual => value <= expected,
        CodexReasoningGuardTemplateFilterOperator::GreaterThan => value > expected,
        CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual => value >= expected,
        CodexReasoningGuardTemplateFilterOperator::In
        | CodexReasoningGuardTemplateFilterOperator::NotIn => false,
    }
}

fn evaluate_bool_filter(value: Option<bool>, filter: &CodexReasoningGuardTemplateFilter) -> bool {
    let Some(value) = value else {
        return false;
    };
    let Some(expected) = filter.bool_value else {
        return false;
    };
    match filter.operator {
        CodexReasoningGuardTemplateFilterOperator::Equals => value == expected,
        CodexReasoningGuardTemplateFilterOperator::NotEquals => value != expected,
        CodexReasoningGuardTemplateFilterOperator::LessThan
        | CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
        | CodexReasoningGuardTemplateFilterOperator::GreaterThan
        | CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual
        | CodexReasoningGuardTemplateFilterOperator::In
        | CodexReasoningGuardTemplateFilterOperator::NotIn => false,
    }
}

fn evaluate_string_filter(value: Option<&str>, filter: &CodexReasoningGuardTemplateFilter) -> bool {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return match filter.operator {
            CodexReasoningGuardTemplateFilterOperator::NotEquals => filter
                .string_value
                .as_deref()
                .map(str::trim)
                .is_some_and(|expected| !expected.is_empty()),
            CodexReasoningGuardTemplateFilterOperator::NotIn => filter
                .string_values
                .iter()
                .any(|expected| !expected.trim().is_empty()),
            _ => false,
        };
    };
    match filter.operator {
        CodexReasoningGuardTemplateFilterOperator::Equals => filter
            .string_value
            .as_deref()
            .map(str::trim)
            .is_some_and(|expected| value == expected),
        CodexReasoningGuardTemplateFilterOperator::NotEquals => filter
            .string_value
            .as_deref()
            .map(str::trim)
            .is_some_and(|expected| value != expected),
        CodexReasoningGuardTemplateFilterOperator::In => filter
            .string_values
            .iter()
            .map(|item| item.trim())
            .any(|expected| value == expected),
        CodexReasoningGuardTemplateFilterOperator::NotIn => !filter
            .string_values
            .iter()
            .map(|item| item.trim())
            .any(|expected| value == expected),
        CodexReasoningGuardTemplateFilterOperator::LessThan
        | CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
        | CodexReasoningGuardTemplateFilterOperator::GreaterThan
        | CodexReasoningGuardTemplateFilterOperator::GreaterThanOrEqual => false,
    }
}

fn detect_final_answer_only_high_xhigh(
    input: CodexReasoningGuardEvaluationInput<'_>,
) -> Option<CodexReasoningGuardMatch> {
    let sample = input.feature_sample?;
    let effort = sample.request_reasoning_effort.as_deref()?;
    if !matches!(effort, "high" | "xhigh") {
        return None;
    }
    if sample.final_answer_only != Some(true)
        || sample.commentary_observed == Some(true)
        || sample.has_tool_call == Some(true)
        || sample.has_reasoning_item == Some(true)
    {
        return None;
    }
    if sample.reasoning_tokens == Some(0) {
        return None;
    }

    Some(CodexReasoningGuardMatch {
        rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
        hit_source: CodexReasoningGuardHitSource::FinalAnswerOnlyHighXhigh,
        reasoning_tokens: sample.reasoning_tokens,
        pointer: sample.reasoning_tokens_pointer,
        compare_mode: None,
        matched_rule_value: None,
        requested_model: input
            .requested_model
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(ToOwned::to_owned),
        rule_source: CODEX_REASONING_GUARD_RULE_SOURCE_FEATURE,
        rule_model: None,
        request_reasoning_effort: sample.request_reasoning_effort.clone(),
        final_answer_only: sample.final_answer_only,
        commentary_observed: sample.commentary_observed,
        has_tool_call: sample.has_tool_call,
        has_reasoning_item: sample.has_reasoning_item,
        rule_action: CodexReasoningGuardTemplateRuleAction::Intercept,
        template_id: None,
        template_name: None,
        rule_id: None,
        rule_name: None,
        rule_token: None,
        rule_formula: None,
        matched_condition: None,
        matched_filter_ids: Vec::new(),
    })
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

pub(super) fn post_match_strategy_label(
    strategy: CodexReasoningGuardPostMatchStrategy,
) -> &'static str {
    match strategy {
        CodexReasoningGuardPostMatchStrategy::RetrySameProvider => "retry_same_provider",
        CodexReasoningGuardPostMatchStrategy::ContinuationRepair => "continuation_repair",
    }
}

pub(super) fn push_special_setting(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name: &str,
    retry_index: u32,
    matched: &CodexReasoningGuardMatch,
    budget: CodexReasoningGuardBudgetDecision,
) {
    push_special_setting_with_strategy(
        special_settings,
        provider_id,
        provider_name,
        retry_index,
        matched,
        budget,
        CodexReasoningGuardPostMatchStrategy::RetrySameProvider,
        Some(budget.action_taken),
        None,
        None,
        None,
        StatusCode::BAD_GATEWAY.as_u16(),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_special_setting_with_strategy(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name: &str,
    retry_index: u32,
    matched: &CodexReasoningGuardMatch,
    budget: CodexReasoningGuardBudgetDecision,
    post_match_strategy: CodexReasoningGuardPostMatchStrategy,
    strategy_outcome: Option<&str>,
    continuation_sent_rounds: Option<u32>,
    continuation_failure_kind: Option<&str>,
    strategy_reason: Option<&str>,
    display_status: u16,
) {
    let mut setting = serde_json::json!({
            "type": "codex_reasoning_guard",
            "scope": "attempt",
            "hit": true,
            "ruleMode": matched.rule_mode,
            "hitSource": matched.hit_source,
            "providerId": provider_id,
            "providerName": provider_name,
            "reasoningTokens": matched.reasoning_tokens,
            "compareMode": matched.compare_mode,
            "compareModeSymbol": matched.compare_mode.map(compare_mode_symbol),
            "matchedRuleValue": matched.matched_rule_value,
            "pointer": matched.pointer,
            "requestedModel": matched.requested_model,
            "ruleSource": matched.rule_source,
            "ruleModel": matched.rule_model,
            "requestReasoningEffort": matched.request_reasoning_effort,
            "finalAnswerOnly": matched.final_answer_only,
            "commentaryObserved": matched.commentary_observed,
            "hasToolCall": matched.has_tool_call,
            "hasReasoningItem": matched.has_reasoning_item,
            "retryAttemptNumber": retry_index,
            "retryAttemptNumberNext": retry_index.saturating_add(1),
            "displayStatus": display_status,
            "action": budget.action_taken,
            "actionTaken": budget.action_taken,
            "backoffApplied": budget.delay_ms > 0,
            "backoffAfterHits": budget.immediate_budget,
            "backoffMs": budget.delay_ms,
            "guardHitNumber": budget.hit_number,
            "guardRetryPhase": budget.phase,
            "guardBudgetRemaining": budget.remaining_budget,
            "guardBudgetTotal": budget.total_budget,
            "guardExhaustedAction": budget.exhausted_action,
            "guardRetryPolicy": budget.retry_wave.map(|wave| match wave.policy {
                CodexReasoningGuardRetryPolicy::Single => "single",
                CodexReasoningGuardRetryPolicy::Concurrent => "concurrent",
            }),
            "guardRetryConcurrency": budget.retry_wave.map(|wave| wave.concurrency),
            "guardRetryIntervalMs": budget.retry_wave.map(|wave| wave.interval_ms),
            "guardRetryMaxAttempts": budget.retry_wave.map(|wave| wave.max_attempts),
            "guardRetryWaveExhausted": budget.retry_wave.map(|wave| wave.exhausted),
            "reason": matched.reason_summary(budget),
    });
    if let Some(object) = setting.as_object_mut() {
        object.insert(
            "guardPostMatchStrategy".to_string(),
            serde_json::Value::String(post_match_strategy_label(post_match_strategy).to_string()),
        );
        object.insert(
            "guardStrategyOutcome".to_string(),
            serde_json::to_value(strategy_outcome).unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "continuationSentRounds".to_string(),
            serde_json::to_value(continuation_sent_rounds).unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "continuationFailureKind".to_string(),
            serde_json::to_value(continuation_failure_kind).unwrap_or(serde_json::Value::Null),
        );
        object.insert(
            "strategyReason".to_string(),
            serde_json::to_value(strategy_reason).unwrap_or(serde_json::Value::Null),
        );
    }
    insert_template_match_evidence(&mut setting, matched);
    response_fixer::push_special_setting(special_settings, setting);
}

pub(super) fn push_decision_special_setting(
    special_settings: &Arc<Mutex<Vec<serde_json::Value>>>,
    provider_id: i64,
    provider_name: &str,
    retry_index: u32,
    matched: &CodexReasoningGuardMatch,
) {
    let mut setting = serde_json::json!({
            "type": "codex_reasoning_guard_decision",
            "scope": "attempt",
            "hit": false,
            "ruleMode": matched.rule_mode,
            "hitSource": matched.hit_source,
            "providerId": provider_id,
            "providerName": provider_name,
            "reasoningTokens": matched.reasoning_tokens,
            "compareMode": matched.compare_mode,
            "compareModeSymbol": matched.compare_mode.map(compare_mode_symbol),
            "matchedRuleValue": matched.matched_rule_value,
            "pointer": matched.pointer,
            "requestedModel": matched.requested_model,
            "ruleSource": matched.rule_source,
            "ruleModel": matched.rule_model,
            "requestReasoningEffort": matched.request_reasoning_effort,
            "finalAnswerOnly": matched.final_answer_only,
            "commentaryObserved": matched.commentary_observed,
            "hasToolCall": matched.has_tool_call,
            "hasReasoningItem": matched.has_reasoning_item,
            "retryAttemptNumber": retry_index,
            "retryAttemptNumberNext": retry_index.saturating_add(1),
            "displayStatus": StatusCode::OK.as_u16(),
            "action": matched.rule_action,
            "actionTaken": "allow",
            "reason": matched.decision_reason_summary(),
    });
    insert_template_match_evidence(&mut setting, matched);
    response_fixer::push_special_setting(special_settings, setting);
}

fn insert_template_match_evidence(
    setting: &mut serde_json::Value,
    matched: &CodexReasoningGuardMatch,
) {
    let Some(object) = setting.as_object_mut() else {
        return;
    };
    object.insert(
        "guardTemplateId".to_string(),
        serde_json::to_value(&matched.template_id).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "guardTemplateName".to_string(),
        serde_json::to_value(&matched.template_name).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedRuleId".to_string(),
        serde_json::to_value(&matched.rule_id).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedRuleName".to_string(),
        serde_json::to_value(&matched.rule_name).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedRuleToken".to_string(),
        serde_json::to_value(matched.rule_token).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedRuleFormula".to_string(),
        serde_json::to_value(matched.rule_formula).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedCondition".to_string(),
        serde_json::to_value(&matched.matched_condition).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedRuleAction".to_string(),
        serde_json::to_value(matched.rule_action).unwrap_or(serde_json::Value::Null),
    );
    object.insert(
        "matchedFilterIds".to_string(),
        serde_json::to_value(&matched.matched_filter_ids).unwrap_or(serde_json::Value::Null),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexReasoningGuardBudgetAction {
    RetrySameProvider,
    ReturnError,
    SwitchProvider,
    SwitchModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardBudgetDecision {
    pub(super) action: CodexReasoningGuardBudgetAction,
    pub(super) hit_number: u32,
    pub(super) phase: &'static str,
    pub(super) delay_ms: u32,
    pub(super) retry_wave: Option<CodexReasoningGuardRetryWave>,
    pub(super) immediate_budget: u32,
    pub(super) delayed_budget: u32,
    pub(super) total_budget: u32,
    pub(super) remaining_budget: u32,
    pub(super) exhausted_action: &'static str,
    pub(super) action_taken: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardBudgetConfig {
    pub(super) immediate_budget: u32,
    pub(super) delayed_budget: u32,
    pub(super) delayed_retry_ms: u32,
    pub(super) exhausted_action: CodexReasoningGuardExhaustedAction,
    pub(super) retry_policy: CodexReasoningGuardRetryPolicy,
    pub(super) concurrent_max: u32,
    pub(super) concurrent_interval_ms: u32,
    pub(super) concurrent_max_attempts: u32,
}

fn exhausted_action_label(action: CodexReasoningGuardExhaustedAction) -> &'static str {
    match action {
        CodexReasoningGuardExhaustedAction::ReturnError => "return_error",
        CodexReasoningGuardExhaustedAction::SwitchProvider => "switch_provider",
        CodexReasoningGuardExhaustedAction::SwitchModel => "switch_model",
    }
}

fn exhausted_budget_action(
    action: CodexReasoningGuardExhaustedAction,
) -> CodexReasoningGuardBudgetAction {
    match action {
        CodexReasoningGuardExhaustedAction::ReturnError => {
            CodexReasoningGuardBudgetAction::ReturnError
        }
        CodexReasoningGuardExhaustedAction::SwitchProvider => {
            CodexReasoningGuardBudgetAction::SwitchProvider
        }
        CodexReasoningGuardExhaustedAction::SwitchModel => {
            CodexReasoningGuardBudgetAction::SwitchModel
        }
    }
}

fn exhausted_action_taken(action: CodexReasoningGuardExhaustedAction) -> &'static str {
    match action {
        CodexReasoningGuardExhaustedAction::ReturnError => "return_guard_error_no_circuit",
        CodexReasoningGuardExhaustedAction::SwitchProvider => "switch_provider_no_circuit",
        CodexReasoningGuardExhaustedAction::SwitchModel => "switch_model_no_circuit",
    }
}

pub(super) fn continuation_repaired_decision(
    current_hits: u32,
    immediate_budget: u32,
    sent_rounds: u32,
    exhausted_action: CodexReasoningGuardExhaustedAction,
) -> CodexReasoningGuardBudgetDecision {
    let hit_number = current_hits.saturating_add(1);
    CodexReasoningGuardBudgetDecision {
        action: CodexReasoningGuardBudgetAction::RetrySameProvider,
        hit_number,
        phase: "continuation",
        delay_ms: 0,
        retry_wave: None,
        immediate_budget,
        delayed_budget: 0,
        total_budget: immediate_budget,
        remaining_budget: immediate_budget.saturating_sub(sent_rounds),
        exhausted_action: exhausted_action_label(exhausted_action),
        action_taken: "continuation_repaired",
    }
}

pub(super) fn continuation_exhausted_decision(
    current_hits: u32,
    immediate_budget: u32,
    exhausted_action: CodexReasoningGuardExhaustedAction,
) -> CodexReasoningGuardBudgetDecision {
    let hit_number = current_hits.saturating_add(1);
    CodexReasoningGuardBudgetDecision {
        action: exhausted_budget_action(exhausted_action),
        hit_number,
        phase: "continuation_exhausted",
        delay_ms: 0,
        retry_wave: None,
        immediate_budget,
        delayed_budget: 0,
        total_budget: immediate_budget,
        remaining_budget: 0,
        exhausted_action: exhausted_action_label(exhausted_action),
        action_taken: exhausted_action_taken(exhausted_action),
    }
}

pub(super) fn budget_decision(
    current_hits: u32,
    config: CodexReasoningGuardBudgetConfig,
) -> CodexReasoningGuardBudgetDecision {
    let hit_number = current_hits.saturating_add(1);
    let total_budget = config
        .immediate_budget
        .saturating_add(config.delayed_budget);
    let wave = retry_wave(
        current_hits,
        config.retry_policy,
        config.concurrent_max,
        config.concurrent_interval_ms,
        config.concurrent_max_attempts,
    );
    let exhausted_action_label_value = exhausted_action_label(config.exhausted_action);

    if hit_number <= config.immediate_budget && !wave.exhausted {
        return CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::RetrySameProvider,
            hit_number,
            phase: "immediate",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget: config.immediate_budget,
            delayed_budget: config.delayed_budget,
            total_budget,
            remaining_budget: total_budget.saturating_sub(hit_number),
            exhausted_action: exhausted_action_label_value,
            action_taken: "retry_same_provider_no_circuit",
        };
    }

    if hit_number <= total_budget && !wave.exhausted {
        return CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::RetrySameProvider,
            hit_number,
            phase: "delayed",
            delay_ms: config.delayed_retry_ms,
            retry_wave: Some(wave),
            immediate_budget: config.immediate_budget,
            delayed_budget: config.delayed_budget,
            total_budget,
            remaining_budget: total_budget.saturating_sub(hit_number),
            exhausted_action: exhausted_action_label_value,
            action_taken: "retry_same_provider_delayed_no_circuit",
        };
    }

    match config.exhausted_action {
        CodexReasoningGuardExhaustedAction::ReturnError => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::ReturnError,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget: config.immediate_budget,
            delayed_budget: config.delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label_value,
            action_taken: "return_guard_error_no_circuit",
        },
        CodexReasoningGuardExhaustedAction::SwitchProvider => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::SwitchProvider,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget: config.immediate_budget,
            delayed_budget: config.delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label_value,
            action_taken: "switch_provider_no_circuit",
        },
        CodexReasoningGuardExhaustedAction::SwitchModel => CodexReasoningGuardBudgetDecision {
            action: CodexReasoningGuardBudgetAction::SwitchModel,
            hit_number,
            phase: "exhausted",
            delay_ms: 0,
            retry_wave: Some(wave),
            immediate_budget: config.immediate_budget,
            delayed_budget: config.delayed_budget,
            total_budget,
            remaining_budget: 0,
            exhausted_action: exhausted_action_label_value,
            action_taken: "switch_model_no_circuit",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CodexReasoningGuardRetryWave {
    pub(super) policy: CodexReasoningGuardRetryPolicy,
    pub(super) hit_number: u32,
    pub(super) concurrency: u32,
    pub(super) interval_ms: u32,
    pub(super) max_attempts: u32,
    pub(super) exhausted: bool,
}

pub(super) fn retry_wave(
    current_hits: u32,
    policy: CodexReasoningGuardRetryPolicy,
    concurrent_max: u32,
    concurrent_interval_ms: u32,
    concurrent_max_attempts: u32,
) -> CodexReasoningGuardRetryWave {
    let hit_number = current_hits.saturating_add(1);
    let max_attempts = concurrent_max_attempts;
    let exhausted = max_attempts > 0 && hit_number > max_attempts;
    let concurrency = match policy {
        CodexReasoningGuardRetryPolicy::Single => 1,
        CodexReasoningGuardRetryPolicy::Concurrent => hit_number.min(concurrent_max.max(1)).max(1),
    };

    CodexReasoningGuardRetryWave {
        policy,
        hit_number,
        concurrency,
        interval_ms: concurrent_interval_ms,
        max_attempts,
        exhausted,
    }
}

pub(super) fn select_next_model_fallback<'a>(
    current_model: Option<&str>,
    fallbacks: &'a [String],
) -> Option<&'a str> {
    let current = current_model
        .map(str::trim)
        .filter(|model| !model.is_empty());
    fallbacks
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .find(|model| Some(*model) != current)
}

pub(super) async fn apply_delay_if_needed(decision: CodexReasoningGuardBudgetDecision) {
    if decision.delay_ms == 0 {
        return;
    }
    tokio::time::sleep(Duration::from_millis(decision.delay_ms as u64)).await;
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
    budget: CodexReasoningGuardBudgetDecision,
) {
    let (outcome, decision) = match budget.action {
        CodexReasoningGuardBudgetAction::RetrySameProvider => {
            ("codex_reasoning_guard_retry", "retry_same_provider")
        }
        CodexReasoningGuardBudgetAction::ReturnError => {
            ("codex_reasoning_guard_exhausted", "abort")
        }
        CodexReasoningGuardBudgetAction::SwitchProvider => {
            ("codex_reasoning_guard_switch_provider", "switch")
        }
        CodexReasoningGuardBudgetAction::SwitchModel => {
            ("codex_reasoning_guard_switch_model", "retry_same_provider")
        }
    };
    attempts.push(FailoverAttempt {
        provider_id,
        provider_name: provider_name.to_string(),
        base_url: base_url.to_string(),
        outcome: outcome.to_string(),
        status: Some(StatusCode::BAD_GATEWAY.as_u16()),
        provider_index: Some(provider_index),
        retry_index: Some(retry_index),
        session_reuse,
        error_category: Some(ErrorCategory::SystemError.as_str()),
        error_code: Some(CODEX_REASONING_GUARD_ERROR_CODE),
        decision: Some(decision),
        reason: Some(matched.reason_summary(budget)),
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

    fn budget_decision_for_test(
        current_hits: u32,
        immediate_budget: u32,
        delayed_budget: u32,
        delayed_retry_ms: u32,
        exhausted_action: CodexReasoningGuardExhaustedAction,
    ) -> CodexReasoningGuardBudgetDecision {
        budget_decision(
            current_hits,
            CodexReasoningGuardBudgetConfig {
                immediate_budget,
                delayed_budget,
                delayed_retry_ms,
                exhausted_action,
                retry_policy: CodexReasoningGuardRetryPolicy::Single,
                concurrent_max: 5,
                concurrent_interval_ms: 1_000,
                concurrent_max_attempts: 10,
            },
        )
    }

    fn template_filter(
        id: &str,
        field: CodexReasoningGuardTemplateFilterField,
        operator: CodexReasoningGuardTemplateFilterOperator,
        number_value: Option<f64>,
        bool_value: Option<bool>,
        string_value: Option<&str>,
        string_values: &[&str],
    ) -> CodexReasoningGuardTemplateFilter {
        CodexReasoningGuardTemplateFilter {
            id: id.to_string(),
            field,
            operator,
            number_value,
            bool_value,
            string_value: string_value.map(ToOwned::to_owned),
            string_values: string_values
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }

    fn template_rule(
        id: &str,
        reasoning_tokens: Option<i64>,
        action: CodexReasoningGuardTemplateRuleAction,
        filters: Vec<CodexReasoningGuardTemplateFilter>,
    ) -> CodexReasoningGuardTemplateRule {
        CodexReasoningGuardTemplateRule {
            id: id.to_string(),
            name: id.to_string(),
            reasoning_tokens,
            reasoning_tokens_formula: None,
            action,
            logic: CodexReasoningGuardTemplateRuleLogic::And,
            filters,
        }
    }

    fn custom_template(
        rules: Vec<CodexReasoningGuardTemplateRule>,
    ) -> CodexReasoningGuardRuleTemplate {
        CodexReasoningGuardRuleTemplate {
            id: "custom-template".to_string(),
            name: "Custom template".to_string(),
            description: String::new(),
            rules,
        }
    }

    fn evaluate_decision_for_test<'a>(
        value: &'a serde_json::Value,
        active_template_id: &'a str,
        custom_templates: &'a [CodexReasoningGuardRuleTemplate],
        feature_sample: Option<&'a CodexReasoningFeatureSample>,
        duration_ms: Option<u128>,
        ttfb_ms: Option<u128>,
    ) -> Option<CodexReasoningGuardEvaluationDecision> {
        evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample,
            },
            active_template_id,
            custom_templates,
            duration_ms,
            ttfb_ms,
        })
    }

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

        assert_eq!(
            matched.rule_mode,
            CodexReasoningGuardRuleMode::ReasoningTokens
        );
        assert_eq!(
            matched.hit_source,
            CodexReasoningGuardHitSource::ReasoningTokens
        );
        assert_eq!(matched.reasoning_tokens, Some(516));
        assert_eq!(matched.matched_rule_value, Some(516));
        assert_eq!(
            matched.compare_mode,
            Some(CodexReasoningGuardCompareMode::Equals)
        );
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
    }

    #[test]
    fn budget_decision_uses_immediate_then_delayed_budget() {
        for current_hits in 0..5 {
            let decision = budget_decision_for_test(
                current_hits,
                5,
                5,
                1_000,
                CodexReasoningGuardExhaustedAction::ReturnError,
            );
            assert_eq!(
                decision.action,
                CodexReasoningGuardBudgetAction::RetrySameProvider
            );
            assert_eq!(decision.phase, "immediate");
            assert_eq!(decision.delay_ms, 0);
            assert_eq!(decision.remaining_budget, 9 - current_hits);
        }

        for current_hits in 5..10 {
            let decision = budget_decision_for_test(
                current_hits,
                5,
                5,
                1_000,
                CodexReasoningGuardExhaustedAction::ReturnError,
            );
            assert_eq!(
                decision.action,
                CodexReasoningGuardBudgetAction::RetrySameProvider
            );
            assert_eq!(decision.phase, "delayed");
            assert_eq!(decision.delay_ms, 1_000);
            assert_eq!(decision.remaining_budget, 9 - current_hits);
        }
    }

    #[test]
    fn budget_decision_exhausts_to_configured_terminal_action() {
        let return_error = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            return_error.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );
        assert_eq!(return_error.phase, "exhausted");
        assert_eq!(return_error.remaining_budget, 0);

        let switch_provider = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::SwitchProvider,
        );
        assert_eq!(
            switch_provider.action,
            CodexReasoningGuardBudgetAction::SwitchProvider
        );
        assert_eq!(switch_provider.exhausted_action, "switch_provider");

        let switch_model = budget_decision_for_test(
            10,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::SwitchModel,
        );
        assert_eq!(
            switch_model.action,
            CodexReasoningGuardBudgetAction::SwitchModel
        );
        assert_eq!(switch_model.exhausted_action, "switch_model");
    }

    #[test]
    fn retry_wave_escalates_concurrent_policy_and_caps_at_max() {
        let first = retry_wave(0, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(first.concurrency, 1);
        assert!(!first.exhausted);

        let second = retry_wave(1, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(second.concurrency, 2);

        let capped = retry_wave(8, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(capped.concurrency, 5);
        assert!(!capped.exhausted);

        let exhausted = retry_wave(10, CodexReasoningGuardRetryPolicy::Concurrent, 5, 1_000, 10);
        assert_eq!(exhausted.concurrency, 5);
        assert!(exhausted.exhausted);
    }

    #[test]
    fn retry_wave_single_policy_never_escalates_concurrency() {
        let wave = retry_wave(4, CodexReasoningGuardRetryPolicy::Single, 5, 1_000, 10);
        assert_eq!(wave.concurrency, 1);
        assert_eq!(wave.hit_number, 5);
    }

    #[test]
    fn budget_decision_exhausts_when_concurrent_max_attempts_is_reached() {
        let decision = budget_decision(
            3,
            CodexReasoningGuardBudgetConfig {
                immediate_budget: 10,
                delayed_budget: 10,
                delayed_retry_ms: 1_000,
                exhausted_action: CodexReasoningGuardExhaustedAction::SwitchProvider,
                retry_policy: CodexReasoningGuardRetryPolicy::Concurrent,
                concurrent_max: 5,
                concurrent_interval_ms: 1_000,
                concurrent_max_attempts: 3,
            },
        );

        assert_eq!(
            decision.action,
            CodexReasoningGuardBudgetAction::SwitchProvider
        );
        assert_eq!(decision.phase, "exhausted");
        assert_eq!(decision.retry_wave.expect("retry wave").hit_number, 4);
        assert!(decision.retry_wave.expect("retry wave").exhausted);
    }

    #[test]
    fn select_next_model_fallback_skips_blank_and_current_model() {
        let fallbacks = vec![
            " ".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.4".to_string(),
        ];
        assert_eq!(
            select_next_model_fallback(Some("gpt-5.5"), &fallbacks),
            Some("gpt-5.4")
        );
        assert_eq!(
            select_next_model_fallback(Some("gpt-5.3"), &fallbacks),
            Some("gpt-5.5")
        );
    }

    #[test]
    fn budget_decision_supports_zero_budget_edges() {
        let delayed_first = budget_decision_for_test(
            0,
            0,
            1,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            delayed_first.action,
            CodexReasoningGuardBudgetAction::RetrySameProvider
        );
        assert_eq!(delayed_first.phase, "delayed");
        assert_eq!(delayed_first.delay_ms, 500);

        let exhausted_first = budget_decision_for_test(
            0,
            0,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            exhausted_first.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );

        let immediate_only = budget_decision_for_test(
            1,
            2,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            immediate_only.action,
            CodexReasoningGuardBudgetAction::RetrySameProvider
        );
        assert_eq!(immediate_only.phase, "immediate");
        assert_eq!(immediate_only.remaining_budget, 0);

        let exhausted_after_immediate = budget_decision_for_test(
            2,
            2,
            0,
            500,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        assert_eq!(
            exhausted_after_immediate.action,
            CodexReasoningGuardBudgetAction::ReturnError
        );
        assert_eq!(exhausted_after_immediate.phase, "exhausted");
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

        assert_eq!(matched.reasoning_tokens, Some(300));
        assert_eq!(matched.matched_rule_value, Some(516));
        assert_eq!(
            matched.compare_mode,
            Some(CodexReasoningGuardCompareMode::LessThanOrEqual)
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

        assert_eq!(matched.matched_rule_value, Some(516));
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

        assert_eq!(matched.matched_rule_value, Some(700));
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

        assert_eq!(matched.matched_rule_value, Some(516));
        assert_eq!(
            matched.rule_source,
            CODEX_REASONING_GUARD_RULE_SOURCE_GLOBAL_DEFAULT
        );
        assert!(matched.rule_model.is_none());
    }

    #[test]
    fn evaluate_decision_default_legacy_template_matches_default_tokens() {
        for token in [516, 1034, 1552] {
            let value = serde_json::json!({
                "usage": { "output_tokens_details": { "reasoning_tokens": token } }
            });

            let decision = evaluate_decision_for_test(
                &value,
                crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
                &[],
                None,
                Some(1_000),
                Some(100),
            )
            .expect("default token should match");

            assert_eq!(
                decision.action,
                CodexReasoningGuardTemplateRuleAction::Intercept
            );
            assert_eq!(decision.matched.reasoning_tokens, Some(token));
            assert_eq!(
                decision.matched.template_id.as_deref(),
                Some(crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID)
            );
        }
    }

    #[test]
    fn evaluate_decision_legacy_template_ignores_saved_equals_tokens() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 777 } }
        });

        let decision = evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                value: &value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample: None,
            },
            active_template_id:
                crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            custom_templates: &[],
            duration_ms: None,
            ttfb_ms: None,
        });

        assert!(
            decision.is_none(),
            "saved legacy equals tokens should not act as hidden runtime matchers"
        );

        let default_value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });
        let default_decision = evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                value: &default_value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample: None,
            },
            active_template_id:
                crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            custom_templates: &[],
            duration_ms: None,
            ttfb_ms: None,
        })
        .expect("builtin legacy template should still match default tokens");
        assert_eq!(default_decision.matched.reasoning_tokens, Some(516));
        assert_eq!(default_decision.matched.rule_token, Some(516));
    }

    #[test]
    fn guard_special_settings_include_summary_reason_for_hits_and_decisions() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });
        let hit = evaluate_decision_for_test(
            &value,
            crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            &[],
            None,
            None,
            None,
        )
        .expect("legacy token should match");
        let special_settings = Arc::new(Mutex::new(Vec::new()));
        let budget = budget_decision_for_test(
            0,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        push_special_setting(&special_settings, 1, "Provider", 0, &hit.matched, budget);
        let settings = special_settings.lock().expect("settings lock").clone();
        let reason = settings[0]
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .expect("hit special setting should include reason");
        assert!(reason.contains("template=builtin-legacy-reasoning-tokens"));
        assert!(reason.contains("rule=builtin-token-516-0"));
        assert!(reason.contains("rule_action=Intercept"));

        let template = custom_template(vec![template_rule(
            "allow-516",
            Some(516),
            CodexReasoningGuardTemplateRuleAction::NoIntercept,
            Vec::new(),
        )]);
        let decision =
            evaluate_decision_for_test(&value, "custom-template", &[template], None, None, None)
                .expect("no_intercept rule should produce a decision");
        let special_settings = Arc::new(Mutex::new(Vec::new()));
        push_decision_special_setting(&special_settings, 1, "Provider", 0, &decision.matched);
        let settings = special_settings.lock().expect("settings lock").clone();
        let reason = settings[0]
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .expect("decision special setting should include reason");
        assert!(reason.contains("template=custom-template"));
        assert!(reason.contains("rule=allow-516"));
        assert!(reason.contains("rule_action=NoIntercept"));
        assert!(reason.contains("action=allow"));
    }

    #[test]
    fn wildcard_threshold_template_reports_matched_rule_value_in_special_settings() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });
        let mut threshold_rule = template_rule(
            "legacy-threshold",
            None,
            CodexReasoningGuardTemplateRuleAction::Intercept,
            vec![
                template_filter(
                    "reasoning-tokens-lte-700",
                    CodexReasoningGuardTemplateFilterField::ReasoningTokens,
                    CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual,
                    Some(700.0),
                    None,
                    None,
                    &[],
                ),
                template_filter(
                    "reasoning-tokens-lte-900",
                    CodexReasoningGuardTemplateFilterField::ReasoningTokens,
                    CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual,
                    Some(900.0),
                    None,
                    None,
                    &[],
                ),
            ],
        );
        threshold_rule.logic = CodexReasoningGuardTemplateRuleLogic::Or;
        let template = custom_template(vec![threshold_rule]);
        let decision =
            evaluate_decision_for_test(&value, "custom-template", &[template], None, None, None)
                .expect("threshold template should match");

        assert_eq!(
            decision.matched.compare_mode,
            Some(CodexReasoningGuardCompareMode::LessThanOrEqual)
        );
        assert_eq!(decision.matched.matched_rule_value, Some(700));

        let special_settings = Arc::new(Mutex::new(Vec::new()));
        let budget = budget_decision_for_test(
            0,
            5,
            5,
            1_000,
            CodexReasoningGuardExhaustedAction::ReturnError,
        );
        push_special_setting(
            &special_settings,
            1,
            "Provider",
            0,
            &decision.matched,
            budget,
        );
        let settings = special_settings.lock().expect("settings lock").clone();
        assert_eq!(
            settings[0].get("compareMode"),
            Some(&serde_json::json!("less_than_or_equal"))
        );
        assert_eq!(
            settings[0].get("matchedRuleValue"),
            Some(&serde_json::json!(700))
        );
    }

    #[test]
    fn evaluate_decision_composite_token_rule_uses_duration_and_tps() {
        let template = custom_template(vec![template_rule(
            "fast-low-rate-516",
            Some(516),
            CodexReasoningGuardTemplateRuleAction::Intercept,
            vec![
                template_filter(
                    "duration-under-30s",
                    CodexReasoningGuardTemplateFilterField::DurationMs,
                    CodexReasoningGuardTemplateFilterOperator::LessThan,
                    Some(30_000.0),
                    None,
                    None,
                    &[],
                ),
                template_filter(
                    "tps-under-60",
                    CodexReasoningGuardTemplateFilterField::Tps,
                    CodexReasoningGuardTemplateFilterOperator::LessThan,
                    Some(60.0),
                    None,
                    None,
                    &[],
                ),
            ],
        )]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": {
                "output_tokens": 100,
                "output_tokens_details": { "reasoning_tokens": 516 }
            }
        });

        let hit = evaluate_decision_for_test(
            &value,
            "custom-template",
            &templates,
            None,
            Some(3_000),
            Some(1_000),
        )
        .expect("100 tokens over generation window 2s should be 50 tps");
        assert_eq!(hit.matched.rule_id.as_deref(), Some("fast-low-rate-516"));
        assert_eq!(
            hit.matched.matched_filter_ids,
            vec!["duration-under-30s".to_string(), "tps-under-60".to_string()]
        );

        let miss = evaluate_decision_for_test(
            &value,
            "custom-template",
            &templates,
            None,
            Some(40_000),
            Some(1_000),
        );
        assert!(miss.is_none());
    }

    #[test]
    fn evaluate_decision_exact_filter_miss_falls_through_to_wildcard() {
        let template = custom_template(vec![
            template_rule(
                "strict-516",
                Some(516),
                CodexReasoningGuardTemplateRuleAction::Intercept,
                vec![template_filter(
                    "duration-under-1s",
                    CodexReasoningGuardTemplateFilterField::DurationMs,
                    CodexReasoningGuardTemplateFilterOperator::LessThan,
                    Some(1_000.0),
                    None,
                    None,
                    &[],
                )],
            ),
            template_rule(
                "wildcard",
                None,
                CodexReasoningGuardTemplateRuleAction::Intercept,
                Vec::new(),
            ),
        ]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let decision = evaluate_decision_for_test(
            &value,
            "custom-template",
            &templates,
            None,
            Some(2_000),
            None,
        )
        .expect("wildcard should match after exact filter miss");

        assert_eq!(decision.matched.rule_id.as_deref(), Some("wildcard"));
        assert_eq!(decision.matched.rule_token, None);
    }

    #[test]
    fn evaluate_decision_exact_no_intercept_stops_before_wildcard() {
        let template = custom_template(vec![
            template_rule(
                "allow-516",
                Some(516),
                CodexReasoningGuardTemplateRuleAction::NoIntercept,
                Vec::new(),
            ),
            template_rule(
                "wildcard-intercept",
                None,
                CodexReasoningGuardTemplateRuleAction::Intercept,
                Vec::new(),
            ),
        ]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let decision =
            evaluate_decision_for_test(&value, "custom-template", &templates, None, None, None)
                .expect("no_intercept rule should still produce a decision");

        assert_eq!(
            decision.action,
            CodexReasoningGuardTemplateRuleAction::NoIntercept
        );
        assert_eq!(decision.matched.rule_id.as_deref(), Some("allow-516"));
    }

    #[test]
    fn evaluate_decision_missing_filter_field_fails_filter() {
        let template = custom_template(vec![template_rule(
            "needs-duration",
            Some(516),
            CodexReasoningGuardTemplateRuleAction::Intercept,
            vec![template_filter(
                "duration-under-30s",
                CodexReasoningGuardTemplateFilterField::DurationMs,
                CodexReasoningGuardTemplateFilterOperator::LessThan,
                Some(30_000.0),
                None,
                None,
                &[],
            )],
        )]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let decision =
            evaluate_decision_for_test(&value, "custom-template", &templates, None, None, None);

        assert!(decision.is_none());
    }

    #[test]
    fn evaluate_decision_missing_requested_model_matches_negative_filter() {
        let template = custom_template(vec![template_rule(
            "global-fallback",
            Some(516),
            CodexReasoningGuardTemplateRuleAction::Intercept,
            vec![template_filter(
                "not-model-specific",
                CodexReasoningGuardTemplateFilterField::RequestedModel,
                CodexReasoningGuardTemplateFilterOperator::NotIn,
                None,
                None,
                None,
                &["gpt-model-specific"],
            )],
        )]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 516 } }
        });

        let decision = evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: None,
                value: &value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample: None,
            },
            active_template_id: "custom-template",
            custom_templates: &templates,
            duration_ms: None,
            ttfb_ms: None,
        })
        .expect("negative requested-model filter should cover missing model fallback");

        assert_eq!(decision.matched.rule_id.as_deref(), Some("global-fallback"));
        assert_eq!(
            decision.matched.matched_filter_ids,
            vec!["not-model-specific".to_string()]
        );
    }

    #[test]
    fn evaluate_decision_no_wildcard_falls_back_to_no_intercept() {
        let template = custom_template(vec![template_rule(
            "token-516",
            Some(516),
            CodexReasoningGuardTemplateRuleAction::Intercept,
            Vec::new(),
        )]);
        let templates = vec![template];
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 777 } }
        });

        let decision =
            evaluate_decision_for_test(&value, "custom-template", &templates, None, None, None);

        assert!(decision.is_none());
    }

    #[test]
    fn evaluate_decision_ignores_legacy_less_than_or_equal_fallback() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 300 } }
        });

        let decision = evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                value: &value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample: None,
            },
            active_template_id:
                crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            custom_templates: &[],
            duration_ms: None,
            ttfb_ms: None,
        });

        assert!(
            decision.is_none(),
            "legacy <= fallback should not act as a hidden runtime matcher"
        );
    }

    #[test]
    fn evaluate_decision_ignores_legacy_model_rules() {
        let value = serde_json::json!({
            "usage": { "output_tokens_details": { "reasoning_tokens": 256 } }
        });
        let decision = evaluate_decision(CodexReasoningGuardDecisionEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5.5"),
                value: &value,
                rule_mode: CodexReasoningGuardRuleMode::ReasoningTokens,
                feature_sample: None,
            },
            active_template_id:
                crate::settings::CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            custom_templates: &[],
            duration_ms: None,
            ttfb_ms: None,
        });

        assert!(
            decision.is_none(),
            "legacy model rules should not act as hidden runtime matchers"
        );
    }

    #[test]
    fn evaluate_feature_mode_matches_final_answer_only_high_xhigh() {
        let response = serde_json::json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "redacted"}]
            }]
        });
        let settings = vec![serde_json::json!({
            "type": "codex_reasoning_effort",
            "effort": "high",
            "rawEffort": "high",
            "pointer": "/reasoning/effort"
        })];
        let sample = super::super::codex_reasoning_features::build_complete_sample(
            "codex",
            CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            None,
            None,
            &settings,
            &response,
        )
        .expect("feature sample");

        let matched = evaluate(CodexReasoningGuardLegacyEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5-codex"),
                value: &response,
                rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
                feature_sample: Some(&sample),
            },
            fallback_compare_mode: CodexReasoningGuardCompareMode::Equals,
            fallback_values: &[516],
            model_rules: &[],
        })
        .expect("feature match");

        assert_eq!(
            matched.hit_source,
            CodexReasoningGuardHitSource::FinalAnswerOnlyHighXhigh
        );
        assert_eq!(matched.request_reasoning_effort.as_deref(), Some("high"));
        assert_eq!(matched.final_answer_only, Some(true));
    }

    #[test]
    fn evaluate_feature_mode_observes_ordinary_zero_reasoning_final_answer_only() {
        let response = serde_json::json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "redacted"}]
            }],
            "usage": {"output_tokens_details": {"reasoning_tokens": 0}}
        });
        let settings = vec![serde_json::json!({
            "type": "codex_reasoning_effort",
            "effort": "high",
            "rawEffort": "high",
            "pointer": "/reasoning/effort"
        })];
        let sample = super::super::codex_reasoning_features::build_complete_sample(
            "codex",
            CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            None,
            None,
            &settings,
            &response,
        )
        .expect("feature sample");

        assert_eq!(sample.intercept_exempt_reason, None);
        assert_eq!(sample.reasoning_tokens, Some(0));
        assert_eq!(sample.final_answer_only, Some(true));

        let matched = evaluate(CodexReasoningGuardLegacyEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5-codex"),
                value: &response,
                rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
                feature_sample: Some(&sample),
            },
            fallback_compare_mode: CodexReasoningGuardCompareMode::Equals,
            fallback_values: &[516],
            model_rules: &[],
        });

        assert!(matched.is_none());
    }

    #[test]
    fn evaluate_feature_mode_respects_compaction_exemption() {
        let response = serde_json::json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "redacted"}]
            }],
            "usage": {"output_tokens_details": {"reasoning_tokens": 0}}
        });
        let settings = vec![serde_json::json!({
            "type": "codex_reasoning_effort",
            "effort": "xhigh",
            "rawEffort": "xhigh",
            "pointer": "/reasoning_effort"
        })];
        let request = serde_json::json!({"request_kind":"context_compaction"});
        let sample = super::super::codex_reasoning_features::build_complete_sample(
            "codex",
            CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            None,
            Some(&request),
            &settings,
            &response,
        )
        .expect("feature sample");

        assert_eq!(
            sample.intercept_exempt_reason,
            Some(super::super::codex_reasoning_features::EXEMPT_REASON_CONTEXT_COMPACTION)
        );

        let matched = evaluate(CodexReasoningGuardLegacyEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5-codex"),
                value: &response,
                rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
                feature_sample: Some(&sample),
            },
            fallback_compare_mode: CodexReasoningGuardCompareMode::Equals,
            fallback_values: &[516],
            model_rules: &[],
        });

        assert!(matched.is_none());
    }

    #[test]
    fn evaluate_feature_mode_intercepts_compaction_with_nonzero_reasoning() {
        let response = serde_json::json!({
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "redacted"}]
            }],
            "usage": {"output_tokens_details": {"reasoning_tokens": 516}}
        });
        let settings = vec![serde_json::json!({
            "type": "codex_reasoning_effort",
            "effort": "xhigh",
            "rawEffort": "xhigh",
            "pointer": "/reasoning_effort"
        })];
        let request = serde_json::json!({"request_kind":"context_compaction"});
        let sample = super::super::codex_reasoning_features::build_complete_sample(
            "codex",
            CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            None,
            Some(&request),
            &settings,
            &response,
        )
        .expect("feature sample");

        // reasoning_tokens == 516 is not exempt, so the sample carries no exempt reason.
        assert_eq!(sample.intercept_exempt_reason, None);

        let matched = evaluate(CodexReasoningGuardLegacyEvaluationInput {
            base: CodexReasoningGuardEvaluationInput {
                cli_key: "codex",
                requested_model: Some("gpt-5-codex"),
                value: &response,
                rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
                feature_sample: Some(&sample),
            },
            fallback_compare_mode: CodexReasoningGuardCompareMode::Equals,
            fallback_values: &[516],
            model_rules: &[],
        })
        .expect("feature match despite compaction request kind");

        assert_eq!(
            matched.hit_source,
            CodexReasoningGuardHitSource::FinalAnswerOnlyHighXhigh
        );
    }
}
