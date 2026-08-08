use crate::circuit_breaker::{CircuitSnapshot, CircuitState, ProbeTrigger};
use crate::settings::ProviderFailbackStrategy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway::proxy::handler) enum PlannedDispatch {
    Direct,
    Probe(ProbeTrigger),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway::proxy::handler) struct PlannedFailbackTarget {
    pub provider_id: i64,
    pub dispatch: PlannedDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway::proxy::handler) enum ProbePlannerDecision {
    Stay {
        confirm_route: bool,
        not_triggered_provider_ids: Vec<i64>,
    },
    Dispatch {
        targets: Vec<PlannedFailbackTarget>,
        reservation_trigger: ProbeTrigger,
        not_triggered_provider_ids: Vec<i64>,
    },
}

pub(in crate::gateway::proxy::handler) struct ProbePlannerInput<'a> {
    pub ordered_candidates: &'a [(i64, CircuitSnapshot)],
    pub bound_provider_id: Option<i64>,
    pub session_recovery_epoch_baseline: u64,
    pub route_changed: bool,
    pub strategy: ProviderFailbackStrategy,
    pub compaction_generation_pending: bool,
    pub codex_compaction_pending: bool,
    pub request_eligible: bool,
    pub now_unix: i64,
}

#[derive(Default)]
pub(in crate::gateway::proxy::handler) struct AccountUsageRecoveryInput<'a> {
    pub provider_recovery_epochs: &'a [(i64, u64)],
    pub blocked_provider_ids: &'a [i64],
    pub session_recovery_epoch_baseline: u64,
}

#[cfg(test)]
pub(in crate::gateway::proxy::handler) fn plan_probe(
    input: ProbePlannerInput<'_>,
) -> ProbePlannerDecision {
    plan_probe_with_account_usage(input, AccountUsageRecoveryInput::default())
}

pub(in crate::gateway::proxy::handler) fn plan_probe_with_account_usage(
    input: ProbePlannerInput<'_>,
    account_usage: AccountUsageRecoveryInput<'_>,
) -> ProbePlannerDecision {
    let stable_index = input
        .bound_provider_id
        .and_then(|provider_id| {
            input
                .ordered_candidates
                .iter()
                .position(|(id, _)| *id == provider_id)
        })
        .unwrap_or(input.ordered_candidates.len());
    let higher = &input.ordered_candidates[..stable_index];

    if higher.is_empty() {
        return ProbePlannerDecision::Stay {
            confirm_route: input.route_changed,
            not_triggered_provider_ids: Vec::new(),
        };
    }
    if !input.request_eligible {
        return ProbePlannerDecision::Stay {
            confirm_route: false,
            not_triggered_provider_ids: Vec::new(),
        };
    }

    if input.bound_provider_id.is_none() {
        let mut unblocked_candidates = higher.iter().enumerate().filter(|(_, (provider_id, _))| {
            !account_usage.blocked_provider_ids.contains(provider_id)
        });
        let Some((first_unblocked_index, (_, first_unblocked_snapshot))) =
            unblocked_candidates.next()
        else {
            return ProbePlannerDecision::Stay {
                confirm_route: input.route_changed,
                not_triggered_provider_ids: Vec::new(),
            };
        };

        let mut all_open = matches!(
            first_unblocked_snapshot.state,
            CircuitState::Open | CircuitState::HalfOpen
        );
        let mut last_unblocked_index = first_unblocked_index;
        for (index, (_, snapshot)) in unblocked_candidates {
            all_open &= matches!(snapshot.state, CircuitState::Open | CircuitState::HalfOpen);
            last_unblocked_index = index;
        }

        let target_end_index = if all_open {
            last_unblocked_index
        } else {
            first_unblocked_index
        };
        let targets = higher
            .iter()
            .take(target_end_index.saturating_add(1))
            .map(|(provider_id, snapshot)| {
                planned_target(*provider_id, snapshot, ProbeTrigger::NewUnboundSession)
            })
            .collect();
        return ProbePlannerDecision::Dispatch {
            targets,
            reservation_trigger: ProbeTrigger::NewUnboundSession,
            not_triggered_provider_ids: Vec::new(),
        };
    }

    let explicit_trigger = if input.route_changed {
        Some(ProbeTrigger::RouteChanged)
    } else if input.codex_compaction_pending || input.compaction_generation_pending {
        Some(ProbeTrigger::NaturalCompaction)
    } else if input.strategy == ProviderFailbackStrategy::Aggressive {
        Some(ProbeTrigger::AggressiveTurn)
    } else {
        None
    };

    if let Some(trigger) = explicit_trigger {
        let targets: Vec<_> = higher
            .iter()
            .filter(|(provider_id, _)| !account_usage.blocked_provider_ids.contains(provider_id))
            .map(|(provider_id, snapshot)| planned_target(*provider_id, snapshot, trigger))
            .collect();
        if targets.is_empty() {
            return ProbePlannerDecision::Stay {
                confirm_route: input.route_changed,
                not_triggered_provider_ids: Vec::new(),
            };
        }
        return ProbePlannerDecision::Dispatch {
            targets,
            reservation_trigger: trigger,
            not_triggered_provider_ids: Vec::new(),
        };
    }

    let mut targets = Vec::new();
    let mut not_triggered_provider_ids = Vec::new();
    let mut reservation_trigger = None;
    for (provider_id, snapshot) in higher {
        if account_usage.blocked_provider_ids.contains(provider_id) {
            continue;
        }
        // Dispatch resets the Provider deadline while the lease is active.
        // Keep concurrent followers on the authoritative common gate so they
        // observe `in_flight` instead of a misleading `not_triggered` result.
        if snapshot.probe_in_flight {
            reservation_trigger.get_or_insert(ProbeTrigger::NaturalMaxWait);
            targets.push(planned_target(
                *provider_id,
                snapshot,
                ProbeTrigger::NaturalMaxWait,
            ));
            continue;
        }

        let account_usage_recovery_epoch = account_usage
            .provider_recovery_epochs
            .iter()
            .find_map(|(candidate_id, epoch)| (*candidate_id == *provider_id).then_some(*epoch))
            .unwrap_or(0);
        if snapshot.state == CircuitState::Closed
            && (snapshot.recovery_epoch > input.session_recovery_epoch_baseline
                || account_usage_recovery_epoch > account_usage.session_recovery_epoch_baseline)
        {
            targets.push(PlannedFailbackTarget {
                provider_id: *provider_id,
                dispatch: PlannedDispatch::Direct,
            });
            continue;
        }

        let trigger = if snapshot
            .natural_probe_due_at
            .is_some_and(|deadline| input.now_unix >= deadline)
        {
            Some(ProbeTrigger::NaturalMaxWait)
        } else if matches!(snapshot.state, CircuitState::Open | CircuitState::HalfOpen)
            && snapshot
                .open_until
                .is_some_and(|deadline| input.now_unix >= deadline)
        {
            Some(ProbeTrigger::MaxOpenWait)
        } else {
            None
        };

        if let Some(trigger) = trigger {
            reservation_trigger.get_or_insert(trigger);
            targets.push(planned_target(*provider_id, snapshot, trigger));
        } else {
            not_triggered_provider_ids.push(*provider_id);
        }
    }

    if targets.is_empty() {
        ProbePlannerDecision::Stay {
            confirm_route: false,
            not_triggered_provider_ids,
        }
    } else {
        ProbePlannerDecision::Dispatch {
            targets,
            // Recovery followers are direct targets and need no probe trigger.
            // This decision-level value only selects optional session
            // reservation behavior, for which natural failback reserves none.
            reservation_trigger: reservation_trigger.unwrap_or(ProbeTrigger::NaturalMaxWait),
            not_triggered_provider_ids,
        }
    }
}

fn planned_target(
    provider_id: i64,
    snapshot: &CircuitSnapshot,
    trigger: ProbeTrigger,
) -> PlannedFailbackTarget {
    PlannedFailbackTarget {
        provider_id,
        dispatch: match snapshot.state {
            CircuitState::Closed => PlannedDispatch::Direct,
            CircuitState::Open | CircuitState::HalfOpen => PlannedDispatch::Probe(trigger),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(state: CircuitState, natural_due: Option<i64>) -> CircuitSnapshot {
        CircuitSnapshot {
            state,
            failure_count: 0,
            failure_threshold: 5,
            open_until: None,
            cooldown_until: None,
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: natural_due,
            recovery_guard_until: None,
            recovery_epoch: 0,
            probe_in_flight: state == CircuitState::HalfOpen,
            state_revision: 1,
            last_trigger_error_code: None,
        }
    }

    fn recovered_snapshot(recovery_epoch: u64) -> CircuitSnapshot {
        let mut snapshot = snapshot(CircuitState::Closed, None);
        snapshot.recovery_epoch = recovery_epoch;
        snapshot
    }

    fn direct(provider_id: i64) -> PlannedFailbackTarget {
        PlannedFailbackTarget {
            provider_id,
            dispatch: PlannedDispatch::Direct,
        }
    }

    fn probe(provider_id: i64, trigger: ProbeTrigger) -> PlannedFailbackTarget {
        PlannedFailbackTarget {
            provider_id,
            dispatch: PlannedDispatch::Probe(trigger),
        }
    }

    #[test]
    fn natural_session_waits_without_trigger() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: vec![1],
            }
        );
    }

    #[test]
    fn natural_session_directs_only_to_recovery_newer_than_its_baseline() {
        let candidates = vec![
            (1, recovered_snapshot(5)),
            (2, recovered_snapshot(4)),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                session_recovery_epoch_baseline: 4,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: vec![2],
            }
        );
    }

    #[test]
    fn natural_session_mixes_recovered_direct_and_due_probes_in_route_order() {
        let mut max_open_due = snapshot(CircuitState::Open, None);
        max_open_due.open_until = Some(90);
        let candidates = vec![
            (1, recovered_snapshot(8)),
            (2, snapshot(CircuitState::Open, Some(90))),
            (3, recovered_snapshot(7)),
            (4, max_open_due),
            (5, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(5),
                session_recovery_epoch_baseline: 7,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    direct(1),
                    probe(2, ProbeTrigger::NaturalMaxWait),
                    probe(4, ProbeTrigger::MaxOpenWait),
                ],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: vec![3],
            }
        );
    }

    #[test]
    fn natural_session_routes_in_flight_probe_to_the_single_flight_gate() {
        let candidates = vec![
            (1, snapshot(CircuitState::HalfOpen, Some(400))),
            (2, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(1, ProbeTrigger::NaturalMaxWait)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_session_observes_not_due_candidate_and_plans_later_due_candidate() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Open, Some(90))),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(2, ProbeTrigger::NaturalMaxWait)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: vec![1],
            }
        );
    }

    #[test]
    fn natural_session_plans_every_due_candidate_in_route_order() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(90))),
            (2, snapshot(CircuitState::Open, Some(80))),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    probe(1, ProbeTrigger::NaturalMaxWait),
                    probe(2, ProbeTrigger::NaturalMaxWait),
                ],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn invalid_stable_session_plans_complete_all_open_recovery() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Open, Some(400))),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: None,
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    probe(1, ProbeTrigger::NewUnboundSession),
                    probe(2, ProbeTrigger::NewUnboundSession),
                ],
                reservation_trigger: ProbeTrigger::NewUnboundSession,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn invalid_stable_session_keeps_mixed_route_recovery_single_target() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Closed, None)),
            (3, snapshot(CircuitState::Open, Some(400))),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: None,
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(1, ProbeTrigger::NewUnboundSession)],
                reservation_trigger: ProbeTrigger::NewUnboundSession,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn aggressive_session_plans_complete_mixed_prefix() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, None)),
            (2, snapshot(CircuitState::Closed, None)),
            (3, snapshot(CircuitState::HalfOpen, None)),
            (4, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(4),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Aggressive,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    probe(1, ProbeTrigger::AggressiveTurn),
                    direct(2),
                    probe(3, ProbeTrigger::AggressiveTurn),
                ],
                reservation_trigger: ProbeTrigger::AggressiveTurn,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_session_does_not_directly_fail_back_to_closed_candidate_without_trigger() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: vec![1],
            }
        );
    }

    #[test]
    fn natural_session_waits_for_closed_candidate_pending_deadline() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, Some(120))),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: vec![1],
            }
        );
    }

    #[test]
    fn natural_session_directly_fails_back_to_closed_candidate_when_deadline_is_due() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, Some(90))),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_compaction_plans_complete_mixed_prefix() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Open, None)),
            (3, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: true,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1), probe(2, ProbeTrigger::NaturalCompaction)],
                reservation_trigger: ProbeTrigger::NaturalCompaction,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn route_change_plans_dynamic_five_provider_prefix() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Open, None)),
            (3, snapshot(CircuitState::HalfOpen, None)),
            (4, snapshot(CircuitState::Closed, None)),
            (5, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(5),
                session_recovery_epoch_baseline: 0,
                route_changed: true,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    direct(1),
                    probe(2, ProbeTrigger::RouteChanged),
                    probe(3, ProbeTrigger::RouteChanged),
                    direct(4),
                ],
                reservation_trigger: ProbeTrigger::RouteChanged,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_session_keeps_per_candidate_dispatch_trigger() {
        let mut max_open_due = snapshot(CircuitState::Open, Some(400));
        max_open_due.open_until = Some(90);
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, Some(90))),
            (2, max_open_due),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                session_recovery_epoch_baseline: 0,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1), probe(2, ProbeTrigger::MaxOpenWait)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn low_priority_route_change_is_confirmed_without_rerouting() {
        let candidates = vec![
            (2, snapshot(CircuitState::Closed, None)),
            (3, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                session_recovery_epoch_baseline: 0,
                route_changed: true,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: true,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn stable_session_suppresses_blocked_natural_target_without_observation() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, Some(90))),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let blocked_provider_ids = vec![1];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn blocked_natural_candidate_does_not_starve_later_due_candidate() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Open, Some(90))),
            (3, snapshot(CircuitState::Closed, None)),
        ];
        let blocked_provider_ids = vec![1];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(3),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(2, ProbeTrigger::NaturalMaxWait)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn compaction_filters_blocked_targets_and_stays_pending_when_none_remain() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Open, None)),
            (3, snapshot(CircuitState::Closed, None)),
        ];
        let blocked_provider_ids = vec![1];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(3),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: true,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(2, ProbeTrigger::NaturalCompaction)],
                reservation_trigger: ProbeTrigger::NaturalCompaction,
                not_triggered_provider_ids: Vec::new(),
            }
        );

        let only_blocked_candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &only_blocked_candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: true,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn route_change_confirms_without_reservation_when_all_targets_are_blocked() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let blocked_provider_ids = vec![1];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: true,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: true,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn unbound_session_keeps_blocked_prefix_before_first_usable_candidate() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let blocked_provider_ids = vec![1];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: None,
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: true,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1), direct(2)],
                reservation_trigger: ProbeTrigger::NewUnboundSession,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn unbound_session_probes_last_unblocked_open_after_blocked_closed_prefix() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
            (3, snapshot(CircuitState::Open, Some(400))),
        ];
        let blocked_provider_ids = vec![1, 2];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: None,
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![
                    direct(1),
                    direct(2),
                    probe(3, ProbeTrigger::NewUnboundSession),
                ],
                reservation_trigger: ProbeTrigger::NewUnboundSession,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn unbound_session_stays_when_every_candidate_is_account_blocked() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Open, Some(400))),
        ];
        let blocked_provider_ids = vec![1, 2];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: None,
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    blocked_provider_ids: &blocked_provider_ids,
                    ..Default::default()
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_session_directs_to_fresh_account_usage_recovery_newer_than_its_baseline() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let account_usage_epochs = vec![(1, 5)];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    provider_recovery_epochs: &account_usage_epochs,
                    blocked_provider_ids: &[],
                    session_recovery_epoch_baseline: 4,
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![direct(1)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }

    #[test]
    fn natural_session_ignores_account_usage_recovery_at_or_before_its_baseline() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let account_usage_epochs = vec![(1, 5)];

        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    provider_recovery_epochs: &account_usage_epochs,
                    blocked_provider_ids: &[],
                    session_recovery_epoch_baseline: 5,
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: vec![1],
            }
        );
    }

    #[test]
    fn account_usage_recovery_never_bypasses_open_or_half_open_circuit_rules() {
        let open_candidates = vec![
            (1, snapshot(CircuitState::Open, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        let account_usage_epochs = vec![(1, 7)];
        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &open_candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    provider_recovery_epochs: &account_usage_epochs,
                    blocked_provider_ids: &[],
                    session_recovery_epoch_baseline: 0,
                },
            ),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_ids: vec![1],
            }
        );

        let half_open_candidates = vec![
            (1, snapshot(CircuitState::HalfOpen, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe_with_account_usage(
                ProbePlannerInput {
                    ordered_candidates: &half_open_candidates,
                    bound_provider_id: Some(2),
                    session_recovery_epoch_baseline: 0,
                    route_changed: false,
                    strategy: ProviderFailbackStrategy::Natural,
                    compaction_generation_pending: false,
                    codex_compaction_pending: false,
                    request_eligible: true,
                    now_unix: 100,
                },
                AccountUsageRecoveryInput {
                    provider_recovery_epochs: &account_usage_epochs,
                    blocked_provider_ids: &[],
                    session_recovery_epoch_baseline: 0,
                },
            ),
            ProbePlannerDecision::Dispatch {
                targets: vec![probe(1, ProbeTrigger::NaturalMaxWait)],
                reservation_trigger: ProbeTrigger::NaturalMaxWait,
                not_triggered_provider_ids: Vec::new(),
            }
        );
    }
}
