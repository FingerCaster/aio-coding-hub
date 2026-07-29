//! Usage: Shared stream finalize helpers (cooldown/circuit/session).

use crate::domain::provider_oauth_limits;

use super::super::proxy::{provider_router, ErrorCategory, GatewayErrorCode};
use super::super::util::now_unix_seconds;
use super::StreamFinalizeCtx;

fn record_stream_failure_args<'a, R: tauri::Runtime>(
    ctx: &'a StreamFinalizeCtx<R>,
    now_unix: i64,
    error_code: Option<&'static str>,
) -> provider_router::RecordCircuitArgs<'a, R> {
    let first_byte_timeout_secs = (error_code == Some(GatewayErrorCode::UpstreamTimeout.as_str()))
        .then_some(ctx.upstream_first_byte_timeout_secs);
    provider_router::RecordCircuitArgs::from_stream_ctx(ctx, now_unix)
        .with_trigger(error_code, first_byte_timeout_secs)
}

pub(super) fn finalize_circuit_and_session<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    error_code: Option<&'static str>,
) -> Option<&'static str> {
    let effective_error_category = if error_code == Some(GatewayErrorCode::StreamAborted.as_str()) {
        Some(ErrorCategory::ClientAbort.as_str())
    } else if matches!(
        error_code,
        Some(code)
            if code == GatewayErrorCode::Fake200.as_str()
                || code == GatewayErrorCode::EmptyResponse.as_str()
    ) {
        Some(ErrorCategory::ProviderError.as_str())
    } else {
        ctx.error_category
    };

    let now_unix = now_unix_seconds() as i64;
    let oauth_quota_exhausted =
        ctx.auth_mode == "oauth" && ctx.fake_200_detected && ctx.fake_200_quota_exhausted;

    if oauth_quota_exhausted {
        if let Err(err) =
            provider_oauth_limits::save_exhausted_snapshot(&ctx.db, ctx.provider_id, None)
        {
            tracing::warn!(
                provider_id = ctx.provider_id,
                "failed to save OAuth exhausted quota snapshot: {err}"
            );
        }
    }

    if error_code.is_some()
        && effective_error_category != Some(ErrorCategory::ClientAbort.as_str())
        && ctx.provider_cooldown_secs > 0
        && !oauth_quota_exhausted
    {
        provider_router::trigger_cooldown(
            ctx.circuit.as_ref(),
            ctx.provider_id,
            now_unix,
            ctx.provider_cooldown_secs,
            ctx.provider_health_neutral,
        );
    }

    if error_code.is_none() && (200..300).contains(&ctx.status) && !ctx.fake_200_detected {
        let _ = provider_router::record_success_and_emit_transition(
            provider_router::RecordCircuitArgs::from_stream_ctx(ctx, now_unix),
        );
        if !ctx.managed_model_route {
            if let Some(session_id) = ctx.session_id.as_deref() {
                ctx.session.bind_success(
                    &ctx.cli_key,
                    session_id,
                    ctx.provider_id,
                    ctx.sort_mode_id,
                    now_unix,
                );
            }
        }
    } else if ctx.fake_200_detected && (200..300).contains(&ctx.status) {
        // Fake 200: upstream returned HTTP 200 but body contained an error payload.
        // Record as failure for circuit breaker; do not bind session.
        if !oauth_quota_exhausted {
            let _ = provider_router::record_failure_and_emit_transition(
                record_stream_failure_args(ctx, now_unix, error_code),
            );
        }
    } else if effective_error_category == Some(ErrorCategory::ProviderError.as_str())
        && !oauth_quota_exhausted
    {
        let _ = provider_router::record_failure_and_emit_transition(record_stream_failure_args(
            ctx, now_unix, error_code,
        ));
    }

    effective_error_category
}

#[cfg(test)]
mod tests {
    use super::finalize_circuit_and_session;
    use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
    use crate::gateway::active_requests::ActiveRequestRegistry;
    use crate::gateway::proxy::{ErrorCategory, GatewayErrorCode};
    use crate::gateway::streams::{StreamActivityTracker, StreamFinalizeCtx};
    use crate::{db, request_logs, session_manager};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn test_stream_finalize_ctx(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
    ) -> StreamFinalizeCtx<tauri::test::MockRuntime> {
        StreamFinalizeCtx {
            app,
            db,
            log_tx,
            plugin_pipeline: crate::gateway::plugins::pipeline::GatewayPluginPipeline::empty_shared(
            ),
            circuit: Arc::new(CircuitBreaker::new(
                CircuitBreakerConfig {
                    failure_threshold: 1,
                    open_duration_secs: 60,
                },
                HashMap::new(),
                None,
            )),
            session: Arc::new(session_manager::SessionManager::new()),
            session_id: Some("sess-stream-finalize".to_string()),
            sort_mode_id: None,
            trace_id: "trace-stream-finalize".to_string(),
            cli_key: "codex".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            observe: true,
            query: None,
            excluded_from_stats: false,
            special_settings: Arc::new(Mutex::new(Vec::new())),
            provider_health_neutral: false,
            status: 200,
            error_category: None,
            error_code: None,
            started: Instant::now(),
            attempt_started: Instant::now(),
            attempts: Vec::new(),
            attempts_json: "[]".to_string(),
            requested_model: None,
            requested_upstream_model: None,
            managed_model_route: false,
            created_at_ms: 1_700_000_000_000,
            created_at: 1_700_000_000,
            provider_cooldown_secs: 0,
            upstream_first_byte_timeout_secs: 300,
            provider_id: 1,
            provider_name: "test-provider".to_string(),
            base_url: "https://upstream.example".to_string(),
            auth_mode: "api_key".to_string(),
            upstream_route_tracker: Arc::new(Mutex::new(crate::usage::SseUsageTracker::new(
                "codex",
            ))),
            observed_upstream_model: Arc::new(Mutex::new(None)),
            observed_upstream_conflicting_model: Arc::new(Mutex::new(None)),
            observed_upstream_reasoning_effort: Arc::new(Mutex::new(None)),
            fake_200_detected: false,
            fake_200_quota_exhausted: false,
            activity: Arc::new(Mutex::new(StreamActivityTracker::new(
                "trace-stream-finalize",
                "codex",
                1_700_000_000_000,
            ))),
            active_requests: Arc::new(ActiveRequestRegistry::default()),
        }
    }

    #[test]
    fn stream_finalizer_records_trigger_error_code_for_provider_failures() {
        let cases = [
            (
                "fake_200",
                200,
                true,
                None,
                GatewayErrorCode::Fake200.as_str(),
            ),
            (
                "empty_response",
                200,
                false,
                None,
                GatewayErrorCode::EmptyResponse.as_str(),
            ),
            (
                "stream_error",
                502,
                false,
                Some(ErrorCategory::ProviderError.as_str()),
                GatewayErrorCode::StreamError.as_str(),
            ),
        ];

        for (case, status, fake_200_detected, error_category, error_code) in cases {
            let app = tauri::test::mock_app();
            let db_dir = tempfile::tempdir().expect("db dir");
            let db =
                db::init_for_tests(&db_dir.path().join(format!("stream-finalize-{case}.sqlite")))
                    .expect("init test db");
            let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
            let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
            ctx.status = status;
            ctx.fake_200_detected = fake_200_detected;
            ctx.error_category = error_category;

            assert_eq!(
                finalize_circuit_and_session(&ctx, Some(error_code)),
                Some(ErrorCategory::ProviderError.as_str()),
                "{case} effective category"
            );

            let snapshot = ctx.circuit.snapshot(
                ctx.provider_id,
                crate::gateway::util::now_unix_seconds() as i64,
            );
            assert_eq!(snapshot.state, CircuitState::Open, "{case} opened circuit");
            assert_eq!(
                snapshot.last_trigger_error_code,
                Some(error_code),
                "{case} retained trigger attribution"
            );
        }
    }

    #[test]
    fn stream_failure_args_include_timeout_seconds_only_for_upstream_timeout() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("stream-finalize-timeout.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);

        let timeout_args = super::record_stream_failure_args(
            &ctx,
            1_700_000_001,
            Some(GatewayErrorCode::UpstreamTimeout.as_str()),
        );
        assert_eq!(
            timeout_args.trigger_error_code,
            Some(GatewayErrorCode::UpstreamTimeout.as_str())
        );
        assert_eq!(timeout_args.first_byte_timeout_secs, Some(300));

        let stream_error_args = super::record_stream_failure_args(
            &ctx,
            1_700_000_001,
            Some(GatewayErrorCode::StreamError.as_str()),
        );
        assert_eq!(
            stream_error_args.trigger_error_code,
            Some(GatewayErrorCode::StreamError.as_str())
        );
        assert_eq!(stream_error_args.first_byte_timeout_secs, None);
    }
}
