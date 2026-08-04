use crate::circuit_breaker::{ProbeCommitResult, ProbeLeaseGuard, ProbeTrigger};
use crate::session_manager::SessionTriggerReservation;
use crate::shared::mutex_ext::MutexExt;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

type PersistCircuitState =
    dyn Fn(&crate::circuit_breaker::CircuitPersistedState) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
struct DurableCircuitPersistence {
    persist: Arc<PersistCircuitState>,
}

impl std::fmt::Debug for DurableCircuitPersistence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DurableCircuitPersistence")
    }
}

#[derive(Debug)]
pub(in crate::gateway) struct RequestDispatchIntent {
    targets: Vec<RequestDispatchTarget>,
    reservation: Arc<Mutex<RequestReservationState>>,
    claimed_provider_ids: Mutex<HashSet<i64>>,
    durable_persistence: Option<DurableCircuitPersistence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::gateway) struct RequestDispatchTarget {
    pub(in crate::gateway) provider_id: i64,
    pub(in crate::gateway) probe_trigger: Option<ProbeTrigger>,
}

impl RequestDispatchTarget {
    pub(in crate::gateway) const fn new(
        provider_id: i64,
        probe_trigger: Option<ProbeTrigger>,
    ) -> Self {
        Self {
            provider_id,
            probe_trigger,
        }
    }
}

impl RequestDispatchIntent {
    #[cfg(test)]
    pub(in crate::gateway) fn new(
        provider_id: i64,
        probe_trigger: Option<ProbeTrigger>,
        reservation: Option<SessionTriggerReservation>,
    ) -> Self {
        Self::new_targets(
            vec![RequestDispatchTarget::new(provider_id, probe_trigger)],
            reservation,
        )
    }

    pub(in crate::gateway) fn new_targets(
        mut targets: Vec<RequestDispatchTarget>,
        reservation: Option<SessionTriggerReservation>,
    ) -> Self {
        let mut seen_provider_ids = HashSet::new();
        targets.retain(|target| seen_provider_ids.insert(target.provider_id));

        Self {
            targets,
            reservation: Arc::new(Mutex::new(match reservation {
                Some(reservation) => RequestReservationState::Pending(reservation),
                None => RequestReservationState::NotRequired,
            })),
            claimed_provider_ids: Mutex::new(HashSet::new()),
            durable_persistence: None,
        }
    }

    #[cfg(test)]
    pub(in crate::gateway) fn new_all_open_recovery(
        provider_id: i64,
        additional_probe_provider_ids: Vec<i64>,
        probe_trigger: ProbeTrigger,
    ) -> Self {
        let targets = std::iter::once(provider_id)
            .chain(additional_probe_provider_ids)
            .map(|provider_id| RequestDispatchTarget::new(provider_id, Some(probe_trigger)))
            .collect();
        Self::new_targets(targets, None)
    }

    pub(in crate::gateway) fn with_durable_persistence(mut self, db: crate::db::Db) -> Self {
        self.durable_persistence = Some(DurableCircuitPersistence {
            persist: Arc::new(move |item| {
                crate::provider_circuit_breakers::upsert_durable(&db, item)
            }),
        });
        self
    }

    #[cfg(test)]
    fn with_test_durable_persistence(
        mut self,
        persist: impl Fn(&crate::circuit_breaker::CircuitPersistedState) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.durable_persistence = Some(DurableCircuitPersistence {
            persist: Arc::new(persist),
        });
        self
    }

    pub(in crate::gateway) fn targets_provider(&self, provider_id: i64) -> bool {
        self.targets
            .iter()
            .any(|target| target.provider_id == provider_id)
    }

    pub(in crate::gateway) fn probe_trigger_for(&self, provider_id: i64) -> Option<ProbeTrigger> {
        self.targets
            .iter()
            .find(|target| target.provider_id == provider_id)
            .and_then(|target| target.probe_trigger)
    }

    pub(in crate::gateway) fn claim_for_provider(
        &self,
        provider_id: i64,
        probe_guard: Option<ProbeLeaseGuard>,
    ) -> Option<Arc<ProviderDispatchOwnership>> {
        if !self.targets_provider(provider_id) {
            return None;
        }
        let mut claimed_provider_ids = self.claimed_provider_ids.lock_or_recover();
        if !claimed_provider_ids.insert(provider_id) {
            return None;
        }
        Some(Arc::new(ProviderDispatchOwnership {
            probe_guard,
            reservation: Arc::clone(&self.reservation),
            state: Mutex::new(DispatchOwnershipState::Pending),
            durable_persistence: self.durable_persistence.clone(),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchOwnershipState {
    Pending,
    Dispatched,
    StreamDeferred,
    Rejected,
    ProbeSucceeded,
    ProbeFailed,
    ProbeAbandoned,
}

#[derive(Debug)]
enum RequestReservationState {
    NotRequired,
    Pending(SessionTriggerReservation),
    Consumed,
    Rejected,
}

#[derive(Debug)]
pub(in crate::gateway) struct ProviderDispatchOwnership {
    probe_guard: Option<ProbeLeaseGuard>,
    reservation: Arc<Mutex<RequestReservationState>>,
    state: Mutex<DispatchOwnershipState>,
    durable_persistence: Option<DurableCircuitPersistence>,
}

impl ProviderDispatchOwnership {
    /// Called synchronously at the first poll of the transport send future.
    /// No await or fallible local preparation may precede this method.
    pub(in crate::gateway) fn commit_at_transport_boundary(&self, now_unix: i64) -> bool {
        let mut state = self.state.lock_or_recover();
        match *state {
            DispatchOwnershipState::Dispatched
            | DispatchOwnershipState::StreamDeferred
            | DispatchOwnershipState::ProbeSucceeded
            | DispatchOwnershipState::ProbeFailed => return true,
            DispatchOwnershipState::Rejected | DispatchOwnershipState::ProbeAbandoned => {
                return false;
            }
            DispatchOwnershipState::Pending => {}
        }

        // Keep the request fail-closed until both the session trigger and any
        // provider probe dispatch state have committed durably.
        let mut reservation_state = self.reservation.lock_or_recover();
        let (trigger_commit, reservation_after_success) =
            match std::mem::replace(&mut *reservation_state, RequestReservationState::Rejected) {
                RequestReservationState::Pending(reservation) => match reservation.commit(now_unix)
                {
                    Some(commit) => (Some(commit), RequestReservationState::Consumed),
                    None => {
                        *state = DispatchOwnershipState::Rejected;
                        return false;
                    }
                },
                allowed_state @ (RequestReservationState::NotRequired
                | RequestReservationState::Consumed) => (None, allowed_state),
                RequestReservationState::Rejected => {
                    *state = DispatchOwnershipState::Rejected;
                    return false;
                }
            };
        drop(reservation_state);
        if let Some(probe_guard) = &self.probe_guard {
            let before_dispatch = match probe_guard.mark_dispatched(now_unix) {
                ProbeCommitResult::Applied(change) => change.before,
                ProbeCommitResult::Stale(_) => {
                    if let Some(trigger_commit) = trigger_commit {
                        if !trigger_commit.rollback() {
                            tracing::warn!(
                                provider_id = probe_guard.token().provider_id,
                                probe_generation = probe_guard.token().generation,
                                "probe dispatch rejection could not roll back a superseded session trigger"
                            );
                        }
                    }
                    *state = DispatchOwnershipState::Rejected;
                    return false;
                }
            };

            let durable_result = match &self.durable_persistence {
                Some(persistence) => probe_guard
                    .persisted_dispatch_state()
                    .ok_or_else(|| {
                        "probe dispatch state became stale before persistence".to_string()
                    })
                    .and_then(|item| (persistence.persist)(&item)),
                None => {
                    #[cfg(test)]
                    {
                        Ok(())
                    }
                    #[cfg(not(test))]
                    {
                        Err("probe dispatch has no durable persistence owner".to_string())
                    }
                }
            };
            if let Err(error) = durable_result {
                let _ = probe_guard.rollback_dispatched(&before_dispatch, now_unix);
                if let Some(trigger_commit) = trigger_commit {
                    if !trigger_commit.rollback() {
                        tracing::warn!(
                            provider_id = probe_guard.token().provider_id,
                            probe_generation = probe_guard.token().generation,
                            "probe dispatch rejection could not roll back a superseded session trigger"
                        );
                    }
                }
                tracing::error!(
                    provider_id = probe_guard.token().provider_id,
                    probe_generation = probe_guard.token().generation,
                    error = %error,
                    "probe dispatch rejected because durable circuit persistence failed"
                );
                *state = DispatchOwnershipState::Rejected;
                return false;
            }
            if probe_guard.persisted_dispatch_state().is_none() {
                let _ = probe_guard.rollback_dispatched(&before_dispatch, now_unix);
                if let Some(trigger_commit) = trigger_commit {
                    if !trigger_commit.rollback() {
                        tracing::warn!(
                            provider_id = probe_guard.token().provider_id,
                            probe_generation = probe_guard.token().generation,
                            "stale probe dispatch could not roll back a superseded session trigger"
                        );
                    }
                }
                *state = DispatchOwnershipState::Rejected;
                return false;
            }
        }

        *self.reservation.lock_or_recover() = reservation_after_success;
        *state = DispatchOwnershipState::Dispatched;
        true
    }

    pub(in crate::gateway) fn is_probe(&self) -> bool {
        self.probe_guard.is_some()
    }

    pub(in crate::gateway) fn defer_probe_terminal_to_stream(&self) {
        if self.probe_guard.is_none() {
            return;
        }

        let mut state = self.state.lock_or_recover();
        if *state == DispatchOwnershipState::Dispatched {
            *state = DispatchOwnershipState::StreamDeferred;
        }
    }

    pub(in crate::gateway) fn is_probe_terminal_deferred(&self) -> bool {
        self.probe_guard.is_some()
            && *self.state.lock_or_recover() == DispatchOwnershipState::StreamDeferred
    }

    pub(in crate::gateway) fn complete_probe_success(
        &self,
        now_unix: i64,
    ) -> Option<ProbeCommitResult> {
        let probe_guard = self.probe_guard.as_ref()?;
        let mut state = self.state.lock_or_recover();
        let result = match *state {
            DispatchOwnershipState::Pending | DispatchOwnershipState::Rejected => {
                let result = probe_guard.abandon(now_unix);
                *state = DispatchOwnershipState::ProbeAbandoned;
                return Some(result);
            }
            DispatchOwnershipState::ProbeAbandoned => probe_guard.abandon(now_unix),
            DispatchOwnershipState::Dispatched
            | DispatchOwnershipState::StreamDeferred
            | DispatchOwnershipState::ProbeSucceeded
            | DispatchOwnershipState::ProbeFailed => probe_guard.complete_success(now_unix),
        };
        *state = DispatchOwnershipState::ProbeSucceeded;
        Some(result)
    }

    pub(in crate::gateway) fn complete_probe_failure(
        &self,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> Option<ProbeCommitResult> {
        let probe_guard = self.probe_guard.as_ref()?;
        let result = {
            let mut state = self.state.lock_or_recover();
            let result = match *state {
                DispatchOwnershipState::Pending | DispatchOwnershipState::Rejected => {
                    let result = probe_guard.abandon(now_unix);
                    *state = DispatchOwnershipState::ProbeAbandoned;
                    return Some(result);
                }
                DispatchOwnershipState::ProbeAbandoned => probe_guard.abandon(now_unix),
                DispatchOwnershipState::Dispatched
                | DispatchOwnershipState::StreamDeferred
                | DispatchOwnershipState::ProbeSucceeded
                | DispatchOwnershipState::ProbeFailed => {
                    probe_guard.complete_failure(now_unix, counted_failure, trigger_error_code)
                }
            };
            *state = DispatchOwnershipState::ProbeFailed;
            result
        };

        Some(match result {
            ProbeCommitResult::Applied(change) => ProbeCommitResult::Applied(
                self.persist_probe_failure_after_unlock(probe_guard, change, now_unix),
            ),
            stale => stale,
        })
    }

    fn persist_probe_failure_after_unlock(
        &self,
        probe_guard: &ProbeLeaseGuard,
        mut change: crate::circuit_breaker::CircuitChange,
        now_unix: i64,
    ) -> crate::circuit_breaker::CircuitChange {
        let persistence = match &self.durable_persistence {
            Some(persistence) => persistence,
            None => {
                #[cfg(test)]
                return change;
                #[cfg(not(test))]
                {
                    change.after = probe_guard
                        .fail_closed_after_persist_error(change.after.state_revision, now_unix);
                    tracing::error!(
                        provider_id = probe_guard.token().provider_id,
                        probe_generation = probe_guard.token().generation,
                        "probe failure has no durable persistence owner; future probes are blocked"
                    );
                    return change;
                }
            }
        };
        let persist_result = probe_guard
            .persisted_state()
            .ok_or_else(|| "probe failure state disappeared before persistence".to_string())
            .and_then(|item| (persistence.persist)(&item));
        let Err(error) = persist_result else {
            return change;
        };

        change.after =
            probe_guard.fail_closed_after_persist_error(change.after.state_revision, now_unix);
        tracing::error!(
            provider_id = probe_guard.token().provider_id,
            probe_generation = probe_guard.token().generation,
            error = %error,
            "probe failure durable persistence failed; future probes are blocked"
        );

        let retry_result = probe_guard
            .persisted_state()
            .ok_or_else(|| "fail-closed probe state disappeared before persistence".to_string())
            .and_then(|item| (persistence.persist)(&item));
        if let Err(retry_error) = retry_result {
            tracing::error!(
                provider_id = probe_guard.token().provider_id,
                probe_generation = probe_guard.token().generation,
                error = %retry_error,
                "fail-closed probe state could not be persisted"
            );
        }
        change
    }

    pub(in crate::gateway) fn record_probe_attempt_failure(
        &self,
        now_unix: i64,
        counted_failure: bool,
        trigger_error_code: Option<&'static str>,
    ) -> Option<ProbeCommitResult> {
        self.probe_guard.as_ref().map(|guard| {
            guard.record_attempt_failure(now_unix, counted_failure, trigger_error_code)
        })
    }

    pub(in crate::gateway) fn probe_trigger(&self) -> Option<ProbeTrigger> {
        self.probe_guard.as_ref().map(|guard| guard.token().trigger)
    }

    pub(in crate::gateway) fn probe_generation(&self) -> Option<u64> {
        self.probe_guard
            .as_ref()
            .map(|guard| guard.token().generation)
    }
}

impl Drop for ProviderDispatchOwnership {
    fn drop(&mut self) {
        let _ = self.complete_probe_failure(
            crate::gateway::util::now_unix_seconds() as i64,
            false,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, ProbeAcquireResult};
    use crate::session_manager::{SessionManager, SessionProbeTrigger};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn open_circuit(now_unix: i64) -> Arc<CircuitBreaker> {
        let circuit = Arc::new(CircuitBreaker::new(
            CircuitBreakerConfig {
                failure_threshold: 1,
                provider_cooldown_secs: 0,
                ..CircuitBreakerConfig::default()
            },
            HashMap::new(),
            None,
        ));
        circuit.record_failure(1, now_unix, None);
        circuit
    }

    fn insert_test_provider(db: &crate::db::Db, provider_id: i64) {
        let conn = db.open_connection().expect("open db");
        conn.execute(
            r#"
            INSERT INTO providers(
              id, provider_uuid, cli_key, name, base_url, api_key_plaintext,
              created_at, updated_at
            ) VALUES (?1, ?2, 'claude', 'Dispatch Test',
                      'https://provider.example', '', 1, 1)
            "#,
            rusqlite::params![provider_id, crate::shared::uuid::new_uuid_v4()],
        )
        .expect("insert provider");
    }

    fn compaction_reservation(
        manager: &Arc<SessionManager>,
        now_unix: i64,
    ) -> SessionTriggerReservation {
        manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now_unix);
        manager.bind_success("claude", "s1", 2, None, now_unix);
        let generation = manager
            .mark_compaction_completed("claude", "s1", now_unix)
            .expect("generation");
        manager
            .try_reserve_probe_trigger(
                "claude",
                "s1",
                SessionProbeTrigger::CompactionGeneration(generation),
                now_unix,
            )
            .expect("reservation")
    }

    #[test]
    fn all_open_recovery_allows_each_planned_provider_to_claim_once() {
        let intent = RequestDispatchIntent::new_all_open_recovery(
            1,
            vec![2, 3],
            ProbeTrigger::NewUnboundSession,
        );

        assert!(intent.targets_provider(1));
        assert!(intent.targets_provider(2));
        assert!(intent.targets_provider(3));
        assert!(!intent.targets_provider(4));
        assert!(intent.claim_for_provider(1, None).is_some());
        assert!(intent.claim_for_provider(1, None).is_none());
        assert!(intent.claim_for_provider(2, None).is_some());
        assert!(intent.claim_for_provider(2, None).is_none());
        assert!(intent.claim_for_provider(3, None).is_some());
        assert!(intent.claim_for_provider(4, None).is_none());
    }

    #[test]
    fn ordered_targets_keep_provider_specific_triggers_and_deduplicate_claims() {
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, None),
                RequestDispatchTarget::new(2, Some(ProbeTrigger::RouteChanged)),
                RequestDispatchTarget::new(3, Some(ProbeTrigger::NaturalMaxWait)),
                RequestDispatchTarget::new(4, None),
                RequestDispatchTarget::new(5, Some(ProbeTrigger::AggressiveTurn)),
                RequestDispatchTarget::new(2, Some(ProbeTrigger::MaxOpenWait)),
            ],
            None,
        );

        assert!(intent.targets_provider(1));
        assert_eq!(intent.probe_trigger_for(1), None);
        assert_eq!(
            intent.probe_trigger_for(2),
            Some(ProbeTrigger::RouteChanged)
        );
        assert_eq!(
            intent.probe_trigger_for(3),
            Some(ProbeTrigger::NaturalMaxWait)
        );
        assert_eq!(intent.probe_trigger_for(4), None);
        assert_eq!(
            intent.probe_trigger_for(5),
            Some(ProbeTrigger::AggressiveTurn)
        );
        assert!(!intent.targets_provider(6));

        assert!(intent.claim_for_provider(2, None).is_some());
        assert!(intent.claim_for_provider(2, None).is_none());
    }

    #[test]
    fn pre_send_skip_keeps_reservation_for_the_next_target() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, None),
                RequestDispatchTarget::new(2, None),
            ],
            Some(reservation),
        );

        let skipped = intent
            .claim_for_provider(1, None)
            .expect("first target ownership");
        drop(skipped);

        let later = intent
            .claim_for_provider(2, None)
            .expect("later target ownership");
        assert!(later.commit_at_transport_boundary(now + 1));
        assert_eq!(
            manager
                .routing_snapshot("claude", "s1", now + 1)
                .expect("session snapshot")
                .consumed_compaction_generation,
            1
        );
    }

    #[test]
    fn multiple_ownerships_consume_request_reservation_only_on_first_send() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, None),
                RequestDispatchTarget::new(2, None),
            ],
            Some(reservation),
        );
        let first = intent
            .claim_for_provider(1, None)
            .expect("first target ownership");
        let second = intent
            .claim_for_provider(2, None)
            .expect("second target ownership");

        assert!(first.commit_at_transport_boundary(now + 1));
        assert!(second.commit_at_transport_boundary(now + 2));
        assert_eq!(
            manager
                .routing_snapshot("claude", "s1", now + 2)
                .expect("session snapshot")
                .consumed_compaction_generation,
            1
        );
    }

    #[test]
    fn dropping_zero_send_intent_releases_reservation_for_a_later_request() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        {
            let _intent = RequestDispatchIntent::new_targets(
                vec![RequestDispatchTarget::new(1, None)],
                Some(reservation),
            );
        }

        assert!(manager
            .try_reserve_probe_trigger(
                "claude",
                "s1",
                SessionProbeTrigger::CompactionGeneration(1),
                now + 1,
            )
            .is_some());
    }

    #[test]
    fn stale_probe_commit_rolls_back_trigger_and_rejects_later_targets() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let circuit = open_circuit(now);
        let token =
            match circuit.try_acquire_probe(1, "trace-1", ProbeTrigger::NaturalCompaction, now) {
                ProbeAcquireResult::Acquired { token, .. } => token,
                other => panic!("expected probe lease, got {other:?}"),
            };
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, Some(ProbeTrigger::NaturalCompaction)),
                RequestDispatchTarget::new(2, None),
            ],
            Some(reservation),
        );
        let ownership = intent
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        let later_ownership = intent.claim_for_provider(2, None).expect("later ownership");

        circuit.reset(1, now + 1);
        assert!(!ownership.commit_at_transport_boundary(now + 2));
        assert!(!later_ownership.commit_at_transport_boundary(now + 2));
        let snapshot = manager
            .routing_snapshot("claude", "s1", now + 2)
            .expect("session snapshot");
        assert_eq!(snapshot.consumed_compaction_generation, 0);
    }

    #[test]
    fn stale_probe_without_reservation_rejects_every_later_target() {
        let now = 1_000;
        let circuit = open_circuit(now);
        let token =
            match circuit.try_acquire_probe(1, "trace-stale", ProbeTrigger::AggressiveTurn, now) {
                ProbeAcquireResult::Acquired { token, .. } => token,
                other => panic!("expected probe lease, got {other:?}"),
            };
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, Some(ProbeTrigger::AggressiveTurn)),
                RequestDispatchTarget::new(2, None),
            ],
            None,
        );
        let ownership = intent
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        let later_ownership = intent.claim_for_provider(2, None).expect("later ownership");

        circuit.reset(1, now + 1);
        let network_count = AtomicUsize::new(0);
        for ownership in [&ownership, &later_ownership] {
            if ownership.commit_at_transport_boundary(now + 2) {
                network_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        assert_eq!(network_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn successful_dispatch_commit_consumes_trigger_once_and_keeps_probe_generation() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let circuit = open_circuit(now);
        let token =
            match circuit.try_acquire_probe(1, "trace-1", ProbeTrigger::NaturalCompaction, now) {
                ProbeAcquireResult::Acquired { token, .. } => token,
                other => panic!("expected probe lease, got {other:?}"),
            };
        let generation = token.generation;
        let intent =
            RequestDispatchIntent::new(1, Some(ProbeTrigger::NaturalCompaction), Some(reservation));
        let ownership = intent
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");

        assert!(ownership.commit_at_transport_boundary(now + 1));
        assert!(ownership.commit_at_transport_boundary(now + 2));
        assert_eq!(ownership.probe_generation(), Some(generation));
        let snapshot = manager
            .routing_snapshot("claude", "s1", now + 2)
            .expect("session snapshot");
        assert_eq!(snapshot.consumed_compaction_generation, 1);
    }

    #[test]
    fn expired_reservation_rejection_blocks_later_targets_and_abandons_probe() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let circuit = open_circuit(now);
        let before = circuit.snapshot(1, now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-expired-reservation",
            ProbeTrigger::NaturalCompaction,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, Some(ProbeTrigger::NaturalCompaction)),
                RequestDispatchTarget::new(2, None),
            ],
            Some(reservation),
        );
        let ownership = intent
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        let later_ownership = intent.claim_for_provider(2, None).expect("later ownership");

        let network_count = AtomicUsize::new(0);
        for ownership in [&ownership, &later_ownership] {
            if ownership.commit_at_transport_boundary(now + 61) {
                network_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        assert_eq!(network_count.load(Ordering::Relaxed), 0);
        let commit = ownership
            .complete_probe_failure(now + 62, true, Some("GW_UPSTREAM_5XX"))
            .expect("probe abandon result");
        assert!(matches!(commit, ProbeCommitResult::Applied(_)));

        let after = circuit.snapshot(1, now + 62);
        assert_eq!(after.state, before.state);
        assert_eq!(after.failure_count, before.failure_count);
        assert_eq!(after.open_until, before.open_until);
        assert_eq!(after.next_probe_at, before.next_probe_at);
        assert_eq!(after.natural_probe_due_at, before.natural_probe_due_at);
        assert!(!after.probe_in_flight);
    }

    #[test]
    fn dispatched_probe_failure_returns_transition_for_terminal_event() {
        let now = 1_000;
        let circuit = open_circuit(now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-dispatched-failure",
            ProbeTrigger::AggressiveTurn,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        assert!(ownership.commit_at_transport_boundary(now + 1));

        let result = ownership
            .complete_probe_failure(now + 2, false, Some("GW_UPSTREAM_5XX"))
            .expect("probe failure result");
        let ProbeCommitResult::Applied(change) = result else {
            panic!("dispatched probe failure should apply");
        };
        assert_eq!(
            change.transition.expect("probe failure transition").reason,
            "PROBE_FAILURE"
        );
    }

    #[test]
    fn stream_handoff_keeps_probe_open_until_terminal_completion() {
        let now = 1_000;
        let circuit = open_circuit(now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-stream-handoff",
            ProbeTrigger::AggressiveTurn,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");

        assert!(ownership.commit_at_transport_boundary(now + 1));
        ownership.defer_probe_terminal_to_stream();
        assert!(ownership.is_probe_terminal_deferred());
        let deferred = circuit.snapshot(1, now + 2);
        assert_eq!(
            deferred.state,
            crate::circuit_breaker::CircuitState::HalfOpen
        );
        assert!(deferred.probe_in_flight);

        let result = ownership
            .complete_probe_success(now + 3)
            .expect("stream terminal success");
        assert!(matches!(result, ProbeCommitResult::Applied(_)));
        let completed = circuit.snapshot(1, now + 3);
        assert_eq!(
            completed.state,
            crate::circuit_breaker::CircuitState::Closed
        );
        assert!(!completed.probe_in_flight);
    }

    #[test]
    fn durable_failure_rolls_back_trigger_lease_and_deadlines_before_zero_network_return() {
        let now = 1_000;
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let circuit = open_circuit(now);
        let before = circuit.snapshot(1, now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-durable-error",
            ProbeTrigger::NaturalCompaction,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership =
            RequestDispatchIntent::new(1, Some(ProbeTrigger::NaturalCompaction), Some(reservation))
                .with_test_durable_persistence(|_| Err("injected durable failure".to_string()))
                .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
                .expect("ownership");

        let network_count = AtomicUsize::new(0);
        if ownership.commit_at_transport_boundary(now + 1) {
            network_count.fetch_add(1, Ordering::Relaxed);
        }
        assert_eq!(network_count.load(Ordering::Relaxed), 0);

        let after = circuit.snapshot(1, now + 1);
        assert!(!after.probe_in_flight);
        assert_eq!(after.open_until, before.open_until);
        assert_eq!(after.probe_reference_at, before.probe_reference_at);
        assert_eq!(after.next_probe_at, before.next_probe_at);
        assert_eq!(after.natural_probe_due_at, before.natural_probe_due_at);
        let session = manager
            .routing_snapshot("claude", "s1", now + 2)
            .expect("session snapshot");
        assert_eq!(session.consumed_compaction_generation, 0);
        assert!(matches!(
            circuit.try_acquire_probe(
                1,
                "trace-after-durable-error",
                ProbeTrigger::NaturalCompaction,
                now + 1,
            ),
            ProbeAcquireResult::Acquired { .. }
        ));
    }

    #[test]
    fn durable_failure_without_reservation_rejects_every_later_target() {
        let now = 1_000;
        let circuit = open_circuit(now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-durable-error-no-reservation",
            ProbeTrigger::AggressiveTurn,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let intent = RequestDispatchIntent::new_targets(
            vec![
                RequestDispatchTarget::new(1, Some(ProbeTrigger::AggressiveTurn)),
                RequestDispatchTarget::new(2, None),
            ],
            None,
        )
        .with_test_durable_persistence(|_| Err("injected durable failure".to_string()));
        let ownership = intent
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        let later_ownership = intent.claim_for_provider(2, None).expect("later ownership");

        let network_count = AtomicUsize::new(0);
        for ownership in [&ownership, &later_ownership] {
            if ownership.commit_at_transport_boundary(now + 1) {
                network_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        assert_eq!(network_count.load(Ordering::Relaxed), 0);
        assert!(!circuit.snapshot(1, now + 1).probe_in_flight);
    }

    #[test]
    fn durable_dispatch_persists_open_row_and_new_deadline_before_network() {
        let now = 1_000;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db =
            crate::db::init_for_tests(&db_dir.path().join("durable-dispatch.db")).expect("init db");
        insert_test_provider(&db, 1);
        assert!(crate::provider_circuit_breakers::load_all(&db)
            .expect("initial load")
            .is_empty());
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            provider_cooldown_secs: 10,
            ..CircuitBreakerConfig::default()
        };
        let circuit = Arc::new(CircuitBreaker::new(config.clone(), HashMap::new(), None));
        circuit.record_failure(1, now, None);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-durable-success",
            ProbeTrigger::AggressiveTurn,
            now + 10,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .with_durable_persistence(db.clone())
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");

        assert!(ownership.commit_at_transport_boundary(now + 10));
        let persisted = crate::provider_circuit_breakers::load_all(&db).expect("durable load");
        let row = persisted.get(&1).expect("durable OPEN row");
        assert_eq!(row.state, crate::circuit_breaker::CircuitState::Open);
        assert_eq!(row.next_probe_at, Some(now + 20));

        let reloaded = CircuitBreaker::new(config, persisted, None);
        assert!(matches!(
            reloaded.try_acquire_probe(
                1,
                "trace-immediate-reload",
                ProbeTrigger::AggressiveTurn,
                now + 10,
            ),
            ProbeAcquireResult::Cooldown(_)
        ));
    }

    #[test]
    fn durable_success_followed_by_admin_reset_rejects_stale_mark_without_resurrection() {
        let now = 1_000;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = crate::db::init_for_tests(&db_dir.path().join("durable-reset-race.db"))
            .expect("init db");
        insert_test_provider(&db, 1);
        let manager = Arc::new(SessionManager::new());
        let reservation = compaction_reservation(&manager, now);
        let circuit = open_circuit(now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-reset-race",
            ProbeTrigger::NaturalCompaction,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let persist_db = db.clone();
        let reset_circuit = Arc::clone(&circuit);
        let ownership =
            RequestDispatchIntent::new(1, Some(ProbeTrigger::NaturalCompaction), Some(reservation))
                .with_test_durable_persistence(move |dispatched| {
                    crate::provider_circuit_breakers::upsert_durable(&persist_db, dispatched)?;
                    reset_circuit.reset(1, now + 1);
                    let tombstone = reset_circuit.persisted_state(1).expect("reset tombstone");
                    crate::provider_circuit_breakers::upsert_durable(&persist_db, &tombstone)
                })
                .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
                .expect("ownership");

        let network_count = AtomicUsize::new(0);
        if ownership.commit_at_transport_boundary(now) {
            network_count.fetch_add(1, Ordering::Relaxed);
        }
        assert_eq!(network_count.load(Ordering::Relaxed), 0);
        assert_eq!(
            circuit.snapshot(1, now + 1).state,
            crate::circuit_breaker::CircuitState::Closed
        );
        let persisted = crate::provider_circuit_breakers::load_all(&db).expect("reload");
        assert_eq!(
            persisted.get(&1).expect("tombstone").state,
            crate::circuit_breaker::CircuitState::Closed
        );
        assert_eq!(
            manager
                .routing_snapshot("claude", "s1", now + 2)
                .expect("session snapshot")
                .consumed_compaction_generation,
            0
        );
    }

    #[test]
    fn terminal_probe_failure_persists_final_deadline_before_reload() {
        let opened_at = 1_000;
        let dispatch_at = 1_030;
        let failed_at = 1_300;
        let db_dir = tempfile::tempdir().expect("db dir");
        let db = crate::db::init_for_tests(&db_dir.path().join("durable-final-failure.db"))
            .expect("init db");
        insert_test_provider(&db, 1);
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            provider_cooldown_secs: 30,
            ..CircuitBreakerConfig::default()
        };
        let circuit = Arc::new(CircuitBreaker::new(config.clone(), HashMap::new(), None));
        circuit.record_failure(1, opened_at, None);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-long-probe",
            ProbeTrigger::AggressiveTurn,
            dispatch_at,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .with_durable_persistence(db.clone())
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        assert!(ownership.commit_at_transport_boundary(dispatch_at));

        let result = ownership
            .complete_probe_failure(failed_at, false, Some("GW_UPSTREAM_5XX"))
            .expect("terminal probe failure");
        let ProbeCommitResult::Applied(change) = result else {
            panic!("terminal failure should apply");
        };
        assert_eq!(change.after.next_probe_at, Some(failed_at + 30));

        let persisted = crate::provider_circuit_breakers::load_all(&db).expect("reload state");
        assert_eq!(
            persisted.get(&1).and_then(|item| item.next_probe_at),
            Some(failed_at + 30)
        );
        let reloaded = CircuitBreaker::new(config, persisted, None);
        assert!(matches!(
            reloaded.try_acquire_probe(
                1,
                "trace-after-crash-window",
                ProbeTrigger::AggressiveTurn,
                failed_at,
            ),
            ProbeAcquireResult::Cooldown(_)
        ));
    }

    #[test]
    fn terminal_probe_failure_persistence_error_blocks_future_probes() {
        let now = 1_000;
        let circuit = open_circuit(now);
        let token = match circuit.try_acquire_probe(
            1,
            "trace-final-persist-error",
            ProbeTrigger::AggressiveTurn,
            now,
        ) {
            ProbeAcquireResult::Acquired { token, .. } => token,
            other => panic!("expected probe lease, got {other:?}"),
        };
        let persist_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&persist_calls);
        let ownership = RequestDispatchIntent::new(1, Some(ProbeTrigger::AggressiveTurn), None)
            .with_test_durable_persistence(move |_| {
                if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    Ok(())
                } else {
                    Err("injected terminal persistence failure".to_string())
                }
            })
            .claim_for_provider(1, Some(ProbeLeaseGuard::new(Arc::clone(&circuit), token)))
            .expect("ownership");
        assert!(ownership.commit_at_transport_boundary(now));

        let result = ownership
            .complete_probe_failure(now + 300, false, Some("GW_UPSTREAM_5XX"))
            .expect("terminal probe failure");
        let ProbeCommitResult::Applied(change) = result else {
            panic!("terminal failure should apply");
        };
        assert_eq!(persist_calls.load(Ordering::Relaxed), 3);
        assert_eq!(change.after.next_probe_at, Some(i64::MAX));
        assert!(matches!(
            circuit.try_acquire_probe(
                1,
                "trace-after-final-persist-error",
                ProbeTrigger::AggressiveTurn,
                now + 10_000,
            ),
            ProbeAcquireResult::Cooldown(_)
        ));
    }
}
