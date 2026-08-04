//! Usage: In-memory circuit breaker to protect providers from repeated failures.

mod types;

pub(crate) use types::MAX_FAILURE_TIMESTAMPS;
pub use types::{
    CircuitBreakerConfig, CircuitChange, CircuitCheck, CircuitPersistedState, CircuitSnapshot,
    CircuitState, CircuitTransition, ProbeAcquireResult, ProbeCommitResult, ProbeLeaseToken,
    ProbeTrigger,
};
use types::{ProbeLeaseState, ProviderHealth, PROBE_LEASE_TTL_SECS, RECOVERY_GUARD_SECS};

pub use types::CircuitBreaker;

use super::mutex_ext::MutexExt;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc::error::TrySendError;

const MAX_PERSIST_BACKLOG: usize = 512;

fn oldest_persist_backlog_provider_id(
    backlog: &HashMap<i64, CircuitPersistedState>,
) -> Option<i64> {
    backlog
        .iter()
        .min_by_key(|(provider_id, item)| (item.updated_at, **provider_id))
        .map(|(provider_id, _)| *provider_id)
}

fn pop_oldest_persist_backlog(
    backlog: &mut HashMap<i64, CircuitPersistedState>,
) -> Option<(i64, CircuitPersistedState)> {
    let provider_id = oldest_persist_backlog_provider_id(backlog)?;
    let item = backlog.remove(&provider_id)?;
    Some((provider_id, item))
}

async fn flush_persist_backlog_until_idle(
    tx: tokio::sync::mpsc::Sender<CircuitPersistedState>,
    backlog: Arc<Mutex<HashMap<i64, CircuitPersistedState>>>,
    scheduled: Arc<AtomicBool>,
) {
    loop {
        loop {
            let permit = match tx.reserve().await {
                Ok(permit) => permit,
                Err(_) => {
                    let pending = backlog.lock_or_recover().len();
                    if pending > 0 {
                        tracing::warn!(
                            pending,
                            "circuit breaker persist channel closed while background backlog flush was pending"
                        );
                    }
                    scheduled.store(false, Ordering::Release);
                    return;
                }
            };

            let Some((_, item)) = pop_oldest_persist_backlog(&mut backlog.lock_or_recover()) else {
                drop(permit);
                break;
            };

            permit.send(item);
        }

        scheduled.store(false, Ordering::Release);

        if backlog.lock_or_recover().is_empty() {
            break;
        }

        if scheduled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            break;
        }
    }
}

impl CircuitBreaker {
    pub fn new(
        config: CircuitBreakerConfig,
        initial: HashMap<i64, CircuitPersistedState>,
        persist_tx: Option<tokio::sync::mpsc::Sender<CircuitPersistedState>>,
    ) -> Self {
        let mut map = HashMap::with_capacity(initial.len());
        let mut normalized = Vec::new();
        for (provider_id, item) in initial {
            let has_recorded_failures = !item.failure_timestamps.is_empty();
            let latest_recorded_failure_at = item
                .failure_timestamps
                .iter()
                .copied()
                .max()
                .map(|timestamp| timestamp.min(i64::MAX as u64) as i64);
            let mut state = item.state;
            let mut half_open_success_count = item.half_open_success_count;
            let mut probe_reference_at = item.probe_reference_at;
            let mut next_probe_at = item.next_probe_at;
            let mut natural_probe_due_at = item.natural_probe_due_at;
            let mut open_until = item.open_until;
            let mut state_revision = item.state_revision;
            let mut changed = false;

            // An in-flight owner is intentionally not persisted. Legacy HALF_OPEN
            // rows therefore restart in the protected OPEN state.
            if state == CircuitState::HalfOpen {
                state = CircuitState::Open;
                half_open_success_count = 0;
                changed = true;
            }
            if state == CircuitState::Open {
                let reference = probe_reference_at.unwrap_or(item.updated_at);
                if probe_reference_at.is_none() {
                    probe_reference_at = Some(reference);
                    changed = true;
                }
                if next_probe_at.is_none() {
                    next_probe_at = Some(reference.saturating_add(config.provider_cooldown_secs));
                    changed = true;
                }
                if natural_probe_due_at.is_none() {
                    natural_probe_due_at =
                        Some(reference.saturating_add(config.natural_probe_max_wait_secs));
                    changed = true;
                }
                if open_until.is_none() {
                    open_until = Some(reference.saturating_add(config.open_duration_secs));
                    changed = true;
                }
            } else if state == CircuitState::Closed
                && (has_recorded_failures || probe_reference_at.is_some())
            {
                let reference = probe_reference_at
                    .or(latest_recorded_failure_at)
                    .unwrap_or(item.updated_at);
                if probe_reference_at.is_none() {
                    probe_reference_at = Some(reference);
                    changed = true;
                }
                if natural_probe_due_at.is_none() {
                    natural_probe_due_at =
                        Some(reference.saturating_add(config.natural_probe_max_wait_secs));
                    changed = true;
                }
            }
            if changed {
                state_revision = state_revision.saturating_add(1).max(1);
            }

            let health = ProviderHealth {
                state,
                failure_timestamps: item.failure_timestamps,
                half_open_success_count,
                open_until,
                cooldown_until: None,
                probe_reference_at,
                next_probe_at,
                natural_probe_due_at,
                recovery_guard_until: item.recovery_guard_until,
                state_revision,
                probe_generation: state_revision,
                probe_lease: None,
                updated_at: item.updated_at,
                // Trigger attribution is in-memory only; lost across restart.
                last_trigger_error_code: None,
            };
            if Self::is_inert_closed_health(&health) && health.state_revision == 0 {
                continue;
            }
            if changed {
                normalized.push(Self::persisted_from_health(provider_id, &health));
            }
            map.insert(provider_id, health);
        }

        let breaker = Self {
            config: std::sync::Mutex::new(config),
            health: std::sync::Mutex::new(map),
            persist_tx,
            persist_backlog: Arc::new(Mutex::new(HashMap::new())),
            persist_backlog_flush_scheduled: Arc::new(AtomicBool::new(false)),
        };
        for item in normalized {
            breaker.try_persist(item);
        }
        breaker
    }

    fn read_config(&self) -> CircuitBreakerConfig {
        self.config.lock_or_recover().clone()
    }

    fn closed_snapshot(cfg: &CircuitBreakerConfig) -> CircuitSnapshot {
        CircuitSnapshot {
            state: CircuitState::Closed,
            failure_count: 0,
            failure_threshold: cfg.failure_threshold,
            open_until: None,
            cooldown_until: None,
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: None,
            recovery_guard_until: None,
            probe_in_flight: false,
            state_revision: 0,
            last_trigger_error_code: None,
        }
    }

    fn is_inert_closed_health(health: &ProviderHealth) -> bool {
        health.state == CircuitState::Closed
            && health.failure_timestamps.is_empty()
            && health.half_open_success_count == 0
            && health.open_until.is_none()
            && health.cooldown_until.is_none()
            && health.probe_reference_at.is_none()
            && health.next_probe_at.is_none()
            && health.natural_probe_due_at.is_none()
            && health.recovery_guard_until.is_none()
            && health.probe_lease.is_none()
    }

    /// Hot-reload circuit breaker configuration.
    pub fn update_config(&self, new_config: CircuitBreakerConfig) {
        let mut upserts: Vec<CircuitPersistedState> = Vec::new();

        let old_config = self.read_config();

        {
            let mut cfg_guard = self.config.lock_or_recover();
            *cfg_guard = new_config.clone();
        }

        let open_timing_changed = old_config.open_duration_secs != new_config.open_duration_secs
            || old_config.provider_cooldown_secs != new_config.provider_cooldown_secs;
        let natural_timing_changed =
            old_config.natural_probe_max_wait_secs != new_config.natural_probe_max_wait_secs;
        if open_timing_changed || natural_timing_changed {
            let mut guard = self.health.lock_or_recover();
            for (&provider_id, entry) in guard.iter_mut() {
                if entry.state == CircuitState::Open {
                    let reference = entry.probe_reference_at.unwrap_or(entry.updated_at);
                    entry.probe_reference_at = Some(reference);
                    entry.open_until =
                        Some(reference.saturating_add(new_config.open_duration_secs));
                    entry.next_probe_at =
                        Some(reference.saturating_add(new_config.provider_cooldown_secs));
                    entry.natural_probe_due_at =
                        Some(reference.saturating_add(new_config.natural_probe_max_wait_secs));
                    Self::bump_revision(entry);
                    upserts.push(Self::persisted_from_health(provider_id, entry));
                } else if natural_timing_changed && entry.state == CircuitState::Closed {
                    if let Some(reference) = entry.probe_reference_at {
                        entry.natural_probe_due_at =
                            Some(reference.saturating_add(new_config.natural_probe_max_wait_secs));
                        Self::bump_revision(entry);
                        upserts.push(Self::persisted_from_health(provider_id, entry));
                    }
                }
            }
        }

        for item in upserts {
            self.try_persist(item);
        }
    }

    #[allow(dead_code)]
    pub fn snapshot(&self, provider_id: i64, now_unix: i64) -> CircuitSnapshot {
        let cfg = self.read_config();
        if provider_id <= 0 {
            return Self::closed_snapshot(&cfg);
        }

        let mut upsert = None;
        let snapshot = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&provider_id) else {
                return Self::closed_snapshot(&cfg);
            };
            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }
            Self::snapshot_from_health(&cfg, entry, now_unix as u64)
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        snapshot
    }

    pub fn should_allow(&self, provider_id: i64, now_unix: i64) -> CircuitCheck {
        let cfg = self.read_config();
        if provider_id <= 0 {
            return CircuitCheck {
                allow: true,
                after: Self::closed_snapshot(&cfg),
                transition: None,
            };
        }

        let mut upsert: Option<CircuitPersistedState> = None;
        let now_u64 = now_unix as u64;

        let (after, allow) = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&provider_id) else {
                return CircuitCheck {
                    allow: true,
                    after: Self::closed_snapshot(&cfg),
                    transition: None,
                };
            };

            if let Some(until) = entry.cooldown_until {
                if now_unix >= until {
                    entry.cooldown_until = None;
                }
            }

            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }

            if entry.state == CircuitState::Closed {
                let before_len = entry.failure_timestamps.len();
                entry.prune_old_failures(now_u64);
                if entry.failure_timestamps.len() != before_len {
                    entry.updated_at = now_unix;
                    Self::bump_revision(entry);
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
            }

            let remove_inert_closed =
                Self::is_inert_closed_health(entry) && entry.state_revision == 0;
            if remove_inert_closed && upsert.is_none() {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }
            let after = Self::snapshot_from_health(&cfg, entry, now_u64);
            let cooldown_active = entry.cooldown_until.map(|t| now_unix < t).unwrap_or(false);
            // OPEN never becomes a generally available HALF_OPEN state. A
            // matching request intent must acquire the provider-scoped lease.
            let allow = entry.state == CircuitState::Closed && !cooldown_active;
            if remove_inert_closed {
                guard.remove(&provider_id);
            }
            (after, allow)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitCheck {
            allow,
            after,
            transition: None,
        }
    }

    pub fn record_success(&self, provider_id: i64, now_unix: i64) -> CircuitChange {
        let cfg = self.read_config();
        if provider_id <= 0 {
            let snap = Self::closed_snapshot(&cfg);
            return CircuitChange {
                before: snap.clone(),
                after: snap,
                transition: None,
            };
        }

        let mut upsert: Option<CircuitPersistedState> = None;
        let transition: Option<CircuitTransition> = None;
        let now_u64 = now_unix as u64;

        let (before, after) = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&provider_id) else {
                let snap = Self::closed_snapshot(&cfg);
                return CircuitChange {
                    before: snap.clone(),
                    after: snap,
                    transition: None,
                };
            };

            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }

            let before = Self::snapshot_from_health(&cfg, entry, now_u64);

            match entry.state {
                CircuitState::Closed => {
                    let persisted_changed = !entry.failure_timestamps.is_empty()
                        || entry.probe_reference_at.is_some()
                        || entry.natural_probe_due_at.is_some();
                    entry.cooldown_until = None;
                    entry.last_trigger_error_code = None;
                    entry.probe_reference_at = None;
                    entry.natural_probe_due_at = None;
                    if persisted_changed {
                        entry.failure_timestamps.clear();
                        entry.updated_at = now_unix;
                        Self::bump_revision(entry);
                        upsert = Some(Self::persisted_from_health(provider_id, entry));
                    }
                }
                // OPEN -> CLOSED is exclusively owned by the current probe
                // token. A late ordinary success must never overwrite it.
                CircuitState::Open | CircuitState::HalfOpen => {}
            }

            let after = Self::snapshot_from_health(&cfg, entry, now_u64);
            (before, after)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitChange {
            before,
            after,
            transition: transition.map(Box::new),
        }
    }

    pub fn record_failure(
        &self,
        provider_id: i64,
        now_unix: i64,
        trigger_error_code: Option<&'static str>,
    ) -> CircuitChange {
        let cfg = self.read_config();
        if provider_id <= 0 {
            let snap = Self::closed_snapshot(&cfg);
            return CircuitChange {
                before: snap.clone(),
                after: snap,
                transition: None,
            };
        }

        let mut upsert: Option<CircuitPersistedState> = None;
        let mut transition: Option<CircuitTransition> = None;
        let now_u64 = now_unix as u64;

        let (before, after) = {
            let mut guard = self.health.lock_or_recover();
            let entry = guard
                .entry(provider_id)
                .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }

            let before = Self::snapshot_from_health(&cfg, entry, now_u64);

            // Remember the most recent attributed failure; an unattributed
            // failure must not erase a known trigger.
            if trigger_error_code.is_some() && entry.state != CircuitState::Open {
                entry.last_trigger_error_code = trigger_error_code;
            }

            match entry.state {
                CircuitState::Closed => {
                    entry.failure_timestamps.push(now_u64);
                    entry.prune_old_failures(now_u64);
                    Self::set_natural_failback_deadline(&cfg, entry, now_unix);
                    entry.updated_at = now_unix;

                    let effective = entry.effective_failure_count(now_u64);
                    let recovery_guard_active = entry
                        .recovery_guard_until
                        .is_some_and(|until| now_unix < until);
                    if recovery_guard_active || effective >= cfg.failure_threshold {
                        let prev = entry.state;
                        Self::protect_open(&cfg, entry, now_unix);

                        let snap = Self::snapshot_from_health(&cfg, entry, now_u64);
                        transition = Some(CircuitTransition {
                            prev_state: prev,
                            next_state: entry.state,
                            reason: if recovery_guard_active {
                                "RECOVERY_GUARD_FAILURE"
                            } else {
                                "FAILURE_THRESHOLD_REACHED"
                            },
                            snapshot: snap,
                        });
                    } else {
                        Self::bump_revision(entry);
                    }
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
                CircuitState::HalfOpen => {
                    let prev = entry.state;
                    entry.failure_timestamps.push(now_u64);
                    entry.prune_old_failures(now_u64);
                    Self::protect_open(&cfg, entry, now_unix);

                    let snap = Self::snapshot_from_health(&cfg, entry, now_u64);
                    transition = Some(CircuitTransition {
                        prev_state: prev,
                        next_state: entry.state,
                        reason: "PROBE_FAILURE",
                        snapshot: snap,
                    });
                    upsert = Some(Self::persisted_from_health(provider_id, entry));
                }
                CircuitState::Open => {}
            }

            let after = Self::snapshot_from_health(&cfg, entry, now_u64);
            (before, after)
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }

        CircuitChange {
            before,
            after,
            transition: transition.map(Box::new),
        }
    }

    pub fn try_acquire_probe(
        &self,
        provider_id: i64,
        owner_trace_id: &str,
        trigger: ProbeTrigger,
        now_unix: i64,
    ) -> ProbeAcquireResult {
        let cfg = self.read_config();
        if provider_id <= 0 || owner_trace_id.trim().is_empty() {
            return ProbeAcquireResult::NotOpen;
        }

        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&provider_id) else {
                return ProbeAcquireResult::NotOpen;
            };
            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(provider_id, entry));
            }

            let snapshot = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
            if entry.state != CircuitState::Open {
                ProbeAcquireResult::NotOpen
            } else if entry.probe_lease.is_some() {
                ProbeAcquireResult::InFlight(snapshot)
            } else if entry
                .next_probe_at
                .is_some_and(|deadline| now_unix < deadline)
            {
                ProbeAcquireResult::Cooldown(snapshot)
            } else {
                entry.probe_generation = entry.probe_generation.saturating_add(1).max(1);
                let token = ProbeLeaseToken {
                    provider_id,
                    generation: entry.probe_generation,
                    owner_trace_id: owner_trace_id.to_string(),
                    trigger,
                };
                entry.probe_lease = Some(ProbeLeaseState {
                    generation: token.generation,
                    owner_trace_id: token.owner_trace_id.clone(),
                    trigger,
                    dispatched_at: None,
                    expires_at: now_unix.saturating_add(PROBE_LEASE_TTL_SECS),
                });
                ProbeAcquireResult::Acquired {
                    token,
                    snapshot: Self::snapshot_from_health(&cfg, entry, now_unix as u64),
                }
            }
        };

        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    pub fn mark_probe_dispatched(
        &self,
        token: &ProbeLeaseToken,
        now_unix: i64,
    ) -> ProbeCommitResult {
        let cfg = self.read_config();
        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&token.provider_id) else {
                return ProbeCommitResult::Stale(Self::closed_snapshot(&cfg));
            };
            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
            }
            if !Self::lease_matches(entry, token) || entry.state != CircuitState::Open {
                ProbeCommitResult::Stale(Self::snapshot_from_health(&cfg, entry, now_unix as u64))
            } else {
                let before = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                if let Some(lease) = entry.probe_lease.as_mut() {
                    lease.dispatched_at.get_or_insert(now_unix);
                    lease.expires_at = now_unix.saturating_add(PROBE_LEASE_TTL_SECS);
                }
                Self::set_open_deadlines(&cfg, entry, now_unix);
                Self::bump_revision(entry);
                let after = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
                ProbeCommitResult::Applied(CircuitChange {
                    before,
                    after,
                    transition: None,
                })
            }
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    pub fn persisted_probe_dispatch_state(
        &self,
        token: &ProbeLeaseToken,
    ) -> Option<CircuitPersistedState> {
        let guard = self.health.lock_or_recover();
        let entry = guard.get(&token.provider_id)?;
        if !Self::lease_matches(entry, token)
            || entry
                .probe_lease
                .as_ref()
                .and_then(|lease| lease.dispatched_at)
                .is_none()
        {
            return None;
        }
        Some(Self::persisted_from_health(token.provider_id, entry))
    }

    pub fn persisted_state(&self, provider_id: i64) -> Option<CircuitPersistedState> {
        let guard = self.health.lock_or_recover();
        guard
            .get(&provider_id)
            .map(|entry| Self::persisted_from_health(provider_id, entry))
    }

    pub fn fail_closed_after_probe_persist_error(
        &self,
        provider_id: i64,
        expected_revision: u64,
        now_unix: i64,
    ) -> CircuitSnapshot {
        let cfg = self.read_config();
        let (snapshot, upsert) = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&provider_id) else {
                return Self::closed_snapshot(&cfg);
            };
            if entry.state == CircuitState::Open && entry.state_revision == expected_revision {
                entry.next_probe_at = Some(i64::MAX);
                Self::bump_revision(entry);
            }
            (
                Self::snapshot_from_health(&cfg, entry, now_unix as u64),
                Self::persisted_from_health(provider_id, entry),
            )
        };
        self.try_persist(upsert);
        snapshot
    }

    pub fn rollback_probe_dispatch(
        &self,
        token: &ProbeLeaseToken,
        before_dispatch: &CircuitSnapshot,
        now_unix: i64,
    ) -> ProbeCommitResult {
        let cfg = self.read_config();
        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&token.provider_id) else {
                return ProbeCommitResult::Stale(Self::closed_snapshot(&cfg));
            };
            let dispatched = entry
                .probe_lease
                .as_ref()
                .and_then(|lease| lease.dispatched_at)
                .is_some();
            if !Self::lease_matches(entry, token) || !dispatched {
                ProbeCommitResult::Stale(Self::snapshot_from_health(&cfg, entry, now_unix as u64))
            } else {
                let before = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                entry.probe_lease = None;
                entry.state = CircuitState::Open;
                entry.open_until = before_dispatch.open_until;
                entry.probe_reference_at = before_dispatch.probe_reference_at;
                entry.next_probe_at = before_dispatch.next_probe_at;
                entry.natural_probe_due_at = before_dispatch.natural_probe_due_at;
                entry.recovery_guard_until = before_dispatch.recovery_guard_until;
                entry.updated_at = now_unix;
                Self::bump_revision(entry);
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
                let after = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                ProbeCommitResult::Applied(CircuitChange {
                    before,
                    after,
                    transition: None,
                })
            }
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    /// Attribute an intermediate failure in the current same-provider retry
    /// chain without releasing the probe lease or resetting its deadlines.
    pub fn record_probe_attempt_failure(
        &self,
        token: &ProbeLeaseToken,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> ProbeCommitResult {
        let cfg = self.read_config();
        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&token.provider_id) else {
                return ProbeCommitResult::Stale(Self::closed_snapshot(&cfg));
            };
            if !Self::lease_matches(entry, token)
                || entry
                    .probe_lease
                    .as_ref()
                    .and_then(|lease| lease.dispatched_at)
                    .is_none()
            {
                ProbeCommitResult::Stale(Self::snapshot_from_health(&cfg, entry, now_unix as u64))
            } else {
                let before = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                if counted_failure {
                    entry.failure_timestamps.push(now_unix as u64);
                    entry.prune_old_failures(now_unix as u64);
                }
                if trigger_error_code.is_some() {
                    entry.last_trigger_error_code = trigger_error_code;
                }
                entry.updated_at = now_unix;
                Self::bump_revision(entry);
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
                let after = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                ProbeCommitResult::Applied(CircuitChange {
                    before,
                    after,
                    transition: None,
                })
            }
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    pub fn complete_probe_success(
        &self,
        token: &ProbeLeaseToken,
        now_unix: i64,
    ) -> ProbeCommitResult {
        let cfg = self.read_config();
        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&token.provider_id) else {
                return ProbeCommitResult::Stale(Self::closed_snapshot(&cfg));
            };
            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
            }
            let dispatched = entry
                .probe_lease
                .as_ref()
                .and_then(|lease| lease.dispatched_at)
                .is_some();
            if !Self::lease_matches(entry, token)
                || entry.state != CircuitState::Open
                || !dispatched
            {
                ProbeCommitResult::Stale(Self::snapshot_from_health(&cfg, entry, now_unix as u64))
            } else {
                let before = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                entry.probe_lease = None;
                entry.state = CircuitState::Closed;
                entry.failure_timestamps.clear();
                entry.half_open_success_count = 0;
                entry.open_until = None;
                entry.cooldown_until = None;
                entry.probe_reference_at = None;
                entry.next_probe_at = None;
                entry.natural_probe_due_at = None;
                entry.recovery_guard_until = Some(now_unix.saturating_add(RECOVERY_GUARD_SECS));
                entry.updated_at = now_unix;
                entry.last_trigger_error_code = None;
                Self::bump_revision(entry);
                let after = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                let transition = Some(CircuitTransition {
                    prev_state: before.state,
                    next_state: after.state,
                    reason: "PROBE_SUCCESS",
                    snapshot: after.clone(),
                });
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
                ProbeCommitResult::Applied(CircuitChange {
                    before,
                    after,
                    transition: transition.map(Box::new),
                })
            }
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    pub fn complete_probe_failure(
        &self,
        token: &ProbeLeaseToken,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> ProbeCommitResult {
        self.finish_probe_failure(token, now_unix, counted_failure, trigger_error_code)
    }

    pub fn abandon_probe(&self, token: &ProbeLeaseToken, now_unix: i64) -> ProbeCommitResult {
        self.finish_probe_failure(token, now_unix, false, None)
    }

    fn finish_probe_failure(
        &self,
        token: &ProbeLeaseToken,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> ProbeCommitResult {
        let cfg = self.read_config();
        let mut upsert = None;
        let result = {
            let mut guard = self.health.lock_or_recover();
            let Some(entry) = guard.get_mut(&token.provider_id) else {
                return ProbeCommitResult::Stale(Self::closed_snapshot(&cfg));
            };
            if Self::expire_runtime_state(&cfg, entry, now_unix) {
                upsert = Some(Self::persisted_from_health(token.provider_id, entry));
            }
            if !Self::lease_matches(entry, token) {
                ProbeCommitResult::Stale(Self::snapshot_from_health(&cfg, entry, now_unix as u64))
            } else {
                let before = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                let dispatched = entry
                    .probe_lease
                    .as_ref()
                    .and_then(|lease| lease.dispatched_at)
                    .is_some();
                entry.probe_lease = None;
                if dispatched {
                    if counted_failure {
                        entry.failure_timestamps.push(now_unix as u64);
                        entry.prune_old_failures(now_unix as u64);
                    }
                    if trigger_error_code.is_some() {
                        entry.last_trigger_error_code = trigger_error_code;
                    }
                    Self::set_open_deadlines(&cfg, entry, now_unix);
                    Self::bump_revision(entry);
                    upsert = Some(Self::persisted_from_health(token.provider_id, entry));
                }
                let after = Self::snapshot_from_health(&cfg, entry, now_unix as u64);
                let transition = dispatched.then(|| CircuitTransition {
                    prev_state: before.state,
                    next_state: after.state,
                    reason: "PROBE_FAILURE",
                    snapshot: after.clone(),
                });
                ProbeCommitResult::Applied(CircuitChange {
                    before,
                    after,
                    transition: transition.map(Box::new),
                })
            }
        };
        if let Some(item) = upsert {
            self.try_persist(item);
        }
        result
    }

    fn lease_matches(entry: &ProviderHealth, token: &ProbeLeaseToken) -> bool {
        entry.probe_lease.as_ref().is_some_and(|lease| {
            lease.generation == token.generation
                && lease.owner_trace_id == token.owner_trace_id
                && lease.trigger == token.trigger
        })
    }

    fn set_natural_failback_deadline(
        cfg: &CircuitBreakerConfig,
        entry: &mut ProviderHealth,
        now_unix: i64,
    ) {
        entry.probe_reference_at = Some(now_unix);
        entry.natural_probe_due_at =
            Some(now_unix.saturating_add(cfg.natural_probe_max_wait_secs.max(1)));
    }

    fn set_open_deadlines(cfg: &CircuitBreakerConfig, entry: &mut ProviderHealth, now_unix: i64) {
        entry.state = CircuitState::Open;
        entry.half_open_success_count = 0;
        Self::set_natural_failback_deadline(cfg, entry, now_unix);
        entry.next_probe_at = Some(now_unix.saturating_add(cfg.provider_cooldown_secs.max(0)));
        entry.open_until = Some(now_unix.saturating_add(cfg.open_duration_secs.max(1)));
        entry.recovery_guard_until = None;
        entry.updated_at = now_unix;
    }

    fn protect_open(cfg: &CircuitBreakerConfig, entry: &mut ProviderHealth, now_unix: i64) {
        entry.probe_lease = None;
        Self::set_open_deadlines(cfg, entry, now_unix);
        Self::bump_revision(entry);
    }

    fn bump_revision(entry: &mut ProviderHealth) {
        entry.state_revision = entry.state_revision.saturating_add(1).max(1);
        entry.probe_generation = entry.probe_generation.max(entry.state_revision);
    }

    fn expire_runtime_state(
        _cfg: &CircuitBreakerConfig,
        entry: &mut ProviderHealth,
        now_unix: i64,
    ) -> bool {
        let mut persisted_changed = false;
        if entry
            .recovery_guard_until
            .is_some_and(|deadline| now_unix >= deadline)
        {
            entry.recovery_guard_until = None;
            entry.updated_at = now_unix;
            Self::bump_revision(entry);
            persisted_changed = true;
        }

        // A dispatched probe is owned by its live request guard. Reclaiming it
        // by wall-clock expiry could admit a second concurrent probe while the
        // first transport is still running. Only undispatched reservations
        // expire; dispatched ownership ends through terminal/drop handling.
        let expired_lease = entry
            .probe_lease
            .as_ref()
            .is_some_and(|lease| lease.dispatched_at.is_none() && now_unix >= lease.expires_at);
        if expired_lease {
            entry.probe_lease = None;
        }
        persisted_changed
    }

    fn snapshot_from_health(
        cfg: &CircuitBreakerConfig,
        health: &ProviderHealth,
        now: u64,
    ) -> CircuitSnapshot {
        let probe_in_flight = health.probe_lease.is_some();
        CircuitSnapshot {
            state: if health.state == CircuitState::Open && probe_in_flight {
                CircuitState::HalfOpen
            } else {
                health.state
            },
            failure_count: health.effective_failure_count(now),
            failure_threshold: cfg.failure_threshold,
            open_until: health.open_until,
            cooldown_until: health.cooldown_until,
            probe_reference_at: health.probe_reference_at,
            next_probe_at: health.next_probe_at,
            natural_probe_due_at: health.natural_probe_due_at,
            recovery_guard_until: health.recovery_guard_until,
            probe_in_flight,
            state_revision: health.state_revision,
            last_trigger_error_code: health.last_trigger_error_code,
        }
    }

    fn persisted_from_health(provider_id: i64, health: &ProviderHealth) -> CircuitPersistedState {
        CircuitPersistedState {
            provider_id,
            state: health.state,
            failure_timestamps: health.failure_timestamps.clone(),
            half_open_success_count: health.half_open_success_count,
            open_until: health.open_until,
            probe_reference_at: health.probe_reference_at,
            next_probe_at: health.next_probe_at,
            natural_probe_due_at: health.natural_probe_due_at,
            recovery_guard_until: health.recovery_guard_until,
            state_revision: health.state_revision,
            updated_at: health.updated_at,
        }
    }

    pub fn trigger_cooldown(
        &self,
        provider_id: i64,
        now_unix: i64,
        cooldown_secs: i64,
    ) -> CircuitSnapshot {
        let cfg = self.read_config();
        let now_u64 = now_unix as u64;
        let cooldown_secs = cooldown_secs.max(0);
        if provider_id <= 0 || cooldown_secs == 0 {
            return self.snapshot(provider_id, now_unix);
        }

        let mut guard = self.health.lock_or_recover();
        let entry = guard
            .entry(provider_id)
            .or_insert_with(|| ProviderHealth::closed(provider_id, now_unix).1);

        let next_until = now_unix.saturating_add(cooldown_secs);
        entry.cooldown_until = Some(match entry.cooldown_until {
            Some(existing) => existing.max(next_until),
            None => next_until,
        });
        entry.updated_at = now_unix;

        Self::snapshot_from_health(&cfg, entry, now_u64)
    }

    pub fn reset(&self, provider_id: i64, now_unix: i64) -> CircuitSnapshot {
        let cfg = self.read_config();
        if provider_id <= 0 {
            return Self::closed_snapshot(&cfg);
        }

        let (upsert, snapshot) = {
            let mut guard = self.health.lock_or_recover();
            let Some(mut entry) = guard.remove(&provider_id) else {
                return Self::closed_snapshot(&cfg);
            };

            entry.state = CircuitState::Closed;
            entry.failure_timestamps.clear();
            entry.half_open_success_count = 0;
            entry.open_until = None;
            entry.cooldown_until = None;
            entry.probe_reference_at = None;
            entry.next_probe_at = None;
            entry.natural_probe_due_at = None;
            entry.recovery_guard_until = None;
            entry.probe_lease = None;
            entry.updated_at = now_unix;
            Self::bump_revision(&mut entry);

            let snapshot = Self::snapshot_from_health(&cfg, &entry, now_unix as u64);
            let upsert = Self::persisted_from_health(provider_id, &entry);
            guard.insert(provider_id, entry);
            (upsert, snapshot)
        };

        self.try_persist(upsert);
        snapshot
    }

    fn try_persist(&self, item: CircuitPersistedState) {
        if let Some(tx) = &self.persist_tx {
            self.flush_persist_backlog(tx);
            match tx.try_send(item) {
                Ok(()) => {}
                Err(TrySendError::Full(item)) => {
                    self.enqueue_persist_backlog(item);
                    self.schedule_persist_backlog_flush(tx);
                }
                Err(TrySendError::Closed(item)) => {
                    tracing::warn!(
                        provider_id = item.provider_id,
                        "circuit breaker persist channel closed; dropping state update"
                    );
                }
            }
        }
    }

    fn flush_persist_backlog(&self, tx: &tokio::sync::mpsc::Sender<CircuitPersistedState>) {
        let mut backlog = self.persist_backlog.lock_or_recover();
        while let Some((provider_id, item)) = pop_oldest_persist_backlog(&mut backlog) {
            match tx.try_send(item) {
                Ok(()) => {}
                Err(TrySendError::Full(item)) => {
                    backlog.insert(provider_id, item);
                    break;
                }
                Err(TrySendError::Closed(item)) => {
                    backlog.insert(provider_id, item);
                    tracing::warn!(
                        pending = backlog.len(),
                        "circuit breaker persist channel closed while flushing backlog"
                    );
                    break;
                }
            }
        }
    }

    fn enqueue_persist_backlog(&self, item: CircuitPersistedState) {
        let provider_id = item.provider_id;
        let mut backlog = self.persist_backlog.lock_or_recover();
        if backlog.get(&provider_id).is_some_and(|current| {
            (current.state_revision, current.updated_at) > (item.state_revision, item.updated_at)
        }) {
            return;
        }
        if backlog.len() >= MAX_PERSIST_BACKLOG && !backlog.contains_key(&provider_id) {
            if let Some(evicted_provider_id) = oldest_persist_backlog_provider_id(&backlog) {
                backlog.remove(&evicted_provider_id);
                tracing::warn!(
                    evicted_provider_id,
                    max_backlog = MAX_PERSIST_BACKLOG,
                    "circuit breaker persist backlog full; evicting oldest pending state"
                );
            }
        }
        backlog.insert(provider_id, item);
        tracing::debug!(
            provider_id,
            pending = backlog.len(),
            "circuit breaker persist queue full; queued latest state for retry"
        );
    }

    fn schedule_persist_backlog_flush(
        &self,
        tx: &tokio::sync::mpsc::Sender<CircuitPersistedState>,
    ) {
        if self
            .persist_backlog_flush_scheduled
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.persist_backlog_flush_scheduled
                .store(false, Ordering::Release);
            return;
        };

        handle.spawn(flush_persist_backlog_until_idle(
            tx.clone(),
            self.persist_backlog.clone(),
            self.persist_backlog_flush_scheduled.clone(),
        ));
    }
}

#[derive(Debug, Clone)]
pub struct ProbeLeaseGuard {
    inner: Arc<ProbeLeaseGuardInner>,
}

#[derive(Debug)]
struct ProbeLeaseGuardInner {
    circuit: Arc<CircuitBreaker>,
    token: ProbeLeaseToken,
    state: Mutex<ProbeLeaseGuardState>,
}

#[derive(Debug, Default)]
struct ProbeLeaseGuardState {
    dispatched: bool,
    finished: bool,
}

impl ProbeLeaseGuard {
    pub fn new(circuit: Arc<CircuitBreaker>, token: ProbeLeaseToken) -> Self {
        Self {
            inner: Arc::new(ProbeLeaseGuardInner {
                circuit,
                token,
                state: Mutex::new(ProbeLeaseGuardState::default()),
            }),
        }
    }

    pub fn token(&self) -> &ProbeLeaseToken {
        &self.inner.token
    }

    pub fn mark_dispatched(&self, now_unix: i64) -> ProbeCommitResult {
        let mut state = self.inner.state.lock_or_recover();
        if state.finished || state.dispatched {
            return ProbeCommitResult::Stale(
                self.inner
                    .circuit
                    .snapshot(self.inner.token.provider_id, now_unix),
            );
        }
        let result = self
            .inner
            .circuit
            .mark_probe_dispatched(&self.inner.token, now_unix);
        if matches!(result, ProbeCommitResult::Applied(_)) {
            state.dispatched = true;
        }
        result
    }

    pub fn persisted_dispatch_state(&self) -> Option<CircuitPersistedState> {
        self.inner
            .circuit
            .persisted_probe_dispatch_state(&self.inner.token)
    }

    pub fn persisted_state(&self) -> Option<CircuitPersistedState> {
        self.inner
            .circuit
            .persisted_state(self.inner.token.provider_id)
    }

    pub fn fail_closed_after_persist_error(
        &self,
        expected_revision: u64,
        now_unix: i64,
    ) -> CircuitSnapshot {
        self.inner.circuit.fail_closed_after_probe_persist_error(
            self.inner.token.provider_id,
            expected_revision,
            now_unix,
        )
    }

    pub fn rollback_dispatched(
        &self,
        before_dispatch: &CircuitSnapshot,
        now_unix: i64,
    ) -> ProbeCommitResult {
        let mut state = self.inner.state.lock_or_recover();
        if state.finished || !state.dispatched {
            return ProbeCommitResult::Stale(
                self.inner
                    .circuit
                    .snapshot(self.inner.token.provider_id, now_unix),
            );
        }
        let result = self.inner.circuit.rollback_probe_dispatch(
            &self.inner.token,
            before_dispatch,
            now_unix,
        );
        state.dispatched = false;
        state.finished = true;
        result
    }

    pub fn complete_success(&self, now_unix: i64) -> ProbeCommitResult {
        let mut state = self.inner.state.lock_or_recover();
        if state.finished {
            return ProbeCommitResult::Stale(
                self.inner
                    .circuit
                    .snapshot(self.inner.token.provider_id, now_unix),
            );
        }
        let result = self
            .inner
            .circuit
            .complete_probe_success(&self.inner.token, now_unix);
        state.finished = true;
        result
    }

    pub fn complete_failure(
        &self,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> ProbeCommitResult {
        let mut state = self.inner.state.lock_or_recover();
        if state.finished {
            return ProbeCommitResult::Stale(
                self.inner
                    .circuit
                    .snapshot(self.inner.token.provider_id, now_unix),
            );
        }
        let result = self.inner.circuit.complete_probe_failure(
            &self.inner.token,
            now_unix,
            counted_failure,
            trigger_error_code,
        );
        state.finished = true;
        result
    }

    pub fn record_attempt_failure(
        &self,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> ProbeCommitResult {
        self.inner.circuit.record_probe_attempt_failure(
            &self.inner.token,
            now_unix,
            counted_failure,
            trigger_error_code,
        )
    }

    pub fn abandon(&self, now_unix: i64) -> ProbeCommitResult {
        let mut state = self.inner.state.lock_or_recover();
        if state.finished {
            return ProbeCommitResult::Stale(
                self.inner
                    .circuit
                    .snapshot(self.inner.token.provider_id, now_unix),
            );
        }
        let result = self
            .inner
            .circuit
            .abandon_probe(&self.inner.token, now_unix);
        state.finished = true;
        result
    }
}

impl Drop for ProbeLeaseGuardInner {
    fn drop(&mut self) {
        let state = self.state.lock_or_recover();
        if state.finished {
            return;
        }
        let now_unix = crate::shared::time::now_unix_seconds();
        if state.dispatched {
            let _ = self
                .circuit
                .complete_probe_failure(&self.token, now_unix, false, None);
        } else {
            let _ = self.circuit.abandon_probe(&self.token, now_unix);
        }
    }
}

#[cfg(test)]
mod tests;
