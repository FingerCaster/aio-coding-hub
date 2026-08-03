use crate::circuit_breaker::{CircuitSnapshot, CircuitState, ProbeTrigger};
use crate::settings::ProviderFailbackStrategy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::gateway::proxy::handler) enum ProbePlannerDecision {
    Stay {
        confirm_route: bool,
        not_triggered_provider_id: Option<i64>,
    },
    DirectClosed {
        provider_id: i64,
        trigger: ProbeTrigger,
    },
    Probe {
        provider_id: i64,
        trigger: ProbeTrigger,
    },
}

pub(in crate::gateway::proxy::handler) struct ProbePlannerInput<'a> {
    pub ordered_candidates: &'a [(i64, CircuitSnapshot)],
    pub bound_provider_id: Option<i64>,
    pub route_changed: bool,
    pub strategy: ProviderFailbackStrategy,
    pub compaction_generation_pending: bool,
    pub codex_compaction_pending: bool,
    pub request_eligible: bool,
    pub now_unix: i64,
}

pub(in crate::gateway::proxy::handler) fn plan_probe(
    input: ProbePlannerInput<'_>,
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
            not_triggered_provider_id: None,
        };
    }
    if !input.request_eligible {
        return ProbePlannerDecision::Stay {
            confirm_route: false,
            not_triggered_provider_id: None,
        };
    }

    let explicit_trigger = if input.bound_provider_id.is_none() {
        Some(ProbeTrigger::NewUnboundSession)
    } else if input.route_changed {
        Some(ProbeTrigger::RouteChanged)
    } else if input.codex_compaction_pending || input.compaction_generation_pending {
        Some(ProbeTrigger::NaturalCompaction)
    } else {
        None
    };

    let candidate =
        if explicit_trigger.is_some() || input.strategy == ProviderFailbackStrategy::Aggressive {
            higher.first()
        } else {
            // Natural deadline triggers may only inspect the highest-priority open
            // candidate. A not-yet-due P1 must not be bypassed to probe a due P2.
            higher.iter().find(|(_, snapshot)| {
                matches!(snapshot.state, CircuitState::Open | CircuitState::HalfOpen)
            })
        };

    let Some((provider_id, snapshot)) = candidate else {
        return ProbePlannerDecision::Stay {
            confirm_route: false,
            not_triggered_provider_id: higher.first().map(|(provider_id, _)| *provider_id),
        };
    };
    let trigger = explicit_trigger.or_else(|| {
        if snapshot
            .natural_probe_due_at
            .is_some_and(|deadline| input.now_unix >= deadline)
        {
            Some(ProbeTrigger::NaturalMaxWait)
        } else if input.strategy == ProviderFailbackStrategy::Aggressive {
            Some(ProbeTrigger::AggressiveTurn)
        } else if snapshot
            .open_until
            .is_some_and(|deadline| input.now_unix >= deadline)
        {
            Some(ProbeTrigger::MaxOpenWait)
        } else {
            None
        }
    });

    if let Some(trigger) = trigger {
        return match snapshot.state {
            CircuitState::Closed => ProbePlannerDecision::DirectClosed {
                provider_id: *provider_id,
                trigger,
            },
            CircuitState::Open | CircuitState::HalfOpen => ProbePlannerDecision::Probe {
                provider_id: *provider_id,
                trigger,
            },
        };
    }

    ProbePlannerDecision::Stay {
        confirm_route: false,
        not_triggered_provider_id: Some(*provider_id),
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
            probe_in_flight: state == CircuitState::HalfOpen,
            state_revision: 1,
            last_trigger_error_code: None,
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
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_id: Some(1),
            }
        );
    }

    #[test]
    fn natural_session_does_not_bypass_highest_open_candidate_for_due_lower_open() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Open, Some(90))),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_id: Some(1),
            }
        );
    }

    #[test]
    fn natural_session_probes_highest_open_candidate_when_it_is_due() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(90))),
            (2, snapshot(CircuitState::Open, Some(80))),
            (3, snapshot(CircuitState::Closed, None)),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(3),
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Probe {
                provider_id: 1,
                trigger: ProbeTrigger::NaturalMaxWait,
            }
        );
    }

    #[test]
    fn invalid_stable_session_probes_first_open_as_new_unbound() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, Some(400))),
            (2, snapshot(CircuitState::Open, Some(400))),
        ];

        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: None,
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Probe {
                provider_id: 1,
                trigger: ProbeTrigger::NewUnboundSession,
            }
        );
    }

    #[test]
    fn aggressive_session_probes_first_open_candidate() {
        let candidates = vec![
            (1, snapshot(CircuitState::Open, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                route_changed: false,
                strategy: ProviderFailbackStrategy::Aggressive,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Probe {
                provider_id: 1,
                trigger: ProbeTrigger::AggressiveTurn,
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
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: false,
                not_triggered_provider_id: Some(1),
            }
        );
    }

    #[test]
    fn natural_compaction_directly_targets_closed_candidate_and_carries_trigger() {
        let candidates = vec![
            (1, snapshot(CircuitState::Closed, None)),
            (2, snapshot(CircuitState::Closed, None)),
        ];
        assert_eq!(
            plan_probe(ProbePlannerInput {
                ordered_candidates: &candidates,
                bound_provider_id: Some(2),
                route_changed: false,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: true,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::DirectClosed {
                provider_id: 1,
                trigger: ProbeTrigger::NaturalCompaction,
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
                route_changed: true,
                strategy: ProviderFailbackStrategy::Natural,
                compaction_generation_pending: false,
                codex_compaction_pending: false,
                request_eligible: true,
                now_unix: 100,
            }),
            ProbePlannerDecision::Stay {
                confirm_route: true,
                not_triggered_provider_id: None,
            }
        );
    }
}
