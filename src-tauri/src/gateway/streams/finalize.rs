//! Usage: Shared stream finalize helpers (cooldown/circuit/session).

use crate::domain::provider_oauth_limits;

use super::super::events::emit_circuit_transition;
use super::super::proxy::{provider_router, ErrorCategory, GatewayErrorCode};
use super::super::util::now_unix_seconds;
use super::types::{StreamTerminalEvidence, StreamTerminalOrigin};
use super::StreamFinalizeCtx;

pub(super) struct StreamFinalization {
    pub(super) error_category: Option<&'static str>,
    pub(super) error_code: Option<&'static str>,
    pub(super) probe_result: Option<&'static str>,
    pub(super) circuit_after: crate::circuit_breaker::CircuitSnapshot,
}

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

fn incomplete_probe_error_code(evidence: StreamTerminalEvidence) -> Option<&'static str> {
    if evidence.trusted_probe_success() {
        return None;
    }

    Some(match evidence.origin {
        StreamTerminalOrigin::ClientAbort
        | StreamTerminalOrigin::CompletionDelivered
        | StreamTerminalOrigin::DirectDrop
        | StreamTerminalOrigin::RelayDrainTimeout => GatewayErrorCode::StreamAborted.as_str(),
        StreamTerminalOrigin::IdleTimeout => GatewayErrorCode::StreamIdleTimeout.as_str(),
        StreamTerminalOrigin::TotalTimeout => GatewayErrorCode::UpstreamTimeout.as_str(),
        StreamTerminalOrigin::Unclassified
        | StreamTerminalOrigin::NormalEof
        | StreamTerminalOrigin::UpstreamReadError
        | StreamTerminalOrigin::TerminalFrame
        | StreamTerminalOrigin::BufferedBodyEof => GatewayErrorCode::StreamError.as_str(),
    })
}

fn stream_terminal_error_category(
    error_code: Option<&'static str>,
    configured_category: Option<&'static str>,
) -> Option<&'static str> {
    let error_code = error_code?;
    if error_code == GatewayErrorCode::StreamAborted.as_str()
        || error_code == GatewayErrorCode::RequestAborted.as_str()
    {
        return Some(ErrorCategory::ClientAbort.as_str());
    }
    if error_code == GatewayErrorCode::Fake200.as_str()
        || error_code == GatewayErrorCode::EmptyResponse.as_str()
    {
        return Some(ErrorCategory::ProviderError.as_str());
    }

    configured_category.or(Some(ErrorCategory::SystemError.as_str()))
}

fn trusted_failback_binding_success(evidence: StreamTerminalEvidence) -> bool {
    match evidence.origin {
        StreamTerminalOrigin::BufferedBodyEof => {
            evidence.normal_eof && !evidence.terminal_error_seen
        }
        _ => evidence.trusted_probe_success(),
    }
}

fn emit_probe_transition<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    change: &crate::circuit_breaker::CircuitChange,
    now_unix: i64,
    error_code: Option<&'static str>,
) {
    if let Some(transition) = change.transition.as_ref() {
        emit_circuit_transition(
            &ctx.app,
            ctx.trace_id.as_str(),
            ctx.cli_key.as_str(),
            ctx.provider_id,
            ctx.provider_name.as_str(),
            ctx.base_url.as_str(),
            transition,
            now_unix,
            error_code,
            (error_code == Some(GatewayErrorCode::UpstreamTimeout.as_str()))
                .then_some(ctx.upstream_first_byte_timeout_secs),
        );
    }
}

fn mark_compaction_completed<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    now_unix: i64,
    terminal_evidence: StreamTerminalEvidence,
) {
    if !ctx.is_compact_request || !terminal_evidence.trusted_probe_success() {
        return;
    }
    if let Some(session_id) = ctx.session_id.as_deref() {
        let _ = ctx
            .session
            .mark_compaction_completed(&ctx.cli_key, session_id, now_unix);
    }
}

pub(super) fn finalize_circuit_and_session<R: tauri::Runtime>(
    ctx: &StreamFinalizeCtx<R>,
    error_code: Option<&'static str>,
    terminal_evidence: StreamTerminalEvidence,
) -> StreamFinalization {
    let probe_ownership = ctx
        .dispatch_ownership
        .as_ref()
        .filter(|ownership| ownership.is_probe());
    let mut effective_error_code = error_code;
    if ctx.fake_200_detected && (200..300).contains(&ctx.status) {
        effective_error_code = Some(GatewayErrorCode::Fake200.as_str());
    } else if probe_ownership.is_some() && effective_error_code.is_none() {
        effective_error_code = incomplete_probe_error_code(terminal_evidence);
    }

    let effective_error_category =
        stream_terminal_error_category(effective_error_code, ctx.error_category);

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

    if effective_error_code.is_some()
        && effective_error_category != Some(ErrorCategory::ClientAbort.as_str())
        && ctx.provider_cooldown_secs > 0
        && !oauth_quota_exhausted
        && probe_ownership.is_none()
    {
        provider_router::trigger_cooldown(
            ctx.circuit.as_ref(),
            ctx.provider_id,
            now_unix,
            ctx.provider_cooldown_secs,
            ctx.provider_health_neutral,
        );
    }

    if let Some(ownership) = probe_ownership {
        let trusted_success = effective_error_code.is_none()
            && (200..300).contains(&ctx.status)
            && !ctx.fake_200_detected
            && terminal_evidence.trusted_probe_success();
        let commit = if trusted_success {
            ownership.complete_probe_success(now_unix)
        } else {
            let counted_failure = !ctx.provider_health_neutral
                && !oauth_quota_exhausted
                && effective_error_category == Some(ErrorCategory::ProviderError.as_str());
            ownership.complete_probe_failure(now_unix, counted_failure, effective_error_code)
        };

        let (probe_result, circuit_after, applied) = match commit {
            Some(crate::circuit_breaker::ProbeCommitResult::Applied(change)) => {
                emit_probe_transition(ctx, &change, now_unix, effective_error_code);
                (
                    if trusted_success { "success" } else { "failed" },
                    change.after,
                    true,
                )
            }
            Some(crate::circuit_breaker::ProbeCommitResult::Stale(snapshot)) => {
                ("failed", snapshot, false)
            }
            None => (
                "failed",
                ctx.circuit.snapshot(ctx.provider_id, now_unix),
                false,
            ),
        };

        if trusted_success && applied && !ctx.managed_model_route {
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
        if trusted_success {
            mark_compaction_completed(ctx, now_unix, terminal_evidence);
        }

        return StreamFinalization {
            error_category: effective_error_category,
            error_code: effective_error_code,
            probe_result: Some(probe_result),
            circuit_after,
        };
    }

    let mut circuit_after = None;
    if effective_error_code.is_none() && (200..300).contains(&ctx.status) && !ctx.fake_200_detected
    {
        let _ = provider_router::record_success_and_emit_transition(
            provider_router::RecordCircuitArgs::from_stream_ctx(ctx, now_unix),
        );
        circuit_after = Some(ctx.circuit.snapshot(ctx.provider_id, now_unix));
        let can_bind_session =
            ctx.dispatch_ownership.is_none() || trusted_failback_binding_success(terminal_evidence);
        if can_bind_session && !ctx.managed_model_route {
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
        mark_compaction_completed(ctx, now_unix, terminal_evidence);
    } else if ctx.fake_200_detected && (200..300).contains(&ctx.status) {
        // Fake 200: upstream returned HTTP 200 but body contained an error payload.
        // Record as failure for circuit breaker; do not bind session.
        if !oauth_quota_exhausted {
            let change = provider_router::record_failure_and_emit_transition(
                record_stream_failure_args(ctx, now_unix, effective_error_code),
            );
            circuit_after = Some(change.after);
        }
    } else if effective_error_category == Some(ErrorCategory::ProviderError.as_str())
        && !oauth_quota_exhausted
    {
        let change = provider_router::record_failure_and_emit_transition(
            record_stream_failure_args(ctx, now_unix, effective_error_code),
        );
        circuit_after = Some(change.after);
    }

    StreamFinalization {
        error_category: effective_error_category,
        error_code: effective_error_code,
        probe_result: None,
        circuit_after: circuit_after
            .unwrap_or_else(|| ctx.circuit.snapshot(ctx.provider_id, now_unix)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{StreamTerminalEvidence, StreamTerminalOrigin};
    use super::finalize_circuit_and_session;
    use crate::circuit_breaker::{
        CircuitBreaker, CircuitBreakerConfig, CircuitState, ProbeAcquireResult, ProbeLeaseGuard,
        ProbeTrigger,
    };
    use crate::gateway::active_requests::ActiveRequestRegistry;
    use crate::gateway::proxy::dispatch::RequestDispatchIntent;
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
                    provider_cooldown_secs: 0,
                    ..CircuitBreakerConfig::default()
                },
                HashMap::new(),
                None,
            )),
            dispatch_ownership: None,
            session: Arc::new(session_manager::SessionManager::new()),
            session_id: Some("sess-stream-finalize".to_string()),
            sort_mode_id: None,
            is_compact_request: false,
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

    fn arm_probe(ctx: &mut StreamFinalizeCtx<tauri::test::MockRuntime>, now_unix: i64) {
        let opened = ctx
            .circuit
            .record_failure(ctx.provider_id, now_unix, Some("TEST_PROBE_OPEN"));
        assert_eq!(opened.after.state, CircuitState::Open);
        let token = match ctx.circuit.try_acquire_probe(
            ctx.provider_id,
            ctx.trace_id.as_str(),
            ProbeTrigger::AggressiveTurn,
            now_unix,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership =
            RequestDispatchIntent::new(ctx.provider_id, Some(ProbeTrigger::AggressiveTurn), None)
                .claim_for_provider(
                    ctx.provider_id,
                    Some(ProbeLeaseGuard::new(ctx.circuit.clone(), token)),
                )
                .expect("claim probe ownership");
        assert!(ownership.commit_at_transport_boundary(now_unix));
        ctx.dispatch_ownership = Some(ownership);
    }

    fn assert_probe_failure_for_evidence(
        case: &str,
        evidence: StreamTerminalEvidence,
        expected_error_code: &'static str,
    ) {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join(format!("probe-{case}.sqlite")))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        arm_probe(&mut ctx, now_unix);

        let result = finalize_circuit_and_session(&ctx, None, evidence);

        assert_eq!(result.error_code, Some(expected_error_code), "{case}");
        assert_eq!(
            result.error_category,
            Some(
                if expected_error_code == GatewayErrorCode::StreamAborted.as_str() {
                    ErrorCategory::ClientAbort.as_str()
                } else {
                    ErrorCategory::SystemError.as_str()
                }
            ),
            "{case} terminal category"
        );
        assert_eq!(result.probe_result, Some("failed"), "{case}");
        assert_eq!(
            ctx.circuit.snapshot(ctx.provider_id, now_unix).state,
            CircuitState::Open,
            "{case} keeps circuit protected"
        );
        assert_eq!(
            ctx.session.get_bound_provider(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                now_unix,
            ),
            Some(2),
            "{case} preserves stable provider"
        );
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
                finalize_circuit_and_session(
                    &ctx,
                    Some(error_code),
                    StreamTerminalEvidence::new(
                        StreamTerminalOrigin::TerminalFrame,
                        false,
                        false,
                        false,
                        true,
                    ),
                )
                .error_category,
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

    #[test]
    fn probe_closes_only_after_trusted_completion_and_normal_eof() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("probe-normal-eof.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        arm_probe(&mut ctx, now_unix);

        let result = finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::NormalEof, true, true, true, false),
        );

        assert_eq!(result.error_code, None);
        assert_eq!(result.probe_result, Some("success"));
        assert_eq!(
            ctx.circuit.snapshot(ctx.provider_id, now_unix).state,
            CircuitState::Closed
        );
        assert_eq!(
            ctx.session.get_bound_provider(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                now_unix,
            ),
            Some(ctx.provider_id)
        );
    }

    #[test]
    fn probe_rejects_fake_200_despite_trusted_terminal_evidence() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db =
            db::init_for_tests(&db_dir.path().join("probe-fake-200.sqlite")).expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.fake_200_detected = true;
        arm_probe(&mut ctx, now_unix);

        let result = finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::NormalEof, true, true, true, false),
        );

        assert_eq!(result.error_code, Some(GatewayErrorCode::Fake200.as_str()));
        assert_eq!(result.probe_result, Some("failed"));
        assert_eq!(
            ctx.circuit.snapshot(ctx.provider_id, now_unix).state,
            CircuitState::Open
        );
    }

    #[test]
    fn probe_rejects_completion_before_abort_direct_drop_and_late_error() {
        assert_probe_failure_for_evidence(
            "completion-before-abort",
            StreamTerminalEvidence::new(StreamTerminalOrigin::ClientAbort, true, true, true, false),
            GatewayErrorCode::StreamAborted.as_str(),
        );
        assert_probe_failure_for_evidence(
            "direct-drop",
            StreamTerminalEvidence::new(StreamTerminalOrigin::DirectDrop, true, false, true, false),
            GatewayErrorCode::StreamAborted.as_str(),
        );
        assert_probe_failure_for_evidence(
            "late-read-error",
            StreamTerminalEvidence::new(
                StreamTerminalOrigin::UpstreamReadError,
                true,
                false,
                true,
                false,
            ),
            GatewayErrorCode::StreamError.as_str(),
        );
        assert_probe_failure_for_evidence(
            "idle-timeout",
            StreamTerminalEvidence::new(
                StreamTerminalOrigin::IdleTimeout,
                false,
                false,
                false,
                false,
            ),
            GatewayErrorCode::StreamIdleTimeout.as_str(),
        );
        assert_probe_failure_for_evidence(
            "terminal-frame",
            StreamTerminalEvidence::new(
                StreamTerminalOrigin::TerminalFrame,
                true,
                false,
                true,
                true,
            ),
            GatewayErrorCode::StreamError.as_str(),
        );
    }

    #[test]
    fn direct_failback_stream_binds_only_after_trusted_terminal_completion() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("direct-failback-terminal.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        let ownership = RequestDispatchIntent::new(ctx.provider_id, None, None)
            .claim_for_provider(ctx.provider_id, None)
            .expect("direct failback ownership");
        assert!(ownership.commit_at_transport_boundary(now_unix));
        ctx.dispatch_ownership = Some(ownership);

        finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::NormalEof, false, true, false, false),
        );
        assert_eq!(
            ctx.session.get_bound_provider(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                now_unix,
            ),
            Some(2)
        );

        finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::NormalEof, true, true, true, false),
        );
        assert_eq!(
            ctx.session.get_bound_provider(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                now_unix,
            ),
            Some(ctx.provider_id)
        );
    }

    #[test]
    fn direct_failback_non_stream_body_eof_can_bind_session() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("direct-failback-body-eof.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );
        let ownership = RequestDispatchIntent::new(ctx.provider_id, None, None)
            .claim_for_provider(ctx.provider_id, None)
            .expect("direct failback ownership");
        assert!(ownership.commit_at_transport_boundary(now_unix));
        ctx.dispatch_ownership = Some(ownership);

        finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(
                StreamTerminalOrigin::BufferedBodyEof,
                false,
                true,
                false,
                false,
            ),
        );
        assert_eq!(
            ctx.session.get_bound_provider(
                ctx.cli_key.as_str(),
                ctx.session_id.as_deref().expect("session"),
                now_unix,
            ),
            Some(ctx.provider_id)
        );
    }

    #[test]
    fn compact_generation_requires_trusted_stream_completion() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = db::init_for_tests(&db_dir.path().join("compact-terminal.sqlite"))
            .expect("init test db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(4);
        let mut ctx = test_stream_finalize_ctx(app.handle().clone(), db, log_tx);
        let now_unix = crate::gateway::util::now_unix_seconds() as i64;
        ctx.is_compact_request = true;
        ctx.session.bind_success(
            ctx.cli_key.as_str(),
            ctx.session_id.as_deref().expect("session"),
            2,
            None,
            now_unix,
        );

        finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::DirectDrop, true, false, true, false),
        );
        assert_eq!(
            ctx.session
                .routing_snapshot(
                    ctx.cli_key.as_str(),
                    ctx.session_id.as_deref().expect("session"),
                    now_unix,
                )
                .expect("session snapshot")
                .completed_compaction_generation,
            0
        );

        finalize_circuit_and_session(
            &ctx,
            None,
            StreamTerminalEvidence::new(StreamTerminalOrigin::NormalEof, true, true, true, false),
        );
        assert_eq!(
            ctx.session
                .routing_snapshot(
                    ctx.cli_key.as_str(),
                    ctx.session_id.as_deref().expect("session"),
                    now_unix,
                )
                .expect("session snapshot")
                .completed_compaction_generation,
            1
        );
    }
}
