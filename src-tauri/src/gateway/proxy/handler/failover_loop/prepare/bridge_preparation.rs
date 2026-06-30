//! Usage: Generic request preparation for explicit non-CX2CC bridge providers.

use super::provider_iterator::SkipReason;
use super::*;
use crate::gateway::proxy::protocol_bridge::{self, BridgeContext};

pub(super) struct BridgePreparationInput<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) input: &'a RequestContext<R>,
    pub(super) provider_id: i64,
    pub(super) provider_name_base: &'a str,
    pub(super) bridge_type: &'a str,
    pub(super) source_id: Option<i64>,
    pub(super) upstream_body_bytes: Bytes,
}

pub(super) struct BridgePreparationResult {
    pub(super) active_bridge_type: String,
    pub(super) bridge_source: Option<(crate::providers::ProviderForGateway, String)>,
    pub(super) effective_credential: String,
    pub(super) provider_base_url_base: String,
    pub(super) upstream_forwarded_path: String,
    pub(super) upstream_query: Option<String>,
    pub(super) upstream_body_bytes: Bytes,
    pub(super) strip_request_content_encoding: bool,
}

pub(super) enum BridgePreparationOutcome {
    Ready(Box<BridgePreparationResult>),
    Terminal(SkipReason),
}

pub(super) async fn prepare<R: tauri::Runtime>(
    args: BridgePreparationInput<'_, R>,
) -> BridgePreparationOutcome {
    let Some(source_id) = args.source_id else {
        return BridgePreparationOutcome::Terminal(SkipReason {
            error_category: "config",
            error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
            reason: "bridge source provider missing".to_string(),
        });
    };

    let (source, source_cli_key) = match crate::providers::get_source_provider_for_gateway(
        &args.input.state.db,
        source_id,
        args.bridge_type,
    ) {
        Ok(pair) => pair,
        Err(err) => {
            tracing::warn!(
                trace_id = %args.input.trace_id,
                provider_id = args.provider_id,
                source_provider_id = source_id,
                bridge_type = args.bridge_type,
                "bridge source provider not found: {err}"
            );
            return BridgePreparationOutcome::Terminal(SkipReason {
                error_category: "config",
                error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                reason: format!("bridge source provider not found: {err}"),
            });
        }
    };

    let effective_credential =
        match resolve_effective_credential(&args.input.state, &source_cli_key, &source).await {
            Ok(credential) => credential,
            Err(err) => {
                return BridgePreparationOutcome::Terminal(SkipReason {
                    error_category: "auth",
                    error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                    reason: format!("bridge source provider credential failed: {err}"),
                });
            }
        };

    let provider_base_url_base = match select_provider_base_url_for_request(
        &args.input.state,
        &source,
        &source_cli_key,
        args.input.provider_base_url_ping_cache_ttl_seconds,
    )
    .await
    {
        Ok(url) => url,
        Err(err) => {
            return BridgePreparationOutcome::Terminal(SkipReason {
                error_category: "translation",
                error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                reason: format!("bridge source base_url failed: {err}"),
            });
        }
    };

    let body_val = match parse_request_json(&args.upstream_body_bytes) {
        Ok(value) => value,
        Err(reason) => {
            emit_gateway_log(
                &args.input.state.app,
                "warn",
                "BRIDGE_TRANSLATE_FAILED",
                format!(
                    "[Bridge] request translation failed bridge={} provider={} err={}",
                    args.bridge_type, args.provider_name_base, reason.reason
                ),
            );
            return BridgePreparationOutcome::Terminal(reason);
        }
    };
    let requested_model = body_val.get("model").and_then(|m| m.as_str()).unwrap_or("");
    let bridge_ctx = BridgeContext {
        claude_models: args
            .input
            .providers
            .iter()
            .find(|p| p.id == args.provider_id)
            .map(|p| p.claude_models.clone())
            .unwrap_or_default(),
        cx2cc_settings: args.input.cx2cc_settings.clone(),
        requested_model: Some(requested_model.to_string()),
        mapped_model: None,
        stream_requested: body_val
            .get("stream")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        is_chatgpt_backend: false,
    };

    let translated = match protocol_bridge::get_bridge(args.bridge_type)
        .ok_or_else(|| format!("bridge not registered: {}", args.bridge_type))
        .and_then(|bridge| {
            bridge
                .translate_request(body_val, &bridge_ctx)
                .map_err(|err| err.to_string())
        }) {
        Ok(translated) => translated,
        Err(err) => {
            emit_gateway_log(
                &args.input.state.app,
                "warn",
                "BRIDGE_TRANSLATE_FAILED",
                format!(
                    "[Bridge] request translation failed bridge={} provider={} err={err}",
                    args.bridge_type, args.provider_name_base
                ),
            );
            return BridgePreparationOutcome::Terminal(SkipReason {
                error_category: "translation",
                error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                reason: format!("bridge translation failed: {err}"),
            });
        }
    };

    let upstream_body_bytes: Bytes = match serde_json::to_vec(&translated.body) {
        Ok(bytes) => bytes.into(),
        Err(err) => {
            return BridgePreparationOutcome::Terminal(SkipReason {
                error_category: "translation",
                error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
                reason: format!("bridge translated body serialization failed: {err}"),
            });
        }
    };

    emit_gateway_log(
        &args.input.state.app,
        "info",
        "BRIDGE_TRANSLATED",
        format!(
            "[Bridge] translated bridge={} provider={} source_cli={} target_path={}",
            args.bridge_type, args.provider_name_base, source_cli_key, translated.target_path
        ),
    );

    BridgePreparationOutcome::Ready(Box::new(BridgePreparationResult {
        active_bridge_type: args.bridge_type.to_string(),
        bridge_source: Some((source, source_cli_key)),
        effective_credential,
        provider_base_url_base,
        upstream_forwarded_path: translated.target_path,
        upstream_query: None,
        upstream_body_bytes,
        strip_request_content_encoding: true,
    }))
}

fn parse_request_json(body: &[u8]) -> Result<serde_json::Value, SkipReason> {
    serde_json::from_slice(body).map_err(|err| SkipReason {
        error_category: "translation",
        error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
        reason: format!("bridge request body is not valid JSON: {err}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_request_json, BridgePreparationOutcome, SkipReason};
    use crate::gateway::proxy::GatewayErrorCode;

    fn terminal_bridge_error(reason: SkipReason) -> BridgePreparationOutcome {
        BridgePreparationOutcome::Terminal(reason)
    }

    #[test]
    fn rejects_invalid_json_without_defaulting_to_empty_object() {
        let err = parse_request_json(br#"{"model":"gpt-4.1""#).unwrap_err();

        assert_eq!(err.error_category, "translation");
        assert_eq!(
            err.error_code,
            GatewayErrorCode::BridgeUnsupportedFeature.as_str()
        );
        assert!(err.reason.contains("not valid JSON"));
    }

    #[test]
    fn explicit_bridge_source_failures_are_terminal() {
        let outcome = terminal_bridge_error(SkipReason {
            error_category: "config",
            error_code: GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
            reason: "bridge source provider missing".into(),
        });

        match outcome {
            BridgePreparationOutcome::Terminal(reason) => {
                assert_eq!(
                    reason.error_code,
                    GatewayErrorCode::BridgeUnsupportedFeature.as_str()
                );
                assert!(reason.reason.contains("bridge source provider missing"));
            }
            BridgePreparationOutcome::Ready(_) => {
                panic!("explicit bridge source failures must not skip/fail over")
            }
        }
    }
}
