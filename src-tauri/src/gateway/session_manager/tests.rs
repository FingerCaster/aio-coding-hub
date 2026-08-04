use super::*;
use axum::http::{header, HeaderMap, HeaderValue};

// ---------------------------------------------------------------------------
// Sliding TTL tests
// ---------------------------------------------------------------------------

#[test]
fn sliding_ttl_refreshes_on_get_bound_provider() {
    let manager = SessionManager::new(); // TTL = 300s
    let t0 = 1000;

    // Create a binding at t0
    manager.bind_success("claude", "s1", 42, None, t0);

    // Access at t0 + 200 (within TTL) — should succeed and refresh
    let t1 = t0 + 200;
    let provider = manager.get_bound_provider("claude", "s1", t1);
    assert_eq!(provider, Some(42));

    // After refresh, binding should survive until t1 + 300 = 1500
    // Access at t0 + 400 (> original t0+300 but < refreshed t1+300)
    let t2 = t0 + 400;
    let provider = manager.get_bound_provider("claude", "s1", t2);
    assert_eq!(
        provider,
        Some(42),
        "binding should still be valid after sliding TTL refresh"
    );
}

#[test]
fn sliding_ttl_expired_without_access() {
    let manager = SessionManager::new(); // TTL = 300s
    let t0 = 1000;

    manager.bind_success("claude", "s1", 42, None, t0);

    // No access in between — check after TTL expires
    let t_expired = t0 + 301;
    let provider = manager.get_bound_provider("claude", "s1", t_expired);
    assert_eq!(
        provider, None,
        "binding should expire without sliding refresh"
    );
}

#[test]
fn sliding_ttl_chain_of_accesses_extends_lifetime() {
    let manager = SessionManager::new(); // TTL = 300s
    let t0 = 1000;

    manager.bind_success("claude", "s1", 42, None, t0);

    // Chain of accesses, each within TTL of the previous
    for i in 1..=5 {
        let t = t0 + i * 200; // 1200, 1400, 1600, 1800, 2000
        let provider = manager.get_bound_provider("claude", "s1", t);
        assert_eq!(provider, Some(42), "access {i} at t={t} should succeed");
    }

    // Last access at 2000 refreshed to 2300. Access at 2299 should work.
    let provider = manager.get_bound_provider("claude", "s1", 2299);
    assert_eq!(provider, Some(42));

    // But 2600 (after last refresh) should fail
    let provider = manager.get_bound_provider("claude", "s1", 2601);
    assert_eq!(provider, None);
}

#[test]
fn sliding_ttl_refreshes_on_get_bound_sort_mode_id() {
    let manager = SessionManager::new();
    let t0 = 1000;

    manager.bind_sort_mode("claude", "s1", Some(7), None, t0);

    // Access at t0 + 200 refreshes TTL
    let t1 = t0 + 200;
    let mode = manager.get_bound_sort_mode_id("claude", "s1", t1);
    assert_eq!(mode, Some(Some(7)));

    // Should survive past original expiry (t0 + 300) because of refresh
    let t2 = t0 + 400;
    let mode = manager.get_bound_sort_mode_id("claude", "s1", t2);
    assert_eq!(
        mode,
        Some(Some(7)),
        "sort_mode binding should survive after sliding refresh"
    );
}

#[test]
fn sliding_ttl_refreshes_on_get_bound_provider_order() {
    let manager = SessionManager::new();
    let t0 = 1000;

    manager.bind_sort_mode("claude", "s1", Some(1), Some(vec![10, 20]), t0);

    // Access at t0 + 200 refreshes
    let t1 = t0 + 200;
    let order = manager.get_bound_provider_order("claude", "s1", t1);
    assert_eq!(order, Some(vec![10, 20]));

    // Should survive past original expiry
    let t2 = t0 + 400;
    let order = manager.get_bound_provider_order("claude", "s1", t2);
    assert_eq!(order, Some(vec![10, 20]));
}

#[test]
fn sliding_ttl_bind_success_refreshes_existing_binding() {
    let manager = SessionManager::new();
    let t0 = 1000;

    manager.bind_success("claude", "s1", 42, None, t0);

    // bind_success again at t0 + 200 with same session
    let t1 = t0 + 200;
    manager.bind_success("claude", "s1", 42, None, t1);

    // Should survive until t1 + 300 = 1500
    let t2 = t0 + 400;
    let provider = manager.get_bound_provider("claude", "s1", t2);
    assert_eq!(provider, Some(42));
}

#[test]
fn live_binding_preserves_its_recovery_epoch_baseline_across_refreshes() {
    let manager = SessionManager::new();
    let t0 = 1_000;

    let initial_request = manager.begin_binding_request().expect("initial request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "claude",
        "s1",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 7, initial_request),
        t0,
    ));
    manager.bind_success("claude", "s1", 2, None, t0 + 100);
    assert_eq!(
        manager.get_bound_provider("claude", "s1", t0 + 200),
        Some(2)
    );

    let refresh_request = manager.begin_binding_request().expect("refresh request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "claude",
        "s1",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 99, refresh_request),
        t0 + 400,
    ));
    assert!(manager.confirm_route(
        "claude",
        "s1",
        &SessionRouteFingerprint::new(None, vec![1, 2]),
        t0 + 500,
    ));

    let snapshot = manager
        .routing_snapshot("claude", "s1", t0 + 501)
        .expect("live binding");
    assert_eq!(snapshot.recovery_epoch_baseline, 7);
}

#[test]
fn later_started_request_wins_binding_regardless_of_completion_order() {
    let manager = SessionManager::new();
    let now = 1_000;
    manager.bind_sort_mode("codex", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("codex", "s1", 2, None, now);

    let older = manager.begin_binding_request().expect("older request");
    let newer = manager.begin_binding_request().expect("newer request");
    assert!(manager.bind_success_for_request("codex", "s1", 1, None, newer, now + 1));
    assert!(!manager.bind_success_for_request("codex", "s1", 2, None, older, now + 2));
    assert_eq!(manager.get_bound_provider("codex", "s1", now + 3), Some(1));

    let older = manager.begin_binding_request().expect("next older request");
    let newer = manager.begin_binding_request().expect("next newer request");
    assert!(manager.bind_success_for_request("codex", "s1", 2, None, older, now + 4));
    assert!(manager.bind_success_for_request("codex", "s1", 1, None, newer, now + 5));
    assert_eq!(manager.get_bound_provider("codex", "s1", now + 6), Some(1));
}

#[test]
fn first_request_creates_and_commits_a_binding() {
    let manager = SessionManager::new();
    let now = 1_000;
    let request = manager.begin_binding_request().expect("binding request");

    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "new-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 7, request),
        now,
    ));
    assert!(manager.bind_success_for_request("codex", "new-session", 1, None, request, now + 1,));
    assert_eq!(
        manager.get_bound_provider("codex", "new-session", now + 2),
        Some(1)
    );
}

#[test]
fn clear_rejects_the_old_incarnation_and_allows_a_new_request() {
    let manager = SessionManager::new();
    let now = 1_000;
    let old_request = manager.begin_binding_request().expect("old request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "cleared-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 0, old_request),
        now,
    ));
    assert!(manager.bind_success_for_request(
        "codex",
        "cleared-session",
        2,
        None,
        old_request,
        now + 1,
    ));

    assert_eq!(manager.clear_cli_bindings("codex"), 1);
    assert!(!manager.bind_success_for_request(
        "codex",
        "cleared-session",
        2,
        None,
        old_request,
        now + 2,
    ));

    let new_request = manager.begin_binding_request().expect("new request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "cleared-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 0, new_request),
        now + 3,
    ));
    assert!(!manager.bind_success_for_request(
        "codex",
        "cleared-session",
        2,
        None,
        old_request,
        now + 4,
    ));
    assert!(manager.bind_success_for_request(
        "codex",
        "cleared-session",
        1,
        None,
        new_request,
        now + 5,
    ));
    assert_eq!(
        manager.get_bound_provider("codex", "cleared-session", now + 6),
        Some(1)
    );
}

#[test]
fn ttl_recreation_rejects_a_request_from_the_expired_incarnation() {
    let manager = SessionManager::new();
    let now = 1_000;
    let old_request = manager.begin_binding_request().expect("old request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "expired-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 0, old_request),
        now,
    ));
    assert!(manager.bind_success_for_request(
        "codex",
        "expired-session",
        2,
        None,
        old_request,
        now,
    ));

    let new_request = manager.begin_binding_request().expect("new request");
    let recreated_at = now + DEFAULT_SESSION_TTL_SECS;
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "expired-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 0, new_request),
        recreated_at,
    ));
    assert!(!manager.bind_success_for_request(
        "codex",
        "expired-session",
        2,
        None,
        old_request,
        recreated_at + 1,
    ));
    assert!(manager.bind_success_for_request(
        "codex",
        "expired-session",
        1,
        None,
        new_request,
        recreated_at + 2,
    ));
    assert_eq!(
        manager.get_bound_provider("codex", "expired-session", recreated_at + 3),
        Some(1)
    );
}

#[test]
fn equal_request_token_is_idempotent_only_for_the_same_provider() {
    let manager = SessionManager::new();
    let now = 1_000;
    let request = manager.begin_binding_request().expect("binding request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "codex",
        "idempotent-session",
        SessionBindingCreation::new(None, Some(vec![1, 2]), 0, request),
        now,
    ));

    assert!(manager.bind_success_for_request(
        "codex",
        "idempotent-session",
        1,
        None,
        request,
        now + 1,
    ));
    assert!(manager.bind_success_for_request(
        "codex",
        "idempotent-session",
        1,
        None,
        request,
        now + 2,
    ));
    assert!(!manager.bind_success_for_request(
        "codex",
        "idempotent-session",
        2,
        None,
        request,
        now + 3,
    ));
    assert_eq!(
        manager.get_bound_provider("codex", "idempotent-session", now + 4),
        Some(1)
    );
}

#[test]
fn binding_request_overflow_never_falls_back_to_an_unversioned_write() {
    let manager = SessionManager::new();
    manager
        .next_binding_request
        .store(u64::MAX, std::sync::atomic::Ordering::Relaxed);

    assert!(manager.begin_binding_request().is_none());
    manager.bind_sort_mode("codex", "overflow-session", None, Some(vec![1]), 1_000);
    manager.bind_success("codex", "overflow-session", 1, None, 1_001);
    assert_eq!(
        manager.get_bound_provider("codex", "overflow-session", 1_002),
        None
    );
}

#[test]
fn recreated_binding_captures_the_then_current_recovery_epoch() {
    let manager = SessionManager::new();
    let t0 = 1_000;

    let initial_request = manager.begin_binding_request().expect("initial request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "claude",
        "expired",
        SessionBindingCreation::new(None, None, 3, initial_request),
        t0,
    ));
    assert!(manager
        .routing_snapshot("claude", "expired", t0 + DEFAULT_SESSION_TTL_SECS + 1)
        .is_none());
    let expired_request = manager
        .begin_binding_request()
        .expect("expired recreation request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "claude",
        "expired",
        SessionBindingCreation::new(None, None, 8, expired_request),
        t0 + DEFAULT_SESSION_TTL_SECS + 2,
    ));
    assert_eq!(
        manager
            .routing_snapshot("claude", "expired", t0 + DEFAULT_SESSION_TTL_SECS + 3)
            .expect("recreated expired binding")
            .recovery_epoch_baseline,
        8
    );

    assert!(manager.clear_bound_provider("claude", "expired", t0 + 400));
    let cleared_request = manager
        .begin_binding_request()
        .expect("cleared recreation request");
    assert!(manager.bind_sort_mode_with_recovery_epoch(
        "claude",
        "expired",
        SessionBindingCreation::new(None, None, 13, cleared_request),
        t0 + 401,
    ));
    assert_eq!(
        manager
            .routing_snapshot("claude", "expired", t0 + 402)
            .expect("recreated cleared binding")
            .recovery_epoch_baseline,
        13
    );
}

#[test]
fn sliding_ttl_lru_eviction_works_with_refreshed_bindings() {
    let manager = SessionManager::new();
    let t0 = 1000;

    // Create two bindings
    manager.bind_success("claude", "old_session", 1, None, t0);
    manager.bind_success("claude", "new_session", 2, None, t0);

    // Refresh only new_session at t0 + 100
    let t1 = t0 + 100;
    manager.get_bound_provider("claude", "new_session", t1);

    // Both active — list should show new_session with higher expires_at
    let active = manager.list_active(t1, 10);
    assert_eq!(active.len(), 2);
    // First (sorted by expires_at desc) should be new_session (refreshed)
    assert_eq!(active[0].session_id, "new_session");
    assert_eq!(active[1].session_id, "old_session");
    assert!(active[0].expires_at > active[1].expires_at);
}

#[test]
fn clear_cli_bindings_removes_only_target_cli() {
    let manager = SessionManager::new();
    let now_unix = 100;

    manager.bind_sort_mode(
        "claude",
        "session_a",
        Some(1),
        Some(vec![101, 102]),
        now_unix,
    );
    manager.bind_sort_mode("claude", "session_b", None, None, now_unix);
    manager.bind_sort_mode("codex", "session_c", Some(2), Some(vec![201]), now_unix);

    assert_eq!(manager.clear_cli_bindings(""), 0);

    let removed = manager.clear_cli_bindings("claude");
    assert_eq!(removed, 2);

    assert_eq!(
        manager.get_bound_sort_mode_id("claude", "session_a", now_unix),
        None
    );
    assert_eq!(
        manager.get_bound_sort_mode_id("claude", "session_b", now_unix),
        None
    );
    assert_eq!(
        manager.get_bound_sort_mode_id("codex", "session_c", now_unix),
        Some(Some(2))
    );
}

#[test]
fn compaction_trigger_is_reserved_once_and_drop_releases_it() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("claude", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("claude", "s1", now + 1)
        .expect("compaction generation");

    let reservation = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 2,
        )
        .expect("first reservation");
    assert!(manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 2,
        )
        .is_none());

    drop(reservation);
    assert!(manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 3,
        )
        .is_some());
}

#[test]
fn trigger_commit_consumes_at_dispatch_and_rollback_restores_opportunity() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("claude", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("claude", "s1", now + 1)
        .expect("compaction generation");
    let reservation = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 2,
        )
        .expect("reservation");

    let commit = reservation.commit(now + 3).expect("commit");
    let consumed = manager
        .routing_snapshot("claude", "s1", now + 3)
        .expect("snapshot");
    assert_eq!(consumed.consumed_compaction_generation, generation);

    assert!(commit.rollback());
    let restored = manager
        .routing_snapshot("claude", "s1", now + 4)
        .expect("snapshot");
    assert_eq!(restored.consumed_compaction_generation, 0);
}

#[test]
fn codex_fingerprint_commit_consumes_observed_generation_without_consuming_a_newer_one() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("codex", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("codex", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("codex", "s1", now + 1)
        .expect("compaction generation");
    let reservation = manager
        .try_reserve_probe_trigger(
            "codex",
            "s1",
            SessionProbeTrigger::CodexCompactionFingerprint {
                fingerprint: "compact-a".to_string(),
                pending_generation: Some(generation),
            },
            now + 2,
        )
        .expect("compound reservation");

    let newer_generation = manager
        .mark_compaction_completed("codex", "s1", now + 3)
        .expect("concurrent newer generation");
    let _commit = reservation.commit(now + 4).expect("compound commit");
    let snapshot = manager
        .routing_snapshot("codex", "s1", now + 5)
        .expect("snapshot");

    assert_eq!(snapshot.consumed_compaction_generation, generation);
    assert_eq!(snapshot.completed_compaction_generation, newer_generation);
    assert_eq!(
        snapshot.last_codex_compaction_fingerprint.as_deref(),
        Some("compact-a")
    );
}

#[test]
fn codex_fingerprint_dispatch_consumes_both_triggers_after_probe_failure() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("codex", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("codex", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("codex", "s1", now + 1)
        .expect("compaction generation");
    let trigger = SessionProbeTrigger::CodexCompactionFingerprint {
        fingerprint: "compact-a".to_string(),
        pending_generation: Some(generation),
    };
    let _commit = manager
        .try_reserve_probe_trigger("codex", "s1", trigger.clone(), now + 2)
        .expect("compound reservation")
        .commit(now + 3)
        .expect("compound commit");

    let snapshot = manager
        .routing_snapshot("codex", "s1", now + 4)
        .expect("snapshot");
    assert_eq!(snapshot.consumed_compaction_generation, generation);
    assert!(manager
        .try_reserve_probe_trigger("codex", "s1", trigger, now + 5)
        .is_none());
    assert!(manager
        .try_reserve_probe_trigger(
            "codex",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 5,
        )
        .is_none());
}

#[test]
fn codex_fingerprint_rollback_restores_both_compound_trigger_fields() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("codex", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("codex", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("codex", "s1", now + 1)
        .expect("compaction generation");
    let commit = manager
        .try_reserve_probe_trigger(
            "codex",
            "s1",
            SessionProbeTrigger::CodexCompactionFingerprint {
                fingerprint: "compact-a".to_string(),
                pending_generation: Some(generation),
            },
            now + 2,
        )
        .expect("compound reservation")
        .commit(now + 3)
        .expect("compound commit");

    assert!(commit.rollback());
    let snapshot = manager
        .routing_snapshot("codex", "s1", now + 4)
        .expect("snapshot");
    assert_eq!(snapshot.consumed_compaction_generation, 0);
    assert_eq!(snapshot.last_codex_compaction_fingerprint, None);
}

#[test]
fn clearing_invalid_binding_discards_all_failback_state_before_rebind() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("codex", "s1", Some(7), Some(vec![2, 1]), now);
    manager.bind_success("codex", "s1", 2, Some(7), now);
    let generation = manager
        .mark_compaction_completed("codex", "s1", now + 1)
        .expect("compaction generation");
    let _commit = manager
        .try_reserve_probe_trigger(
            "codex",
            "s1",
            SessionProbeTrigger::CodexCompactionFingerprint {
                fingerprint: "old-compact".to_string(),
                pending_generation: Some(generation),
            },
            now + 2,
        )
        .expect("old reservation")
        .commit(now + 3)
        .expect("old commit");

    assert!(manager.clear_bound_provider("codex", "s1", now + 4));
    assert!(manager.routing_snapshot("codex", "s1", now + 4).is_none());

    manager.bind_success("codex", "s1", 1, None, now + 5);
    let rebound = manager
        .routing_snapshot("codex", "s1", now + 6)
        .expect("rebound snapshot");
    assert_eq!(manager.get_bound_provider("codex", "s1", now + 6), Some(1));
    assert_eq!(rebound.route, SessionRouteFingerprint::new(None, vec![]));
    assert_eq!(rebound.completed_compaction_generation, 0);
    assert_eq!(rebound.consumed_compaction_generation, 0);
    assert_eq!(rebound.last_codex_compaction_fingerprint, None);
}

#[test]
fn trigger_rollback_ignores_unrelated_revision_changes() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("claude", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("claude", "s1", now + 1)
        .expect("compaction generation");
    let reservation = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 2,
        )
        .expect("reservation");
    let commit = reservation.commit(now + 3).expect("commit");

    let next_generation = manager
        .mark_compaction_completed("claude", "s1", now + 4)
        .expect("unrelated completed generation update");
    assert_eq!(next_generation, generation + 1);
    assert!(manager.confirm_route(
        "claude",
        "s1",
        &SessionRouteFingerprint::new(Some(9), vec![2, 1]),
        now + 5,
    ));

    assert!(commit.rollback());
    let restored = manager
        .routing_snapshot("claude", "s1", now + 6)
        .expect("snapshot");
    assert_eq!(restored.completed_compaction_generation, generation + 1);
    assert_eq!(restored.consumed_compaction_generation, 0);
    assert_eq!(restored.route.sort_mode_id, Some(9));
    assert_eq!(restored.route.provider_order, vec![2, 1]);
}

#[test]
fn trigger_rollback_rejects_when_same_field_has_a_newer_commit() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("claude", "s1", 2, None, now);
    let first_generation = manager
        .mark_compaction_completed("claude", "s1", now + 1)
        .expect("first generation");
    let first_commit = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(first_generation),
            now + 2,
        )
        .expect("first reservation")
        .commit(now + 3)
        .expect("first commit");

    let second_generation = manager
        .mark_compaction_completed("claude", "s1", now + 4)
        .expect("second generation");
    let _second_commit = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(second_generation),
            now + 5,
        )
        .expect("second reservation")
        .commit(now + 6)
        .expect("second commit");

    assert!(!first_commit.rollback());
    let snapshot = manager
        .routing_snapshot("claude", "s1", now + 7)
        .expect("snapshot");
    assert_eq!(snapshot.consumed_compaction_generation, second_generation);
}

#[test]
fn expired_trigger_reservation_cannot_commit_and_can_be_reserved_again() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_sort_mode("claude", "s1", None, Some(vec![1, 2]), now);
    manager.bind_success("claude", "s1", 2, None, now);
    let generation = manager
        .mark_compaction_completed("claude", "s1", now + 1)
        .expect("compaction generation");
    let reservation = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            now + 2,
        )
        .expect("reservation");

    let expired_at = now + 2 + TRIGGER_RESERVATION_TTL_SECS;
    assert!(reservation.commit(expired_at).is_none());
    assert!(manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::CompactionGeneration(generation),
            expired_at,
        )
        .is_some());
}

#[test]
fn route_commit_rollback_restores_absent_provider_order_exactly() {
    let manager = Arc::new(SessionManager::new());
    let now = 1_000;
    manager.bind_success("claude", "s1", 2, None, now);
    let route = SessionRouteFingerprint::new(Some(7), vec![1, 2]);
    let reservation = manager
        .try_reserve_probe_trigger(
            "claude",
            "s1",
            SessionProbeTrigger::RouteChanged(route),
            now + 1,
        )
        .expect("route reservation");

    let commit = reservation.commit(now + 2).expect("route commit");
    assert!(commit.rollback());

    let guard = manager.bindings.lock_or_recover();
    let binding = guard
        .iter()
        .find_map(|(key, binding)| (key.session_id == "s1").then_some(binding))
        .expect("binding");
    assert!(binding.provider_order.is_none());
}

#[test]
fn extract_session_id_fallback_uses_message_fingerprint_and_ignores_user_agent() {
    let body = serde_json::json!({
        "messages": [
            { "role": "user", "content": "hello" },
            { "role": "assistant", "content": "world" }
        ]
    });

    let mut h1 = HeaderMap::new();
    h1.insert(header::USER_AGENT, HeaderValue::from_static("ua-1"));
    let mut h2 = HeaderMap::new();
    h2.insert(header::USER_AGENT, HeaderValue::from_static("ua-2"));

    let id1 = SessionManager::extract_session_id_from_json(&h1, Some(&body)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&h2, Some(&body)).expect("sid 2");
    assert_eq!(id1, id2);
}

#[test]
fn grok_stable_headers_take_priority_and_request_id_is_ignored() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-grok-session-id",
        HeaderValue::from_static("grok-session-stable"),
    );
    headers.insert(
        "x-grok-conv-id",
        HeaderValue::from_static("grok-conversation-stable"),
    );
    headers.insert(
        "x-grok-req-id",
        HeaderValue::from_static("grok-request-unique"),
    );
    headers.insert("session_id", HeaderValue::from_static("generic-session"));
    let body = serde_json::json!({ "session_id": "json-session" });

    assert_eq!(
        SessionManager::extract_session_id_from_json(&headers, Some(&body)).as_deref(),
        Some("grok-session-stable")
    );

    headers.remove("x-grok-session-id");
    assert_eq!(
        SessionManager::extract_session_id_from_json(&headers, Some(&body)).as_deref(),
        Some("grok-conversation-stable")
    );

    headers.remove("x-grok-conv-id");
    headers.remove("session_id");
    assert_eq!(
        SessionManager::extract_session_id_from_json(&headers, Some(&body)).as_deref(),
        Some("json-session")
    );
}

#[test]
fn extract_session_id_fallback_changes_when_message_fingerprint_changes() {
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_static("ua"));

    let body1 = serde_json::json!({
        "messages": [{ "role": "user", "content": "hello" }]
    });
    let body2 = serde_json::json!({
        "messages": [{ "role": "user", "content": "goodbye" }]
    });

    let id1 = SessionManager::extract_session_id_from_json(&headers, Some(&body1)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&headers, Some(&body2)).expect("sid 2");
    assert_ne!(id1, id2);
}

#[test]
fn extract_session_id_fallback_uses_only_first_three_segments() {
    let headers = HeaderMap::new();

    let body_with_four = serde_json::json!({
        "messages": [
            { "role": "user", "content": "a" },
            { "role": "assistant", "content": "b" },
            { "role": "user", "content": "c" },
            { "role": "assistant", "content": "d" }
        ]
    });
    let body_with_three = serde_json::json!({
        "messages": [
            { "role": "user", "content": "a" },
            { "role": "assistant", "content": "b" },
            { "role": "user", "content": "c" }
        ]
    });

    let id1 =
        SessionManager::extract_session_id_from_json(&headers, Some(&body_with_four)).expect("sid");
    let id2 = SessionManager::extract_session_id_from_json(&headers, Some(&body_with_three))
        .expect("sid");
    assert_eq!(id1, id2);
}

#[test]
fn extract_session_id_fallback_treats_content_parts_equivalent_to_string_content() {
    let headers = HeaderMap::new();

    let body_parts = serde_json::json!({
        "messages": [
            { "role": "user", "content": [{ "text": "he" }, { "text": "llo" }] }
        ]
    });
    let body_string = serde_json::json!({
        "messages": [
            { "role": "user", "content": "hello" }
        ]
    });

    let id1 =
        SessionManager::extract_session_id_from_json(&headers, Some(&body_parts)).expect("sid");
    let id2 =
        SessionManager::extract_session_id_from_json(&headers, Some(&body_string)).expect("sid");
    assert_eq!(id1, id2);
}

#[test]
fn extract_session_id_fallback_supports_input_string_shape() {
    let body = serde_json::json!({ "input": "hello" });

    let mut h1 = HeaderMap::new();
    h1.insert(header::USER_AGENT, HeaderValue::from_static("ua-1"));
    let mut h2 = HeaderMap::new();
    h2.insert(header::USER_AGENT, HeaderValue::from_static("ua-2"));

    let id1 = SessionManager::extract_session_id_from_json(&h1, Some(&body)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&h2, Some(&body)).expect("sid 2");
    assert_eq!(id1, id2);
}

#[test]
fn extract_session_id_fallback_samples_large_input_text_with_tail() {
    let headers = HeaderMap::new();
    let prefix = "a".repeat(SESSION_FINGERPRINT_TEXT_SAMPLE_BYTES + 1024);
    let body1 = serde_json::json!({ "input": format!("{prefix}tail-a") });
    let body2 = serde_json::json!({ "input": format!("{prefix}tail-b") });

    let id1 = SessionManager::extract_session_id_from_json(&headers, Some(&body1)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&headers, Some(&body2)).expect("sid 2");

    assert_ne!(id1, id2);
}

#[test]
fn extract_session_id_fallback_bounds_content_part_scanning() {
    let headers = HeaderMap::new();
    let mut parts = Vec::new();
    for index in 0..SESSION_FINGERPRINT_CONTENT_PARTS_MAX_ITEMS {
        parts.push(serde_json::json!({ "text": format!("part-{index};") }));
    }

    let mut parts_with_extra_a = parts.clone();
    parts_with_extra_a.push(serde_json::json!({ "text": "ignored-a" }));
    let mut parts_with_extra_b = parts;
    parts_with_extra_b.push(serde_json::json!({ "text": "ignored-b" }));

    let body1 = serde_json::json!({
        "messages": [{ "role": "user", "content": parts_with_extra_a }]
    });
    let body2 = serde_json::json!({
        "messages": [{ "role": "user", "content": parts_with_extra_b }]
    });

    let id1 = SessionManager::extract_session_id_from_json(&headers, Some(&body1)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&headers, Some(&body2)).expect("sid 2");

    assert_eq!(id1, id2);
}

#[test]
fn extract_session_id_fallback_distinguishes_different_api_keys() {
    let body = serde_json::json!({ "messages": [{ "role": "user", "content": "hello" }] });

    let mut h1 = HeaderMap::new();
    h1.insert("x-api-key", HeaderValue::from_static("key-a-123456789"));
    let mut h2 = HeaderMap::new();
    h2.insert("x-api-key", HeaderValue::from_static("key-b-123456789"));

    let id1 = SessionManager::extract_session_id_from_json(&h1, Some(&body)).expect("sid 1");
    let id2 = SessionManager::extract_session_id_from_json(&h2, Some(&body)).expect("sid 2");
    assert_ne!(id1, id2);
}

#[test]
fn sanitize_session_id_truncates_without_splitting_utf8() {
    let raw = format!("{}{}", "a".repeat(MAX_SESSION_ID_LEN - 1), "é");

    let sanitized = sanitize_session_id(&raw).expect("sanitized");

    assert_eq!(sanitized.len(), MAX_SESSION_ID_LEN - 1);
    assert!(sanitized.ends_with('a'));
}

#[test]
fn sanitize_session_id_removes_log_injection_controls_before_truncating() {
    let raw = format!("{}\n{}", "a".repeat(MAX_SESSION_ID_LEN), "tail");

    let sanitized = sanitize_session_id(&raw).expect("sanitized");

    assert_eq!(sanitized.len(), MAX_SESSION_ID_LEN);
    assert!(!sanitized.contains('\n'));
}

#[test]
fn sanitize_deterministic_part_truncates_without_splitting_utf8() {
    let raw = format!("{}{}", "a".repeat(MAX_SESSION_ID_LEN - 1), "é");

    let sanitized = sanitize_deterministic_part(&raw).expect("sanitized");

    assert_eq!(sanitized.len(), MAX_SESSION_ID_LEN - 1);
    assert!(sanitized.ends_with('a'));
}

#[test]
fn sanitize_deterministic_part_removes_log_injection_controls_before_truncating() {
    let raw = format!("{}\n{}", "a".repeat(MAX_SESSION_ID_LEN), "tail");

    let sanitized = sanitize_deterministic_part(&raw).expect("sanitized");

    assert_eq!(sanitized.len(), MAX_SESSION_ID_LEN);
    assert!(!sanitized.contains('\n'));
}
