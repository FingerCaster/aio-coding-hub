//! Usage: Stream finalization context for gateway body relays.

use crate::gateway::active_requests::ActiveRequestRegistry;
use crate::gateway::plugins::pipeline::GatewayPluginPipeline;
use crate::{circuit_breaker, db, request_logs, session_manager};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::super::events::FailoverAttempt;

const ACTIVITY_FLUSH_INTERVAL_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gateway) enum StreamTerminalOrigin {
    Unclassified,
    NormalEof,
    ProtocolTerminal,
    CompletionDelivered,
    UpstreamReadError,
    IdleTimeout,
    TotalTimeout,
    TerminalFrame,
    ClientAbort,
    DirectDrop,
    RelayDrainTimeout,
    BufferedBodyEof,
}

impl StreamTerminalOrigin {
    pub(in crate::gateway) fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified => "unclassified",
            Self::NormalEof => "normal_eof",
            Self::ProtocolTerminal => "protocol_terminal",
            Self::CompletionDelivered => "completion_delivered",
            Self::UpstreamReadError => "upstream_read_error",
            Self::IdleTimeout => "idle_timeout",
            Self::TotalTimeout => "total_timeout",
            Self::TerminalFrame => "terminal_frame",
            Self::ClientAbort => "client_abort",
            Self::DirectDrop => "direct_drop",
            Self::RelayDrainTimeout => "relay_drain_timeout",
            Self::BufferedBodyEof => "buffered_body_eof",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gateway) struct StreamTerminalEvidence {
    pub(in crate::gateway) completion_seen: bool,
    pub(in crate::gateway) normal_eof: bool,
    pub(in crate::gateway) usage_seen: bool,
    pub(in crate::gateway) terminal_error_seen: bool,
    pub(in crate::gateway) origin: StreamTerminalOrigin,
}

impl StreamTerminalEvidence {
    pub(in crate::gateway) fn new(
        origin: StreamTerminalOrigin,
        completion_seen: bool,
        normal_eof: bool,
        usage_seen: bool,
        terminal_error_seen: bool,
    ) -> Self {
        Self {
            completion_seen,
            normal_eof,
            usage_seen,
            terminal_error_seen,
            origin,
        }
    }

    pub(in crate::gateway) fn trusted_probe_success(self) -> bool {
        let trusted_terminal = match self.origin {
            StreamTerminalOrigin::NormalEof => self.normal_eof && self.completion_seen,
            StreamTerminalOrigin::ProtocolTerminal => self.completion_seen && self.usage_seen,
            StreamTerminalOrigin::CompletionDelivered => self.completion_seen && self.usage_seen,
            _ => false,
        };
        trusted_terminal && !self.terminal_error_seen
    }
}

pub(in crate::gateway) struct StreamActivityTracker {
    trace_id: String,
    cli_key: String,
    created_at_ms: i64,
    last_activity_ms: i64,
    last_flushed_activity_ms: i64,
    chunk_count: i64,
}

impl StreamActivityTracker {
    pub(in crate::gateway) fn new(trace_id: &str, cli_key: &str, created_at_ms: i64) -> Self {
        Self {
            trace_id: trace_id.to_string(),
            cli_key: cli_key.to_string(),
            created_at_ms,
            last_activity_ms: created_at_ms,
            last_flushed_activity_ms: created_at_ms,
            chunk_count: 0,
        }
    }

    pub(in crate::gateway) fn observe_chunk_at(&mut self, now_ms: i64) -> bool {
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.last_activity_ms = now_ms.max(self.last_activity_ms).max(self.created_at_ms);
        if self
            .last_activity_ms
            .saturating_sub(self.last_flushed_activity_ms)
            < ACTIVITY_FLUSH_INTERVAL_MS
        {
            return false;
        }
        self.last_flushed_activity_ms = self.last_activity_ms;
        true
    }

    pub(in crate::gateway) fn last_activity_ms(&self) -> i64 {
        self.last_activity_ms
    }

    pub(in crate::gateway) fn details_json(&self, terminal_signal: Option<&str>) -> Option<String> {
        serde_json::to_string(&serde_json::json!({
            "trace_id": self.trace_id,
            "cli_key": self.cli_key,
            "chunk_count": self.chunk_count,
            "last_activity_ms": self.last_activity_ms,
            "terminal_signal": terminal_signal,
        }))
        .ok()
    }

    pub(in crate::gateway) fn terminal_details_json(
        &self,
        terminal_signal: Option<&str>,
        evidence: StreamTerminalEvidence,
    ) -> Option<String> {
        serde_json::to_string(&serde_json::json!({
            "trace_id": self.trace_id,
            "cli_key": self.cli_key,
            "chunk_count": self.chunk_count,
            "last_activity_ms": self.last_activity_ms,
            "terminal_signal": terminal_signal,
            "terminal_origin": evidence.origin.as_str(),
            "completion_seen": evidence.completion_seen,
            "normal_eof": evidence.normal_eof,
            "usage_seen": evidence.usage_seen,
            "terminal_error_seen": evidence.terminal_error_seen,
        }))
        .ok()
    }
}

pub(in crate::gateway) struct StreamFinalizeCtx<R: tauri::Runtime = tauri::Wry> {
    pub(in crate::gateway) app: tauri::AppHandle<R>,
    pub(in crate::gateway) db: db::Db,
    pub(in crate::gateway) log_tx: tokio::sync::mpsc::Sender<request_logs::RequestLogInsert>,
    pub(in crate::gateway) plugin_pipeline: Arc<GatewayPluginPipeline>,
    pub(in crate::gateway) circuit: Arc<circuit_breaker::CircuitBreaker>,
    pub(in crate::gateway) dispatch_ownership:
        Option<Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
    pub(in crate::gateway) session: Arc<session_manager::SessionManager>,
    pub(in crate::gateway) session_id: Option<String>,
    pub(in crate::gateway) session_binding_request: Option<session_manager::SessionBindingRequest>,
    pub(in crate::gateway) sort_mode_id: Option<i64>,
    pub(in crate::gateway) is_compact_request: bool,
    pub(in crate::gateway) trace_id: String,
    pub(in crate::gateway) cli_key: String,
    pub(in crate::gateway) method: String,
    pub(in crate::gateway) path: String,
    pub(in crate::gateway) observe: bool,
    pub(in crate::gateway) query: Option<String>,
    pub(in crate::gateway) excluded_from_stats: bool,
    pub(in crate::gateway) special_settings: Arc<Mutex<Vec<serde_json::Value>>>,
    pub(in crate::gateway) provider_health_neutral: bool,
    pub(in crate::gateway) status: u16,
    pub(in crate::gateway) error_category: Option<&'static str>,
    pub(in crate::gateway) error_code: Option<&'static str>,
    pub(in crate::gateway) started: Instant,
    pub(in crate::gateway) attempt_started: Instant,
    pub(in crate::gateway) attempts: Vec<FailoverAttempt>,
    pub(in crate::gateway) attempts_json: String,
    pub(in crate::gateway) requested_model: Option<String>,
    pub(in crate::gateway) requested_upstream_model: Option<String>,
    pub(in crate::gateway) managed_model_route: bool,
    pub(in crate::gateway) created_at_ms: i64,
    pub(in crate::gateway) created_at: i64,
    pub(in crate::gateway) provider_cooldown_secs: i64,
    pub(in crate::gateway) upstream_first_byte_timeout_secs: u32,
    pub(in crate::gateway) upstream_retry_policy: crate::settings::UpstreamRetryPolicy,
    pub(in crate::gateway) detect_stream_internal_errors: bool,
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) provider_name: String,
    pub(in crate::gateway) base_url: String,
    pub(in crate::gateway) auth_mode: String,
    pub(in crate::gateway) use_upstream_usage_metrics: bool,
    pub(in crate::gateway) upstream_route_tracker: Arc<Mutex<crate::usage::SseUsageTracker>>,
    pub(in crate::gateway) observed_upstream_model: Arc<Mutex<Option<String>>>,
    pub(in crate::gateway) observed_upstream_conflicting_model: Arc<Mutex<Option<String>>>,
    pub(in crate::gateway) observed_upstream_reasoning_effort: Arc<Mutex<Option<String>>>,
    pub(in crate::gateway) fake_200_detected: bool,
    pub(in crate::gateway) fake_200_quota_exhausted: bool,
    pub(in crate::gateway) activity: Arc<Mutex<StreamActivityTracker>>,
    pub(in crate::gateway) active_requests: Arc<ActiveRequestRegistry>,
}

#[cfg(test)]
mod tests {
    use super::{StreamTerminalEvidence, StreamTerminalOrigin};

    #[test]
    fn probe_terminal_success_requires_completion_and_normal_eof() {
        assert!(StreamTerminalEvidence::new(
            StreamTerminalOrigin::NormalEof,
            true,
            true,
            false,
            false,
        )
        .trusted_probe_success());

        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::NormalEof,
            false,
            true,
            true,
            false,
        )
        .trusted_probe_success());
    }

    #[test]
    fn completion_before_client_abort_is_not_probe_success() {
        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::ClientAbort,
            true,
            true,
            true,
            false,
        )
        .trusted_probe_success());
    }

    #[test]
    fn delivered_completion_is_trusted_only_with_usage_and_no_terminal_error() {
        assert!(StreamTerminalEvidence::new(
            StreamTerminalOrigin::CompletionDelivered,
            true,
            false,
            true,
            false,
        )
        .trusted_probe_success());
        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::CompletionDelivered,
            true,
            false,
            false,
            false,
        )
        .trusted_probe_success());
        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::CompletionDelivered,
            true,
            false,
            true,
            true,
        )
        .trusted_probe_success());
    }

    #[test]
    fn protocol_terminal_is_trusted_without_claiming_transport_eof() {
        assert!(StreamTerminalEvidence::new(
            StreamTerminalOrigin::ProtocolTerminal,
            true,
            false,
            true,
            false,
        )
        .trusted_probe_success());
        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::ProtocolTerminal,
            true,
            true,
            false,
            false,
        )
        .trusted_probe_success());
    }

    #[test]
    fn direct_drop_and_late_read_error_are_not_probe_success() {
        for origin in [
            StreamTerminalOrigin::DirectDrop,
            StreamTerminalOrigin::UpstreamReadError,
            StreamTerminalOrigin::RelayDrainTimeout,
        ] {
            assert!(
                !StreamTerminalEvidence::new(origin, true, false, true, false)
                    .trusted_probe_success()
            );
        }
    }

    #[test]
    fn terminal_error_rejects_otherwise_complete_normal_eof() {
        assert!(!StreamTerminalEvidence::new(
            StreamTerminalOrigin::NormalEof,
            true,
            true,
            true,
            true,
        )
        .trusted_probe_success());
    }
}
