//! Usage: Types for circuit breaker state machine.

use tokio::sync::mpsc;

/// Failures older than this window (in seconds) are not counted toward the threshold.
pub(super) const FAILURE_WINDOW_SECS: u64 = 300;

/// Hard cap on stored failure timestamps to prevent unbounded memory growth.
pub(crate) const MAX_FAILURE_TIMESTAMPS: usize = 256;

pub(super) const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
pub(super) const DEFAULT_OPEN_DURATION_SECS: i64 = 30 * 60;
pub(super) const DEFAULT_PROVIDER_COOLDOWN_SECS: i64 = 30;
pub(super) const DEFAULT_NATURAL_PROBE_MAX_WAIT_SECS: i64 = 300;
pub(super) const RECOVERY_GUARD_SECS: i64 = 300;
pub(super) const PROBE_LEASE_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "CLOSED",
            Self::Open => "OPEN",
            Self::HalfOpen => "HALF_OPEN",
        }
    }

    pub fn from_str(raw: &str) -> Self {
        match raw {
            "OPEN" => Self::Open,
            "HALF_OPEN" => Self::HalfOpen,
            _ => Self::Closed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub open_duration_secs: i64,
    pub provider_cooldown_secs: i64,
    pub natural_probe_max_wait_secs: i64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            open_duration_secs: DEFAULT_OPEN_DURATION_SECS,
            provider_cooldown_secs: DEFAULT_PROVIDER_COOLDOWN_SECS,
            natural_probe_max_wait_secs: DEFAULT_NATURAL_PROBE_MAX_WAIT_SECS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeTrigger {
    NewUnboundSession,
    RouteChanged,
    NaturalCompaction,
    NaturalMaxWait,
    AggressiveTurn,
    MaxOpenWait,
}

impl ProbeTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewUnboundSession => "new_unbound_session",
            Self::RouteChanged => "route_changed",
            Self::NaturalCompaction => "natural_compaction",
            Self::NaturalMaxWait => "natural_max_wait",
            Self::AggressiveTurn => "aggressive_turn",
            Self::MaxOpenWait => "max_open_wait",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeLeaseToken {
    pub provider_id: i64,
    pub generation: u64,
    pub owner_trace_id: String,
    pub trigger: ProbeTrigger,
}

#[derive(Debug, Clone)]
pub struct CircuitSnapshot {
    pub state: CircuitState,
    pub failure_count: u32,
    pub failure_threshold: u32,
    pub open_until: Option<i64>,
    pub cooldown_until: Option<i64>,
    pub probe_reference_at: Option<i64>,
    pub next_probe_at: Option<i64>,
    pub natural_probe_due_at: Option<i64>,
    pub recovery_guard_until: Option<i64>,
    /// Explicit lease ownership marker retained for gate diagnostics.
    #[cfg_attr(not(test), allow(dead_code))]
    pub probe_in_flight: bool,
    pub state_revision: u64,
    /// Error code of the most recent attributed failure (in-memory only;
    /// intentionally not persisted, lost across restart).
    pub last_trigger_error_code: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CircuitTransition {
    pub prev_state: CircuitState,
    pub next_state: CircuitState,
    pub reason: &'static str,
    pub snapshot: CircuitSnapshot,
}

#[derive(Debug, Clone)]
pub struct CircuitChange {
    pub before: CircuitSnapshot,
    pub after: CircuitSnapshot,
    pub transition: Option<Box<CircuitTransition>>,
}

#[derive(Debug, Clone)]
pub struct CircuitCheck {
    pub allow: bool,
    pub after: CircuitSnapshot,
    pub transition: Option<CircuitTransition>,
}

#[derive(Debug, Clone)]
pub enum ProbeAcquireResult {
    Acquired {
        token: ProbeLeaseToken,
        snapshot: CircuitSnapshot,
    },
    NotOpen,
    Cooldown(CircuitSnapshot),
    InFlight(CircuitSnapshot),
}

#[derive(Debug, Clone)]
pub enum ProbeCommitResult {
    Applied(CircuitChange),
    Stale(CircuitSnapshot),
}

#[derive(Debug, Clone)]
pub struct CircuitPersistedState {
    pub provider_id: i64,
    pub state: CircuitState,
    pub failure_timestamps: Vec<u64>,
    pub half_open_success_count: u32,
    pub open_until: Option<i64>,
    pub probe_reference_at: Option<i64>,
    pub next_probe_at: Option<i64>,
    pub natural_probe_due_at: Option<i64>,
    pub recovery_guard_until: Option<i64>,
    pub state_revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub(super) struct ProbeLeaseState {
    pub(super) generation: u64,
    pub(super) owner_trace_id: String,
    pub(super) trigger: ProbeTrigger,
    pub(super) dispatched_at: Option<i64>,
    pub(super) expires_at: i64,
}

#[derive(Debug, Clone)]
pub(super) struct ProviderHealth {
    pub(super) state: CircuitState,
    pub(super) failure_timestamps: Vec<u64>,
    pub(super) half_open_success_count: u32,
    pub(super) open_until: Option<i64>,
    pub(super) cooldown_until: Option<i64>,
    pub(super) probe_reference_at: Option<i64>,
    pub(super) next_probe_at: Option<i64>,
    pub(super) natural_probe_due_at: Option<i64>,
    pub(super) recovery_guard_until: Option<i64>,
    pub(super) state_revision: u64,
    pub(super) probe_generation: u64,
    pub(super) probe_lease: Option<ProbeLeaseState>,
    pub(super) updated_at: i64,
    /// Most recent attributed failure error code; cleared when the circuit
    /// returns to Closed. Never persisted.
    pub(super) last_trigger_error_code: Option<&'static str>,
}

impl ProviderHealth {
    pub(super) fn closed(provider_id: i64, now_unix: i64) -> (i64, Self) {
        (
            provider_id,
            Self {
                state: CircuitState::Closed,
                failure_timestamps: Vec::new(),
                half_open_success_count: 0,
                open_until: None,
                cooldown_until: None,
                probe_reference_at: None,
                next_probe_at: None,
                natural_probe_due_at: None,
                recovery_guard_until: None,
                state_revision: 0,
                probe_generation: 0,
                probe_lease: None,
                updated_at: now_unix,
                last_trigger_error_code: None,
            },
        )
    }

    /// Count failures within the sliding window.
    pub(super) fn effective_failure_count(&self, now: u64) -> u32 {
        let cutoff = now.saturating_sub(FAILURE_WINDOW_SECS);
        let count = self
            .failure_timestamps
            .iter()
            .filter(|&&ts| ts > cutoff)
            .count();
        count.min(u32::MAX as usize) as u32
    }

    /// Remove failure timestamps that have fallen outside the window,
    /// and enforce a hard cap to prevent unbounded growth.
    pub(super) fn prune_old_failures(&mut self, now: u64) {
        let cutoff = now.saturating_sub(FAILURE_WINDOW_SECS);
        self.failure_timestamps.retain(|&ts| ts > cutoff);
        if self.failure_timestamps.len() > MAX_FAILURE_TIMESTAMPS {
            let excess = self.failure_timestamps.len() - MAX_FAILURE_TIMESTAMPS;
            self.failure_timestamps.drain(..excess);
        }
    }
}

use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

#[derive(Debug)]
pub struct CircuitBreaker {
    pub(super) config: Mutex<CircuitBreakerConfig>,
    pub(super) health: Mutex<HashMap<i64, ProviderHealth>>,
    pub(super) persist_tx: Option<mpsc::Sender<CircuitPersistedState>>,
    pub(super) persist_backlog: Arc<Mutex<HashMap<i64, CircuitPersistedState>>>,
    pub(super) persist_backlog_flush_scheduled: Arc<AtomicBool>,
}
