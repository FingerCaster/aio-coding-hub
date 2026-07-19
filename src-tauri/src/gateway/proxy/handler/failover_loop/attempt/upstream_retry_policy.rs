//! Usage: Shared upstream transient retry policy decisions.

use super::*;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryPolicyMatch {
    HttpRule,
    Transport(crate::settings::UpstreamTransportRetryKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HttpRetryRuleMatch<'a> {
    pub(super) rule_index: usize,
    pub(super) description: &'a str,
}

fn enabled_http_rules_for_status(
    policy: &crate::settings::UpstreamRetryPolicy,
    status: u16,
) -> impl Iterator<Item = (usize, &crate::settings::UpstreamHttpRetryRule)> {
    policy
        .http_rules
        .iter()
        .enumerate()
        .filter(move |(_, rule)| rule.enabled && rule.status_code == status)
}

pub(super) fn match_code_only_http_retry_rule(
    policy: &crate::settings::UpstreamRetryPolicy,
    status: u16,
) -> Option<HttpRetryRuleMatch<'_>> {
    if !policy.enabled {
        return None;
    }
    enabled_http_rules_for_status(policy, status)
        .find(|(_, rule)| rule.body_contains.is_empty())
        .map(|(rule_index, rule)| HttpRetryRuleMatch {
            rule_index,
            description: &rule.description,
        })
}

pub(super) fn has_content_http_retry_rule(
    policy: &crate::settings::UpstreamRetryPolicy,
    status: u16,
) -> bool {
    policy.enabled
        && enabled_http_rules_for_status(policy, status)
            .any(|(_, rule)| !rule.body_contains.is_empty())
}

pub(super) fn match_content_http_retry_rule<'a>(
    policy: &'a crate::settings::UpstreamRetryPolicy,
    status: u16,
    body: &[u8],
) -> Option<HttpRetryRuleMatch<'a>> {
    if !policy.enabled || body.is_empty() {
        return None;
    }
    let haystack = String::from_utf8_lossy(body).to_lowercase();
    enabled_http_rules_for_status(policy, status)
        .filter(|(_, rule)| !rule.body_contains.is_empty())
        .find(|(_, rule)| {
            rule.body_contains
                .iter()
                .any(|needle| haystack.contains(&needle.to_lowercase()))
        })
        .map(|(rule_index, rule)| HttpRetryRuleMatch {
            rule_index,
            description: &rule.description,
        })
}

pub(super) fn retry_rule_reason(matched: HttpRetryRuleMatch<'_>) -> String {
    let description: String = matched
        .description
        .chars()
        .filter(|character| !character.is_control())
        .take(crate::settings::MAX_UPSTREAM_RETRY_POLICY_DESCRIPTION_CHARS)
        .collect::<String>()
        .trim()
        .to_string();
    // Attempt reasons use comma-delimited key=value fields downstream.
    let description = description
        .replace('%', "%25")
        .replace(',', "%2C")
        .replace('=', "%3D");
    if description.is_empty() {
        format!("retry_rule={}", matched.rule_index + 1)
    } else {
        format!(
            "retry_rule={} retry_rule_description={description}",
            matched.rule_index + 1
        )
    }
}

pub(super) fn policy_matches(
    policy: &crate::settings::UpstreamRetryPolicy,
    matched: RetryPolicyMatch,
    configured_retries_used: u32,
) -> bool {
    if !policy.enabled || configured_retries_used >= policy.max_retries {
        return false;
    }

    match matched {
        RetryPolicyMatch::HttpRule => true,
        RetryPolicyMatch::Transport(kind) => policy.transport_errors.contains(&kind),
    }
}

pub(super) fn should_retry_same_provider(
    policy: &crate::settings::UpstreamRetryPolicy,
    matched: RetryPolicyMatch,
    configured_retries_used: u32,
    retry_index: u32,
    max_attempts_per_provider: u32,
) -> bool {
    policy_matches(policy, matched, configured_retries_used)
        && retry_index < max_attempts_per_provider
}

pub(super) fn transient_failure_decision(
    is_count_tokens: bool,
    matched: RetryPolicyMatch,
    policy: &crate::settings::UpstreamRetryPolicy,
    configured_retries_used: u32,
    retry_index: u32,
    max_attempts_per_provider: u32,
) -> (FailoverDecision, bool) {
    if is_count_tokens {
        return (FailoverDecision::Abort, false);
    }

    if should_retry_same_provider(
        policy,
        matched,
        configured_retries_used,
        retry_index,
        max_attempts_per_provider,
    ) {
        return (FailoverDecision::RetrySameProvider, true);
    }

    (FailoverDecision::SwitchProvider, false)
}

pub(super) fn retry_policy_backoff_delay(
    policy: &crate::settings::UpstreamRetryPolicy,
) -> Option<Duration> {
    (policy.backoff_ms > 0).then(|| Duration::from_millis(policy.backoff_ms as u64))
}

pub(super) fn should_record_circuit_failure(
    policy: &crate::settings::UpstreamRetryPolicy,
    configured_retry: bool,
) -> bool {
    !configured_retry || policy.counts_toward_circuit_breaker
}

#[cfg(test)]
mod tests {
    use super::{
        has_content_http_retry_rule, match_code_only_http_retry_rule,
        match_content_http_retry_rule, retry_rule_reason, should_record_circuit_failure,
        transient_failure_decision, FailoverDecision, RetryPolicyMatch,
    };
    use crate::settings::{UpstreamHttpRetryRule, UpstreamRetryPolicy, UpstreamTransportRetryKind};

    #[test]
    fn transient_failure_decision_retries_default_http_and_transport_once() {
        for matched in [
            RetryPolicyMatch::HttpRule,
            RetryPolicyMatch::Transport(UpstreamTransportRetryKind::Read),
            RetryPolicyMatch::Transport(UpstreamTransportRetryKind::Timeout),
        ] {
            let (decision, configured_retry) = transient_failure_decision(
                false,
                matched,
                &UpstreamRetryPolicy::default(),
                0,
                1,
                2,
            );
            assert!(matches!(decision, FailoverDecision::RetrySameProvider));
            assert!(configured_retry);
        }
    }

    #[test]
    fn transient_failure_decision_switches_when_policy_is_disabled_or_unmatched() {
        let disabled = UpstreamRetryPolicy {
            enabled: false,
            ..Default::default()
        };
        let (disabled_decision, disabled_retry) =
            transient_failure_decision(false, RetryPolicyMatch::HttpRule, &disabled, 0, 1, 2);
        assert!(matches!(
            disabled_decision,
            FailoverDecision::SwitchProvider
        ));
        assert!(!disabled_retry);

        let unmatched = UpstreamRetryPolicy {
            transport_errors: vec![UpstreamTransportRetryKind::Connect],
            ..Default::default()
        };
        let (unmatched_decision, unmatched_retry) = transient_failure_decision(
            false,
            RetryPolicyMatch::Transport(UpstreamTransportRetryKind::Timeout),
            &unmatched,
            0,
            1,
            2,
        );
        assert!(matches!(
            unmatched_decision,
            FailoverDecision::SwitchProvider
        ));
        assert!(!unmatched_retry);
    }

    #[test]
    fn transient_failure_decision_respects_attempt_limits_and_count_tokens_abort() {
        let (at_limit_decision, at_limit_retry) = transient_failure_decision(
            false,
            RetryPolicyMatch::HttpRule,
            &UpstreamRetryPolicy::default(),
            0,
            2,
            2,
        );
        assert!(matches!(
            at_limit_decision,
            FailoverDecision::SwitchProvider
        ));
        assert!(!at_limit_retry);

        let (count_tokens_decision, count_tokens_retry) = transient_failure_decision(
            true,
            RetryPolicyMatch::HttpRule,
            &UpstreamRetryPolicy::default(),
            0,
            1,
            2,
        );
        assert!(matches!(count_tokens_decision, FailoverDecision::Abort));
        assert!(!count_tokens_retry);
    }

    #[test]
    fn configured_retry_budget_is_independent_from_total_attempt_index() {
        let policy = UpstreamRetryPolicy::default();
        let (available, configured_retry) =
            transient_failure_decision(false, RetryPolicyMatch::HttpRule, &policy, 0, 2, 3);
        assert!(matches!(available, FailoverDecision::RetrySameProvider));
        assert!(configured_retry);

        let (exhausted, configured_retry) =
            transient_failure_decision(false, RetryPolicyMatch::HttpRule, &policy, 1, 2, 3);
        assert!(matches!(exhausted, FailoverDecision::SwitchProvider));
        assert!(!configured_retry);
    }

    #[test]
    fn should_record_circuit_failure_skips_only_configured_retries_when_requested() {
        let default_policy = UpstreamRetryPolicy::default();
        assert!(!should_record_circuit_failure(&default_policy, true));
        assert!(should_record_circuit_failure(&default_policy, false));

        let counted_policy = UpstreamRetryPolicy {
            counts_toward_circuit_breaker: true,
            ..Default::default()
        };
        assert!(should_record_circuit_failure(&counted_policy, true));
    }

    #[test]
    fn http_rules_match_code_only_and_content_conditions() {
        let policy = UpstreamRetryPolicy {
            http_rules: vec![
                UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 429,
                    body_contains: vec!["quota".to_string(), "temporary".to_string()],
                    description: "quota retry".to_string(),
                },
                UpstreamHttpRetryRule::status_only(503),
            ],
            transport_errors: Vec::new(),
            ..Default::default()
        };

        assert!(match_code_only_http_retry_rule(&policy, 429).is_none());
        assert!(has_content_http_retry_rule(&policy, 429));
        let matched = match_content_http_retry_rule(
            &policy,
            429,
            br#"{"error":{"message":"QUOTA ExHaUsTeD"}}"#,
        )
        .expect("content match");
        assert_eq!(matched.rule_index, 0);
        assert_eq!(
            retry_rule_reason(matched),
            "retry_rule=1 retry_rule_description=quota retry"
        );

        let code_only = match_code_only_http_retry_rule(&policy, 503).expect("code-only match");
        assert_eq!(code_only.rule_index, 1);
        assert!(match_content_http_retry_rule(&policy, 429, b"").is_none());
        assert!(match_content_http_retry_rule(&policy, 429, b"rate limited").is_none());
    }

    #[test]
    fn http_rules_use_literal_or_matching_and_ignore_disabled_rules() {
        let policy = UpstreamRetryPolicy {
            http_rules: vec![
                UpstreamHttpRetryRule {
                    enabled: false,
                    status_code: 500,
                    body_contains: Vec::new(),
                    description: String::new(),
                },
                UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 500,
                    body_contains: vec!["[a-z]+".to_string(), "*.json".to_string()],
                    description: "line\nbreak".to_string(),
                },
            ],
            transport_errors: Vec::new(),
            ..Default::default()
        };

        assert!(match_code_only_http_retry_rule(&policy, 500).is_none());
        assert!(match_content_http_retry_rule(&policy, 500, b"value abc").is_none());
        let matched = match_content_http_retry_rule(&policy, 500, b"literal *.JSON marker")
            .expect("literal wildcard match");
        assert_eq!(
            retry_rule_reason(matched),
            "retry_rule=2 retry_rule_description=linebreak"
        );
    }

    #[test]
    fn retry_rule_reason_escapes_request_log_field_delimiters() {
        let reason = retry_rule_reason(super::HttpRetryRuleMatch {
            rule_index: 0,
            description: "quota, rule=fake upstream_body=SYNTHETIC%VALUE",
        });

        assert_eq!(
            reason,
            "retry_rule=1 retry_rule_description=quota%2C rule%3Dfake upstream_body%3DSYNTHETIC%25VALUE"
        );
        assert!(!reason.contains(", rule="));
        assert!(!reason.contains("upstream_body="));
    }

    #[test]
    fn http_rules_or_across_contents_and_rules() {
        let policy = UpstreamRetryPolicy {
            http_rules: vec![
                UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 500,
                    body_contains: vec!["first".to_string(), "second".to_string()],
                    description: String::new(),
                },
                UpstreamHttpRetryRule {
                    enabled: true,
                    status_code: 500,
                    body_contains: vec!["third".to_string()],
                    description: String::new(),
                },
            ],
            transport_errors: Vec::new(),
            ..Default::default()
        };

        assert_eq!(
            match_content_http_retry_rule(&policy, 500, b"SECOND condition")
                .expect("second content matches")
                .rule_index,
            0
        );
        assert_eq!(
            match_content_http_retry_rule(&policy, 500, b"third condition")
                .expect("second rule matches")
                .rule_index,
            1
        );
    }

    #[test]
    fn http_rules_handle_non_utf8_without_panicking_and_match_only_valid_fragments() {
        let policy = UpstreamRetryPolicy {
            http_rules: vec![UpstreamHttpRetryRule {
                enabled: true,
                status_code: 500,
                body_contains: vec!["quota".to_string()],
                description: String::new(),
            }],
            transport_errors: Vec::new(),
            ..Default::default()
        };

        assert!(match_content_http_retry_rule(&policy, 500, &[0xff, 0xfe]).is_none());
        assert!(match_content_http_retry_rule(&policy, 500, b"\xffQUOTA exhausted").is_some());
    }
}
