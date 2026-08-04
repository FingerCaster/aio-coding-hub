use super::types::*;
use super::*;
use types::MAX_FAILURE_TIMESTAMPS;

fn breaker() -> CircuitBreaker {
    CircuitBreaker::new(CircuitBreakerConfig::default(), HashMap::new(), None)
}

fn open_provider(cb: &CircuitBreaker, provider_id: i64, now_unix: i64) -> i64 {
    for offset in 0..DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(provider_id, now_unix + i64::from(offset), None);
    }
    cb.snapshot(provider_id, now_unix + i64::from(DEFAULT_FAILURE_THRESHOLD))
        .next_probe_at
        .expect("next probe deadline")
}

fn acquire_probe(
    cb: &CircuitBreaker,
    provider_id: i64,
    owner: &str,
    now_unix: i64,
) -> ProbeLeaseToken {
    match cb.try_acquire_probe(provider_id, owner, ProbeTrigger::NaturalMaxWait, now_unix) {
        ProbeAcquireResult::Acquired { token, .. } => token,
        other => panic!("expected acquired probe, got {other:?}"),
    }
}

#[test]
fn closed_to_open_after_threshold() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        let change = cb.record_failure(pid, now + i as i64, None);
        if i < DEFAULT_FAILURE_THRESHOLD {
            assert_eq!(change.after.state, CircuitState::Closed);
        }
    }

    let snap = cb.snapshot(pid, now + 100);
    assert_eq!(snap.state, CircuitState::Open);
    assert!(snap.open_until.is_some());
}

#[test]
fn open_expiry_remains_protected_without_an_explicit_probe_lease() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    open_provider(&cb, pid, now);
    let open_until = cb.snapshot(pid, now + 10).open_until.expect("open_until");

    let check = cb.should_allow(pid, open_until);
    assert!(!check.allow);
    assert_eq!(check.after.state, CircuitState::Open);
    assert!(check.transition.is_none());
}

#[test]
fn probe_lease_is_provider_scoped_single_flight() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let first = acquire_probe(&cb, pid, "trace-a", due);

    assert!(matches!(
        cb.try_acquire_probe(pid, "trace-b", ProbeTrigger::AggressiveTurn, due),
        ProbeAcquireResult::InFlight(_)
    ));
    assert_eq!(first.provider_id, pid);
    assert_eq!(cb.snapshot(pid, due).state, CircuitState::HalfOpen);
}

#[test]
fn expired_open_deadline_remains_immediately_probe_eligible_after_normal_reload() {
    let config = CircuitBreakerConfig {
        failure_threshold: 1,
        provider_cooldown_secs: 10,
        ..CircuitBreakerConfig::default()
    };
    let provider_id = 1;
    let opened_at = 1_000;
    let original = CircuitBreaker::new(config.clone(), HashMap::new(), None);
    original.record_failure(provider_id, opened_at, None);
    let persisted = original.persisted_state(provider_id).expect("OPEN row");
    let due_at = persisted.next_probe_at.expect("probe deadline");
    let reloaded = CircuitBreaker::new(config, HashMap::from([(provider_id, persisted)]), None);

    assert!(matches!(
        reloaded.try_acquire_probe(
            provider_id,
            "normal-reload",
            ProbeTrigger::NaturalMaxWait,
            due_at,
        ),
        ProbeAcquireResult::Acquired { .. }
    ));
}

#[test]
fn one_complete_probe_success_closes_and_starts_recovery_guard() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let token = acquire_probe(&cb, pid, "trace-a", due);
    assert!(matches!(
        cb.mark_probe_dispatched(&token, due),
        ProbeCommitResult::Applied(_)
    ));

    let result = cb.complete_probe_success(&token, due + 1);
    let ProbeCommitResult::Applied(change) = result else {
        panic!("probe success should apply");
    };
    assert_eq!(change.after.state, CircuitState::Closed);
    assert_eq!(change.after.failure_count, 0);
    assert_eq!(change.after.recovery_guard_until, Some(due + 301));
    assert_eq!(
        change.transition.expect("transition").reason,
        "PROBE_SUCCESS"
    );
}

#[test]
fn ordinary_success_cannot_close_an_open_probe_or_clear_recovery_guard() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let token = acquire_probe(&cb, pid, "trace-a", due);
    let _ = cb.mark_probe_dispatched(&token, due);

    let late = cb.record_success(pid, due + 1);
    assert_eq!(late.after.state, CircuitState::HalfOpen);
    let ProbeCommitResult::Applied(recovered) = cb.complete_probe_success(&token, due + 2) else {
        panic!("probe success should apply");
    };
    let guard_until = recovered.after.recovery_guard_until;
    let ordinary = cb.record_success(pid, due + 3);
    assert_eq!(ordinary.after.recovery_guard_until, guard_until);
}

#[test]
fn probe_failure_resets_deadlines_and_late_generation_is_stale() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let old_token = acquire_probe(&cb, pid, "trace-a", due);
    let _ = cb.mark_probe_dispatched(&old_token, due);
    let ProbeCommitResult::Applied(failed) =
        cb.complete_probe_failure(&old_token, due + 2, true, Some("GW_UPSTREAM_5XX"))
    else {
        panic!("probe failure should apply");
    };
    assert_eq!(failed.after.state, CircuitState::Open);
    assert_eq!(
        failed.after.next_probe_at,
        Some(due + 2 + DEFAULT_PROVIDER_COOLDOWN_SECS)
    );

    let next_due = failed.after.next_probe_at.expect("next due");
    let new_token = acquire_probe(&cb, pid, "trace-b", next_due);
    let _ = cb.mark_probe_dispatched(&new_token, next_due);
    assert!(matches!(
        cb.complete_probe_success(&old_token, next_due + 1),
        ProbeCommitResult::Stale(_)
    ));
    assert_eq!(cb.snapshot(pid, next_due + 1).state, CircuitState::HalfOpen);
}

#[test]
fn undispatched_probe_completion_abandons_without_resetting_deadlines() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let before = cb.snapshot(pid, due);
    let token = acquire_probe(&cb, pid, "trace-undispatched", due);

    let ProbeCommitResult::Applied(abandoned) =
        cb.complete_probe_failure(&token, due + 1, true, Some("GW_UPSTREAM_5XX"))
    else {
        panic!("undispatched probe abandon should apply");
    };

    assert_eq!(abandoned.after.state, CircuitState::Open);
    assert_eq!(abandoned.after.failure_count, before.failure_count);
    assert_eq!(abandoned.after.open_until, before.open_until);
    assert_eq!(abandoned.after.next_probe_at, before.next_probe_at);
    assert_eq!(
        abandoned.after.natural_probe_due_at,
        before.natural_probe_due_at
    );
    assert!(!abandoned.after.probe_in_flight);
    assert!(abandoned.transition.is_none());
}

#[test]
fn dispatched_probe_is_not_reclaimed_by_wall_clock_but_undispatched_lease_is() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let token = acquire_probe(&cb, pid, "trace-a", due);
    let _ = cb.mark_probe_dispatched(&token, due);
    let much_later = due + PROBE_LEASE_TTL_SECS + 1;
    assert!(cb.snapshot(pid, much_later).probe_in_flight);
    assert!(matches!(
        cb.try_acquire_probe(pid, "trace-b", ProbeTrigger::NaturalMaxWait, much_later),
        ProbeAcquireResult::InFlight(_)
    ));

    let cb = breaker();
    let due = open_provider(&cb, pid, now);
    let _undispatched = acquire_probe(&cb, pid, "trace-a", due);
    let much_later = due + PROBE_LEASE_TTL_SECS + 1;
    assert!(!cb.snapshot(pid, much_later).probe_in_flight);
    assert!(matches!(
        cb.try_acquire_probe(pid, "trace-b", ProbeTrigger::NaturalMaxWait, much_later),
        ProbeAcquireResult::Acquired { .. }
    ));
}

#[test]
fn recovery_guard_survives_ordinary_success_and_reopens_on_first_failure() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let token = acquire_probe(&cb, pid, "trace-a", due);
    let _ = cb.mark_probe_dispatched(&token, due);
    let ProbeCommitResult::Applied(recovered) = cb.complete_probe_success(&token, due + 1) else {
        panic!("probe success should apply");
    };
    let guard_until = recovered.after.recovery_guard_until.expect("guard");

    let ordinary = cb.record_success(pid, due + 2);
    assert_eq!(ordinary.after.recovery_guard_until, Some(guard_until));
    let failed = cb.record_failure(pid, due + 3, Some("GW_UPSTREAM_5XX"));
    assert_eq!(failed.after.state, CircuitState::Open);
    assert_eq!(
        failed.transition.expect("transition").reason,
        "RECOVERY_GUARD_FAILURE"
    );
}

#[test]
fn expired_recovery_guard_returns_to_normal_failure_threshold() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            provider_cooldown_secs: 0,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let token = acquire_probe(&cb, pid, "trace-a", due);
    let _ = cb.mark_probe_dispatched(&token, due);
    let ProbeCommitResult::Applied(recovered) = cb.complete_probe_success(&token, due + 1) else {
        panic!("probe success should apply");
    };
    let guard_until = recovered.after.recovery_guard_until.expect("guard");

    assert!(cb.snapshot(pid, guard_until).recovery_guard_until.is_none());
    let first = cb.record_failure(pid, guard_until, None);
    assert_eq!(first.after.state, CircuitState::Closed);
    assert_eq!(first.after.failure_count, 1);
}

#[test]
fn success_clears_failure_timestamps() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    cb.record_failure(pid, now, None);
    let before = cb.snapshot(pid, now + 1);
    assert_eq!(before.failure_count, 1);
    assert_eq!(before.probe_reference_at, Some(now));
    assert_eq!(
        before.natural_probe_due_at,
        Some(now + DEFAULT_NATURAL_PROBE_MAX_WAIT_SECS)
    );

    cb.record_success(pid, now + 2);
    let after = cb.snapshot(pid, now + 3);
    assert_eq!(after.failure_count, 0);
    assert_eq!(after.state, CircuitState::Closed);
    assert!(after.probe_reference_at.is_none());
    assert!(after.natural_probe_due_at.is_none());
}

#[test]
fn closed_failure_rearms_natural_deadline_from_latest_failure() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 5,
            natural_probe_max_wait_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;

    let first = cb.record_failure(pid, now, None);
    assert_eq!(first.after.state, CircuitState::Closed);
    assert_eq!(first.after.probe_reference_at, Some(now));
    assert_eq!(first.after.natural_probe_due_at, Some(now + 60));

    let second = cb.record_failure(pid, now + 20, None);
    assert_eq!(second.after.state, CircuitState::Closed);
    assert_eq!(second.after.probe_reference_at, Some(now + 20));
    assert_eq!(second.after.natural_probe_due_at, Some(now + 80));
}

#[test]
fn closed_pending_natural_deadline_survives_reload() {
    let config = CircuitBreakerConfig {
        failure_threshold: 5,
        natural_probe_max_wait_secs: 60,
        ..CircuitBreakerConfig::default()
    };
    let pid = 1;
    let now = 1_000;
    let original = CircuitBreaker::new(config.clone(), HashMap::new(), None);
    original.record_failure(pid, now, None);
    let persisted = original.persisted_state(pid).expect("CLOSED pending row");

    let reloaded = CircuitBreaker::new(config, HashMap::from([(pid, persisted)]), None);
    let snapshot = reloaded.snapshot(pid, now + 1);
    assert_eq!(snapshot.state, CircuitState::Closed);
    assert_eq!(snapshot.probe_reference_at, Some(now));
    assert_eq!(snapshot.natural_probe_due_at, Some(now + 60));
}

#[test]
fn legacy_closed_failure_reload_arms_deadline_from_latest_failure() {
    let config = CircuitBreakerConfig {
        failure_threshold: 5,
        natural_probe_max_wait_secs: 60,
        ..CircuitBreakerConfig::default()
    };
    let pid = 1;
    let now = 1_000;
    let mut persisted = persisted_state(pid, now + 30);
    persisted.failure_timestamps = vec![now as u64, (now + 20) as u64];

    let reloaded = CircuitBreaker::new(config, HashMap::from([(pid, persisted)]), None);
    let snapshot = reloaded.snapshot(pid, now + 31);
    assert_eq!(snapshot.state, CircuitState::Closed);
    assert_eq!(snapshot.probe_reference_at, Some(now + 20));
    assert_eq!(snapshot.natural_probe_due_at, Some(now + 80));
}

#[test]
fn failures_within_window_counted_correctly() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;

    // Record 2 failures within the window
    cb.record_failure(pid, now, None);
    cb.record_failure(pid, now + 10, None);

    let snap = cb.snapshot(pid, now + 20);
    assert_eq!(snap.state, CircuitState::Closed);
    assert_eq!(snap.failure_count, 2);

    // Third failure within window trips the breaker
    let change = cb.record_failure(pid, now + 20, None);
    assert_eq!(change.after.state, CircuitState::Open);
}

#[test]
fn failures_older_than_window_not_counted() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now: i64 = 1_000;

    // Record 2 failures
    cb.record_failure(pid, now, None);
    cb.record_failure(pid, now + 1, None);

    // Jump forward past the window (300s)
    let later = now + (FAILURE_WINDOW_SECS as i64) + 10;

    // Old failures should have decayed
    let snap = cb.snapshot(pid, later);
    assert_eq!(snap.failure_count, 0);

    // Need 3 fresh failures to trip, not 1
    cb.record_failure(pid, later, None);
    let snap = cb.snapshot(pid, later + 1);
    assert_eq!(snap.state, CircuitState::Closed);
    assert_eq!(snap.failure_count, 1);

    cb.record_failure(pid, later + 2, None);
    let snap = cb.snapshot(pid, later + 3);
    assert_eq!(snap.state, CircuitState::Closed);
    assert_eq!(snap.failure_count, 2);

    let change = cb.record_failure(pid, later + 3, None);
    assert_eq!(change.after.state, CircuitState::Open);
}

#[test]
fn should_allow_prunes_expired_closed_failures_and_keeps_revision_tombstone() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        Some(tx),
    );
    let provider_id = 1;
    let now = 1_000;

    cb.record_failure(provider_id, now, None);
    let _ = rx.try_recv().expect("failure state persisted");
    assert_eq!(cb.health.lock().expect("health lock").len(), 1);

    let later = now + (FAILURE_WINDOW_SECS as i64) + 1;
    let check = cb.should_allow(provider_id, later);

    assert!(check.allow);
    assert_eq!(check.after.state, CircuitState::Closed);
    assert_eq!(check.after.failure_count, 0);
    let guard = cb.health.lock().expect("health lock");
    assert_eq!(guard.len(), 1);
    assert!(guard.get(&provider_id).expect("tombstone").state_revision > 0);
    drop(guard);

    let persisted = rx.try_recv().expect("pruned state persisted");
    assert_eq!(persisted.provider_id, provider_id);
    assert_eq!(persisted.state, CircuitState::Closed);
    assert!(persisted.failure_timestamps.is_empty());
}

#[test]
fn persist_queue_full_keeps_latest_state_in_bounded_backlog_and_flushes_later() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        Some(tx),
    );
    let now = 1_000;

    cb.record_failure(1, now, None);
    cb.record_failure(2, now, None);

    {
        let backlog = cb.persist_backlog.lock().expect("backlog lock");
        assert!(backlog.contains_key(&2));
    }

    let first = rx.try_recv().expect("first state queued");
    assert_eq!(first.provider_id, 1);

    cb.record_failure(3, now, None);

    let flushed = rx.try_recv().expect("backlog state flushed first");
    assert_eq!(flushed.provider_id, 2);

    let backlog = cb.persist_backlog.lock().expect("backlog lock");
    assert!(!backlog.contains_key(&2));
    assert!(backlog.contains_key(&3));
}

#[tokio::test]
async fn persist_backlog_flushes_in_background_without_future_state_changes() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        Some(tx),
    );
    let now = 1_000;

    cb.record_failure(1, now, None);
    cb.record_failure(2, now, None);

    {
        let backlog = cb.persist_backlog.lock().expect("backlog lock");
        assert!(backlog.contains_key(&2));
    }

    let first = rx.recv().await.expect("first state queued");
    assert_eq!(first.provider_id, 1);

    let flushed = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("background backlog flush should not require another circuit update")
        .expect("backlog state queued");
    assert_eq!(flushed.provider_id, 2);

    let backlog = cb.persist_backlog.lock().expect("backlog lock");
    assert!(backlog.is_empty());
}

#[tokio::test]
async fn persist_backlog_background_flush_sends_latest_state_after_waiting_for_capacity() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        Some(tx),
    );
    let now = 1_000;

    cb.record_failure(1, now, None);
    cb.record_failure(2, now, None);
    cb.record_failure(2, now + 1, None);

    let first = rx.recv().await.expect("first state queued");
    assert_eq!(first.provider_id, 1);

    let flushed = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("background backlog flush should resume when capacity is available")
        .expect("backlog state queued");
    assert_eq!(flushed.provider_id, 2);
    assert_eq!(flushed.updated_at, now + 1);
    assert_eq!(
        flushed.failure_timestamps,
        vec![now as u64, (now + 1) as u64]
    );
}

fn persisted_state(provider_id: i64, updated_at: i64) -> CircuitPersistedState {
    CircuitPersistedState {
        provider_id,
        state: CircuitState::Closed,
        failure_timestamps: vec![updated_at as u64],
        half_open_success_count: 0,
        open_until: None,
        probe_reference_at: None,
        next_probe_at: None,
        natural_probe_due_at: None,
        recovery_guard_until: None,
        state_revision: updated_at.max(0) as u64,
        updated_at,
    }
}

#[test]
fn persist_backlog_flushes_oldest_updated_state_first() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        Some(tx),
    );

    cb.record_failure(1, 1_000, None);
    {
        let mut backlog = cb.persist_backlog.lock().expect("backlog lock");
        backlog.insert(3, persisted_state(3, 3_000));
        backlog.insert(2, persisted_state(2, 2_000));
    }

    let first = rx.try_recv().expect("first state queued");
    assert_eq!(first.provider_id, 1);

    cb.record_failure(4, 4_000, None);

    let flushed = rx.try_recv().expect("oldest backlog state flushed first");
    assert_eq!(flushed.provider_id, 2);

    let backlog = cb.persist_backlog.lock().expect("backlog lock");
    assert!(backlog.contains_key(&3));
    assert!(backlog.contains_key(&4));
}

#[test]
fn persist_backlog_evicts_oldest_updated_state_at_capacity() {
    let cb = breaker();

    {
        let mut backlog = cb.persist_backlog.lock().expect("backlog lock");
        for index in 0..MAX_PERSIST_BACKLOG {
            let provider_id = (index + 1) as i64;
            backlog.insert(
                provider_id,
                persisted_state(provider_id, 1_000 + index as i64),
            );
        }
    }

    cb.enqueue_persist_backlog(persisted_state(9_999, 9_999));

    let backlog = cb.persist_backlog.lock().expect("backlog lock");
    assert_eq!(backlog.len(), MAX_PERSIST_BACKLOG);
    assert!(!backlog.contains_key(&1));
    assert!(backlog.contains_key(&2));
    assert!(backlog.contains_key(&9_999));
}

#[test]
fn persist_backlog_never_replaces_newer_closed_tombstone_with_old_open_state() {
    let cb = breaker();
    let mut old_open = persisted_state(7, 2_000);
    old_open.state = CircuitState::Open;
    old_open.open_until = Some(9_000);
    old_open.state_revision = 10;
    let mut tombstone = persisted_state(7, 1_500);
    tombstone.failure_timestamps.clear();
    tombstone.state_revision = 11;

    cb.enqueue_persist_backlog(old_open.clone());
    cb.enqueue_persist_backlog(tombstone.clone());
    old_open.updated_at = 9_999;
    cb.enqueue_persist_backlog(old_open);

    let backlog = cb.persist_backlog.lock().expect("backlog lock");
    let retained = backlog.get(&7).expect("provider state");
    assert_eq!(retained.state, CircuitState::Closed);
    assert_eq!(retained.state_revision, 11);
}

#[test]
fn reset_clears_open_and_cooldown() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64, None);
    }

    let open = cb.snapshot(pid, now + 10);
    assert_eq!(open.state, CircuitState::Open);

    let reset = cb.reset(pid, now + 20);
    assert_eq!(reset.state, CircuitState::Closed);
    assert_eq!(reset.failure_count, 0);
    assert!(reset.open_until.is_none());
    assert!(reset.cooldown_until.is_none());

    let allow = cb.should_allow(pid, now + 21);
    assert!(allow.allow);
}

#[test]
fn reset_clears_in_flight_probe() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;
    let due = open_provider(&cb, pid, now);
    let _token = acquire_probe(&cb, pid, "trace-a", due);
    let snap = cb.snapshot(pid, due);
    assert_eq!(snap.state, CircuitState::HalfOpen);

    let reset = cb.reset(pid, due + 1);
    assert_eq!(reset.state, CircuitState::Closed);
    assert_eq!(reset.failure_count, 0);
}

#[test]
fn update_config_recalculates_open_until() {
    let cb = breaker(); // default: 30min open duration
    let pid = 1;
    let now = 1_000;

    // Trip the circuit breaker
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64, None);
    }

    let snap = cb.snapshot(pid, now + 10);
    assert_eq!(snap.state, CircuitState::Open);
    let original_open_until = snap.open_until.expect("open_until");
    // Default: open_until = updated_at + 30*60
    assert_eq!(
        original_open_until,
        (now + DEFAULT_FAILURE_THRESHOLD as i64) + DEFAULT_OPEN_DURATION_SECS
    );

    // Hot-reload config: reduce to 60 seconds
    cb.update_config(CircuitBreakerConfig {
        failure_threshold: DEFAULT_FAILURE_THRESHOLD,
        open_duration_secs: 60,
        ..CircuitBreakerConfig::default()
    });

    let snap_after = cb.snapshot(pid, now + 10);
    assert_eq!(snap_after.state, CircuitState::Open);
    let new_open_until = snap_after.open_until.expect("open_until");
    // New: open_until = updated_at + 60
    assert_eq!(
        new_open_until,
        (now + DEFAULT_FAILURE_THRESHOLD as i64) + 60
    );
    assert!(new_open_until < original_open_until);

    // The longest-open deadline creates an eligible probe opportunity but
    // never opens the public gate to concurrent requests.
    let check = cb.should_allow(pid, new_open_until);
    assert!(!check.allow);
    let token = acquire_probe(&cb, pid, "trace-a", new_open_until);
    assert_eq!(token.provider_id, pid);
    assert_eq!(
        cb.snapshot(pid, new_open_until).state,
        CircuitState::HalfOpen
    );
}

#[test]
fn update_config_recalculates_closed_natural_deadline() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 5,
            natural_probe_max_wait_secs: 300,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;
    cb.record_failure(pid, now, None);

    cb.update_config(CircuitBreakerConfig {
        failure_threshold: 5,
        natural_probe_max_wait_secs: 60,
        ..CircuitBreakerConfig::default()
    });

    let snapshot = cb.snapshot(pid, now + 1);
    assert_eq!(snapshot.state, CircuitState::Closed);
    assert_eq!(snapshot.probe_reference_at, Some(now));
    assert_eq!(snapshot.natural_probe_due_at, Some(now + 60));
}

#[test]
fn failure_timestamps_capped_at_max() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: (MAX_FAILURE_TIMESTAMPS as u32) + 100,
            open_duration_secs: 60,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now: i64 = 10_000;

    // Record more failures than the hard cap, all within the window
    for i in 0..(MAX_FAILURE_TIMESTAMPS + 50) {
        cb.record_failure(pid, now + i as i64, None);
    }

    let snap = cb.snapshot(pid, now + (MAX_FAILURE_TIMESTAMPS + 50) as i64);
    // failure_count should be capped at MAX_FAILURE_TIMESTAMPS
    assert!(
        snap.failure_count <= MAX_FAILURE_TIMESTAMPS as u32,
        "failure_count {} exceeded hard cap {}",
        snap.failure_count,
        MAX_FAILURE_TIMESTAMPS,
    );
    // Circuit should still be Closed because threshold is set very high
    assert_eq!(snap.state, CircuitState::Closed);
}

#[test]
fn healthy_read_success_and_missing_reset_do_not_create_closed_entries() {
    let cb = breaker();
    let now = 1_000;

    let check = cb.should_allow(10, now);
    assert!(check.allow);
    assert_eq!(check.after.state, CircuitState::Closed);

    let snap = cb.snapshot(11, now);
    assert_eq!(snap.state, CircuitState::Closed);

    let success = cb.record_success(12, now);
    assert_eq!(success.before.state, CircuitState::Closed);
    assert_eq!(success.after.state, CircuitState::Closed);
    assert!(success.transition.is_none());

    let reset = cb.reset(13, now);
    assert_eq!(reset.state, CircuitState::Closed);

    assert_eq!(cb.health.lock().expect("health lock").len(), 0);
}

#[test]
fn reset_keeps_revision_tombstone_after_failure() {
    let cb = breaker();
    let provider_id = 1;
    let now = 1_000;

    cb.record_failure(provider_id, now, None);
    assert_eq!(cb.health.lock().expect("health lock").len(), 1);

    let reset = cb.reset(provider_id, now + 1);
    assert_eq!(reset.state, CircuitState::Closed);
    assert_eq!(reset.failure_count, 0);
    let guard = cb.health.lock().expect("health lock");
    assert_eq!(guard.len(), 1);
    let tombstone = guard.get(&provider_id).expect("tombstone");
    assert!(tombstone.state_revision > 0);
    assert!(CircuitBreaker::is_inert_closed_health(tombstone));
}

#[test]
fn initial_inert_closed_state_is_not_loaded_into_runtime_health() {
    let mut initial = HashMap::new();
    initial.insert(
        1,
        CircuitPersistedState {
            provider_id: 1,
            state: CircuitState::Closed,
            failure_timestamps: Vec::new(),
            half_open_success_count: 0,
            open_until: None,
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: None,
            recovery_guard_until: None,
            state_revision: 0,
            updated_at: 1_000,
        },
    );
    initial.insert(
        2,
        CircuitPersistedState {
            provider_id: 2,
            state: CircuitState::Closed,
            failure_timestamps: vec![1_000],
            half_open_success_count: 0,
            open_until: None,
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: None,
            recovery_guard_until: None,
            state_revision: 0,
            updated_at: 1_000,
        },
    );

    let cb = CircuitBreaker::new(CircuitBreakerConfig::default(), initial, None);
    let guard = cb.health.lock().expect("health lock");

    assert!(!guard.contains_key(&1));
    assert!(guard.contains_key(&2));
}

#[test]
fn update_config_new_failures_use_new_duration() {
    let cb = CircuitBreaker::new(
        CircuitBreakerConfig {
            failure_threshold: 2,
            open_duration_secs: 600,
            ..CircuitBreakerConfig::default()
        },
        HashMap::new(),
        None,
    );
    let pid = 1;
    let now = 1_000;

    // Hot-reload to shorter duration BEFORE tripping
    cb.update_config(CircuitBreakerConfig {
        failure_threshold: 2,
        open_duration_secs: 30,
        ..CircuitBreakerConfig::default()
    });

    // Trip the circuit
    cb.record_failure(pid, now, None);
    cb.record_failure(pid, now + 1, None);

    let snap = cb.snapshot(pid, now + 2);
    assert_eq!(snap.state, CircuitState::Open);
    // open_until should use the new 30s duration, not the original 600s
    let open_until = snap.open_until.expect("open_until");
    assert_eq!(open_until, (now + 1) + 30);
}

#[test]
fn record_failure_remembers_trigger_error_code_until_closed() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;

    // Attributed failures trip the circuit and remember the trigger.
    for i in 1..=DEFAULT_FAILURE_THRESHOLD {
        cb.record_failure(pid, now + i as i64, Some("GW_UPSTREAM_TIMEOUT"));
    }
    let snap = cb.snapshot(pid, now + 10);
    assert_eq!(snap.state, CircuitState::Open);
    assert_eq!(snap.last_trigger_error_code, Some("GW_UPSTREAM_TIMEOUT"));

    // Acquiring the provider-scoped lease keeps the trigger for attribution.
    let due = snap.next_probe_at.expect("next probe");
    let token = acquire_probe(&cb, pid, "trace-a", due);
    let check = cb.snapshot(pid, due);
    assert_eq!(check.state, CircuitState::HalfOpen);
    assert_eq!(check.last_trigger_error_code, Some("GW_UPSTREAM_TIMEOUT"));

    let _ = cb.mark_probe_dispatched(&token, due);
    let ProbeCommitResult::Applied(change) = cb.complete_probe_success(&token, due + 1) else {
        panic!("probe success should apply");
    };
    assert_eq!(change.after.state, CircuitState::Closed);
    assert_eq!(change.after.last_trigger_error_code, None);
}

#[test]
fn unattributed_failure_keeps_known_trigger_error_code() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;

    cb.record_failure(pid, now, Some("GW_UPSTREAM_5XX"));
    let change = cb.record_failure(pid, now + 1, None);
    assert_eq!(
        change.after.last_trigger_error_code,
        Some("GW_UPSTREAM_5XX")
    );
}

#[test]
fn closed_success_clears_trigger_error_code() {
    let cb = breaker();
    let pid = 1;
    let now = 1_000;

    cb.record_failure(pid, now, Some("GW_UPSTREAM_5XX"));
    let change = cb.record_success(pid, now + 1);
    assert_eq!(change.after.state, CircuitState::Closed);
    assert_eq!(change.after.last_trigger_error_code, None);
}
