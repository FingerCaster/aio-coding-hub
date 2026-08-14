use crate::gateway::proxy::gemini_oauth::GeminiOAuthResponseMode;
use serde_json::Value;

const MAX_REASONING_EFFORT_CHARS: usize = 64;

fn text_at<'a>(body: &'a Value, path: &[&str]) -> Option<&'a str> {
    let value = path.iter().try_fold(body, |value, key| value.get(*key))?;
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn extract(
    body: &[u8],
    upstream_path: &str,
    gemini_oauth_mode: Option<GeminiOAuthResponseMode>,
) -> Option<String> {
    if matches!(
        gemini_oauth_mode,
        Some(GeminiOAuthResponseMode::CountTokens)
    ) {
        return None;
    }

    let body: Value = serde_json::from_slice(body).ok()?;
    let value = match gemini_oauth_mode {
        Some(GeminiOAuthResponseMode::GenerateContent)
        | Some(GeminiOAuthResponseMode::StreamGenerateContent) => text_at(
            &body,
            &[
                "request",
                "generationConfig",
                "thinkingConfig",
                "thinkingLevel",
            ],
        ),
        Some(GeminiOAuthResponseMode::CountTokens) => None,
        None => {
            let path = upstream_path.trim_end_matches('/');
            if path.ends_with("/responses") {
                text_at(&body, &["reasoning", "effort"])
            } else if path.ends_with("/chat/completions") {
                text_at(&body, &["reasoning_effort"])
            } else if path.ends_with("/messages") {
                text_at(&body, &["output_config", "effort"])
            } else if path.ends_with(":generateContent") || path.ends_with(":streamGenerateContent")
            {
                text_at(
                    &body,
                    &["generationConfig", "thinkingConfig", "thinkingLevel"],
                )
            } else {
                None
            }
        }
    }?;

    Some(value.chars().take(MAX_REASONING_EFFORT_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_value(
        body: &Value,
        upstream_path: &str,
        gemini_oauth_mode: Option<GeminiOAuthResponseMode>,
    ) -> Option<String> {
        extract(
            &serde_json::to_vec(body).expect("serialize request body"),
            upstream_path,
            gemini_oauth_mode,
        )
    }

    #[test]
    fn extracts_final_fields_for_claude_codex_chat_and_gemini() {
        let body = serde_json::json!({
            "reasoning": { "effort": " high " },
            "reasoning_effort": "low",
            "output_config": { "effort": "max" },
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "medium", "thinkingBudget": 8192 }
            }
        });

        assert_eq!(
            extract_value(&body, "/v1/responses", None).as_deref(),
            Some("high")
        );
        assert_eq!(
            extract_value(&body, "/v1/chat/completions", None).as_deref(),
            Some("low")
        );
        assert_eq!(
            extract_value(&body, "/v1/messages", None).as_deref(),
            Some("max")
        );
        assert_eq!(
            extract_value(&body, "/v1beta/models/gemini:generateContent", None).as_deref(),
            Some("medium")
        );
    }

    #[test]
    fn extracts_wrapped_gemini_and_ignores_count_tokens() {
        let wrapped = serde_json::json!({
            "request": {
                "generationConfig": {
                    "thinkingConfig": { "thinkingLevel": "xhigh", "thinkingBudget": 8192 }
                }
            }
        });

        assert_eq!(
            extract_value(
                &wrapped,
                "/v1internal:generateContent",
                Some(GeminiOAuthResponseMode::GenerateContent)
            )
            .as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            extract_value(
                &wrapped,
                "/v1internal:countTokens",
                Some(GeminiOAuthResponseMode::CountTokens)
            ),
            None
        );
    }

    #[test]
    fn ignores_empty_non_string_and_inferred_thinking_values() {
        for body in [
            serde_json::json!({"reasoning": {"effort": "  "}}),
            serde_json::json!({"reasoning": {"effort": 8}}),
            serde_json::json!({"thinking": {"type": "enabled", "budget_tokens": 8192}}),
        ] {
            assert_eq!(extract_value(&body, "/v1/responses", None), None);
        }

        assert_eq!(
            extract_value(
                &serde_json::json!({
                    "output_config": {},
                    "thinking": {"type": "adaptive", "budget_tokens": 8192}
                }),
                "/v1/messages",
                None
            ),
            None
        );
        assert_eq!(
            extract_value(
                &serde_json::json!({
                    "generationConfig": {"thinkingConfig": {"thinkingBudget": 8192}}
                }),
                "/v1beta/models/gemini:generateContent",
                None
            ),
            None
        );
    }

    #[test]
    fn preserves_future_explicit_string_values() {
        let body = serde_json::json!({"reasoning": {"effort": "future-ultra"}});
        assert_eq!(
            extract_value(&body, "/responses", None).as_deref(),
            Some("future-ultra")
        );
    }

    #[test]
    fn bounds_persisted_effort_evidence() {
        let effort = "x".repeat(MAX_REASONING_EFFORT_CHARS + 1);
        let body = serde_json::json!({"reasoning": {"effort": effort}});
        let extracted = extract_value(&body, "/responses", None).expect("explicit effort");

        assert_eq!(extracted.chars().count(), MAX_REASONING_EFFORT_CHARS);
    }

    #[test]
    fn observes_effort_from_the_final_cx2cc_bridged_request() {
        use crate::gateway::proxy::protocol_bridge::{get_bridge, BridgeContext};

        let ctx = BridgeContext {
            claude_models: crate::domain::providers::ClaudeModels::default(),
            cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
            requested_model: Some("claude-sonnet-4-20250514".into()),
            mapped_model: None,
            stream_requested: false,
            is_chatgpt_backend: false,
        };
        let bridge = get_bridge("cx2cc").expect("CX2CC bridge");

        let translated = bridge
            .translate_request(
                serde_json::json!({
                    "model": "claude-sonnet-4-20250514",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "thinking": {"type": "adaptive"},
                    "output_config": {"effort": "future-effort"}
                }),
                &ctx,
            )
            .expect("translate Anthropic request");

        assert_eq!(translated.target_path, "/v1/responses");
        assert_eq!(
            extract_value(&translated.body, &translated.target_path, None).as_deref(),
            Some("future-effort")
        );

        let thinking_only = bridge
            .translate_request(
                serde_json::json!({
                    "model": "claude-sonnet-4-20250514",
                    "max_tokens": 1024,
                    "messages": [{"role": "user", "content": "Hello"}],
                    "thinking": {"type": "enabled", "budget_tokens": 8192}
                }),
                &ctx,
            )
            .expect("translate Anthropic request");

        assert_eq!(
            extract_value(&thinking_only.body, &thinking_only.target_path, None),
            None
        );
    }
}
