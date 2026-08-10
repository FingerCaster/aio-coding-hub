//! Usage: Best-effort drop guard to log client-aborted requests.

use crate::gateway::active_requests::ActiveRequestRegistry;
use crate::gateway::events::FailoverAttempt;
use crate::gateway::plugins::pipeline::GatewayPluginPipeline;
use crate::gateway::response_fixer;
use crate::{db, request_logs};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::request_end::{
    emit_request_event_and_spawn_request_log, RequestCompletion, RequestEndArgs,
    RequestEndContextArgs, RequestEndDeps,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestAbortReason {
    ClientCancelled,
    GatewayShutdown,
}

pub(super) struct RequestAbortGuard<R: tauri::Runtime = tauri::Wry> {
    app: tauri::AppHandle<R>,
    db: db::Db,
    log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
    plugin_pipeline: Arc<GatewayPluginPipeline>,
    active_requests: Arc<ActiveRequestRegistry>,
    trace_id: String,
    cli_key: String,
    method: String,
    path: String,
    observe: bool,
    query: Option<String>,
    session_id: Option<String>,
    requested_model: Option<String>,
    special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    in_flight_attempt: Option<FailoverAttempt>,
    dispatch_ownership: Option<Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
    infinite_retry_ledger: Option<crate::gateway::infinite_retry::SharedInfiniteRetryLedger>,
    created_at_ms: i64,
    created_at: i64,
    started: Instant,
    abort_reason: RequestAbortReason,
    armed: bool,
}

impl<R: tauri::Runtime> RequestAbortGuard<R> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        app: tauri::AppHandle<R>,
        db: db::Db,
        log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
        plugin_pipeline: Arc<GatewayPluginPipeline>,
        active_requests: Arc<ActiveRequestRegistry>,
        trace_id: String,
        cli_key: String,
        method: String,
        path: String,
        observe: bool,
        query: Option<String>,
        session_id: Option<String>,
        requested_model: Option<String>,
        special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
        created_at_ms: i64,
        created_at: i64,
        started: Instant,
    ) -> Self {
        Self {
            app,
            db,
            log_tx,
            plugin_pipeline,
            active_requests,
            trace_id,
            cli_key,
            method,
            path,
            observe,
            query,
            session_id,
            requested_model,
            special_settings,
            in_flight_attempt: None,
            dispatch_ownership: None,
            infinite_retry_ledger: None,
            created_at_ms,
            created_at,
            started,
            abort_reason: RequestAbortReason::ClientCancelled,
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn update_requested_model(&mut self, requested_model: Option<String>) {
        self.requested_model = requested_model;
    }

    /// Take ownership of this guard, leaving a disarmed placeholder behind.
    /// This is useful when you need to pass the guard to a sub-function while
    /// keeping the parent struct borrowable.
    pub(super) fn take(&mut self) -> Self {
        let taken = Self {
            app: self.app.clone(),
            db: self.db.clone(),
            log_tx: self.log_tx.clone(),
            plugin_pipeline: self.plugin_pipeline.clone(),
            active_requests: self.active_requests.clone(),
            trace_id: std::mem::take(&mut self.trace_id),
            cli_key: std::mem::take(&mut self.cli_key),
            method: std::mem::take(&mut self.method),
            path: std::mem::take(&mut self.path),
            observe: self.observe,
            query: self.query.take(),
            session_id: self.session_id.take(),
            requested_model: self.requested_model.take(),
            special_settings: Arc::clone(&self.special_settings),
            in_flight_attempt: self.in_flight_attempt.take(),
            dispatch_ownership: self.dispatch_ownership.take(),
            infinite_retry_ledger: self.infinite_retry_ledger.take(),
            created_at_ms: self.created_at_ms,
            created_at: self.created_at,
            started: self.started,
            abort_reason: self.abort_reason,
            armed: self.armed,
        };
        self.armed = false; // disarm the leftover shell
        taken
    }

    pub(super) fn capture_in_flight_attempt(&mut self, attempt: &FailoverAttempt) {
        self.in_flight_attempt = Some(attempt.clone());
    }

    pub(super) fn replace_dispatch_ownership(
        &mut self,
        ownership: Option<Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
    ) {
        self.dispatch_ownership = ownership;
    }

    pub(super) fn attach_infinite_retry_ledger(
        &mut self,
        ledger: crate::gateway::infinite_retry::SharedInfiniteRetryLedger,
    ) {
        self.infinite_retry_ledger = Some(ledger);
    }

    pub(super) fn mark_gateway_shutdown(&mut self) {
        self.abort_reason = RequestAbortReason::GatewayShutdown;
    }
}

impl<R: tauri::Runtime> Drop for RequestAbortGuard<R> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let probe_ownership = self
            .dispatch_ownership
            .as_ref()
            .filter(|ownership| ownership.is_probe());
        if let Some(ownership) = probe_ownership {
            let _ = ownership.complete_probe_failure(
                crate::gateway::util::now_unix_seconds() as i64,
                false,
                None,
            );
        }
        if !self.observe {
            return;
        }

        let duration_ms = self.started.elapsed().as_millis();
        let mut abort_attempts: Vec<FailoverAttempt> =
            self.in_flight_attempt.iter().cloned().collect();
        if probe_ownership.is_some() {
            if let Some(attempt) = abort_attempts.last_mut() {
                attempt.probe_result = Some("failed");
            }
        }
        let (log_usage_metrics, log_cost_usd_femto, activity_details_json) =
            if let Some(ledger) = self.infinite_retry_ledger.as_ref() {
                let stop_reason = match self.abort_reason {
                    RequestAbortReason::ClientCancelled => "client_cancelled",
                    RequestAbortReason::GatewayShutdown => "gateway_shutdown",
                };
                let (snapshot, usage_metrics, cost_usd_femto) = match ledger.lock() {
                    Ok(mut ledger) => {
                        ledger.stop(stop_reason);
                        (
                            Some(ledger.snapshot()),
                            ledger.usage_metrics(),
                            ledger.cost_usd_femto(),
                        )
                    }
                    Err(_) => (None, None, None),
                };
                let mut activity_details_json = None;
                if let Some(snapshot) = snapshot {
                    let now_ms = crate::gateway::util::now_unix_millis() as i64;
                    self.active_requests.update_infinite_retry_progress(
                        self.trace_id.as_str(),
                        snapshot.phase,
                        snapshot.rounds.parse().unwrap_or(u64::MAX),
                        snapshot.attempts.parse().unwrap_or(u64::MAX),
                        now_ms,
                    );
                    let _ = crate::infinite_retry_provider_usage::replace_for_trace(
                        &self.db,
                        self.trace_id.as_str(),
                        snapshot.providers.as_slice(),
                        now_ms,
                    );
                    if let Ok(details) = serde_json::to_string(&snapshot) {
                        activity_details_json = Some(details.clone());
                        let _ = request_logs::touch_activity(
                            &self.db,
                            self.trace_id.as_str(),
                            self.cli_key.as_str(),
                            now_ms,
                            Some(details),
                        );
                    }
                }
                (usage_metrics, cost_usd_femto, activity_details_json)
            } else {
                (None, None, None)
            };
        let completion = match self.abort_reason {
            RequestAbortReason::ClientCancelled => RequestCompletion::client_abort_with_log_usage(
                log_usage_metrics,
                log_cost_usd_femto,
            ),
            RequestAbortReason::GatewayShutdown => {
                RequestCompletion::gateway_shutdown_with_log_usage(
                    log_usage_metrics,
                    log_cost_usd_femto,
                )
            }
        }
        .with_log_activity_details_json(activity_details_json);

        emit_request_event_and_spawn_request_log(
            RequestEndArgs::from_context(RequestEndContextArgs {
                deps: RequestEndDeps::new(
                    &self.app,
                    &self.db,
                    &self.log_tx,
                    &self.plugin_pipeline,
                    &self.active_requests,
                ),
                trace_id: self.trace_id.as_str(),
                cli_key: self.cli_key.as_str(),
                method: self.method.as_str(),
                path: self.path.as_str(),
                observe: self.observe,
                query: self.query.as_deref(),
                excluded_from_stats: false,
                duration_ms,
                attempts: abort_attempts.as_slice(),
                special_settings_json: response_fixer::special_settings_json(
                    &self.special_settings,
                ),
                session_id: self.session_id.clone(),
                requested_model: self.requested_model.clone(),
                created_at_ms: self.created_at_ms,
                created_at: self.created_at,
            })
            .with_completion(completion),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit_breaker::{
        CircuitBreaker, CircuitBreakerConfig, ProbeAcquireResult, ProbeLeaseGuard, ProbeTrigger,
    };
    use crate::gateway::proxy::dispatch::RequestDispatchIntent;
    use std::collections::HashMap;

    #[tokio::test(flavor = "current_thread")]
    async fn gateway_shutdown_persists_distinct_stop_reason_and_status() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db =
            crate::db::init_for_tests(&db_dir.path().join("abort-shutdown.db")).expect("init db");
        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel(1);
        let active_requests = Arc::new(ActiveRequestRegistry::default());
        active_requests.register(crate::gateway::active_requests::ActiveRequestStart {
            trace_id: "trace-gateway-shutdown".to_string(),
            cli_key: "codex".to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            query: None,
            session_id: Some("session-gateway-shutdown".to_string()),
            requested_model: Some("gpt-5".to_string()),
            created_at_ms: 1_000_000,
            codex_infinite_retry_test: true,
        });
        let ledger = Arc::new(Mutex::new(
            crate::gateway::infinite_retry::InfiniteRetryLedger::default(),
        ));
        ledger.lock().expect("ledger").start_round(true);
        let mut guard = RequestAbortGuard::new(
            app.handle().clone(),
            db,
            log_tx,
            GatewayPluginPipeline::empty_shared(),
            Arc::clone(&active_requests),
            "trace-gateway-shutdown".to_string(),
            "codex".to_string(),
            "POST".to_string(),
            "/v1/responses".to_string(),
            true,
            None,
            Some("session-gateway-shutdown".to_string()),
            Some("gpt-5".to_string()),
            Arc::new(Mutex::new(Vec::new())),
            1_000_000,
            1_000,
            Instant::now(),
        );
        guard.attach_infinite_retry_ledger(ledger);
        guard.mark_gateway_shutdown();

        drop(guard);

        let log = tokio::time::timeout(std::time::Duration::from_secs(2), log_rx.recv())
            .await
            .expect("shutdown log should be enqueued")
            .expect("log channel should stay open");
        assert_eq!(log.status, Some(499));
        assert_eq!(
            log.error_code.as_deref(),
            Some(crate::gateway::proxy::GatewayErrorCode::RequestInterruptedByGatewayStop.as_str())
        );
        let activity: serde_json::Value = serde_json::from_str(
            log.activity_details_json
                .as_deref()
                .expect("infinite retry activity details"),
        )
        .expect("activity details json");
        assert_eq!(activity["phase"], "stopped");
        assert_eq!(activity["stopReason"], "gateway_shutdown");
        assert!(active_requests.snapshot().is_empty());
    }

    #[test]
    fn cloned_abort_attempt_keeps_provider_context() {
        let attempt = FailoverAttempt {
            provider_id: 12,
            provider_name: "Claude Bridge".to_string(),
            base_url: "https://example.com".to_string(),
            outcome: "started".to_string(),
            status: None,
            provider_index: Some(1),
            retry_index: Some(1),
            session_reuse: Some(true),
            error_category: None,
            error_code: None,
            decision: None,
            reason: None,
            selection_method: Some("session_reuse"),
            reason_code: None,
            attempt_started_ms: Some(123),
            attempt_duration_ms: Some(0),
            circuit_state_before: Some("CLOSED"),
            circuit_state_after: None,
            circuit_failure_count: Some(0),
            circuit_failure_threshold: Some(5),
            probe: None,
            probe_trigger: None,
            probe_result: None,
            probe_generation: None,
            circuit_recover_at_unix: None,
            circuit_trigger_error_code: None,
            provider_bridged: Some(true),
            timeout_secs: None,
            stream_internal_error: None,
            requested_upstream_model: None,
        };

        let logged_attempts: Vec<FailoverAttempt> = Some(attempt.clone()).iter().cloned().collect();
        assert_eq!(logged_attempts.len(), 1);
        assert_eq!(logged_attempts[0].provider_id, 12);
        assert_eq!(logged_attempts[0].provider_name, "Claude Bridge");
        assert_eq!(logged_attempts[0].outcome, "started");
    }

    #[test]
    fn replacing_probe_ownership_with_none_prevents_next_provider_abort_misattribution() {
        let app = tauri::test::mock_app();
        let db_dir = tempfile::tempdir().expect("db dir");
        let db =
            crate::db::init_for_tests(&db_dir.path().join("abort-ownership.db")).expect("init db");
        let (log_tx, _log_rx) = tokio::sync::mpsc::channel(1);
        let circuit = Arc::new(CircuitBreaker::new(
            CircuitBreakerConfig {
                failure_threshold: 1,
                provider_cooldown_secs: 0,
                ..CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        circuit.record_failure(1, 1_000, None);
        let token =
            match circuit.try_acquire_probe(1, "p1-probe", ProbeTrigger::AggressiveTurn, 1_000) {
                ProbeAcquireResult::Acquired { token, .. } => token,
                other => panic!("expected P1 probe lease, got {other:?}"),
            };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("P1 ownership");
        assert!(ownership.commit_at_transport_boundary(1_000));

        let mut guard = RequestAbortGuard::new(
            app.handle().clone(),
            db,
            log_tx,
            GatewayPluginPipeline::empty_shared(),
            Arc::new(ActiveRequestRegistry::default()),
            "trace-p1-p2".to_string(),
            "claude".to_string(),
            "POST".to_string(),
            "/v1/messages".to_string(),
            false,
            None,
            None,
            None,
            Arc::new(Mutex::new(Vec::new())),
            1_000_000,
            1_000,
            Instant::now(),
        );
        guard.replace_dispatch_ownership(Some(Arc::clone(&ownership)));
        // P2 has no probe ownership. Dropping the request during P2 must not
        // complete or attribute P1's probe token.
        guard.replace_dispatch_ownership(None);
        drop(guard);

        assert!(circuit.snapshot(1, 1_001).probe_in_flight);
    }
}
