//! End-to-end regression tests for the surviving CX2CC protocol bridge.

#[cfg(test)]
mod tests {
    use crate::gateway::proxy::protocol_bridge::{get_bridge, registry, BridgeContext};
    use crate::settings::DEFAULT_CX2CC_FALLBACK_MODEL;
    use serde_json::json;

    fn cx2cc_ctx() -> BridgeContext {
        BridgeContext {
            claude_models: crate::domain::providers::ClaudeModels::default(),
            cx2cc_settings: crate::gateway::proxy::cx2cc::settings::Cx2ccSettings::default(),
            requested_model: Some("claude-sonnet-4-20250514".into()),
            mapped_model: None,
            stream_requested: false,
            is_chatgpt_backend: false,
        }
    }

    fn cx2cc_ctx_with_legacy_settings() -> BridgeContext {
        let mut ctx = cx2cc_ctx();
        ctx.cx2cc_settings.model_reasoning_effort = Some("medium".to_string());
        ctx.cx2cc_settings.service_tier = Some("flex".to_string());
        ctx.cx2cc_settings.disable_response_storage = true;
        ctx
    }

    #[test]
    fn registry_only_exposes_the_surviving_builtin_bridge() {
        let types = registry::available_bridge_types();
        assert!(types.contains(&"cx2cc"));
        assert_eq!(get_bridge("cx2cc").unwrap().bridge_type, "cx2cc");

        for removed in [
            "codex_to_openai_chat",
            "codex_to_openai_responses",
            "codex_to_anthropic_messages",
        ] {
            assert!(!types.contains(&removed));
            assert!(get_bridge(removed).is_none());
        }
    }

    #[test]
    fn cx2cc_translates_anthropic_request_to_openai_responses() {
        let bridge = get_bridge("cx2cc").unwrap();
        let translated = bridge
            .translate_request(
                json!({
                    "model": "claude-sonnet-4-20250514",
                    "max_tokens": 1024,
                    "system": "You are helpful.",
                    "messages": [{"role": "user", "content": "Hello"}]
                }),
                &cx2cc_ctx(),
            )
            .expect("translate Anthropic request");

        assert_eq!(translated.target_path, "/v1/responses");
        assert_eq!(translated.body["model"], DEFAULT_CX2CC_FALLBACK_MODEL);
        assert_eq!(translated.body["instructions"], "You are helpful.");
        assert_eq!(translated.body["max_output_tokens"], 1024);
        assert_eq!(translated.body["input"][0]["role"], "user");
        assert_eq!(
            translated.body["input"][0]["content"][0]["type"],
            "input_text"
        );
        assert_eq!(translated.body["input"][0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn cx2cc_preserves_reasoning_state_and_effort_precedence() {
        let bridge = get_bridge("cx2cc").unwrap();
        let translate = |extra: serde_json::Value| {
            let mut body = json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello"}]
            });
            body.as_object_mut()
                .expect("request object")
                .extend(extra.as_object().expect("extra fields").clone());
            bridge
                .translate_request(body, &cx2cc_ctx_with_legacy_settings())
                .expect("translate Anthropic request")
                .body
        };

        let absent = translate(json!({}));
        assert!(absent.get("reasoning").is_none());
        assert_eq!(absent["service_tier"], "flex");
        assert_eq!(absent["store"], false);

        let disabled = translate(json!({
            "thinking": {"type": "disabled"},
            "output_config": {"effort": "high"}
        }));
        assert_eq!(disabled["reasoning"]["effort"], "none");

        let enabled_without_effort = translate(json!({
            "thinking": {"type": "enabled"}
        }));
        assert!(enabled_without_effort.get("reasoning").is_none());

        let enabled_with_effort = translate(json!({
            "thinking": {"type": "enabled"},
            "output_config": {"effort": "high"}
        }));
        assert_eq!(enabled_with_effort["reasoning"]["effort"], "high");

        let adaptive_without_effort = translate(json!({
            "thinking": {"type": "adaptive"}
        }));
        assert!(adaptive_without_effort.get("reasoning").is_none());

        let adaptive_with_unknown_effort = translate(json!({
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "future-effort"}
        }));
        assert_eq!(
            adaptive_with_unknown_effort["reasoning"]["effort"],
            "future-effort"
        );

        let unknown_effort = translate(json!({
            "output_config": {"effort": "future-effort"}
        }));
        assert_eq!(unknown_effort["reasoning"]["effort"], "future-effort");
    }

    #[test]
    fn cx2cc_round_trip_preserves_requested_model_content_and_usage() {
        let bridge = get_bridge("cx2cc").unwrap();
        let ctx = cx2cc_ctx();
        let translated_request = bridge
            .translate_request(
                json!({
                    "model": "claude-sonnet-4-20250514",
                    "max_tokens": 512,
                    "messages": [{"role": "user", "content": "Say hello"}]
                }),
                &ctx,
            )
            .expect("translate request");

        let translated_response = bridge
            .translate_response(
                json!({
                    "id": "resp_round_trip",
                    "model": translated_request.body["model"],
                    "status": "completed",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "Hello!"}]
                    }],
                    "usage": {"input_tokens": 8, "output_tokens": 2}
                }),
                &ctx,
            )
            .expect("translate response");

        assert_eq!(translated_response["type"], "message");
        assert_eq!(translated_response["model"], "claude-sonnet-4-20250514");
        assert_eq!(translated_response["stop_reason"], "end_turn");
        assert_eq!(translated_response["content"][0]["text"], "Hello!");
        assert_eq!(translated_response["usage"]["input_tokens"], 8);
        assert_eq!(translated_response["usage"]["output_tokens"], 2);
    }

    #[test]
    fn cx2cc_sse_round_trip_preserves_cache_usage() {
        let bridge = get_bridge("cx2cc").unwrap();
        let sse_bytes = bridge
            .translate_response_to_sse(
                json!({
                    "id": "resp_cache_sse",
                    "model": "gpt-5.4",
                    "status": "completed",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "cached response"}]
                    }],
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": 10,
                        "input_tokens_details": {"cached_tokens": 80}
                    }
                }),
                &cx2cc_ctx(),
            )
            .expect("translate response to SSE");
        let usage = crate::usage::parse_usage_from_json_or_sse_bytes("claude", &sse_bytes)
            .expect("extract translated SSE usage");

        assert_eq!(usage.metrics.input_tokens, Some(100));
        assert_eq!(usage.metrics.output_tokens, Some(10));
        assert_eq!(usage.metrics.cache_read_input_tokens, Some(80));
    }
}
