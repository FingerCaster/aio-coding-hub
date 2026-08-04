//! Middleware: resolves session routing and selects providers with session binding.

use super::{MiddlewareAction, ProxyContext};
use crate::gateway::events::{decision_chain as dc, FailoverAttempt};
use crate::gateway::proxy::handler::early_error::{
    build_early_error_log_ctx, early_error_contract, force_provider_if_requested,
    push_special_setting, respond_early_error_with_enqueue, respond_invalid_cli_key_with_spawn,
    respond_provider_selection_failed_with_spawn, EarlyErrorKind,
};
use crate::gateway::proxy::handler::provider_selection::probe_planner::{
    plan_probe, ProbePlannerDecision, ProbePlannerInput,
};
use crate::gateway::proxy::handler::provider_selection::{
    resolve_session_bound_provider_id, resolve_session_routing_decision,
    select_providers_with_session_binding, ProviderSelection,
};
use crate::gateway::response_fixer;
use crate::session_manager::{SessionProbeTrigger, SessionRouteFingerprint};
use std::sync::Arc;

pub(in crate::gateway::proxy::handler) struct ProviderResolutionMiddleware;

const SESSION_ID_DIAGNOSTIC_SUFFIX_LEN: usize = 8;

impl ProviderResolutionMiddleware {
    pub(in crate::gateway::proxy::handler) async fn run<R: tauri::Runtime>(
        mut ctx: ProxyContext<R>,
    ) -> MiddlewareAction<R> {
        // --- session routing decision ---
        let decision = resolve_session_routing_decision(
            &ctx.headers,
            ctx.introspection_json.as_ref(),
            ctx.is_claude_count_tokens,
        );
        ctx.session_id = decision.session_id;
        ctx.allow_session_reuse = decision.allow_session_reuse && ctx.managed_model_route.is_none();

        // --- provider selection ---
        // Runs rusqlite queries; keep them off the async worker via the bounded
        // blocking pool (pool.get can block up to 5s under DB contention).
        let selection_result = {
            let state = ctx.state.clone();
            let cli_key = ctx.cli_key.clone();
            let session_id = ctx.session_id.clone();
            let created_at = ctx.created_at;
            let managed_provider_identity = ctx
                .managed_model_route
                .as_ref()
                .map(|route| (route.provider_id, route.provider_uuid.clone()));
            crate::blocking::run("gateway_provider_selection", move || {
                if let Some((provider_id, provider_uuid)) = managed_provider_identity {
                    let providers =
                        crate::providers::get_enabled_direct_codex_for_gateway_by_identity(
                            &state.db,
                            provider_id,
                            &provider_uuid,
                        )?
                        .into_iter()
                        .collect();
                    Ok(ProviderSelection {
                        effective_sort_mode_id: None,
                        providers,
                        bound_provider_order: None,
                        active_sort_mode_id: None,
                        session_bound_sort_mode_id: None,
                        latest_provider_order: Vec::new(),
                        route_changed: false,
                    })
                } else {
                    select_providers_with_session_binding(
                        &state,
                        &cli_key,
                        session_id.as_deref(),
                        created_at,
                    )
                }
            })
            .await
        };
        let selection = match selection_result {
            Ok(s) => s,
            Err(err) => {
                let log_ctx = build_early_error_log_ctx(&ctx);
                let special_settings_json =
                    response_fixer::special_settings_json(&ctx.special_settings);
                // A rejected cli key is the caller's fault (400); everything
                // else here is infrastructure (DB pool / blocking pool) and
                // must not be misfiled as a client error.
                let resp = if err.code() == "SEC_INVALID_INPUT" {
                    respond_invalid_cli_key_with_spawn(
                        &log_ctx,
                        special_settings_json,
                        ctx.session_id.clone(),
                        ctx.requested_model.clone(),
                        err.to_string(),
                    )
                } else {
                    respond_provider_selection_failed_with_spawn(
                        &log_ctx,
                        special_settings_json,
                        ctx.session_id.clone(),
                        ctx.requested_model.clone(),
                        err.to_string(),
                    )
                };
                return MiddlewareAction::ShortCircuit(resp);
            }
        };

        let initial_provider_ids = provider_ids(&selection.providers);
        let latest_route = SessionRouteFingerprint::new(
            selection.effective_sort_mode_id,
            selection.latest_provider_order.clone(),
        );
        let route_changed = selection.route_changed;
        ctx.effective_sort_mode_id = selection.effective_sort_mode_id;
        ctx.providers = selection.providers;

        // --- forced provider ---
        let forced_provider_missing = force_provider_if_requested(
            &mut ctx.providers,
            ctx.forced_provider_id,
            &ctx.special_settings,
        );

        // --- session bound provider ---
        ctx.session_bound_provider_id = resolve_session_bound_provider_id(
            ctx.state.session.as_ref(),
            ctx.state.circuit.as_ref(),
            &ctx.cli_key,
            ctx.session_id.as_deref(),
            ctx.created_at,
            ctx.allow_session_reuse,
            ctx.forced_provider_id,
            &mut ctx.providers,
            selection.bound_provider_order.as_deref(),
        );

        // --- no enabled provider guard ---
        if ctx.providers.is_empty() {
            let final_provider_ids = provider_ids(&ctx.providers);

            push_special_setting(
                &ctx.special_settings,
                no_enabled_provider_diagnostic(&NoEnabledProviderDiagnosticArgs {
                    cli_key: &ctx.cli_key,
                    active_sort_mode_id: selection.active_sort_mode_id,
                    effective_sort_mode_id: ctx.effective_sort_mode_id,
                    session_bound_sort_mode_id: selection.session_bound_sort_mode_id,
                    session_id: ctx.session_id.as_deref(),
                    session_bound_provider_id: ctx.session_bound_provider_id,
                    forced_provider_id: ctx.forced_provider_id,
                    initial_provider_ids: &initial_provider_ids,
                    final_provider_ids: &final_provider_ids,
                    forced_provider_missing,
                }),
            );
            let contract = early_error_contract(EarlyErrorKind::NoEnabledProvider);
            let message = no_enabled_provider_message(&ctx.cli_key);
            let session_id = ctx.session_id.take();
            let requested_model = ctx.requested_model.take();
            let special_settings_json =
                response_fixer::special_settings_json(&ctx.special_settings);
            let log_ctx = build_early_error_log_ctx(&ctx);

            let resp = respond_early_error_with_enqueue(
                &log_ctx,
                contract,
                message,
                special_settings_json,
                session_id,
                requested_model,
            )
            .await;
            return MiddlewareAction::ShortCircuit(resp);
        }

        plan_request_failback(&mut ctx, &latest_route, route_changed);

        MiddlewareAction::Continue(Box::new(ctx))
    }
}

fn plan_request_failback<R: tauri::Runtime>(
    ctx: &mut ProxyContext<R>,
    latest_route: &SessionRouteFingerprint,
    route_changed: bool,
) {
    let Some(runtime_settings) = ctx.runtime_settings.as_ref() else {
        return;
    };
    let session_snapshot = ctx.session_id.as_deref().and_then(|session_id| {
        ctx.state
            .session
            .routing_snapshot(&ctx.cli_key, session_id, ctx.created_at)
    });
    // Reuse resolution already validated the persisted binding against the
    // current candidates and circuit. An OPEN binding remains persisted for
    // diagnostics, but it is not a stable provider for probe planning.
    let bound_provider_id = ctx.session_bound_provider_id;
    let compaction_generation = session_snapshot.as_ref().and_then(|snapshot| {
        (snapshot.completed_compaction_generation > snapshot.consumed_compaction_generation)
            .then_some(snapshot.completed_compaction_generation)
    });
    let codex_compaction_pending = ctx
        .codex_compaction_fingerprint
        .as_ref()
        .is_some_and(|value| {
            session_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.last_codex_compaction_fingerprint.as_deref())
                != Some(value.as_str())
        });
    let request_eligible = ctx.observe_request
        && is_model_generation_request(&ctx.cli_key, &ctx.req_method, &ctx.forwarded_path)
        && !ctx.is_claude_count_tokens
        && !ctx.is_codex_model_discovery
        && !ctx.is_compact_request
        && !ctx.is_warmup_request
        && !ctx.provider_health_neutral
        && ctx.managed_model_route.is_none()
        && ctx.forced_provider_id.is_none()
        && ctx.providers.len() > 1;

    let ordered_candidates: Vec<_> = latest_route
        .provider_order
        .iter()
        .filter(|provider_id| {
            ctx.providers
                .iter()
                .any(|provider| provider.id == **provider_id)
        })
        .map(|provider_id| {
            (
                *provider_id,
                ctx.state.circuit.snapshot(*provider_id, ctx.created_at),
            )
        })
        .collect();
    let all_open_recovery_provider_ids =
        all_open_recovery_provider_ids(request_eligible, bound_provider_id, &ordered_candidates);
    let decision = plan_probe(ProbePlannerInput {
        ordered_candidates: &ordered_candidates,
        bound_provider_id,
        route_changed,
        strategy: runtime_settings.provider_failback_strategy,
        compaction_generation_pending: compaction_generation.is_some(),
        codex_compaction_pending,
        request_eligible,
        now_unix: ctx.created_at,
    });

    match decision {
        ProbePlannerDecision::Stay {
            confirm_route,
            not_triggered_provider_id,
        } => {
            if let Some(provider_id) = not_triggered_provider_id {
                if let Some(provider) = ctx
                    .providers
                    .iter()
                    .find(|provider| provider.id == provider_id)
                {
                    let snapshot = ctx.state.circuit.snapshot(provider_id, ctx.created_at);
                    ctx.probe_observations.push(not_triggered_probe_observation(
                        provider.id,
                        &provider.name,
                        provider.base_urls.first().map(String::as_str).unwrap_or(""),
                        &snapshot,
                        ctx.started.elapsed().as_millis(),
                    ));
                }
            }
            if confirm_route {
                if let Some(session_id) = ctx.session_id.as_deref() {
                    ctx.state.session.confirm_route(
                        &ctx.cli_key,
                        session_id,
                        latest_route,
                        ctx.created_at,
                    );
                }
            }
        }
        ProbePlannerDecision::DirectClosed {
            provider_id,
            trigger,
        } => {
            let reservation =
                reserve_planner_trigger(ctx, trigger, latest_route, compaction_generation);
            let Some(reservation) = reservation else {
                return;
            };
            move_provider_to_front(&mut ctx.providers, provider_id);
            ctx.dispatch_intent = Some(Arc::new(
                crate::gateway::proxy::dispatch::RequestDispatchIntent::new(
                    provider_id,
                    None,
                    reservation,
                )
                .with_durable_persistence(ctx.state.db.clone()),
            ));
        }
        ProbePlannerDecision::Probe {
            provider_id,
            trigger,
        } => {
            let reservation =
                reserve_planner_trigger(ctx, trigger, latest_route, compaction_generation);
            let Some(reservation) = reservation else {
                return;
            };
            move_provider_to_front(&mut ctx.providers, provider_id);
            let intent = if trigger == crate::circuit_breaker::ProbeTrigger::NewUnboundSession {
                all_open_recovery_provider_ids
                    .filter(|provider_ids| provider_ids.first() == Some(&provider_id))
                    .map(|provider_ids| {
                        crate::gateway::proxy::dispatch::RequestDispatchIntent::new_all_open_recovery(
                            provider_id,
                            provider_ids.into_iter().skip(1).collect(),
                            trigger,
                        )
                    })
            } else {
                None
            }
            .unwrap_or_else(|| {
                crate::gateway::proxy::dispatch::RequestDispatchIntent::new(
                    provider_id,
                    Some(trigger),
                    reservation,
                )
            });
            ctx.dispatch_intent = Some(Arc::new(
                intent.with_durable_persistence(ctx.state.db.clone()),
            ));
        }
    }
}

fn all_open_recovery_provider_ids(
    request_eligible: bool,
    bound_provider_id: Option<i64>,
    ordered_candidates: &[(i64, crate::circuit_breaker::CircuitSnapshot)],
) -> Option<Vec<i64>> {
    (request_eligible
        && bound_provider_id.is_none()
        && ordered_candidates.len() > 1
        && ordered_candidates.iter().all(|(_, snapshot)| {
            matches!(
                snapshot.state,
                crate::circuit_breaker::CircuitState::Open
                    | crate::circuit_breaker::CircuitState::HalfOpen
            )
        }))
    .then(|| {
        ordered_candidates
            .iter()
            .map(|(provider_id, _)| *provider_id)
            .collect()
    })
}

fn not_triggered_probe_observation(
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    snapshot: &crate::circuit_breaker::CircuitSnapshot,
    elapsed_ms: u128,
) -> FailoverAttempt {
    let recover_at = [
        snapshot.natural_probe_due_at,
        snapshot.open_until,
        snapshot.next_probe_at,
        snapshot.cooldown_until,
    ]
    .into_iter()
    .flatten()
    .min();

    FailoverAttempt {
        provider_id,
        provider_name: provider_name.to_string(),
        base_url: base_url.to_string(),
        outcome: "skipped".to_string(),
        status: None,
        provider_index: None,
        retry_index: None,
        session_reuse: None,
        error_category: None,
        error_code: None,
        decision: Some("skip"),
        reason: Some("natural failback conditions not met".to_string()),
        selection_method: Some(dc::SELECTION_METHOD_FILTERED),
        reason_code: None,
        attempt_started_ms: Some(elapsed_ms),
        attempt_duration_ms: Some(0),
        circuit_state_before: Some(snapshot.state.as_str()),
        circuit_state_after: Some(snapshot.state.as_str()),
        circuit_failure_count: Some(snapshot.failure_count),
        circuit_failure_threshold: Some(snapshot.failure_threshold),
        probe: Some(false),
        probe_trigger: None,
        probe_result: Some("not_triggered"),
        probe_generation: None,
        circuit_recover_at_unix: recover_at,
        circuit_trigger_error_code: snapshot.last_trigger_error_code,
        provider_bridged: None,
        timeout_secs: None,
        requested_upstream_model: None,
    }
}

fn is_model_generation_request(
    cli_key: &str,
    method: &axum::http::Method,
    forwarded_path: &str,
) -> bool {
    if *method != axum::http::Method::POST {
        return false;
    }
    let path = forwarded_path.trim_end_matches('/');
    match cli_key {
        "claude" => path == "/v1/messages",
        "codex" => matches!(path, "/responses" | "/v1/responses" | "/v1/codex/responses"),
        "gemini" => {
            matches!(
                path,
                "/v1internal:generateContent" | "/v1internal:streamGenerateContent"
            ) || ((path.starts_with("/v1/models/") || path.starts_with("/v1beta/models/"))
                && (path.ends_with(":generateContent") || path.ends_with(":streamGenerateContent")))
        }
        "grok" => matches!(
            path,
            "/responses" | "/v1/responses" | "/chat/completions" | "/v1/chat/completions"
        ),
        _ => false,
    }
}

fn reserve_planner_trigger<R: tauri::Runtime>(
    ctx: &ProxyContext<R>,
    trigger: crate::circuit_breaker::ProbeTrigger,
    latest_route: &SessionRouteFingerprint,
    compaction_generation: Option<u64>,
) -> Option<Option<crate::session_manager::SessionTriggerReservation>> {
    match trigger {
        crate::circuit_breaker::ProbeTrigger::RouteChanged => {
            reserve_trigger(ctx, SessionProbeTrigger::RouteChanged(latest_route.clone()))
        }
        crate::circuit_breaker::ProbeTrigger::NaturalCompaction => {
            if let Some(fingerprint) = ctx.codex_compaction_fingerprint.clone() {
                reserve_trigger(
                    ctx,
                    SessionProbeTrigger::CodexCompactionFingerprint {
                        fingerprint,
                        pending_generation: compaction_generation,
                    },
                )
            } else if let Some(generation) = compaction_generation {
                reserve_trigger(ctx, SessionProbeTrigger::CompactionGeneration(generation))
            } else {
                None
            }
        }
        _ => Some(None),
    }
}

fn reserve_trigger<R: tauri::Runtime>(
    ctx: &ProxyContext<R>,
    trigger: SessionProbeTrigger,
) -> Option<Option<crate::session_manager::SessionTriggerReservation>> {
    let session_id = ctx.session_id.as_deref()?;
    ctx.state
        .session
        .try_reserve_probe_trigger(&ctx.cli_key, session_id, trigger, ctx.created_at)
        .map(Some)
}

fn move_provider_to_front(
    providers: &mut [crate::providers::ProviderForGateway],
    provider_id: i64,
) {
    if let Some(index) = providers
        .iter()
        .position(|provider| provider.id == provider_id)
    {
        providers[..=index].rotate_right(1);
    }
}

pub(in crate::gateway::proxy::handler) fn no_enabled_provider_message(cli_key: &str) -> String {
    format!("no enabled provider for cli_key={cli_key}")
}

struct NoEnabledProviderDiagnosticArgs<'a> {
    cli_key: &'a str,
    active_sort_mode_id: Option<i64>,
    effective_sort_mode_id: Option<i64>,
    session_bound_sort_mode_id: Option<Option<i64>>,
    session_id: Option<&'a str>,
    session_bound_provider_id: Option<i64>,
    forced_provider_id: Option<i64>,
    initial_provider_ids: &'a [i64],
    final_provider_ids: &'a [i64],
    forced_provider_missing: bool,
}

fn no_enabled_provider_diagnostic(args: &NoEnabledProviderDiagnosticArgs<'_>) -> serde_json::Value {
    let sort_mode = match args.effective_sort_mode_id {
        Some(id) => serde_json::json!({"kind": "custom", "modeId": id}),
        None => serde_json::json!({"kind": "default", "modeId": serde_json::Value::Null}),
    };
    let cleared_reason = if args.forced_provider_missing {
        "forced_provider_not_in_candidates"
    } else if args.effective_sort_mode_id.is_some() {
        "empty_sort_mode_candidates"
    } else {
        "empty_default_candidates"
    };

    serde_json::json!({
        "type": "provider_selection_diagnostic",
        "scope": "request",
        "hit": true,
        "reason": "no_enabled_provider",
        "clearedReason": cleared_reason,
        "cliKey": args.cli_key,
        "sortMode": sort_mode,
        "activeSortModeId": args.active_sort_mode_id,
        "effectiveSortModeId": args.effective_sort_mode_id,
        "sessionBoundSortModeId": args.session_bound_sort_mode_id,
        "sortModeSource": if args.session_bound_sort_mode_id.is_some() {
            "session_bound"
        } else {
            "active"
        },
        "sessionIdPresent": args.session_id.is_some(),
        "sessionIdSuffix": args.session_id.map(diagnostic_session_suffix),
        "sessionBoundProviderId": args.session_bound_provider_id,
        "forcedProviderId": args.forced_provider_id,
        "forcedProviderMissing": args.forced_provider_missing,
        "candidateProviderIdsBeforeForce": args.initial_provider_ids,
        "candidateProviderCountBeforeForce": args.initial_provider_ids.len(),
        "candidateProviderIdsAfterForce": args.final_provider_ids,
        "candidateProviderCountAfterForce": args.final_provider_ids.len(),
    })
}

fn provider_ids(providers: &[crate::providers::ProviderForGateway]) -> Vec<i64> {
    providers.iter().map(|provider| provider.id).collect()
}

fn diagnostic_session_suffix(session_id: &str) -> String {
    let suffix: Vec<char> = session_id
        .chars()
        .rev()
        .take(SESSION_ID_DIAGNOSTIC_SUFFIX_LEN)
        .collect();
    suffix.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_snapshot() -> crate::circuit_breaker::CircuitSnapshot {
        crate::circuit_breaker::CircuitSnapshot {
            state: crate::circuit_breaker::CircuitState::Open,
            failure_count: 5,
            failure_threshold: 5,
            open_until: Some(130),
            cooldown_until: None,
            probe_reference_at: Some(100),
            next_probe_at: Some(120),
            natural_probe_due_at: Some(400),
            recovery_guard_until: None,
            probe_in_flight: false,
            state_revision: 7,
            last_trigger_error_code: Some("GW_UPSTREAM_5XX"),
        }
    }

    #[test]
    fn all_open_recovery_requires_an_unbound_eligible_route_with_no_closed_fallback() {
        let first = open_snapshot();
        let second = open_snapshot();
        let all_open = vec![(11, first.clone()), (22, second.clone())];

        assert_eq!(
            all_open_recovery_provider_ids(true, None, &all_open),
            Some(vec![11, 22])
        );
        assert_eq!(
            all_open_recovery_provider_ids(true, Some(22), &all_open),
            None
        );
        assert_eq!(all_open_recovery_provider_ids(false, None, &all_open), None);

        let mut closed = second;
        closed.state = crate::circuit_breaker::CircuitState::Closed;
        assert_eq!(
            all_open_recovery_provider_ids(true, None, &[(11, first), (22, closed)]),
            None
        );
    }

    #[test]
    fn not_triggered_observation_is_structured_and_unnumbered() {
        let attempt = not_triggered_probe_observation(
            11,
            "preferred",
            "https://preferred.example",
            &open_snapshot(),
            9,
        );

        assert_eq!(attempt.outcome, "skipped");
        assert_eq!(attempt.decision, Some("skip"));
        assert_eq!(attempt.probe, Some(false));
        assert_eq!(attempt.probe_result, Some("not_triggered"));
        assert_eq!(attempt.provider_index, None);
        assert_eq!(attempt.retry_index, None);
        assert_eq!(attempt.circuit_recover_at_unix, Some(120));

        let value = serde_json::to_value(&attempt).expect("serialize observation");
        assert_eq!(
            value.get("probe_result"),
            Some(&serde_json::json!("not_triggered"))
        );
        assert_eq!(value.get("probe"), Some(&serde_json::json!(false)));
        assert_eq!(value.get("provider_index"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("retry_index"), Some(&serde_json::Value::Null));

        let mut closed_pending = open_snapshot();
        closed_pending.state = crate::circuit_breaker::CircuitState::Closed;
        closed_pending.failure_count = 1;
        closed_pending.open_until = None;
        closed_pending.next_probe_at = None;
        let closed_attempt = not_triggered_probe_observation(
            11,
            "preferred",
            "https://preferred.example",
            &closed_pending,
            9,
        );
        assert_eq!(closed_attempt.circuit_state_before, Some("CLOSED"));
        assert_eq!(closed_attempt.circuit_recover_at_unix, Some(400));
    }

    #[test]
    fn no_enabled_provider_message_preserves_cli_key() {
        assert_eq!(
            no_enabled_provider_message("codex"),
            "no enabled provider for cli_key=codex"
        );
    }

    #[test]
    fn probe_eligibility_accepts_only_known_model_generation_paths() {
        let eligible = [
            ("claude", "/v1/messages"),
            ("claude", "/v1/messages/"),
            ("codex", "/responses"),
            ("codex", "/v1/responses"),
            ("codex", "/v1/codex/responses/"),
            ("gemini", "/v1beta/models/gemini-2.5-pro:generateContent"),
            ("gemini", "/v1/models/gemini-2.5-pro:streamGenerateContent"),
            ("gemini", "/v1internal:generateContent"),
            ("grok", "/v1/responses"),
            ("grok", "/v1/chat/completions"),
        ];
        for (cli_key, path) in eligible {
            assert!(
                is_model_generation_request(cli_key, &axum::http::Method::POST, path),
                "expected eligible path: {cli_key} {path}"
            );
        }

        let ineligible = [
            ("claude", "/v1/messages/count_tokens"),
            ("claude", "/v1/ping"),
            ("codex", "/v1/responses/compact"),
            ("codex", "/v1/models"),
            ("codex", "/v1/ping"),
            ("gemini", "/v1beta/models/gemini-2.5-pro:countTokens"),
            ("gemini", "/v1beta/models"),
            ("grok", "/v1/models"),
            ("grok", "/v1/ping"),
            ("unknown", "/v1/responses"),
        ];
        for (cli_key, path) in ineligible {
            assert!(
                !is_model_generation_request(cli_key, &axum::http::Method::POST, path),
                "expected fail-closed path: {cli_key} {path}"
            );
        }
        assert!(!is_model_generation_request(
            "claude",
            &axum::http::Method::GET,
            "/v1/messages"
        ));
    }

    #[test]
    fn no_enabled_provider_diagnostic_marks_empty_active_candidates() {
        let value = no_enabled_provider_diagnostic(&NoEnabledProviderDiagnosticArgs {
            cli_key: "claude",
            active_sort_mode_id: Some(6),
            effective_sort_mode_id: Some(6),
            session_bound_sort_mode_id: None,
            session_id: Some("01234567-89ab-cdef-0123-456789abcdef"),
            session_bound_provider_id: None,
            forced_provider_id: None,
            initial_provider_ids: &[],
            final_provider_ids: &[],
            forced_provider_missing: false,
        });

        assert_eq!(
            value.get("type").and_then(|v| v.as_str()),
            Some("provider_selection_diagnostic")
        );
        assert_eq!(
            value.get("clearedReason").and_then(|v| v.as_str()),
            Some("empty_sort_mode_candidates")
        );
        assert_eq!(
            value.pointer("/sortMode/kind").and_then(|v| v.as_str()),
            Some("custom")
        );
        assert_eq!(
            value.get("activeSortModeId").and_then(|v| v.as_i64()),
            Some(6)
        );
        assert_eq!(
            value.get("effectiveSortModeId").and_then(|v| v.as_i64()),
            Some(6)
        );
        assert_eq!(
            value.get("sortModeSource").and_then(|v| v.as_str()),
            Some("active")
        );
        assert_eq!(
            value.get("sessionIdPresent").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            value.get("sessionIdSuffix").and_then(|v| v.as_str()),
            Some("89abcdef")
        );
        assert!(!value.to_string().contains("01234567-89ab-cdef"));
        assert_eq!(
            value
                .get("candidateProviderCountBeforeForce")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
    }

    #[test]
    fn no_enabled_provider_diagnostic_marks_forced_provider_missing() {
        let value = no_enabled_provider_diagnostic(&NoEnabledProviderDiagnosticArgs {
            cli_key: "claude",
            active_sort_mode_id: Some(7),
            effective_sort_mode_id: None,
            session_bound_sort_mode_id: Some(None),
            session_id: None,
            session_bound_provider_id: Some(11),
            forced_provider_id: Some(99),
            initial_provider_ids: &[11, 22],
            final_provider_ids: &[],
            forced_provider_missing: true,
        });

        assert_eq!(
            value.get("clearedReason").and_then(|v| v.as_str()),
            Some("forced_provider_not_in_candidates")
        );
        assert_eq!(
            value.pointer("/sortMode/kind").and_then(|v| v.as_str()),
            Some("default")
        );
        assert_eq!(
            value.get("activeSortModeId").and_then(|v| v.as_i64()),
            Some(7)
        );
        assert_eq!(
            value.get("sortModeSource").and_then(|v| v.as_str()),
            Some("session_bound")
        );
        assert_eq!(
            value
                .get("candidateProviderIdsBeforeForce")
                .and_then(|v| v.as_array())
                .map(|items| items.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>()),
            Some(vec![11, 22])
        );
        assert_eq!(
            value.get("forcedProviderId").and_then(|v| v.as_i64()),
            Some(99)
        );
        assert_eq!(
            value.get("forcedProviderMissing").and_then(|v| v.as_bool()),
            Some(true)
        );
    }
}
