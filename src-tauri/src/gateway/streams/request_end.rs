//! Usage: Shared helpers to finalize stream requests (event + request log).

use super::finalize::finalize_circuit_and_session;
use super::types::{StreamTerminalEvidence, StreamTerminalOrigin};
use super::StreamFinalizeCtx;
use crate::gateway::active_requests::ActiveRequestFinishReason;
use crate::gateway::proxy::{
    spawn_enqueue_request_log_with_backpressure, status_override, ErrorCategory, GatewayErrorCode,
    RequestLogEnqueueArgs,
};
use crate::gateway::response_fixer;

pub(super) struct StreamRequestCompletion {
    pub(super) error_code: Option<&'static str>,
    pub(super) ttfb_ms: Option<u128>,
    pub(super) visible_ttfb_ms: Option<u128>,
    pub(super) requested_model: Option<String>,
    pub(super) usage_metrics: Option<crate::usage::UsageMetrics>,
    pub(super) usage: Option<crate::usage::UsageExtract>,
    pub(super) terminal_signal: Option<&'static str>,
    pub(super) terminal_evidence: StreamTerminalEvidence,
}

impl StreamRequestCompletion {
    pub(super) fn success(
        ttfb_ms: Option<u128>,
        visible_ttfb_ms: Option<u128>,
        requested_model: Option<String>,
        usage_metrics: Option<crate::usage::UsageMetrics>,
        usage: Option<crate::usage::UsageExtract>,
    ) -> Self {
        Self {
            error_code: None,
            ttfb_ms,
            visible_ttfb_ms,
            requested_model,
            usage_metrics,
            usage,
            terminal_signal: Some("completed"),
            terminal_evidence: StreamTerminalEvidence::new(
                StreamTerminalOrigin::Unclassified,
                false,
                false,
                false,
                false,
            ),
        }
    }

    pub(super) fn failure(
        error_code: &'static str,
        ttfb_ms: Option<u128>,
        visible_ttfb_ms: Option<u128>,
        requested_model: Option<String>,
        usage_metrics: Option<crate::usage::UsageMetrics>,
        usage: Option<crate::usage::UsageExtract>,
    ) -> Self {
        Self {
            error_code: Some(error_code),
            ttfb_ms,
            visible_ttfb_ms,
            requested_model,
            usage_metrics,
            usage,
            terminal_signal: Some("error"),
            terminal_evidence: StreamTerminalEvidence::new(
                StreamTerminalOrigin::Unclassified,
                false,
                false,
                false,
                false,
            ),
        }
    }

    pub(super) fn from_error_code(
        error_code: Option<&'static str>,
        ttfb_ms: Option<u128>,
        visible_ttfb_ms: Option<u128>,
        requested_model: Option<String>,
        usage_metrics: Option<crate::usage::UsageMetrics>,
        usage: Option<crate::usage::UsageExtract>,
    ) -> Self {
        match error_code {
            Some(code) => Self::failure(
                code,
                ttfb_ms,
                visible_ttfb_ms,
                requested_model,
                usage_metrics,
                usage,
            ),
            None => Self::success(
                ttfb_ms,
                visible_ttfb_ms,
                requested_model,
                usage_metrics,
                usage,
            ),
        }
    }

    pub(super) fn with_terminal_signal(mut self, terminal_signal: Option<&'static str>) -> Self {
        self.terminal_signal = terminal_signal;
        self
    }

    pub(super) fn with_terminal_evidence(
        mut self,
        terminal_evidence: StreamTerminalEvidence,
    ) -> Self {
        self.terminal_evidence = terminal_evidence;
        self
    }
}

fn ensure_stream_client_abort_setting<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    duration_ms: u128,
    ttfb_ms: Option<u128>,
    error_code: Option<&'static str>,
) {
    if error_code != Some(GatewayErrorCode::StreamAborted.as_str()) {
        return;
    }

    let already_recorded = ctx
        .special_settings
        .lock()
        .map(|guard| {
            guard.iter().any(|entry| {
                entry.get("type").and_then(serde_json::Value::as_str) == Some("client_abort")
                    && entry.get("scope").and_then(serde_json::Value::as_str) == Some("stream")
            })
        })
        .unwrap_or(false);

    if already_recorded {
        return;
    }

    let duration_ms_i64 = duration_ms.min(i64::MAX as u128) as i64;
    let ttfb_ms_i64 = ttfb_ms.and_then(|value| {
        if value >= duration_ms {
            return None;
        }
        Some(value.min(i64::MAX as u128) as i64)
    });

    response_fixer::push_special_setting(
        &ctx.special_settings,
        serde_json::json!({
            "type": "client_abort",
            "scope": "stream",
            "reason": "stream_finalized_aborted",
            "detected_by": "stream_finalize",
            "duration_ms": duration_ms_i64,
            "ttfb_ms": ttfb_ms_i64,
            "ts": crate::gateway::util::now_unix_seconds() as i64,
        }),
    );
}

fn status_for_stream_request_log(status: u16, error_code: Option<&'static str>) -> u16 {
    status_override::effective_status(Some(status), error_code).unwrap_or(status)
}

fn stream_error_category(category: Option<&'static str>) -> ErrorCategory {
    match category {
        Some(value) if value == ErrorCategory::ClientAbort.as_str() => ErrorCategory::ClientAbort,
        Some(value) if value == ErrorCategory::ProviderError.as_str() => {
            ErrorCategory::ProviderError
        }
        Some(value) if value == ErrorCategory::NonRetryableClientError.as_str() => {
            ErrorCategory::NonRetryableClientError
        }
        Some(value) if value == ErrorCategory::ResourceNotFound.as_str() => {
            ErrorCategory::ResourceNotFound
        }
        _ => ErrorCategory::SystemError,
    }
}

fn active_request_finish_reason(error_code: Option<&'static str>) -> ActiveRequestFinishReason {
    match error_code {
        Some(code)
            if code == GatewayErrorCode::StreamAborted.as_str()
                || code == GatewayErrorCode::RequestAborted.as_str() =>
        {
            ActiveRequestFinishReason::ClientAborted
        }
        Some(_) => ActiveRequestFinishReason::Failed,
        None => ActiveRequestFinishReason::Completed,
    }
}

fn mark_last_stream_attempt_terminal_failure(
    attempt: &mut crate::gateway::events::FailoverAttempt,
    upstream_status: u16,
    error_category: Option<&'static str>,
    error_code: &'static str,
    duration_ms: u128,
    terminal_evidence: StreamTerminalEvidence,
) {
    let category = stream_error_category(error_category);
    let decision = "abort";
    let effective_status = status_for_stream_request_log(upstream_status, Some(error_code));

    attempt.outcome = format!("stream_error: code={error_code}");
    attempt.status = Some(effective_status);
    attempt.error_category = Some(category.as_str());
    attempt.error_code = Some(error_code);
    attempt.decision = Some(decision);
    attempt.reason = Some(format!(
        "stream terminal failure: origin={} code={}",
        terminal_evidence.origin.as_str(),
        error_code,
    ));
    attempt.reason_code = Some(category.reason_code());
    attempt.attempt_duration_ms = Some(duration_ms);
    if attempt.probe == Some(true) {
        attempt.probe_result = Some("failed");
    }
}

pub(super) fn emit_request_event_and_spawn_request_log<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    mut completion: StreamRequestCompletion,
) {
    let duration_ms = ctx.started.elapsed().as_millis();
    let finalization =
        finalize_circuit_and_session(ctx, completion.error_code, completion.terminal_evidence);
    completion.error_code = finalization.error_code;
    if completion.error_code.is_some() {
        completion.terminal_signal = Some("error");
    }
    let effective_error_category = finalization.error_category;
    if !ctx.observe {
        return;
    }
    ensure_stream_client_abort_setting(ctx, duration_ms, completion.ttfb_ms, completion.error_code);

    // The initial attempt event is emitted when streaming starts. Persist the
    // complete terminal state on that same attempt so route-hop/attempt counts
    // stay stable while the final request event reflects the real outcome.
    let mut attempts = ctx.attempts.clone();
    let mut attempts_changed = false;
    if let Some(last) = attempts.last_mut() {
        if let Some(probe_result) = finalization.probe_result {
            last.probe_result = Some(probe_result);
        }
        last.circuit_state_after = Some(finalization.circuit_after.state.as_str());
        last.circuit_failure_count = Some(finalization.circuit_after.failure_count);
        last.circuit_failure_threshold = Some(finalization.circuit_after.failure_threshold);
        attempts_changed = true;
        if let Some(error_code) = completion.error_code {
            mark_last_stream_attempt_terminal_failure(
                last,
                ctx.status,
                effective_error_category,
                error_code,
                duration_ms,
                completion.terminal_evidence,
            );
        }
    }
    let attempts_json = if attempts_changed {
        serde_json::to_string(&attempts).unwrap_or_else(|_| "[]".to_string())
    } else {
        ctx.attempts_json.clone()
    };

    let (last_activity_ms, activity_details_json) = ctx
        .activity
        .lock()
        .map(|activity| {
            (
                Some(activity.last_activity_ms()),
                activity.terminal_details_json(
                    completion.terminal_signal,
                    completion.terminal_evidence,
                ),
            )
        })
        .unwrap_or((None, None));

    let (log_args, attempts) = RequestLogEnqueueArgs::from_stream_request_end_parts(
        ctx.trace_id.clone(),
        ctx.cli_key.clone(),
        ctx.session_id.clone(),
        ctx.method.clone(),
        ctx.path.clone(),
        ctx.query.clone(),
        ctx.excluded_from_stats,
        response_fixer::special_settings_json(&ctx.special_settings),
        status_for_stream_request_log(ctx.status, completion.error_code),
        completion.error_code,
        duration_ms,
        completion.ttfb_ms,
        completion.visible_ttfb_ms,
        attempts,
        attempts_json,
        completion.requested_model,
        ctx.created_at_ms,
        last_activity_ms,
        activity_details_json,
        ctx.created_at,
        completion.usage,
    );

    ctx.active_requests.finish(
        ctx.trace_id.as_str(),
        active_request_finish_reason(completion.error_code),
    );

    log_args.emit_gateway_request_event(
        &ctx.app,
        effective_error_category,
        completion.ttfb_ms,
        completion.visible_ttfb_ms,
        attempts,
        completion.usage_metrics,
    );

    spawn_enqueue_request_log_with_backpressure(
        ctx.app.clone(),
        ctx.db.clone(),
        ctx.log_tx.clone(),
        log_args,
        Some(ctx.plugin_pipeline.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        emit_request_event_and_spawn_request_log, status_for_stream_request_log,
        StreamRequestCompletion, StreamTerminalEvidence, StreamTerminalOrigin,
    };
    use crate::gateway::active_requests::{ActiveRequestRegistry, ActiveRequestStart};
    use crate::gateway::events::FailoverAttempt;
    use crate::gateway::proxy::dispatch::RequestDispatchIntent;
    use crate::gateway::proxy::{ErrorCategory, GatewayErrorCode};
    use crate::gateway::streams::{StreamActivityTracker, StreamFinalizeCtx};
    use crate::{circuit_breaker, db, request_logs, session_manager};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn active_request_start(trace_id: &str) -> ActiveRequestStart {
        ActiveRequestStart {
            trace_id: trace_id.to_string(),
            cli_key: "codex".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            query: None,
            session_id: Some("sess-stream-end".to_string()),
            requested_model: Some("gpt-5".to_string()),
            created_at_ms: 1_700_000_000_000,
        }
    }

    fn test_stream_finalize_ctx(
        app: tauri::AppHandle<tauri::test::MockRuntime>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
        active_requests: Arc<ActiveRequestRegistry>,
    ) -> StreamFinalizeCtx<tauri::test::MockRuntime> {
        StreamFinalizeCtx {
            app,
            db,
            log_tx,
            plugin_pipeline: crate::gateway::plugins::pipeline::GatewayPluginPipeline::empty_shared(
            ),
            circuit: Arc::new(circuit_breaker::CircuitBreaker::new(
                circuit_breaker::CircuitBreakerConfig::default(),
                HashMap::new(),
                None,
            )),
            dispatch_ownership: None,
            session: Arc::new(session_manager::SessionManager::new()),
            session_id: Some("sess-stream-end".to_string()),
            sort_mode_id: None,
            is_compact_request: false,
            trace_id: "trace-stream-end".to_string(),
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
                "trace-stream-end",
                "codex",
                1_700_000_000_000,
            ))),
            active_requests,
        }
    }

    fn probe_attempt() -> FailoverAttempt {
        FailoverAttempt {
            provider_id: 1,
            provider_name: "test-provider".to_string(),
            base_url: "https://upstream.example".to_string(),
            outcome: "success".to_string(),
            status: Some(200),
            provider_index: Some(0),
            retry_index: Some(0),
            session_reuse: Some(false),
            error_category: None,
            error_code: None,
            decision: Some("success"),
            reason: None,
            selection_method: Some("circuit_probe"),
            reason_code: Some("request_success"),
            attempt_started_ms: Some(0),
            attempt_duration_ms: Some(1),
            circuit_state_before: Some("OPEN"),
            circuit_state_after: None,
            circuit_failure_count: Some(1),
            circuit_failure_threshold: Some(1),
            probe: Some(true),
            probe_trigger: Some("aggressive_turn"),
            probe_result: Some("started"),
            probe_generation: Some(1),
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(false),
            timeout_secs: None,
            requested_upstream_model: Some("gpt-5".to_string()),
        }
    }

    fn arm_probe(ctx: &mut StreamFinalizeCtx<tauri::test::MockRuntime>, now_unix: i64) {
        ctx.circuit = Arc::new(circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                provider_cooldown_secs: 0,
                ..circuit_breaker::CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        ctx.circuit
            .record_failure(ctx.provider_id, now_unix, Some("TEST_PROBE_OPEN"));
        let token = match ctx.circuit.try_acquire_probe(
            ctx.provider_id,
            ctx.trace_id.as_str(),
            circuit_breaker::ProbeTrigger::AggressiveTurn,
            now_unix,
        ) {
            circuit_breaker::ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(
            ctx.provider_id,
            Some(circuit_breaker::ProbeTrigger::AggressiveTurn),
            None,
        )
        .claim_for_provider(
            ctx.provider_id,
            Some(circuit_breaker::ProbeLeaseGuard::new(
                Arc::clone(&ctx.circuit),
                token,
            )),
        )
        .expect("claim probe ownership");
        assert!(ownership.commit_at_transport_boundary(now_unix));
        ctx.dispatch_ownership = Some(ownership);
    }

    #[test]
    fn stream_request_completion_builds_success_without_error_code() {
        let completion = StreamRequestCompletion::success(
            Some(8),
            Some(21),
            Some("gpt-5".to_string()),
            None,
            None,
        );

        assert!(completion.error_code.is_none());
        assert_eq!(completion.ttfb_ms, Some(8));
        assert_eq!(completion.visible_ttfb_ms, Some(21));
        assert_eq!(completion.requested_model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn stream_request_completion_keeps_terminal_fields_together() {
        let usage_metrics = crate::usage::UsageMetrics::default();
        let completion = StreamRequestCompletion::failure(
            GatewayErrorCode::StreamError.as_str(),
            Some(12),
            Some(44),
            Some("gpt-5".to_string()),
            Some(usage_metrics),
            None,
        );

        assert_eq!(
            completion.error_code,
            Some(GatewayErrorCode::StreamError.as_str())
        );
        assert_eq!(completion.ttfb_ms, Some(12));
        assert_eq!(completion.visible_ttfb_ms, Some(44));
        assert_eq!(completion.requested_model.as_deref(), Some("gpt-5"));
        assert!(completion.usage_metrics.is_some());
        assert!(completion.usage.is_none());
    }

    #[test]
    fn stream_error_status_for_log_maps_http_200_to_502() {
        assert_eq!(
            status_for_stream_request_log(200, Some(GatewayErrorCode::StreamError.as_str())),
            502
        );
        assert_eq!(
            status_for_stream_request_log(200, Some(GatewayErrorCode::Fake200.as_str())),
            502
        );
        assert_eq!(
            status_for_stream_request_log(200, Some(GatewayErrorCode::EmptyResponse.as_str())),
            502
        );
        assert_eq!(
            status_for_stream_request_log(499, Some(GatewayErrorCode::StreamAborted.as_str())),
            499
        );
        assert_eq!(
            status_for_stream_request_log(200, Some(GatewayErrorCode::StreamIdleTimeout.as_str())),
            524
        );
    }

    #[test]
    fn observed_stream_request_end_finishes_active_request() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("stream-request-end.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-stream-end"));
        let ctx =
            test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests.clone());

        emit_request_event_and_spawn_request_log(
            &ctx,
            StreamRequestCompletion::success(None, None, Some("gpt-5".to_string()), None, None),
        );

        assert!(active_requests.snapshot().is_empty());
    }

    #[test]
    fn observed_stream_abort_finishes_active_request() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("stream-request-abort.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-stream-end"));
        let ctx =
            test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests.clone());

        emit_request_event_and_spawn_request_log(
            &ctx,
            StreamRequestCompletion::failure(
                GatewayErrorCode::StreamAborted.as_str(),
                None,
                None,
                Some("gpt-5".to_string()),
                None,
                None,
            ),
        );

        assert!(active_requests.snapshot().is_empty());
    }

    #[tokio::test]
    async fn probe_terminal_result_reserializes_attempts_json() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("stream-probe-attempt-log.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(active_request_start("trace-stream-end"));
        let mut ctx =
            test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests.clone());
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        arm_probe(&mut ctx, now_unix);
        ctx.attempts = vec![probe_attempt()];
        ctx.attempts_json = r#"[{"probe_result":"started","cached":true}]"#.to_string();

        emit_request_event_and_spawn_request_log(
            &ctx,
            StreamRequestCompletion::success(
                Some(5),
                Some(5),
                Some("gpt-5".to_string()),
                None,
                None,
            )
            .with_terminal_evidence(StreamTerminalEvidence::new(
                StreamTerminalOrigin::NormalEof,
                true,
                true,
                false,
                false,
            )),
        );

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts.as_array().map(Vec::len), Some(1));
        assert_eq!(attempts[0]["outcome"], "success");
        assert_eq!(attempts[0]["status"], 200);
        assert_eq!(attempts[0]["decision"], "success");
        assert_eq!(attempts[0]["reason_code"], "request_success");
        assert!(attempts[0]["error_code"].is_null());
        assert_eq!(attempts[0]["probe_result"], "success");
        assert_eq!(attempts[0]["circuit_state_after"], "CLOSED");
        assert_eq!(log.status, Some(200));
        assert!(log.error_code.is_none());
        assert!(attempts[0].get("cached").is_none());
        assert!(active_requests.snapshot().is_empty());
    }

    #[tokio::test]
    async fn stream_terminal_failure_rewrites_attempt_semantics() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("stream-failed-attempt-log.sqlite"))
            .expect("init test db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx, active_requests);
        let mut attempt = probe_attempt();
        attempt.probe = None;
        attempt.probe_trigger = None;
        attempt.probe_result = None;
        attempt.probe_generation = None;
        attempt.selection_method = Some("session_reuse");
        ctx.attempts = vec![attempt];
        ctx.attempts_json = r#"[{"outcome":"success","decision":"success"}]"#.to_string();

        emit_request_event_and_spawn_request_log(
            &ctx,
            StreamRequestCompletion::failure(
                GatewayErrorCode::StreamAborted.as_str(),
                Some(5),
                Some(5),
                Some("gpt-5".to_string()),
                None,
                None,
            )
            .with_terminal_evidence(StreamTerminalEvidence::new(
                StreamTerminalOrigin::ClientAbort,
                false,
                false,
                false,
                false,
            )),
        );

        let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
            .await
            .expect("request log should be enqueued")
            .expect("request log channel should stay open");
        let attempts: serde_json::Value =
            serde_json::from_str(&log.attempts_json).expect("attempts json");
        assert_eq!(attempts[0]["status"], 499);
        assert_eq!(attempts[0]["decision"], "abort");
        assert_eq!(attempts[0]["error_category"], "CLIENT_ABORT");
        assert_eq!(
            attempts[0]["error_code"],
            GatewayErrorCode::StreamAborted.as_str()
        );
        assert_eq!(
            attempts[0]["reason_code"],
            ErrorCategory::ClientAbort.reason_code()
        );
        assert_eq!(attempts[0]["circuit_state_after"], "CLOSED");
        assert!(attempts[0]["reason"]
            .as_str()
            .is_some_and(|value| value.contains("origin=client_abort")));
        assert!(attempts[0]["outcome"].as_str().is_some_and(|value| value
            .starts_with("stream_error:")
            && !value.contains("decision=success")));
    }

    #[tokio::test]
    async fn probe_client_abort_and_direct_drop_fail_the_started_attempt() {
        for (case, origin, normal_eof) in [
            ("client-abort", StreamTerminalOrigin::ClientAbort, true),
            ("direct-drop", StreamTerminalOrigin::DirectDrop, false),
        ] {
            let app = tauri::test::mock_app();
            let db_dir = tempfile::tempdir().expect("db dir");
            let db = db::init_for_tests(
                &db_dir
                    .path()
                    .join(format!("stream-probe-{case}-attempt-log.sqlite")),
            )
            .expect("init test db");
            let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(4);
            let active_requests = Arc::new(ActiveRequestRegistry::default());
            let mut ctx = test_stream_finalize_ctx(
                app.handle().clone(),
                db,
                log_tx,
                Arc::clone(&active_requests),
            );
            let now_unix = crate::gateway::util::now_unix_seconds() as i64;
            ctx.session.bind_success(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                2,
                None,
                now_unix,
            );
            arm_probe(&mut ctx, now_unix);
            ctx.attempts = vec![probe_attempt()];
            ctx.attempts_json = serde_json::to_string(&ctx.attempts).expect("attempts json");

            emit_request_event_and_spawn_request_log(
                &ctx,
                StreamRequestCompletion::success(
                    Some(5),
                    Some(5),
                    Some("gpt-5".to_string()),
                    None,
                    None,
                )
                .with_terminal_evidence(StreamTerminalEvidence::new(
                    origin, true, normal_eof, true, false,
                )),
            );

            let log = tokio::time::timeout(Duration::from_secs(2), log_rx.recv())
                .await
                .expect("request log should be enqueued")
                .expect("request log channel should stay open");
            let attempts: serde_json::Value =
                serde_json::from_str(&log.attempts_json).expect("attempts json");
            assert_eq!(attempts.as_array().map(Vec::len), Some(1), "{case}");
            assert_ne!(attempts[0]["outcome"], "success", "{case}");
            assert_eq!(attempts[0]["status"], 499, "{case}");
            assert_eq!(attempts[0]["decision"], "abort", "{case}");
            assert_eq!(attempts[0]["error_category"], "CLIENT_ABORT", "{case}");
            assert_eq!(
                attempts[0]["error_code"],
                GatewayErrorCode::StreamAborted.as_str(),
                "{case}"
            );
            assert_eq!(
                attempts[0]["reason_code"],
                ErrorCategory::ClientAbort.reason_code(),
                "{case}"
            );
            assert_eq!(attempts[0]["probe_result"], "failed", "{case}");
            assert_eq!(attempts[0]["circuit_state_after"], "OPEN", "{case}");
            assert!(
                attempts[0]["reason"]
                    .as_str()
                    .is_some_and(|value| value.contains(origin.as_str())),
                "{case}"
            );
            assert_eq!(log.status, Some(499), "{case}");
            assert_eq!(
                log.error_code.as_deref(),
                Some(GatewayErrorCode::StreamAborted.as_str()),
                "{case}"
            );
            assert_eq!(
                ctx.session.get_bound_provider(
                    ctx.cli_key.as_str(),
                    ctx.session_id.as_deref().expect("session"),
                    now_unix,
                ),
                Some(2),
                "{case}"
            );
            assert!(active_requests.snapshot().is_empty(), "{case}");
        }
    }
}
