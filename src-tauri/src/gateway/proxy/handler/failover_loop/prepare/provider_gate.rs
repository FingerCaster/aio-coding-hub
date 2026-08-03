//! Usage: Provider gating helpers (circuit allow/skip + event emission).

use super::context::CommonCtx;
use crate::circuit_breaker;
use crate::gateway::proxy::provider_router;
use crate::gateway::util::now_unix_seconds;

pub(super) struct ProviderGateInput<'a, R: tauri::Runtime = tauri::Wry> {
    pub(super) ctx: CommonCtx<'a, R>,
    pub(super) provider_id: i64,
    pub(super) provider_name_base: &'a String,
    pub(super) provider_base_url_display: &'a String,
    pub(super) earliest_available_unix: &'a mut Option<i64>,
    pub(super) skipped_open: &'a mut usize,
    pub(super) skipped_cooldown: &'a mut usize,
    /// Filled with the circuit snapshot when the gate denies (see
    /// `provider_router::GateProviderArgs::deny_snapshot`).
    pub(super) deny_snapshot: &'a mut Option<circuit_breaker::CircuitSnapshot>,
    pub(super) probe_skip: &'a mut Option<provider_router::ProbeGateSkip>,
    pub(super) dispatch_intent:
        Option<&'a std::sync::Arc<crate::gateway::proxy::dispatch::RequestDispatchIntent>>,
}

pub(super) struct ProviderGateAllow {
    pub(super) circuit_after: circuit_breaker::CircuitSnapshot,
    pub(super) dispatch_ownership:
        Option<std::sync::Arc<crate::gateway::proxy::dispatch::ProviderDispatchOwnership>>,
}

pub(super) fn gate_provider<R: tauri::Runtime>(
    input: ProviderGateInput<'_, R>,
) -> Option<ProviderGateAllow> {
    let ProviderGateInput {
        ctx,
        provider_id,
        provider_name_base,
        provider_base_url_display,
        earliest_available_unix,
        skipped_open,
        skipped_cooldown,
        deny_snapshot,
        probe_skip,
        dispatch_intent,
    } = input;

    let now_unix = now_unix_seconds() as i64;
    let mut probe_token = None;
    let targeted_intent = dispatch_intent.filter(|intent| intent.targets_provider(provider_id));
    let circuit_after = provider_router::gate_provider(provider_router::GateProviderArgs {
        app: Some(&ctx.state.app),
        circuit: ctx.state.circuit.as_ref(),
        trace_id: ctx.trace_id.as_str(),
        cli_key: ctx.cli_key.as_str(),
        provider_id,
        provider_name: provider_name_base.as_str(),
        provider_base_url_display: provider_base_url_display.as_str(),
        now_unix,
        earliest_available_unix,
        skipped_open,
        skipped_cooldown,
        deny_snapshot,
        probe_trigger: targeted_intent.and_then(|intent| intent.probe_trigger()),
        probe_token: &mut probe_token,
        probe_skip,
    });
    let Some(circuit_after) = circuit_after else {
        if let Some(intent) = targeted_intent {
            intent.release_unclaimed_reservation();
        }
        return None;
    };

    let dispatch_ownership = targeted_intent.and_then(|intent| {
        let probe_guard = probe_token
            .map(|token| circuit_breaker::ProbeLeaseGuard::new(ctx.state.circuit.clone(), token));
        intent.claim_for_provider(provider_id, probe_guard)
    });
    if targeted_intent.is_some() && dispatch_ownership.is_none() {
        return None;
    }

    Some(ProviderGateAllow {
        circuit_after,
        dispatch_ownership,
    })
}
