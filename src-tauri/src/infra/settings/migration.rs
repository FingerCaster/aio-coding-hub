//! Usage: Schema migrations and input sanitization for settings upgrades.

use super::defaults::*;
use super::types::{
    AppSettings, CodexHomeMode, CodexReasoningGuardCompareMode, CodexReasoningGuardExhaustedAction,
    CodexReasoningGuardModelRule, CodexReasoningGuardPostMatchStrategy,
    CodexReasoningGuardRuleMode, CodexReasoningGuardRuleTemplate,
    CodexReasoningGuardTemplateFilter, CodexReasoningGuardTemplateFilterField,
    CodexReasoningGuardTemplateFilterOperator, CodexReasoningGuardTemplateRule,
    CodexReasoningGuardTemplateRuleAction, CodexReasoningGuardTemplateRuleLogic,
    UpstreamRetryPolicy,
};
use crate::shared::error::AppResult;
use std::collections::HashSet;

pub(super) fn normalize_cli_priority_order(input: &[String]) -> Vec<String> {
    let mut order = Vec::with_capacity(crate::shared::cli_key::SUPPORTED_CLI_KEYS.len());

    for cli_key in input {
        if !crate::shared::cli_key::is_supported_cli_key(cli_key) {
            continue;
        }
        if order.iter().any(|item| item == cli_key) {
            continue;
        }
        order.push(cli_key.clone());
    }

    for cli_key in crate::shared::cli_key::SUPPORTED_CLI_KEYS {
        if order.iter().any(|item| item == cli_key) {
            continue;
        }
        order.push(cli_key.to_string());
    }

    order
}

pub(super) fn normalize_codex_home_override(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.eq_ignore_ascii_case("config.toml") {
        return String::new();
    }

    for suffix in ["/config.toml", "\\config.toml"] {
        if trimmed.len() > suffix.len()
            && trimmed[trimmed.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            return trimmed[..trimmed.len() - suffix.len()]
                .trim_end_matches(['/', '\\'])
                .to_string();
        }
    }

    trimmed.to_string()
}

pub(super) fn sanitize_codex_home_override(settings: &mut AppSettings) -> bool {
    let normalized = normalize_codex_home_override(&settings.codex_home_override);
    let mut changed = settings.codex_home_override != normalized;
    settings.codex_home_override = normalized;

    if settings.codex_home_mode != CodexHomeMode::Custom && !settings.codex_home_override.is_empty()
    {
        settings.codex_home_override.clear();
        changed = true;
    }

    if settings.codex_home_mode == CodexHomeMode::Custom && settings.codex_home_override.is_empty()
    {
        settings.codex_home_mode = CodexHomeMode::UserHomeDefault;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_cli_priority_order(settings: &mut AppSettings) -> bool {
    let normalized = normalize_cli_priority_order(&settings.cli_priority_order);
    let changed = settings.cli_priority_order != normalized;
    settings.cli_priority_order = normalized;
    changed
}

pub(super) fn sanitize_codex_provider_test_model(settings: &mut AppSettings) -> bool {
    let normalized = settings.codex_provider_test_model.trim();
    let next = if normalized.is_empty() {
        DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string()
    } else {
        normalized.to_string()
    };
    let changed = settings.codex_provider_test_model != next;
    settings.codex_provider_test_model = next;
    changed
}

pub(super) fn sanitize_codex_reasoning_guard_hit_label(settings: &mut AppSettings) -> bool {
    let normalized = settings.codex_reasoning_guard_hit_label.trim();
    let next = if normalized.is_empty() {
        DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL.to_string()
    } else {
        normalized.to_string()
    };
    let changed = settings.codex_reasoning_guard_hit_label != next;
    settings.codex_reasoning_guard_hit_label = next;
    changed
}

pub(super) fn sanitize_codex_reasoning_guard_model_rules(settings: &mut AppSettings) -> bool {
    let mut changed = false;
    let mut seen_models = HashSet::new();
    let mut normalized = Vec::with_capacity(settings.codex_reasoning_guard_model_rules.len());

    for rule in &settings.codex_reasoning_guard_model_rules {
        let requested_model = rule.requested_model.trim().to_string();
        if requested_model.is_empty() {
            changed = true;
            continue;
        }
        if !seen_models.insert(requested_model.clone()) {
            changed = true;
            continue;
        }

        let normalized_rule = CodexReasoningGuardModelRule {
            requested_model,
            compare_mode: rule.compare_mode,
            reasoning_equals: if rule.reasoning_equals.is_empty() {
                changed = true;
                DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.to_vec()
            } else {
                rule.reasoning_equals.clone()
            },
        };
        if &normalized_rule != rule {
            changed = true;
        }
        normalized.push(normalized_rule);
    }

    if normalized.len() > MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN {
        normalized.truncate(MAX_CODEX_REASONING_GUARD_MODEL_RULES_LEN);
        changed = true;
    }

    if settings.codex_reasoning_guard_model_rules != normalized {
        settings.codex_reasoning_guard_model_rules = normalized;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_codex_reasoning_guard_runtime_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    let active_template_id = settings
        .codex_reasoning_guard_active_template_id
        .trim()
        .to_string();
    let active_template_id = if active_template_id.is_empty() {
        CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID.to_string()
    } else {
        active_template_id
    };
    if settings.codex_reasoning_guard_active_template_id != active_template_id {
        settings.codex_reasoning_guard_active_template_id = active_template_id;
        changed = true;
    }

    if settings.codex_reasoning_guard_concurrent_max == 0 {
        settings.codex_reasoning_guard_concurrent_max =
            DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX;
        changed = true;
    }
    if settings.codex_reasoning_guard_concurrent_max > MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX {
        settings.codex_reasoning_guard_concurrent_max = MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX;
        changed = true;
    }
    if settings.codex_reasoning_guard_concurrent_interval_ms
        > MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS
    {
        settings.codex_reasoning_guard_concurrent_interval_ms =
            MAX_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS;
        changed = true;
    }
    if settings.codex_reasoning_guard_concurrent_max_attempts
        > MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS
    {
        settings.codex_reasoning_guard_concurrent_max_attempts =
            MAX_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS;
        changed = true;
    }
    if settings.codex_reasoning_guard_continuation_max_rounds == 0 {
        settings.codex_reasoning_guard_continuation_max_rounds =
            DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS;
        changed = true;
    }
    if settings.codex_reasoning_guard_continuation_max_rounds
        > MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS
    {
        settings.codex_reasoning_guard_continuation_max_rounds =
            MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS;
        changed = true;
    }
    if settings.codex_reasoning_guard_continuation_max_output_tokens
        > MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
    {
        settings.codex_reasoning_guard_continuation_max_output_tokens =
            MAX_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS;
        changed = true;
    }

    let mut seen_models = HashSet::new();
    let mut normalized = Vec::with_capacity(settings.codex_reasoning_guard_model_fallbacks.len());
    for model in &settings.codex_reasoning_guard_model_fallbacks {
        let model = model.trim().to_string();
        if model.is_empty() {
            changed = true;
            continue;
        }
        if !seen_models.insert(model.clone()) {
            changed = true;
            continue;
        }
        normalized.push(model);
    }
    if normalized.len() > MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN {
        normalized.truncate(MAX_CODEX_REASONING_GUARD_MODEL_FALLBACKS_LEN);
        changed = true;
    }
    if settings.codex_reasoning_guard_model_fallbacks != normalized {
        settings.codex_reasoning_guard_model_fallbacks = normalized;
        changed = true;
    }

    changed
}

fn legacy_equals_custom_template(values: &[i64]) -> CodexReasoningGuardRuleTemplate {
    let mut seen_tokens = HashSet::new();
    CodexReasoningGuardRuleTemplate {
        id: "custom-legacy-reasoning-tokens".to_string(),
        name: "Legacy custom reasoning tokens".to_string(),
        description: "Migrated from the legacy global equals token list.".to_string(),
        rules: values
            .iter()
            .copied()
            .filter(|value| seen_tokens.insert(*value))
            .enumerate()
            .map(|(index, value)| CodexReasoningGuardTemplateRule {
                id: format!("legacy-token-{value}-{index}"),
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

fn legacy_reasoning_token_values(values: &[i64]) -> Vec<i64> {
    let source = if values.is_empty() {
        DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
    } else {
        values
    };
    let mut seen_tokens = HashSet::new();
    source
        .iter()
        .copied()
        .filter(|value| seen_tokens.insert(*value))
        .collect()
}

fn legacy_less_than_or_equal_thresholds(values: &[i64]) -> Vec<i64> {
    let mut thresholds = legacy_reasoning_token_values(values);
    thresholds.sort_unstable();
    thresholds
}

fn legacy_number_filter(
    id: String,
    operator: CodexReasoningGuardTemplateFilterOperator,
    value: i64,
) -> CodexReasoningGuardTemplateFilter {
    CodexReasoningGuardTemplateFilter {
        id,
        field: CodexReasoningGuardTemplateFilterField::ReasoningTokens,
        operator,
        number_value: Some(value as f64),
        bool_value: None,
        string_value: None,
        string_values: Vec::new(),
    }
}

fn legacy_requested_model_filter(model: &str) -> CodexReasoningGuardTemplateFilter {
    CodexReasoningGuardTemplateFilter {
        id: format!("requested-model-{}", sanitize_template_id_part(model)),
        field: CodexReasoningGuardTemplateFilterField::RequestedModel,
        operator: CodexReasoningGuardTemplateFilterOperator::Equals,
        number_value: None,
        bool_value: None,
        string_value: Some(model.to_string()),
        string_values: Vec::new(),
    }
}

fn legacy_requested_model_filter_for_models(
    models: Vec<String>,
) -> CodexReasoningGuardTemplateFilter {
    if models.len() == 1 {
        return legacy_requested_model_filter(&models[0]);
    }

    let id_suffix = sanitize_template_id_parts(&models);
    CodexReasoningGuardTemplateFilter {
        id: format!("requested-models-{id_suffix}"),
        field: CodexReasoningGuardTemplateFilterField::RequestedModel,
        operator: CodexReasoningGuardTemplateFilterOperator::In,
        number_value: None,
        bool_value: None,
        string_value: None,
        string_values: models,
    }
}

fn legacy_requested_model_exclusion_filter_for_models(
    models: Vec<String>,
) -> CodexReasoningGuardTemplateFilter {
    if models.len() == 1 {
        return CodexReasoningGuardTemplateFilter {
            id: format!(
                "requested-model-not-{}",
                sanitize_template_id_part(&models[0])
            ),
            field: CodexReasoningGuardTemplateFilterField::RequestedModel,
            operator: CodexReasoningGuardTemplateFilterOperator::NotEquals,
            number_value: None,
            bool_value: None,
            string_value: Some(models[0].clone()),
            string_values: Vec::new(),
        };
    }

    let id_suffix = sanitize_template_id_parts(&models);
    CodexReasoningGuardTemplateFilter {
        id: format!("requested-models-not-{id_suffix}"),
        field: CodexReasoningGuardTemplateFilterField::RequestedModel,
        operator: CodexReasoningGuardTemplateFilterOperator::NotIn,
        number_value: None,
        bool_value: None,
        string_value: None,
        string_values: models,
    }
}

fn sanitize_template_id_parts(values: &[String]) -> String {
    let mut output = String::new();
    for value in values {
        let part = sanitize_template_id_part(value);
        let separator_len = usize::from(!output.is_empty());
        if output.len() + separator_len + part.len() > 48 {
            break;
        }
        if !output.is_empty() {
            output.push('-');
        }
        output.push_str(&part);
    }
    if output.is_empty() {
        "models".to_string()
    } else {
        output
    }
}

fn legacy_bounded_rule_name(prefix: &str, suffix: &str) -> String {
    let max_prefix_len = MAX_CODEX_REASONING_GUARD_TEMPLATE_NAME_LEN.saturating_sub(suffix.len());
    let prefix = prefix.chars().take(max_prefix_len).collect::<String>();
    format!("{prefix}{suffix}")
}

fn legacy_token_rule_filters(
    exact_models: Vec<String>,
    model_specific_models: &[String],
    includes_global_fallback: bool,
) -> (
    CodexReasoningGuardTemplateRuleLogic,
    Vec<CodexReasoningGuardTemplateFilter>,
) {
    let mut filters = Vec::new();
    if !exact_models.is_empty() {
        filters.push(legacy_requested_model_filter_for_models(exact_models));
    }
    if includes_global_fallback && !model_specific_models.is_empty() {
        filters.push(legacy_requested_model_exclusion_filter_for_models(
            model_specific_models.to_vec(),
        ));
    }
    let logic = if filters.len() > 1 {
        CodexReasoningGuardTemplateRuleLogic::Or
    } else {
        CodexReasoningGuardTemplateRuleLogic::And
    };
    (logic, filters)
}

fn sanitize_template_id_part(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
        if output.len() >= 48 {
            break;
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        "model".to_string()
    } else {
        output.to_string()
    }
}

fn legacy_less_than_or_equal_custom_template(values: &[i64]) -> CodexReasoningGuardRuleTemplate {
    let thresholds = legacy_less_than_or_equal_thresholds(values);
    CodexReasoningGuardRuleTemplate {
        id: "custom-legacy-reasoning-tokens".to_string(),
        name: "Legacy custom reasoning tokens".to_string(),
        description: "Migrated from the legacy global less-than-or-equal token rule.".to_string(),
        rules: thresholds
            .into_iter()
            .map(|value| CodexReasoningGuardTemplateRule {
                id: format!("legacy-reasoning-tokens-threshold-{value}"),
                name: format!("legacy reasoning_tokens <= {value}"),
                reasoning_tokens: None,
                reasoning_tokens_formula: None,
                action: CodexReasoningGuardTemplateRuleAction::Intercept,
                logic: CodexReasoningGuardTemplateRuleLogic::And,
                filters: vec![legacy_number_filter(
                    format!("reasoning-tokens-lte-{value}"),
                    CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual,
                    value,
                )],
            })
            .collect(),
    }
}

fn push_legacy_token_rule(
    rules: &mut Vec<CodexReasoningGuardTemplateRule>,
    value: i64,
    models: Vec<String>,
    includes_global_fallback: bool,
    model_specific_models: &[String],
) {
    let model_id = sanitize_template_id_parts(&models);
    let model_name = if models.len() == 1 {
        models[0].clone()
    } else if models.is_empty() {
        "global".to_string()
    } else {
        models.join(", ")
    };
    let (logic, filters) =
        legacy_token_rule_filters(models, model_specific_models, includes_global_fallback);
    rules.push(CodexReasoningGuardTemplateRule {
        id: if includes_global_fallback && model_id == "models" {
            format!("legacy-global-token-{value}")
        } else {
            format!("legacy-model-{model_id}-token-{value}")
        },
        name: legacy_bounded_rule_name(&model_name, &format!(" reasoning_tokens == {value}")),
        reasoning_tokens: Some(value),
        reasoning_tokens_formula: None,
        action: CodexReasoningGuardTemplateRuleAction::Intercept,
        logic,
        filters,
    });
}

fn push_legacy_threshold_rule(
    rules: &mut Vec<CodexReasoningGuardTemplateRule>,
    value: i64,
    models: Vec<String>,
    includes_global_fallback: bool,
    model_specific_models: &[String],
) {
    let model_id = sanitize_template_id_parts(&models);
    let model_name = if models.len() == 1 {
        models[0].clone()
    } else if models.is_empty() {
        "global".to_string()
    } else {
        models.join(", ")
    };

    let mut filters = Vec::new();
    if !models.is_empty() {
        filters.push(legacy_requested_model_filter_for_models(models));
    }
    if includes_global_fallback && !model_specific_models.is_empty() {
        filters.push(legacy_requested_model_exclusion_filter_for_models(
            model_specific_models.to_vec(),
        ));
    }
    filters.push(legacy_number_filter(
        format!("reasoning-tokens-lte-{value}"),
        CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual,
        value,
    ));

    rules.push(CodexReasoningGuardTemplateRule {
        id: if includes_global_fallback && model_id == "models" {
            format!("legacy-global-reasoning-tokens-threshold-{value}")
        } else {
            format!("legacy-model-{model_id}-threshold-{value}")
        },
        name: legacy_bounded_rule_name(&model_name, &format!(" reasoning_tokens <= {value}")),
        reasoning_tokens: None,
        reasoning_tokens_formula: None,
        action: CodexReasoningGuardTemplateRuleAction::Intercept,
        logic: CodexReasoningGuardTemplateRuleLogic::And,
        filters,
    });
}

fn legacy_model_rule_custom_template(
    settings: &AppSettings,
) -> Option<CodexReasoningGuardRuleTemplate> {
    let mut rules = Vec::new();
    let mut exact_model_groups: Vec<(i64, Vec<String>)> = Vec::new();
    let mut threshold_model_groups: Vec<(i64, Vec<String>)> = Vec::new();
    let mut model_specific_models: Vec<String> = Vec::new();

    for model_rule in &settings.codex_reasoning_guard_model_rules {
        let requested_model = model_rule.requested_model.trim();
        if requested_model.is_empty() {
            return None;
        }
        if !model_specific_models
            .iter()
            .any(|model| model == requested_model)
        {
            model_specific_models.push(requested_model.to_string());
        }
        let values = legacy_reasoning_token_values(&model_rule.reasoning_equals);
        match model_rule.compare_mode {
            CodexReasoningGuardCompareMode::Equals => {
                for value in values {
                    if let Some((_, models)) = exact_model_groups
                        .iter_mut()
                        .find(|(group_value, _)| *group_value == value)
                    {
                        if !models.iter().any(|model| model == requested_model) {
                            models.push(requested_model.to_string());
                        }
                    } else {
                        exact_model_groups.push((value, vec![requested_model.to_string()]));
                    }
                }
            }
            CodexReasoningGuardCompareMode::LessThanOrEqual => {
                for value in legacy_less_than_or_equal_thresholds(&model_rule.reasoning_equals) {
                    if let Some((_, models)) = threshold_model_groups
                        .iter_mut()
                        .find(|(group_value, _)| *group_value == value)
                    {
                        if !models.iter().any(|model| model == requested_model) {
                            models.push(requested_model.to_string());
                        }
                    } else {
                        threshold_model_groups.push((value, vec![requested_model.to_string()]));
                    }
                }
            }
        }
    }

    threshold_model_groups.sort_by_key(|(value, _)| *value);

    if settings.codex_reasoning_guard_compare_mode
        == CodexReasoningGuardCompareMode::LessThanOrEqual
    {
        for (value, models) in exact_model_groups {
            push_legacy_token_rule(&mut rules, value, models, false, &model_specific_models);
        }
        for (value, models) in threshold_model_groups {
            push_legacy_threshold_rule(&mut rules, value, models, false, &model_specific_models);
        }
        for value in
            legacy_less_than_or_equal_thresholds(&settings.codex_reasoning_guard_reasoning_equals)
        {
            push_legacy_threshold_rule(&mut rules, value, Vec::new(), true, &model_specific_models);
        }

        if rules.len() > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES {
            return None;
        }

        return (!rules.is_empty()).then(|| CodexReasoningGuardRuleTemplate {
            id: "custom-legacy-reasoning-tokens".to_string(),
            name: "Legacy custom reasoning tokens".to_string(),
            description: "Migrated from legacy model-specific token rules.".to_string(),
            rules,
        });
    }

    if settings.codex_reasoning_guard_compare_mode != CodexReasoningGuardCompareMode::Equals {
        return None;
    }

    for (value, models) in threshold_model_groups {
        push_legacy_threshold_rule(&mut rules, value, models, false, &model_specific_models);
    }

    let mut token_groups: Vec<(i64, Vec<String>, bool)> = exact_model_groups
        .into_iter()
        .map(|(value, models)| (value, models, false))
        .collect();
    for value in legacy_reasoning_token_values(&settings.codex_reasoning_guard_reasoning_equals) {
        if let Some((_, _, includes_global_fallback)) = token_groups
            .iter_mut()
            .find(|(group_value, _, _)| *group_value == value)
        {
            *includes_global_fallback = true;
        } else {
            token_groups.push((value, Vec::new(), true));
        }
    }

    for (value, models, includes_global_fallback) in token_groups {
        push_legacy_token_rule(
            &mut rules,
            value,
            models,
            includes_global_fallback,
            &model_specific_models,
        );
    }

    if rules.len() > MAX_CODEX_REASONING_GUARD_TEMPLATE_RULES {
        return None;
    }

    (!rules.is_empty()).then(|| CodexReasoningGuardRuleTemplate {
        id: "custom-legacy-reasoning-tokens".to_string(),
        name: "Legacy custom reasoning tokens".to_string(),
        description: "Migrated from legacy model-specific token rules.".to_string(),
        rules,
    })
}

fn upsert_codex_reasoning_guard_custom_template(
    settings: &mut AppSettings,
    template: CodexReasoningGuardRuleTemplate,
) -> bool {
    settings.codex_reasoning_guard_active_template_id = template.id.clone();
    if let Some(existing) = settings
        .codex_reasoning_guard_custom_templates
        .iter_mut()
        .find(|item| item.id == template.id)
    {
        *existing = template;
        true
    } else if settings.codex_reasoning_guard_custom_templates.len()
        < MAX_CODEX_REASONING_GUARD_CUSTOM_TEMPLATES
    {
        settings
            .codex_reasoning_guard_custom_templates
            .push(template);
        true
    } else if let Some(last_template) = settings.codex_reasoning_guard_custom_templates.last_mut() {
        *last_template = template;
        true
    } else {
        settings.codex_reasoning_guard_active_template_id =
            CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string();
        false
    }
}

fn apply_codex_reasoning_guard_rule_template_migration(settings: &mut AppSettings) -> bool {
    let previous_active_template_id = settings.codex_reasoning_guard_active_template_id.clone();
    let previous_custom_templates = settings.codex_reasoning_guard_custom_templates.clone();

    match settings.codex_reasoning_guard_rule_mode {
        CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh => {
            settings.codex_reasoning_guard_active_template_id =
                CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID.to_string();
        }
        CodexReasoningGuardRuleMode::ReasoningTokens
            if settings.codex_reasoning_guard_compare_mode
                == CodexReasoningGuardCompareMode::Equals
                && settings.codex_reasoning_guard_model_rules.is_empty()
                && settings.codex_reasoning_guard_reasoning_equals.as_slice()
                    != DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS =>
        {
            let migrated =
                legacy_equals_custom_template(&settings.codex_reasoning_guard_reasoning_equals);
            upsert_codex_reasoning_guard_custom_template(settings, migrated);
        }
        CodexReasoningGuardRuleMode::ReasoningTokens
            if settings.codex_reasoning_guard_compare_mode
                == CodexReasoningGuardCompareMode::LessThanOrEqual
                && settings.codex_reasoning_guard_model_rules.is_empty() =>
        {
            let migrated = legacy_less_than_or_equal_custom_template(
                &settings.codex_reasoning_guard_reasoning_equals,
            );
            upsert_codex_reasoning_guard_custom_template(settings, migrated);
        }
        CodexReasoningGuardRuleMode::ReasoningTokens
            if !settings.codex_reasoning_guard_model_rules.is_empty() =>
        {
            if let Some(migrated) = legacy_model_rule_custom_template(settings) {
                upsert_codex_reasoning_guard_custom_template(settings, migrated);
            } else {
                settings.codex_reasoning_guard_active_template_id =
                    CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string();
            }
        }
        CodexReasoningGuardRuleMode::ReasoningTokens
            if settings.codex_reasoning_guard_compare_mode
                == CodexReasoningGuardCompareMode::Equals
                && settings.codex_reasoning_guard_model_rules.is_empty() =>
        {
            settings.codex_reasoning_guard_active_template_id =
                CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string();
        }
        CodexReasoningGuardRuleMode::ReasoningTokens => {
            settings.codex_reasoning_guard_active_template_id =
                CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID.to_string();
        }
    }

    settings.codex_reasoning_guard_active_template_id != previous_active_template_id
        || settings.codex_reasoning_guard_custom_templates != previous_custom_templates
}

fn raw_schema_version(raw_settings_json: &serde_json::Value) -> Option<u32> {
    raw_settings_json
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn repair_codex_reasoning_guard_rule_template_selection(
    settings: &mut AppSettings,
    schema_version_present: bool,
    raw_settings_json: &serde_json::Value,
) -> bool {
    let active_template_id = settings.codex_reasoning_guard_active_template_id.trim();
    match active_template_id {
        "" | CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID => {}
        CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID => {
            let legacy_schema = !schema_version_present
                || raw_schema_version(raw_settings_json).is_some_and(|version| {
                    version < SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES
                });
            if !legacy_schema {
                return false;
            }
        }
        _ => return false,
    }

    apply_codex_reasoning_guard_rule_template_migration(settings)
}

pub(super) fn sanitize_failover_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.failover_max_attempts_per_provider == 0 {
        settings.failover_max_attempts_per_provider = DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER;
        changed = true;
    }
    if settings.failover_max_providers_to_try == 0 {
        settings.failover_max_providers_to_try = DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY;
        changed = true;
    }

    if settings.failover_max_attempts_per_provider > MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER {
        settings.failover_max_attempts_per_provider = MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER;
        changed = true;
    }

    if settings.failover_max_providers_to_try > MAX_FAILOVER_MAX_PROVIDERS_TO_TRY {
        settings.failover_max_providers_to_try = MAX_FAILOVER_MAX_PROVIDERS_TO_TRY;
        changed = true;
    }

    let providers = settings.failover_max_providers_to_try.max(1);
    let max_attempts_for_providers = (MAX_FAILOVER_TOTAL_ATTEMPTS / providers).max(1);
    if settings.failover_max_attempts_per_provider > max_attempts_for_providers {
        settings.failover_max_attempts_per_provider = max_attempts_for_providers;
        changed = true;
    }

    changed
}

pub fn sanitize_upstream_retry_policy(policy: &mut UpstreamRetryPolicy) -> bool {
    let mut changed = false;

    policy.status_codes.retain(|status| {
        let keep = (400..=599).contains(status);
        changed |= !keep;
        keep
    });
    policy.status_codes.sort_unstable();
    policy.status_codes.dedup();
    if policy.status_codes.len() > MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES {
        policy
            .status_codes
            .truncate(MAX_UPSTREAM_RETRY_POLICY_STATUS_CODES);
        changed = true;
    }

    let mut seen_transport_errors = HashSet::new();
    policy.transport_errors.retain(|kind| {
        let keep = seen_transport_errors.insert(*kind);
        changed |= !keep;
        keep
    });
    if policy.transport_errors.len() > MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS {
        policy
            .transport_errors
            .truncate(MAX_UPSTREAM_RETRY_POLICY_TRANSPORT_ERRORS);
        changed = true;
    }

    if policy.max_retries > MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES {
        policy.max_retries = MAX_UPSTREAM_RETRY_POLICY_MAX_RETRIES;
        changed = true;
    }
    if policy.backoff_ms > MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS {
        policy.backoff_ms = MAX_UPSTREAM_RETRY_POLICY_BACKOFF_MS;
        changed = true;
    }

    // Keep a disabled policy editable, but make an enabled empty policy useful.
    if policy.enabled && policy.status_codes.is_empty() && policy.transport_errors.is_empty() {
        let defaults = UpstreamRetryPolicy::default();
        policy.status_codes = defaults.status_codes;
        policy.transport_errors = defaults.transport_errors;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_circuit_breaker_settings(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.circuit_breaker_failure_threshold == 0 {
        settings.circuit_breaker_failure_threshold = DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD;
        changed = true;
    }
    if settings.circuit_breaker_open_duration_minutes == 0 {
        settings.circuit_breaker_open_duration_minutes =
            DEFAULT_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES;
        changed = true;
    }

    if settings.circuit_breaker_failure_threshold > MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD {
        settings.circuit_breaker_failure_threshold = MAX_CIRCUIT_BREAKER_FAILURE_THRESHOLD;
        changed = true;
    }
    if settings.circuit_breaker_open_duration_minutes > MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES {
        settings.circuit_breaker_open_duration_minutes = MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_log_retention_days(settings: &mut AppSettings) -> bool {
    if settings.log_retention_days > MAX_LOG_RETENTION_DAYS {
        settings.log_retention_days = MAX_LOG_RETENTION_DAYS;
        return true;
    }
    false
}

pub(super) fn sanitize_provider_cooldown_seconds(settings: &mut AppSettings) -> bool {
    if settings.provider_cooldown_seconds > MAX_PROVIDER_COOLDOWN_SECONDS {
        settings.provider_cooldown_seconds = MAX_PROVIDER_COOLDOWN_SECONDS;
        return true;
    }
    false
}

pub(super) fn sanitize_provider_base_url_ping_cache_ttl_seconds(
    settings: &mut AppSettings,
) -> bool {
    let mut changed = false;

    if settings.provider_base_url_ping_cache_ttl_seconds == 0 {
        settings.provider_base_url_ping_cache_ttl_seconds =
            DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS;
        changed = true;
    }

    if settings.provider_base_url_ping_cache_ttl_seconds
        > MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
    {
        settings.provider_base_url_ping_cache_ttl_seconds =
            MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_upstream_timeouts(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.upstream_first_byte_timeout_seconds > MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS {
        settings.upstream_first_byte_timeout_seconds = MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds > MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS {
        settings.upstream_stream_idle_timeout_seconds = MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds > 0
        && settings.upstream_stream_idle_timeout_seconds < MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS
    {
        settings.upstream_stream_idle_timeout_seconds = MIN_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
        changed = true;
    }
    if settings.upstream_request_timeout_non_streaming_seconds
        > MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS
    {
        settings.upstream_request_timeout_non_streaming_seconds =
            MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS;
        changed = true;
    }

    changed
}

pub(super) fn sanitize_response_fixer_limits(settings: &mut AppSettings) -> bool {
    let mut changed = false;

    if settings.response_fixer_max_json_depth == 0 {
        settings.response_fixer_max_json_depth = DEFAULT_RESPONSE_FIXER_MAX_JSON_DEPTH;
        changed = true;
    }
    if settings.response_fixer_max_json_depth > MAX_RESPONSE_FIXER_MAX_JSON_DEPTH {
        settings.response_fixer_max_json_depth = MAX_RESPONSE_FIXER_MAX_JSON_DEPTH;
        changed = true;
    }

    if settings.response_fixer_max_fix_size == 0 {
        settings.response_fixer_max_fix_size = DEFAULT_RESPONSE_FIXER_MAX_FIX_SIZE;
        changed = true;
    }
    if settings.response_fixer_max_fix_size > MAX_RESPONSE_FIXER_MAX_FIX_SIZE {
        settings.response_fixer_max_fix_size = MAX_RESPONSE_FIXER_MAX_FIX_SIZE;
        changed = true;
    }

    changed
}

// -- Schema migrations --

fn migrate_disable_upstream_timeouts(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v7: Align defaults with "0 = disabled" semantics and migrate existing configs to disabled.
    if schema_version_present && settings.schema_version >= SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS
    {
        return false;
    }

    let mut changed = false;

    // If the schema version is missing, force a write to persist the current schema_version so we
    // don't re-run migrations on every startup.
    if !schema_version_present {
        changed = true;
    }

    if settings.schema_version != SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS {
        settings.schema_version = SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS;
        changed = true;
    }

    if settings.upstream_first_byte_timeout_seconds != 0 {
        settings.upstream_first_byte_timeout_seconds = 0;
        changed = true;
    }
    if settings.upstream_stream_idle_timeout_seconds != 0 {
        settings.upstream_stream_idle_timeout_seconds = 0;
        changed = true;
    }
    if settings.upstream_request_timeout_non_streaming_seconds != 0 {
        settings.upstream_request_timeout_non_streaming_seconds = 0;
        changed = true;
    }

    changed
}

/// Generic schema migration helper for versions that only bump `schema_version`.
///
/// Returns `true` if the settings were modified (i.e. migration was applied).
/// Migrations that need additional field changes (e.g. `migrate_disable_upstream_timeouts`)
/// should NOT use this helper.
fn migrate_bump_schema_version(
    settings: &mut AppSettings,
    schema_version_present: bool,
    target_version: u32,
) -> bool {
    if schema_version_present && settings.schema_version >= target_version {
        return false;
    }

    let mut changed = false;

    // If schema_version is missing, force a write to persist schema_version so we don't keep
    // "migrating" on every startup.
    if !schema_version_present {
        changed = true;
    }

    if settings.schema_version != target_version {
        settings.schema_version = target_version;
        changed = true;
    }

    changed
}

fn migrate_add_gateway_rectifiers(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v8: Add CCH v0.4.1-aligned gateway rectifier toggles (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_GATEWAY_RECTIFIERS,
    )
}

fn migrate_add_circuit_breaker_notice(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v9: Add circuit breaker notice toggle (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CIRCUIT_BREAKER_NOTICE,
    )
}

fn migrate_add_provider_base_url_ping_cache_ttl(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v10: Add provider ping cache ttl (seconds), default 60.
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_PROVIDER_BASE_URL_PING_CACHE_TTL,
    )
}

fn migrate_add_codex_session_id_completion(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v11: Add Codex Session ID completion toggle (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_SESSION_ID_COMPLETION,
    )
}

fn migrate_add_gateway_network_settings(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v12: Add gateway listen mode + WSL network settings (default disabled / all CLI enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_GATEWAY_NETWORK_SETTINGS,
    )
}

fn migrate_add_response_fixer_limits(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v13: Add response fixer config limits (max_json_depth / max_fix_size).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_RESPONSE_FIXER_LIMITS,
    )
}

fn migrate_add_cli_proxy_startup_recovery(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v14: Add CLI proxy startup recovery toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CLI_PROXY_STARTUP_RECOVERY,
    )
}

fn migrate_add_cache_anomaly_monitor(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v15: Add cache anomaly monitor toggle (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CACHE_ANOMALY_MONITOR,
    )
}

fn migrate_add_wsl_host_address_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_WSL_HOST_ADDRESS_MODE,
    )
}

fn migrate_add_task_complete_notify(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v17: Add task complete notification toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_TASK_COMPLETE_NOTIFY,
    )
}

fn migrate_add_cch_base_config(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v18: Add verbose provider error + thinking budget rectifier + claude metadata.user_id injection.
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CCH_BASE_CONFIG,
    )
}

fn migrate_add_start_minimized(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v19: Add start_minimized toggle (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_START_MINIMIZED,
    )
}

fn migrate_add_show_home_heatmap(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v20: Add homepage heatmap visibility toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_SHOW_HOME_HEATMAP,
    )
}

fn migrate_add_home_usage_period(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v21: Add homepage usage window selector (default last15).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_HOME_USAGE_PERIOD,
    )
}

fn migrate_add_show_home_usage(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v22: Add homepage usage visibility toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_SHOW_HOME_USAGE,
    )
}

fn migrate_add_codex_home_override(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v23: Add persisted Codex config directory override (default empty = use default resolution).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_HOME_OVERRIDE,
    )
}

fn migrate_add_codex_home_mode(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v24: Split Codex home resolution into explicit user-home default / follow CODEX_HOME / custom.
    let needs_mode_default =
        !schema_version_present || settings.schema_version < SCHEMA_VERSION_ADD_CODEX_HOME_MODE;
    let changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_HOME_MODE,
    );

    if needs_mode_default {
        settings.codex_home_mode = if settings.codex_home_override.trim().is_empty() {
            CodexHomeMode::UserHomeDefault
        } else {
            CodexHomeMode::Custom
        };
    }

    changed
}

fn migrate_add_notification_sound(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v25: Add notification sound toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_NOTIFICATION_SOUND,
    )
}

fn migrate_add_cx2cc_settings(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CX2CC_SETTINGS,
    ) {
        return false;
    }
    if settings.cx2cc_fallback_model_opus.is_empty() {
        settings.cx2cc_fallback_model_opus = DEFAULT_CX2CC_FALLBACK_MODEL.to_string();
    }
    if settings.cx2cc_fallback_model_sonnet.is_empty() {
        settings.cx2cc_fallback_model_sonnet = DEFAULT_CX2CC_FALLBACK_MODEL.to_string();
    }
    if settings.cx2cc_fallback_model_haiku.is_empty() {
        settings.cx2cc_fallback_model_haiku = DEFAULT_CX2CC_FALLBACK_MODEL.to_string();
    }
    if settings.cx2cc_fallback_model_main.is_empty() {
        settings.cx2cc_fallback_model_main = DEFAULT_CX2CC_FALLBACK_MODEL.to_string();
    }
    settings.cx2cc_disable_response_storage = true;
    settings.cx2cc_enable_reasoning_to_thinking = true;
    settings.cx2cc_drop_stop_sequences = true;
    settings.cx2cc_clean_schema = true;
    settings.cx2cc_filter_batch_tool = true;
    true
}

fn migrate_enable_default_upstream_timeouts(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // Fresh installs already pick up the new defaults from `AppSettings::default`.
    // Existing installs must preserve explicit `0 = disabled` choices.
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS,
    )
}

fn migrate_add_billing_header_rectifier(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v28: Add billing header rectifier toggle (default enabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_BILLING_HEADER_RECTIFIER,
    )
}

fn migrate_add_cli_priority_order(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v29: Add global CLI priority order for tab rendering and default selection.
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CLI_PRIORITY_ORDER,
    ) {
        return false;
    }

    settings.cli_priority_order = normalize_cli_priority_order(&settings.cli_priority_order);
    true
}

fn migrate_raise_stream_idle_timeout_default(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_RAISE_STREAM_IDLE_TIMEOUT_DEFAULT,
    ) {
        return false;
    }

    // Users who got the old 120s default should be bumped to 300s.
    // Users who explicitly set other values (including 0 = disabled) keep their choice.
    if settings.upstream_stream_idle_timeout_seconds == 120 {
        settings.upstream_stream_idle_timeout_seconds =
            DEFAULT_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS;
    }
    true
}

fn migrate_add_upstream_proxy(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    // v31: Add upstream proxy settings (default disabled, empty URL).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_PROXY,
    )
}

fn migrate_add_upstream_proxy_credentials(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v32: Add upstream proxy username/password settings (default empty).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_PROXY_CREDENTIALS,
    )
}

fn migrate_add_codex_oauth_compatible_proxy_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v33: Add Codex OAuth compatible CLI proxy mode (default disabled).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_OAUTH_COMPATIBLE_PROXY_MODE,
    )
}

fn migrate_add_codex_reasoning_guard(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v34: Add Codex reasoning guard defaults.
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD,
    )
}

fn migrate_add_codex_reasoning_guard_compare_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v35: Add Codex reasoning guard compare mode (default equals).
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_COMPARE_MODE,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_compare_mode = CodexReasoningGuardCompareMode::Equals;
    true
}

fn migrate_update_releases_url_to_fork(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v36: Point the default update release URL at this fork's release page.
    if schema_version_present
        && settings.schema_version >= SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK
    {
        return false;
    }

    let mut changed = migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK,
    );

    let current = settings.update_releases_url.trim().to_string();
    if current.is_empty() || current == LEGACY_UPDATE_RELEASES_URL {
        settings.update_releases_url = DEFAULT_UPDATE_RELEASES_URL.to_string();
        changed = true;
    }

    changed
}

pub(super) const SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK: u32 = 36;

fn migrate_add_codex_reasoning_guard_model_rules(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    // v37: Add model-specific Codex reasoning guard rules (default empty).
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_MODEL_RULES,
    )
}

fn migrate_add_codex_provider_test_model(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_PROVIDER_TEST_MODEL,
    ) {
        return false;
    }

    if settings.codex_provider_test_model.trim().is_empty() {
        settings.codex_provider_test_model = DEFAULT_CODEX_PROVIDER_TEST_MODEL.to_string();
        return true;
    }

    false
}

fn migrate_add_upstream_retry_policy(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_UPSTREAM_RETRY_POLICY,
    ) {
        return false;
    }

    sanitize_upstream_retry_policy(&mut settings.upstream_retry_policy)
}

fn migrate_add_codex_reasoning_guard_backoff(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_BACKOFF,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_backoff_after_hits =
        DEFAULT_CODEX_REASONING_GUARD_BACKOFF_AFTER_HITS;
    settings.codex_reasoning_guard_backoff_ms = DEFAULT_CODEX_REASONING_GUARD_BACKOFF_MS;
    true
}

fn migrate_update_codex_reasoning_guard_defaults(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_UPDATE_CODEX_REASONING_GUARD_DEFAULTS,
    ) {
        return false;
    }

    if settings.codex_reasoning_guard_reasoning_equals == [516] {
        settings.codex_reasoning_guard_reasoning_equals =
            DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.to_vec();
    }
    true
}

fn migrate_add_codex_reasoning_guard_budget(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_BUDGET,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_immediate_retry_budget =
        DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET;
    settings.codex_reasoning_guard_delayed_retry_budget =
        DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET;
    settings.codex_reasoning_guard_delayed_retry_ms =
        DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS;
    settings.codex_reasoning_guard_exhausted_action =
        CodexReasoningGuardExhaustedAction::ReturnError;
    true
}

fn migrate_add_codex_reasoning_guard_retry_policy(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RETRY_POLICY,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_retry_policy = Default::default();
    settings.codex_reasoning_guard_concurrent_max = DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX;
    settings.codex_reasoning_guard_concurrent_interval_ms =
        DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_INTERVAL_MS;
    settings.codex_reasoning_guard_concurrent_max_attempts =
        DEFAULT_CODEX_REASONING_GUARD_CONCURRENT_MAX_ATTEMPTS;
    settings.codex_reasoning_guard_model_fallbacks = Vec::new();
    true
}

fn migrate_add_codex_reasoning_guard_hit_label(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_HIT_LABEL,
    ) {
        return false;
    }

    if settings.codex_reasoning_guard_hit_label.trim().is_empty() {
        settings.codex_reasoning_guard_hit_label =
            DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL.to_string();
        return true;
    }

    false
}

fn migrate_add_codex_reasoning_guard_rule_mode(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_rule_mode = CodexReasoningGuardRuleMode::ReasoningTokens;
    true
}

fn migrate_add_codex_reasoning_guard_rule_templates(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES,
    ) {
        return false;
    }

    apply_codex_reasoning_guard_rule_template_migration(settings);
    true
}

fn migrate_add_codex_reasoning_guard_continuation_repair(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_CONTINUATION_REPAIR,
    )
}

fn migrate_unify_codex_reasoning_guard(
    settings: &mut AppSettings,
    schema_version_present: bool,
) -> bool {
    if !migrate_bump_schema_version(
        settings,
        schema_version_present,
        SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD,
    ) {
        return false;
    }

    settings.codex_reasoning_guard_rule_mode = CodexReasoningGuardRuleMode::ReasoningTokens;
    settings.codex_reasoning_guard_compare_mode = CodexReasoningGuardCompareMode::Equals;
    settings.codex_reasoning_guard_reasoning_equals =
        DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS.to_vec();
    // Schema 48 intentionally replaces the pre-unification guard rule surface
    // with the 518*n-2 continuation-repair template. Older custom templates
    // and model rules used incompatible legacy semantics, so they are reset
    // only for users that have not already reached this schema.
    settings.codex_reasoning_guard_model_rules.clear();
    settings.codex_reasoning_guard_active_template_id =
        CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID.to_string();
    settings.codex_reasoning_guard_custom_templates.clear();
    settings.codex_reasoning_guard_post_match_strategy =
        CodexReasoningGuardPostMatchStrategy::ContinuationRepair;
    settings.codex_reasoning_guard_exhausted_action =
        CodexReasoningGuardExhaustedAction::ReturnError;
    true
}

type SettingsMigration = fn(&mut AppSettings, bool) -> bool;

const SETTINGS_MIGRATIONS: [SettingsMigration; 42] = [
    migrate_disable_upstream_timeouts,
    migrate_add_gateway_rectifiers,
    migrate_add_circuit_breaker_notice,
    migrate_add_provider_base_url_ping_cache_ttl,
    migrate_add_codex_session_id_completion,
    migrate_add_gateway_network_settings,
    migrate_add_response_fixer_limits,
    migrate_add_cli_proxy_startup_recovery,
    migrate_add_cache_anomaly_monitor,
    migrate_add_wsl_host_address_mode,
    migrate_add_task_complete_notify,
    migrate_add_cch_base_config,
    migrate_add_start_minimized,
    migrate_add_show_home_heatmap,
    migrate_add_home_usage_period,
    migrate_add_show_home_usage,
    migrate_add_codex_home_override,
    migrate_add_codex_home_mode,
    migrate_add_notification_sound,
    migrate_add_cx2cc_settings,
    migrate_enable_default_upstream_timeouts,
    migrate_add_billing_header_rectifier,
    migrate_add_cli_priority_order,
    migrate_raise_stream_idle_timeout_default,
    migrate_add_upstream_proxy,
    migrate_add_upstream_proxy_credentials,
    migrate_add_codex_oauth_compatible_proxy_mode,
    migrate_add_codex_reasoning_guard,
    migrate_add_codex_reasoning_guard_compare_mode,
    migrate_update_releases_url_to_fork,
    migrate_add_codex_reasoning_guard_model_rules,
    migrate_add_codex_provider_test_model,
    migrate_add_upstream_retry_policy,
    migrate_add_codex_reasoning_guard_backoff,
    migrate_update_codex_reasoning_guard_defaults,
    migrate_add_codex_reasoning_guard_budget,
    migrate_add_codex_reasoning_guard_retry_policy,
    migrate_add_codex_reasoning_guard_hit_label,
    migrate_add_codex_reasoning_guard_rule_mode,
    migrate_add_codex_reasoning_guard_rule_templates,
    migrate_add_codex_reasoning_guard_continuation_repair,
    migrate_unify_codex_reasoning_guard,
];

fn apply_settings_migrations(settings: &mut AppSettings, schema_version_present: bool) -> bool {
    let mut changed = false;
    for migration in SETTINGS_MIGRATIONS {
        changed |= migration(settings, schema_version_present);
    }
    changed
}

pub(super) fn repair_settings(
    settings: &mut AppSettings,
    schema_version_present: bool,
    raw_settings_json: &serde_json::Value,
) -> AppResult<bool> {
    let mut repaired = apply_settings_migrations(settings, schema_version_present);
    repaired |= sanitize_log_retention_days(settings);
    repaired |= sanitize_failover_settings(settings);
    repaired |= sanitize_upstream_retry_policy(&mut settings.upstream_retry_policy);
    repaired |= sanitize_circuit_breaker_settings(settings);
    repaired |= sanitize_provider_cooldown_seconds(settings);
    repaired |= sanitize_provider_base_url_ping_cache_ttl_seconds(settings);
    repaired |= sanitize_upstream_timeouts(settings);
    repaired |= sanitize_response_fixer_limits(settings);
    repaired |= sanitize_codex_home_override(settings);
    repaired |= sanitize_codex_provider_test_model(settings);
    repaired |= sanitize_codex_reasoning_guard_hit_label(settings);
    repaired |= sanitize_codex_reasoning_guard_model_rules(settings);
    repaired |= repair_codex_reasoning_guard_rule_template_selection(
        settings,
        schema_version_present,
        raw_settings_json,
    );
    repaired |= sanitize_codex_reasoning_guard_runtime_settings(settings);
    repaired |= sanitize_cli_priority_order(settings);
    let canonical = super::persistence::canonical_settings_json(settings)?;
    repaired |= raw_settings_json != &canonical;
    Ok(repaired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::settings::types::default_cli_priority_order;

    // -- sanitize_failover_settings --

    #[test]
    fn sanitize_failover_resets_zero_attempts_to_default() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 0,
            failover_max_providers_to_try: 3,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_attempts_per_provider,
            DEFAULT_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER
        );
    }

    #[test]
    fn sanitize_failover_resets_zero_providers_to_default() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 3,
            failover_max_providers_to_try: 0,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_providers_to_try,
            DEFAULT_FAILOVER_MAX_PROVIDERS_TO_TRY
        );
    }

    #[test]
    fn sanitize_failover_clamps_excessive_attempts() {
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 999,
            failover_max_providers_to_try: 1,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        assert_eq!(
            s.failover_max_attempts_per_provider,
            MAX_FAILOVER_MAX_ATTEMPTS_PER_PROVIDER
        );
    }

    #[test]
    fn sanitize_codex_reasoning_guard_hit_label_trims_and_defaults_blank() {
        let mut blank = AppSettings {
            codex_reasoning_guard_hit_label: "   ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_reasoning_guard_hit_label(&mut blank));
        assert_eq!(
            blank.codex_reasoning_guard_hit_label,
            DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL
        );

        let mut custom = AppSettings {
            codex_reasoning_guard_hit_label: "  守卫命中  ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_reasoning_guard_hit_label(&mut custom));
        assert_eq!(custom.codex_reasoning_guard_hit_label, "守卫命中");
    }

    #[test]
    fn sanitize_failover_clamps_total_product() {
        // 20 * 20 = 400 > MAX_FAILOVER_TOTAL_ATTEMPTS (100)
        let mut s = AppSettings {
            failover_max_attempts_per_provider: 20,
            failover_max_providers_to_try: 20,
            ..Default::default()
        };
        assert!(sanitize_failover_settings(&mut s));
        // attempts_per_provider should be clamped to 100/20 = 5
        assert_eq!(s.failover_max_attempts_per_provider, 5);
    }

    #[test]
    fn sanitize_failover_no_change_for_valid_values() {
        let mut s = AppSettings::default();
        assert!(!sanitize_failover_settings(&mut s));
    }

    // -- sanitize_circuit_breaker_settings --

    #[test]
    fn sanitize_circuit_breaker_resets_zero_threshold() {
        let mut s = AppSettings {
            circuit_breaker_failure_threshold: 0,
            ..Default::default()
        };
        assert!(sanitize_circuit_breaker_settings(&mut s));
        assert_eq!(
            s.circuit_breaker_failure_threshold,
            DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD
        );
    }

    #[test]
    fn sanitize_circuit_breaker_clamps_excessive_duration() {
        let mut s = AppSettings {
            circuit_breaker_open_duration_minutes: 99999,
            ..Default::default()
        };
        assert!(sanitize_circuit_breaker_settings(&mut s));
        assert_eq!(
            s.circuit_breaker_open_duration_minutes,
            MAX_CIRCUIT_BREAKER_OPEN_DURATION_MINUTES
        );
    }

    #[test]
    fn sanitize_circuit_breaker_no_change_for_valid_values() {
        let mut s = AppSettings::default();
        assert!(!sanitize_circuit_breaker_settings(&mut s));
    }

    // -- sanitize_log_retention_days --

    #[test]
    fn sanitize_log_retention_days_clamps_excessive_value() {
        let mut s = AppSettings {
            log_retention_days: MAX_LOG_RETENTION_DAYS + 1,
            ..Default::default()
        };
        assert!(sanitize_log_retention_days(&mut s));
        assert_eq!(s.log_retention_days, MAX_LOG_RETENTION_DAYS);
    }

    #[test]
    fn sanitize_log_retention_days_leaves_valid_value() {
        let mut s = AppSettings {
            log_retention_days: 30,
            ..Default::default()
        };
        assert!(!sanitize_log_retention_days(&mut s));
        assert_eq!(s.log_retention_days, 30);
    }

    // -- sanitize_provider_cooldown_seconds --

    #[test]
    fn sanitize_cooldown_clamps_excessive_value() {
        let mut s = AppSettings {
            provider_cooldown_seconds: MAX_PROVIDER_COOLDOWN_SECONDS + 1,
            ..Default::default()
        };
        assert!(sanitize_provider_cooldown_seconds(&mut s));
        assert_eq!(s.provider_cooldown_seconds, MAX_PROVIDER_COOLDOWN_SECONDS);
    }

    #[test]
    fn sanitize_cooldown_allows_zero() {
        let mut s = AppSettings {
            provider_cooldown_seconds: 0,
            ..Default::default()
        };
        assert!(!sanitize_provider_cooldown_seconds(&mut s));
        assert_eq!(s.provider_cooldown_seconds, 0);
    }

    // -- sanitize_provider_base_url_ping_cache_ttl_seconds --

    #[test]
    fn sanitize_ping_cache_ttl_resets_zero_to_default() {
        let mut s = AppSettings {
            provider_base_url_ping_cache_ttl_seconds: 0,
            ..Default::default()
        };
        assert!(sanitize_provider_base_url_ping_cache_ttl_seconds(&mut s));
        assert_eq!(
            s.provider_base_url_ping_cache_ttl_seconds,
            DEFAULT_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
        );
    }

    #[test]
    fn sanitize_ping_cache_ttl_clamps_excessive_value() {
        let mut s = AppSettings {
            provider_base_url_ping_cache_ttl_seconds: MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
                + 1,
            ..Default::default()
        };
        assert!(sanitize_provider_base_url_ping_cache_ttl_seconds(&mut s));
        assert_eq!(
            s.provider_base_url_ping_cache_ttl_seconds,
            MAX_PROVIDER_BASE_URL_PING_CACHE_TTL_SECONDS
        );
    }

    // -- sanitize_upstream_timeouts --

    #[test]
    fn sanitize_upstream_timeouts_clamps_excessive_values() {
        let mut s = AppSettings {
            upstream_first_byte_timeout_seconds: MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS + 1,
            upstream_stream_idle_timeout_seconds: MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS + 1,
            upstream_request_timeout_non_streaming_seconds:
                MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS + 1,
            ..Default::default()
        };
        assert!(sanitize_upstream_timeouts(&mut s));
        assert_eq!(
            s.upstream_first_byte_timeout_seconds,
            MAX_UPSTREAM_FIRST_BYTE_TIMEOUT_SECONDS
        );
        assert_eq!(
            s.upstream_stream_idle_timeout_seconds,
            MAX_UPSTREAM_STREAM_IDLE_TIMEOUT_SECONDS
        );
        assert_eq!(
            s.upstream_request_timeout_non_streaming_seconds,
            MAX_UPSTREAM_REQUEST_TIMEOUT_NON_STREAMING_SECONDS
        );
    }

    #[test]
    fn sanitize_upstream_timeouts_allows_zero_disabled() {
        let mut s = AppSettings {
            upstream_first_byte_timeout_seconds: 0,
            upstream_stream_idle_timeout_seconds: 0,
            upstream_request_timeout_non_streaming_seconds: 0,
            ..Default::default()
        };
        assert!(!sanitize_upstream_timeouts(&mut s));
    }

    // -- sanitize_response_fixer_limits --

    #[test]
    fn sanitize_response_fixer_resets_zero_depth_to_default() {
        let mut s = AppSettings {
            response_fixer_max_json_depth: 0,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_json_depth,
            DEFAULT_RESPONSE_FIXER_MAX_JSON_DEPTH
        );
    }

    #[test]
    fn sanitize_response_fixer_clamps_excessive_depth() {
        let mut s = AppSettings {
            response_fixer_max_json_depth: MAX_RESPONSE_FIXER_MAX_JSON_DEPTH + 1,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_json_depth,
            MAX_RESPONSE_FIXER_MAX_JSON_DEPTH
        );
    }

    #[test]
    fn sanitize_response_fixer_resets_zero_size_to_default() {
        let mut s = AppSettings {
            response_fixer_max_fix_size: 0,
            ..Default::default()
        };
        assert!(sanitize_response_fixer_limits(&mut s));
        assert_eq!(
            s.response_fixer_max_fix_size,
            DEFAULT_RESPONSE_FIXER_MAX_FIX_SIZE
        );
    }

    // -- migrate_bump_schema_version --

    #[test]
    fn migrate_bump_skips_when_already_at_target() {
        let mut s = AppSettings {
            schema_version: 10,
            ..Default::default()
        };
        assert!(!migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 10);
    }

    #[test]
    fn migrate_bump_skips_when_above_target() {
        let mut s = AppSettings {
            schema_version: 12,
            ..Default::default()
        };
        assert!(!migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 12);
    }

    #[test]
    fn migrate_bump_applies_when_below_target() {
        let mut s = AppSettings {
            schema_version: 8,
            ..Default::default()
        };
        assert!(migrate_bump_schema_version(&mut s, true, 10));
        assert_eq!(s.schema_version, 10);
    }

    #[test]
    fn migrate_bump_forces_write_when_schema_version_absent() {
        let mut s = AppSettings {
            schema_version: 10,
            ..Default::default()
        };
        // schema_version_present = false forces a write even if version matches
        assert!(migrate_bump_schema_version(&mut s, false, 10));
    }

    // -- migrate_disable_upstream_timeouts --

    #[test]
    fn migrate_disable_upstream_timeouts_resets_nonzero_values() {
        let mut s = AppSettings {
            schema_version: 5,
            upstream_first_byte_timeout_seconds: 30,
            upstream_stream_idle_timeout_seconds: 60,
            upstream_request_timeout_non_streaming_seconds: 120,
            ..Default::default()
        };
        assert!(migrate_disable_upstream_timeouts(&mut s, true));
        assert_eq!(s.upstream_first_byte_timeout_seconds, 0);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 0);
        assert_eq!(s.upstream_request_timeout_non_streaming_seconds, 0);
        assert_eq!(s.schema_version, SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS);
    }

    #[test]
    fn migrate_disable_upstream_timeouts_skips_when_already_migrated() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_DISABLE_UPSTREAM_TIMEOUTS,
            upstream_first_byte_timeout_seconds: 30,
            ..Default::default()
        };
        assert!(!migrate_disable_upstream_timeouts(&mut s, true));
        // Value should NOT be reset since migration is already applied
        assert_eq!(s.upstream_first_byte_timeout_seconds, 30);
    }

    #[test]
    fn migrate_enable_default_upstream_timeouts_preserves_disabled_values() {
        let mut s = AppSettings {
            schema_version: 26,
            upstream_first_byte_timeout_seconds: 0,
            upstream_stream_idle_timeout_seconds: 0,
            ..Default::default()
        };

        assert!(migrate_enable_default_upstream_timeouts(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS
        );
        assert_eq!(s.upstream_first_byte_timeout_seconds, 0);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 0);
    }

    #[test]
    fn migrate_enable_default_upstream_timeouts_keeps_existing_nonzero_values() {
        let mut s = AppSettings {
            schema_version: 26,
            upstream_first_byte_timeout_seconds: 15,
            upstream_stream_idle_timeout_seconds: 45,
            ..Default::default()
        };

        assert!(migrate_enable_default_upstream_timeouts(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ENABLE_DEFAULT_UPSTREAM_TIMEOUTS
        );
        assert_eq!(s.upstream_first_byte_timeout_seconds, 15);
        assert_eq!(s.upstream_stream_idle_timeout_seconds, 45);
    }

    // -- GatewayListenMode --

    #[test]
    fn gateway_listen_mode_default_is_localhost() {
        assert_eq!(
            super::super::types::GatewayListenMode::default(),
            super::super::types::GatewayListenMode::Localhost,
        );
    }

    // -- AppSettings default --

    #[test]
    fn app_settings_default_has_current_schema_version() {
        let s = AppSettings::default();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn app_settings_default_has_expected_port() {
        let s = AppSettings::default();
        assert_eq!(s.preferred_port, DEFAULT_GATEWAY_PORT);
    }

    #[test]
    fn app_settings_default_shows_home_heatmap() {
        let s = AppSettings::default();
        assert!(s.show_home_heatmap);
    }

    #[test]
    fn app_settings_default_shows_home_usage() {
        let s = AppSettings::default();
        assert!(s.show_home_usage);
    }

    #[test]
    fn app_settings_default_has_empty_codex_home_override() {
        let s = AppSettings::default();
        assert!(s.codex_home_override.is_empty());
    }

    #[test]
    fn app_settings_default_uses_user_home_default_codex_mode() {
        let s = AppSettings::default();
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
    }

    #[test]
    fn app_settings_default_uses_last15_home_usage_period() {
        use super::super::types::HomeUsagePeriod;
        let s = AppSettings::default();
        assert_eq!(s.home_usage_period, HomeUsagePeriod::Last15);
    }

    #[test]
    fn app_settings_default_sets_cli_priority_order() {
        let s = AppSettings::default();
        assert_eq!(s.cli_priority_order, default_cli_priority_order());
    }

    #[test]
    fn app_settings_default_cache_anomaly_monitor_disabled() {
        let s = AppSettings::default();
        assert!(!s.enable_cache_anomaly_monitor);
    }

    #[test]
    fn app_settings_default_codex_oauth_compatible_proxy_mode_disabled() {
        let s = AppSettings::default();
        assert!(!s.codex_oauth_compatible_proxy_mode);
    }

    #[test]
    fn migrate_add_codex_oauth_compatible_proxy_mode_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 32,
            ..Default::default()
        };
        assert!(migrate_add_codex_oauth_compatible_proxy_mode(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_OAUTH_COMPATIBLE_PROXY_MODE
        );
        assert!(!s.codex_oauth_compatible_proxy_mode);
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 33,
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD);
        assert!(s.codex_reasoning_guard_enabled);
        assert_eq!(
            s.codex_reasoning_guard_reasoning_equals,
            vec![516, 1034, 1552]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_compare_mode_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 34,
            ..Default::default()
        };
        s.codex_reasoning_guard_compare_mode = CodexReasoningGuardCompareMode::LessThanOrEqual;
        assert!(migrate_add_codex_reasoning_guard_compare_mode(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_COMPARE_MODE
        );
        assert_eq!(
            s.codex_reasoning_guard_compare_mode,
            CodexReasoningGuardCompareMode::Equals
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_backoff_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 39,
            codex_reasoning_guard_backoff_after_hits: 0,
            codex_reasoning_guard_backoff_ms: 0,
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_backoff(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_BACKOFF
        );
        assert_eq!(s.codex_reasoning_guard_backoff_after_hits, 5);
        assert_eq!(s.codex_reasoning_guard_backoff_ms, 1_000);
    }

    #[test]
    fn migrate_update_codex_reasoning_guard_defaults_updates_only_old_default() {
        let mut s = AppSettings {
            schema_version: 40,
            codex_reasoning_guard_reasoning_equals: vec![516],
            ..Default::default()
        };
        assert!(migrate_update_codex_reasoning_guard_defaults(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_UPDATE_CODEX_REASONING_GUARD_DEFAULTS
        );
        assert_eq!(
            s.codex_reasoning_guard_reasoning_equals,
            vec![516, 1034, 1552]
        );

        let mut custom = AppSettings {
            schema_version: 40,
            codex_reasoning_guard_reasoning_equals: vec![777],
            ..Default::default()
        };
        assert!(migrate_update_codex_reasoning_guard_defaults(
            &mut custom,
            true
        ));
        assert_eq!(custom.codex_reasoning_guard_reasoning_equals, vec![777]);
    }

    #[test]
    fn migrate_update_releases_url_to_fork_rewrites_legacy_default() {
        let mut s = AppSettings {
            schema_version: 35,
            update_releases_url: LEGACY_UPDATE_RELEASES_URL.to_string(),
            ..Default::default()
        };
        assert!(migrate_update_releases_url_to_fork(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK);
        assert_eq!(s.update_releases_url, DEFAULT_UPDATE_RELEASES_URL);
    }

    #[test]
    fn migrate_update_releases_url_to_fork_preserves_custom_url() {
        let mut s = AppSettings {
            schema_version: 35,
            update_releases_url: "https://mirror.example.invalid/releases".to_string(),
            ..Default::default()
        };
        assert!(migrate_update_releases_url_to_fork(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_UPDATE_RELEASES_URL_TO_FORK);
        assert_eq!(
            s.update_releases_url,
            "https://mirror.example.invalid/releases"
        );
    }

    #[test]
    fn migrate_add_cache_anomaly_monitor_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 14,
            ..Default::default()
        };
        assert!(migrate_add_cache_anomaly_monitor(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CACHE_ANOMALY_MONITOR);
    }

    #[test]
    fn migrate_add_wsl_host_address_mode_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 15,
            ..Default::default()
        };
        assert!(migrate_add_wsl_host_address_mode(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_WSL_HOST_ADDRESS_MODE);
    }

    #[test]
    fn migrate_add_show_home_heatmap_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 19,
            ..Default::default()
        };
        assert!(migrate_add_show_home_heatmap(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_SHOW_HOME_HEATMAP);
    }

    #[test]
    fn migrate_add_home_usage_period_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 20,
            ..Default::default()
        };
        assert!(migrate_add_home_usage_period(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_HOME_USAGE_PERIOD);
    }

    #[test]
    fn migrate_add_show_home_usage_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 21,
            ..Default::default()
        };
        assert!(migrate_add_show_home_usage(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_SHOW_HOME_USAGE);
    }

    #[test]
    fn migrate_add_codex_home_override_bumps_schema_version() {
        let mut s = AppSettings {
            schema_version: 22,
            ..Default::default()
        };
        assert!(migrate_add_codex_home_override(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CODEX_HOME_OVERRIDE);
    }

    #[test]
    fn migrate_add_codex_home_mode_bumps_schema_version_and_defaults_to_user_home() {
        let mut s = AppSettings {
            schema_version: 23,
            ..Default::default()
        };
        assert!(migrate_add_codex_home_mode(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CODEX_HOME_MODE);
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
    }

    #[test]
    fn migrate_add_codex_home_mode_preserves_legacy_custom_override_as_custom_mode() {
        let mut s = AppSettings {
            schema_version: 23,
            codex_home_override: r"D:\Work\.codex".to_string(),
            ..Default::default()
        };
        assert!(migrate_add_codex_home_mode(&mut s, true));
        assert_eq!(s.codex_home_mode, CodexHomeMode::Custom);
    }

    #[test]
    fn sanitize_cli_priority_order_normalizes_invalid_duplicates_and_missing() {
        let mut s = AppSettings {
            cli_priority_order: vec![
                "codex".to_string(),
                "unknown".to_string(),
                "codex".to_string(),
                "claude".to_string(),
            ],
            ..Default::default()
        };
        assert!(sanitize_cli_priority_order(&mut s));
        assert_eq!(
            s.cli_priority_order,
            vec![
                "codex".to_string(),
                "claude".to_string(),
                "gemini".to_string()
            ]
        );
    }

    #[test]
    fn migrate_add_cli_priority_order_bumps_schema_and_fills_default_order() {
        let mut s = AppSettings {
            schema_version: 28,
            cli_priority_order: Vec::new(),
            ..Default::default()
        };
        assert!(migrate_add_cli_priority_order(&mut s, true));
        assert_eq!(s.schema_version, SCHEMA_VERSION_ADD_CLI_PRIORITY_ORDER);
        assert_eq!(s.cli_priority_order, default_cli_priority_order());
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_budget_bumps_schema_and_fills_defaults() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_UPDATE_CODEX_REASONING_GUARD_DEFAULTS,
            codex_reasoning_guard_immediate_retry_budget: 0,
            codex_reasoning_guard_delayed_retry_budget: 0,
            codex_reasoning_guard_delayed_retry_ms: 0,
            codex_reasoning_guard_exhausted_action:
                CodexReasoningGuardExhaustedAction::SwitchProvider,
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_budget(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_BUDGET
        );
        assert_eq!(
            s.codex_reasoning_guard_immediate_retry_budget,
            DEFAULT_CODEX_REASONING_GUARD_IMMEDIATE_RETRY_BUDGET
        );
        assert_eq!(
            s.codex_reasoning_guard_delayed_retry_budget,
            DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_BUDGET
        );
        assert_eq!(
            s.codex_reasoning_guard_delayed_retry_ms,
            DEFAULT_CODEX_REASONING_GUARD_DELAYED_RETRY_MS
        );
        assert_eq!(
            s.codex_reasoning_guard_exhausted_action,
            CodexReasoningGuardExhaustedAction::ReturnError
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_hit_label_bumps_schema_and_fills_default() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RETRY_POLICY,
            codex_reasoning_guard_hit_label: String::new(),
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_hit_label(&mut s, true));
        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_HIT_LABEL
        );
        assert_eq!(
            s.codex_reasoning_guard_hit_label,
            DEFAULT_CODEX_REASONING_GUARD_HIT_LABEL
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_maps_default_legacy_template() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut s, true
        ));
        assert_eq!(
            s.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID
        );
        assert!(s.codex_reasoning_guard_custom_templates.is_empty());
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_continuation_repair_bumps_schema_and_defaults_disabled() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES,
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_continuation_repair(
            &mut s, true
        ));

        assert_eq!(
            s.schema_version,
            SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_CONTINUATION_REPAIR
        );
        assert!(!s.codex_reasoning_guard_continuation_repair_enabled);
        assert_eq!(
            s.codex_reasoning_guard_continuation_max_rounds,
            DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_ROUNDS
        );
        assert_eq!(
            s.codex_reasoning_guard_continuation_max_output_tokens,
            DEFAULT_CODEX_REASONING_GUARD_CONTINUATION_MAX_OUTPUT_TOKENS
        );
    }

    #[test]
    fn migrate_unify_codex_reasoning_guard_resets_legacy_rules_before_schema_48() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_CONTINUATION_REPAIR,
            codex_reasoning_guard_rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
            codex_reasoning_guard_reasoning_equals: vec![700],
            codex_reasoning_guard_active_template_id: "custom-old".to_string(),
            codex_reasoning_guard_custom_templates: vec![CodexReasoningGuardRuleTemplate {
                id: "custom-old".to_string(),
                name: "Old custom".to_string(),
                description: "legacy custom rule".to_string(),
                rules: vec![CodexReasoningGuardTemplateRule {
                    id: "rule-old".to_string(),
                    name: "Old <=700".to_string(),
                    reasoning_tokens: Some(700),
                    ..Default::default()
                }],
            }],
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-old".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![700],
            }],
            codex_reasoning_guard_post_match_strategy:
                CodexReasoningGuardPostMatchStrategy::RetrySameProvider,
            codex_reasoning_guard_exhausted_action:
                CodexReasoningGuardExhaustedAction::SwitchProvider,
            ..Default::default()
        };

        assert!(migrate_unify_codex_reasoning_guard(&mut s, true));

        assert_eq!(s.schema_version, SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD);
        assert_eq!(
            s.codex_reasoning_guard_rule_mode,
            CodexReasoningGuardRuleMode::ReasoningTokens
        );
        assert_eq!(
            s.codex_reasoning_guard_compare_mode,
            CodexReasoningGuardCompareMode::Equals
        );
        assert_eq!(
            s.codex_reasoning_guard_reasoning_equals,
            DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
        );
        assert!(s.codex_reasoning_guard_model_rules.is_empty());
        assert_eq!(
            s.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
        );
        assert!(s.codex_reasoning_guard_custom_templates.is_empty());
        assert_eq!(
            s.codex_reasoning_guard_post_match_strategy,
            CodexReasoningGuardPostMatchStrategy::ContinuationRepair
        );
        assert_eq!(
            s.codex_reasoning_guard_exhausted_action,
            CodexReasoningGuardExhaustedAction::ReturnError
        );
    }

    #[test]
    fn migrate_unify_codex_reasoning_guard_does_not_overwrite_schema_48_settings() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD,
            codex_reasoning_guard_active_template_id: "custom-current".to_string(),
            codex_reasoning_guard_custom_templates: vec![CodexReasoningGuardRuleTemplate {
                id: "custom-current".to_string(),
                name: "Current custom".to_string(),
                description: "user edited after upgrade".to_string(),
                ..Default::default()
            }],
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-current".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![1552],
            }],
            codex_reasoning_guard_post_match_strategy:
                CodexReasoningGuardPostMatchStrategy::RetrySameProvider,
            codex_reasoning_guard_exhausted_action:
                CodexReasoningGuardExhaustedAction::SwitchProvider,
            ..Default::default()
        };

        assert!(!migrate_unify_codex_reasoning_guard(&mut s, true));

        assert_eq!(s.codex_reasoning_guard_active_template_id, "custom-current");
        assert_eq!(s.codex_reasoning_guard_custom_templates.len(), 1);
        assert_eq!(s.codex_reasoning_guard_model_rules.len(), 1);
        assert_eq!(
            s.codex_reasoning_guard_post_match_strategy,
            CodexReasoningGuardPostMatchStrategy::RetrySameProvider
        );
        assert_eq!(
            s.codex_reasoning_guard_exhausted_action,
            CodexReasoningGuardExhaustedAction::SwitchProvider
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_maps_feature_mode_template() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_rule_mode: CodexReasoningGuardRuleMode::FinalAnswerOnlyHighXhigh,
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut s, true
        ));
        assert_eq!(
            s.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_FINAL_ANSWER_ONLY_HIGH_XHIGH_ID
        );
        assert!(s.codex_reasoning_guard_custom_templates.is_empty());
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_converts_custom_global_equals() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![777, 888],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut s, true
        ));
        assert_eq!(
            s.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        assert_eq!(s.codex_reasoning_guard_custom_templates.len(), 1);
        assert_eq!(
            s.codex_reasoning_guard_custom_templates[0]
                .rules
                .iter()
                .filter_map(|rule| rule.reasoning_tokens)
                .collect::<Vec<_>>(),
            vec![777, 888]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_deduplicates_custom_global_equals() {
        let mut s = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![777, 777, 888, 777],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut s, true
        ));
        assert_eq!(
            s.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        assert_eq!(
            s.codex_reasoning_guard_custom_templates[0]
                .rules
                .iter()
                .filter_map(|rule| rule.reasoning_tokens)
                .collect::<Vec<_>>(),
            vec![777, 888]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_converts_global_less_than_or_equal() {
        let mut less_equal = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
            codex_reasoning_guard_reasoning_equals: vec![700, 900],
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut less_equal,
            true
        ));
        assert_eq!(
            less_equal.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        let template = &less_equal.codex_reasoning_guard_custom_templates[0];
        assert_eq!(
            template
                .rules
                .iter()
                .flat_map(|rule| {
                    assert_eq!(rule.reasoning_tokens, None);
                    rule.filters.iter()
                })
                .map(|filter| {
                    assert_eq!(
                        filter.field,
                        CodexReasoningGuardTemplateFilterField::ReasoningTokens
                    );
                    assert_eq!(
                        filter.operator,
                        CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
                    );
                    filter.number_value
                })
                .collect::<Vec<_>>(),
            vec![Some(700.0), Some(900.0)]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_converts_model_equals_rules() {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-5.5".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![256],
            }],
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        assert_eq!(
            model_rule.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let model_specific = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(256))
            .expect("model-specific token rule should be generated");
        assert_eq!(
            model_specific.filters[0].field,
            CodexReasoningGuardTemplateFilterField::RequestedModel
        );
        assert_eq!(
            model_specific.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::Equals
        );
        assert_eq!(
            model_specific.filters[0].string_value.as_deref(),
            Some("gpt-5.5")
        );
        let global_fallback = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(516))
            .expect("global fallback token rule should be generated");
        assert_eq!(
            global_fallback.filters[0].field,
            CodexReasoningGuardTemplateFilterField::RequestedModel
        );
        assert_eq!(
            global_fallback.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::NotEquals
        );
        assert_eq!(
            global_fallback.filters[0].string_value.as_deref(),
            Some("gpt-5.5")
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_groups_model_equals_rules_by_token() {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-5.5".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: vec![256],
                },
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-5.4".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: vec![256],
                },
            ],
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));

        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let model_specific = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(256))
            .expect("same-token model-specific rule should be generated");
        assert_eq!(model_specific.filters.len(), 1);
        assert_eq!(
            model_specific.filters[0].field,
            CodexReasoningGuardTemplateFilterField::RequestedModel
        );
        assert_eq!(
            model_specific.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::In
        );
        assert_eq!(
            model_specific.filters[0].string_values,
            vec!["gpt-5.5".to_string(), "gpt-5.4".to_string()]
        );
        let global_fallback = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(516))
            .expect("global fallback token rule should be generated");
        assert_eq!(
            global_fallback.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::NotIn
        );
        assert_eq!(
            global_fallback.filters[0].string_values,
            vec!["gpt-5.5".to_string(), "gpt-5.4".to_string()]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_merges_overlapping_global_and_model_tokens()
    {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516, 1034],
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-5.5".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![516],
            }],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        assert_eq!(
            template
                .rules
                .iter()
                .filter(|rule| rule.reasoning_tokens == Some(516))
                .count(),
            1
        );
        let overlapping_rule = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(516))
            .expect("overlapping token rule should be generated");
        assert_eq!(
            overlapping_rule.logic,
            CodexReasoningGuardTemplateRuleLogic::Or
        );
        assert!(overlapping_rule.filters.iter().any(|filter| {
            filter.operator == CodexReasoningGuardTemplateFilterOperator::Equals
                && filter.string_value.as_deref() == Some("gpt-5.5")
        }));
        assert!(overlapping_rule.filters.iter().any(|filter| {
            filter.operator == CodexReasoningGuardTemplateFilterOperator::NotEquals
                && filter.string_value.as_deref() == Some("gpt-5.5")
        }));

        let global_only = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(1034))
            .expect("global fallback token rule should be generated");
        assert_eq!(
            global_only.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::NotEquals
        );
        assert_eq!(
            global_only.filters[0].string_value.as_deref(),
            Some("gpt-5.5")
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_mixes_single_model_lte_with_exact_rules() {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-lte".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                    reasoning_equals: vec![700, 900],
                },
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-exact".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: vec![256],
                },
            ],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let wildcard_thresholds = template
            .rules
            .iter()
            .filter(|rule| {
                rule.reasoning_tokens.is_none()
                    && rule.filters.iter().any(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::RequestedModel
                            && filter.operator == CodexReasoningGuardTemplateFilterOperator::Equals
                            && filter.string_value.as_deref() == Some("gpt-lte")
                    })
            })
            .map(|rule| {
                rule.filters
                    .iter()
                    .find(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::ReasoningTokens
                    })
                    .and_then(|filter| filter.number_value)
            })
            .collect::<Vec<_>>();
        assert_eq!(wildcard_thresholds, vec![Some(700.0), Some(900.0)]);

        let global_fallback = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(516))
            .expect("global fallback token rule should be generated");
        assert_eq!(
            global_fallback.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::NotIn
        );
        assert_eq!(
            global_fallback.filters[0].string_values,
            vec!["gpt-lte".to_string(), "gpt-exact".to_string()]
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_converts_global_lte_with_exact_model_rules()
    {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
            codex_reasoning_guard_reasoning_equals: vec![700, 900],
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-exact".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::Equals,
                reasoning_equals: vec![256],
            }],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let model_exact = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(256))
            .expect("model exact rule should be generated");
        assert_eq!(
            model_exact.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::Equals
        );
        assert_eq!(
            model_exact.filters[0].string_value.as_deref(),
            Some("gpt-exact")
        );

        let global_thresholds = template
            .rules
            .iter()
            .filter(|rule| {
                rule.reasoning_tokens.is_none()
                    && rule.filters.iter().any(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::RequestedModel
                            && filter.operator
                                == CodexReasoningGuardTemplateFilterOperator::NotEquals
                            && filter.string_value.as_deref() == Some("gpt-exact")
                    })
            })
            .map(|rule| {
                rule.filters
                    .iter()
                    .find(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::ReasoningTokens
                    })
                    .and_then(|filter| filter.number_value)
            })
            .collect::<Vec<_>>();
        assert_eq!(global_thresholds, vec![Some(700.0), Some(900.0)]);
    }

    #[test]
    fn repair_settings_unifies_schema_46_global_lte_with_exact_model_rule() {
        let raw = serde_json::json!({
            "schema_version": SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES,
            "codex_reasoning_guard_active_template_id": CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID,
            "codex_reasoning_guard_rule_mode": "reasoning_tokens",
            "codex_reasoning_guard_compare_mode": "less_than_or_equal",
            "codex_reasoning_guard_reasoning_equals": [700, 900],
            "codex_reasoning_guard_model_rules": [
                {
                    "requested_model": "gpt-exact",
                    "compare_mode": "equals",
                    "reasoning_equals": [256]
                }
            ]
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();

        assert!(repair_settings(&mut settings, true, &raw).unwrap());
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD
        );
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
        );
        assert!(settings.codex_reasoning_guard_custom_templates.is_empty());
        assert!(settings.codex_reasoning_guard_model_rules.is_empty());
        assert_eq!(
            settings.codex_reasoning_guard_reasoning_equals,
            DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
        );
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_preserves_legacy_rules_when_old_template_slots_full(
    ) {
        const LEGACY_CUSTOM_TEMPLATE_CAP: usize = 16;
        let mut settings = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![777],
            codex_reasoning_guard_custom_templates: (0..LEGACY_CUSTOM_TEMPLATE_CAP)
                .map(|index| CodexReasoningGuardRuleTemplate {
                    id: format!("custom-template-{index}"),
                    name: format!("Custom template {index}"),
                    description: String::new(),
                    rules: vec![CodexReasoningGuardTemplateRule {
                        id: format!("rule-{index}"),
                        name: format!("rule {index}"),
                        reasoning_tokens: Some(index as i64),
                        reasoning_tokens_formula: None,
                        action: CodexReasoningGuardTemplateRuleAction::Intercept,
                        logic: CodexReasoningGuardTemplateRuleLogic::And,
                        filters: Vec::new(),
                    }],
                })
                .collect(),
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut settings,
            true
        ));
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        assert_eq!(
            settings.codex_reasoning_guard_custom_templates.len(),
            LEGACY_CUSTOM_TEMPLATE_CAP + 1
        );
        assert!(settings
            .codex_reasoning_guard_custom_templates
            .iter()
            .any(|template| template.id == "custom-legacy-reasoning-tokens"
                && template
                    .rules
                    .iter()
                    .any(|rule| rule.reasoning_tokens == Some(777))));
        super::super::persistence::validate_bounds(&settings).unwrap();
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_preserves_rules_above_old_template_cap() {
        const LEGACY_TEMPLATE_RULE_CAP: usize = 64;
        let values = (0..=LEGACY_TEMPLATE_RULE_CAP)
            .map(|value| value as i64 + 10)
            .collect::<Vec<_>>();
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: values
                .chunks(MAX_CODEX_REASONING_GUARD_REASONING_EQUALS_LEN)
                .enumerate()
                .map(|(index, chunk)| CodexReasoningGuardModelRule {
                    requested_model: format!("gpt-many-{index}"),
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: chunk.to_vec(),
                })
                .collect(),
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        assert_eq!(
            model_rule.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        assert!(template.rules.len() > LEGACY_TEMPLATE_RULE_CAP);
        assert!(template
            .rules
            .iter()
            .any(|rule| rule.reasoning_tokens == Some(10)));
        assert!(template
            .rules
            .iter()
            .any(|rule| rule.reasoning_tokens == Some(74)));
        super::super::persistence::validate_bounds(&model_rule).unwrap();
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_keeps_generated_model_rules_valid() {
        let long_model_a = format!("gpt-{}", "a".repeat(120));
        let long_model_b = format!("gpt-{}", "b".repeat(120));
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![
                CodexReasoningGuardModelRule {
                    requested_model: long_model_a,
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: vec![256],
                },
                CodexReasoningGuardModelRule {
                    requested_model: long_model_b,
                    compare_mode: CodexReasoningGuardCompareMode::Equals,
                    reasoning_equals: vec![256],
                },
            ],
            ..Default::default()
        };

        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        super::super::persistence::validate_bounds(&model_rule).unwrap();
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_converts_single_model_less_than_or_equal_rule(
    ) {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_model_rules: vec![CodexReasoningGuardModelRule {
                requested_model: "gpt-5.5".to_string(),
                compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                reasoning_equals: vec![700, 900],
            }],
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        assert_eq!(
            model_rule.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let wildcard_thresholds = template
            .rules
            .iter()
            .filter(|rule| {
                rule.reasoning_tokens.is_none()
                    && rule.filters.iter().any(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::RequestedModel
                            && filter.string_value.as_deref() == Some("gpt-5.5")
                    })
            })
            .map(|rule| {
                rule.filters
                    .iter()
                    .find(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::ReasoningTokens
                    })
                    .and_then(|filter| filter.number_value)
            })
            .collect::<Vec<_>>();
        assert_eq!(wildcard_thresholds, vec![Some(700.0), Some(900.0)]);
    }

    #[test]
    fn migrate_add_codex_reasoning_guard_rule_templates_groups_model_lte_rules_by_threshold() {
        let mut model_rule = AppSettings {
            schema_version: SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_MODE,
            codex_reasoning_guard_reasoning_equals: vec![516],
            codex_reasoning_guard_model_rules: vec![
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-lte-a".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                    reasoning_equals: vec![700],
                },
                CodexReasoningGuardModelRule {
                    requested_model: "gpt-lte-b".to_string(),
                    compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
                    reasoning_equals: vec![700],
                },
            ],
            ..Default::default()
        };
        assert!(migrate_add_codex_reasoning_guard_rule_templates(
            &mut model_rule,
            true
        ));
        assert_eq!(
            model_rule.codex_reasoning_guard_active_template_id,
            "custom-legacy-reasoning-tokens"
        );
        let template = &model_rule.codex_reasoning_guard_custom_templates[0];
        let threshold_rule = template
            .rules
            .iter()
            .find(|rule| {
                rule.reasoning_tokens.is_none()
                    && rule.filters.iter().any(|filter| {
                        filter.field == CodexReasoningGuardTemplateFilterField::ReasoningTokens
                            && filter.operator
                                == CodexReasoningGuardTemplateFilterOperator::LessThanOrEqual
                            && filter.number_value == Some(700.0)
                    })
            })
            .expect("grouped model LTE threshold rule should be generated");
        let model_filter = threshold_rule
            .filters
            .iter()
            .find(|filter| filter.field == CodexReasoningGuardTemplateFilterField::RequestedModel)
            .expect("grouped threshold should keep a requested-model filter");
        assert_eq!(
            model_filter.operator,
            CodexReasoningGuardTemplateFilterOperator::In
        );
        assert_eq!(
            model_filter.string_values,
            vec!["gpt-lte-a".to_string(), "gpt-lte-b".to_string()]
        );

        let global_fallback = template
            .rules
            .iter()
            .find(|rule| rule.reasoning_tokens == Some(516))
            .expect("global fallback token rule should be generated");
        assert_eq!(
            global_fallback.filters[0].operator,
            CodexReasoningGuardTemplateFilterOperator::NotIn
        );
        assert_eq!(
            global_fallback.filters[0].string_values,
            vec!["gpt-lte-a".to_string(), "gpt-lte-b".to_string()]
        );
    }

    #[test]
    fn repair_settings_unifies_schema_46_legacy_rule_template_selection() {
        let raw = serde_json::json!({
            "schema_version": SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES,
            "codex_reasoning_guard_active_template_id": CODEX_REASONING_GUARD_TEMPLATE_LEGACY_COMPATIBILITY_ID,
            "codex_reasoning_guard_rule_mode": "reasoning_tokens",
            "codex_reasoning_guard_compare_mode": "less_than_or_equal",
            "codex_reasoning_guard_reasoning_equals": [700]
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();

        assert!(repair_settings(&mut settings, true, &raw).unwrap());
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD
        );
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
        );
        assert!(settings.codex_reasoning_guard_custom_templates.is_empty());
        assert_eq!(
            settings.codex_reasoning_guard_reasoning_equals,
            DEFAULT_CODEX_REASONING_GUARD_REASONING_EQUALS
        );
    }

    #[test]
    fn repair_rule_template_selection_preserves_active_custom_template() {
        let mut settings = AppSettings {
            codex_reasoning_guard_active_template_id: "custom-current".to_string(),
            codex_reasoning_guard_compare_mode: CodexReasoningGuardCompareMode::LessThanOrEqual,
            codex_reasoning_guard_reasoning_equals: vec![700],
            ..Default::default()
        };

        assert!(!repair_codex_reasoning_guard_rule_template_selection(
            &mut settings,
            true,
            &serde_json::json!({"schema_version": SCHEMA_VERSION})
        ));
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            "custom-current"
        );
        assert!(settings.codex_reasoning_guard_custom_templates.is_empty());
    }

    #[test]
    fn repair_settings_unifies_schema_46_builtin_legacy_selection() {
        let raw = serde_json::json!({
            "schema_version": SCHEMA_VERSION_ADD_CODEX_REASONING_GUARD_RULE_TEMPLATES,
            "codex_reasoning_guard_active_template_id": CODEX_REASONING_GUARD_TEMPLATE_LEGACY_REASONING_TOKENS_ID,
            "codex_reasoning_guard_rule_mode": "reasoning_tokens",
            "codex_reasoning_guard_compare_mode": "less_than_or_equal",
            "codex_reasoning_guard_reasoning_equals": [700],
            "codex_reasoning_guard_model_rules": [
                {
                    "requested_model": "gpt-stale",
                    "compare_mode": "equals",
                    "reasoning_equals": [256]
                }
            ]
        });
        let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap();

        assert!(repair_settings(&mut settings, true, &raw).unwrap());
        assert_eq!(
            settings.schema_version,
            SCHEMA_VERSION_UNIFY_CODEX_REASONING_GUARD
        );
        assert_eq!(
            settings.codex_reasoning_guard_active_template_id,
            CODEX_REASONING_GUARD_TEMPLATE_REASONING_TOKENS_518N_MINUS_2_ID
        );
        assert!(settings.codex_reasoning_guard_custom_templates.is_empty());
        assert!(settings.codex_reasoning_guard_model_rules.is_empty());
    }

    #[test]
    fn normalize_codex_home_override_keeps_directory_input() {
        assert_eq!(
            normalize_codex_home_override(r"  C:\Users\me\.codex  "),
            r"C:\Users\me\.codex"
        );
    }

    #[test]
    fn normalize_codex_home_override_converts_config_toml_to_parent_dir() {
        assert_eq!(
            normalize_codex_home_override(r"C:\Users\me\.codex\config.toml"),
            r"C:\Users\me\.codex"
        );
    }

    #[test]
    fn sanitize_codex_home_override_trims_and_normalizes() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::Custom,
            codex_home_override: " ~/.codex/config.toml ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_override, "~/.codex");
    }

    #[test]
    fn sanitize_codex_home_override_demotes_empty_custom_mode_to_user_home_default() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::Custom,
            codex_home_override: "   ".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_mode, CodexHomeMode::UserHomeDefault);
        assert!(s.codex_home_override.is_empty());
    }

    #[test]
    fn sanitize_codex_home_override_clears_override_when_mode_is_not_custom() {
        let mut s = AppSettings {
            codex_home_mode: CodexHomeMode::FollowCodexHome,
            codex_home_override: r"D:\Work\.codex".to_string(),
            ..Default::default()
        };
        assert!(sanitize_codex_home_override(&mut s));
        assert_eq!(s.codex_home_mode, CodexHomeMode::FollowCodexHome);
        assert!(s.codex_home_override.is_empty());
    }
}
