//! Resolve server-managed `aio/<model_uuid>` aliases before provider selection.

use super::{MiddlewareAction, ProxyContext};
use crate::gateway::managed_model_route::{push_initial_special_setting, ManagedModelRoute};
use crate::gateway::proxy::handler::early_error::{
    build_early_error_log_ctx, early_error_contract, push_special_setting,
    respond_early_error_with_enqueue, EarlyErrorKind,
};
use crate::gateway::response_fixer;
use crate::gateway::util::RequestedModelLocation;
use axum::body::Bytes;

const MANAGED_MODEL_PREFIX: &str = "aio/";

pub(in crate::gateway::proxy::handler) struct ManagedModelRouteMiddleware;

impl ManagedModelRouteMiddleware {
    pub(in crate::gateway::proxy::handler) async fn run<R: tauri::Runtime>(
        mut ctx: ProxyContext<R>,
    ) -> MiddlewareAction<R> {
        let Some(canonical_model) = ctx
            .requested_model
            .as_deref()
            .filter(|model| model.starts_with(MANAGED_MODEL_PREFIX))
            .map(str::to_string)
        else {
            return MiddlewareAction::Continue(Box::new(ctx));
        };

        if ctx.cli_key != "codex" {
            return reject(ctx, canonical_model, "unsupported_cli").await;
        }

        let resolution = {
            let db = ctx.state.db.clone();
            let canonical_model = canonical_model.clone();
            crate::blocking::run("gateway_managed_model_resolve", move || {
                crate::domain::provider_models::resolve_managed_model_alias(&db, &canonical_model)
            })
            .await
        };

        let binding = match resolution {
            Ok(Some(binding)) => binding,
            Ok(None) => return reject(ctx, canonical_model, "invalid_or_missing_binding").await,
            Err(err) => {
                let contract = early_error_contract(EarlyErrorKind::ProviderSelectionFailed);
                let requested_model = ctx.requested_model.clone();
                let session_id = ctx.session_id.clone();
                let special_settings_json =
                    response_fixer::special_settings_json(&ctx.special_settings);
                let log_ctx = build_early_error_log_ctx(&ctx);
                let response = respond_early_error_with_enqueue(
                    &log_ctx,
                    contract,
                    format!("failed to resolve managed model binding: {err}"),
                    special_settings_json,
                    session_id,
                    requested_model,
                )
                .await;
                return MiddlewareAction::ShortCircuit(response);
            }
        };

        if ctx
            .forced_provider_id
            .is_some_and(|provider_id| provider_id != binding.provider_id)
        {
            return reject(ctx, canonical_model, "forced_provider_conflict").await;
        }

        let route = ManagedModelRoute {
            canonical_model,
            model_uuid: binding.model_uuid,
            provider_id: binding.provider_id,
            provider_uuid: binding.provider_uuid,
            remote_model_id: binding.remote_model_id,
        };
        if !rewrite_request_model(&mut ctx, &route.remote_model_id) {
            return reject(ctx, route.canonical_model, "request_model_rewrite_failed").await;
        }
        push_initial_special_setting(&ctx.special_settings, &route);
        ctx.managed_model_route = Some(route);
        MiddlewareAction::Continue(Box::new(ctx))
    }
}

fn rewrite_request_model<R: tauri::Runtime>(
    ctx: &mut ProxyContext<R>,
    remote_model_id: &str,
) -> bool {
    match ctx
        .requested_model_location
        .unwrap_or(RequestedModelLocation::BodyJson)
    {
        RequestedModelLocation::BodyJson => {
            let Some(mut root) = ctx.introspection_json.take() else {
                return false;
            };
            if !crate::gateway::proxy::model_rewrite::replace_model_in_body_json(
                &mut root,
                remote_model_id,
            ) {
                ctx.introspection_json = Some(root);
                return false;
            }
            let Ok(encoded) = serde_json::to_vec(&root) else {
                ctx.introspection_json = Some(root);
                return false;
            };
            let encoded = Bytes::from(encoded);
            ctx.body_bytes = encoded.clone();
            if let Some(body) = ctx.request_body_state.as_mut() {
                body.replace_decoded(encoded);
            }
            ctx.introspection_json = Some(root);
            true
        }
        RequestedModelLocation::Query => {
            let Some(query) = ctx.query.as_deref() else {
                return false;
            };
            let rewritten = crate::gateway::proxy::model_rewrite::replace_model_in_query(
                query,
                remote_model_id,
            );
            if rewritten == query {
                return false;
            }
            ctx.query = Some(rewritten);
            true
        }
        RequestedModelLocation::Path => {
            let Some(rewritten) = crate::gateway::proxy::model_rewrite::replace_model_in_path(
                &ctx.forwarded_path,
                remote_model_id,
            ) else {
                return false;
            };
            ctx.forwarded_path = rewritten;
            true
        }
    }
}

async fn reject<R: tauri::Runtime>(
    ctx: ProxyContext<R>,
    canonical_model: String,
    reason: &'static str,
) -> MiddlewareAction<R> {
    push_special_setting(
        &ctx.special_settings,
        serde_json::json!({
            "type": "aio_managed_model_route_rejected",
            "scope": "request",
            "canonicalModel": canonical_model,
            "reason": reason,
        }),
    );
    let contract = early_error_contract(EarlyErrorKind::ManagedModelInvalid);
    let requested_model = ctx.requested_model.clone();
    let session_id = ctx.session_id.clone();
    let special_settings_json = response_fixer::special_settings_json(&ctx.special_settings);
    let log_ctx = build_early_error_log_ctx(&ctx);
    let response = respond_early_error_with_enqueue(
        &log_ctx,
        contract,
        "invalid or unavailable AIO managed model alias".to_string(),
        special_settings_json,
        session_id,
        requested_model,
    )
    .await;
    MiddlewareAction::ShortCircuit(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_prefix_is_exact_and_case_sensitive() {
        assert!("aio/11111111-1111-4111-8111-111111111111".starts_with(MANAGED_MODEL_PREFIX));
        assert!(!"AIO/11111111-1111-4111-8111-111111111111".starts_with(MANAGED_MODEL_PREFIX));
        assert!(!"gpt-5.5".starts_with(MANAGED_MODEL_PREFIX));
    }
}
