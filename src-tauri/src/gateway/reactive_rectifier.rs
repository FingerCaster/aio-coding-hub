//! Ordered registry for the fork's existing narrow reactive rectifiers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReactiveRectifierKind {
    ThinkingSignature,
    ThinkingBudget,
}

const ANTHROPIC_REGISTRY: [ReactiveRectifierKind; 2] = [
    ReactiveRectifierKind::ThinkingSignature,
    ReactiveRectifierKind::ThinkingBudget,
];

pub(super) fn detect(
    cli_key: &str,
    message: &str,
    signature_enabled: bool,
    budget_enabled: bool,
) -> Option<(ReactiveRectifierKind, &'static str)> {
    if cli_key != "claude" {
        return None;
    }
    for kind in ANTHROPIC_REGISTRY {
        let trigger = match kind {
            ReactiveRectifierKind::ThinkingSignature if signature_enabled => {
                super::thinking_signature_rectifier::detect_trigger(message)
            }
            ReactiveRectifierKind::ThinkingBudget if budget_enabled => {
                super::thinking_budget_rectifier::detect_trigger(message)
            }
            _ => None,
        };
        if let Some(trigger) = trigger {
            return Some((kind, trigger));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_errors_and_other_protocols_do_not_gain_retries() {
        for message in [
            "invalid request",
            "invalid request: missing input",
            "unknown model",
        ] {
            assert!(detect("claude", message, true, true).is_none());
        }
        assert!(detect("codex", "invalid signature in thinking block", true, true).is_none());
    }

    #[test]
    fn registered_signature_respects_the_existing_enable_switch() {
        let message = "invalid signature in thinking block";
        assert_eq!(
            detect("claude", message, true, false).map(|hit| hit.0),
            Some(ReactiveRectifierKind::ThinkingSignature)
        );
        assert!(detect("claude", message, false, false).is_none());
    }
}
