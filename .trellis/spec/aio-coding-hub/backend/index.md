# AIO Coding Hub Backend Specs

Rules for the root application's Rust backend and local gateway runtime.

## Topics

- [Gateway attempt budget contract](./gateway-attempt-budget-contract.md):
  per-request provider attempts, reserved internal retries, strict model
  discovery, and cross-request circuit-breaker accounting.
- [Codex managed model route contract](../cross-layer/codex-managed-model-route-contract.md):
  readable profile aliases plus legacy UUID lookup, complete picker catalog
  lifecycle, one-provider routing, same-provider retry, and terminal
  wire-vs-observed route evidence.

## Pre-Development Checklist

When changing gateway retry or circuit behavior:

1. Read [Gateway attempt budget contract](./gateway-attempt-budget-contract.md).
2. Identify whether each counter is request-scoped or persisted across requests.
3. Trace the effective provider retry policy, including provider overrides.
4. Keep strict helper routes explicit instead of relying on shared retry math.

When changing managed Codex alias routing or model-route detection:

1. Read [Codex managed model route contract](../cross-layer/codex-managed-model-route-contract.md).
2. Keep the managed provider as the only candidate while preserving common
   gates and same-provider retry.
3. Prove later terminal matched/unobserved evidence cannot leave a stale severe
   mapping from an earlier attempt.

## Quality Check

- Unit-test the attempt-budget calculation at its boundary values.
- Run route-level tests that exercise real provider retries and failover.
- Verify circuit failure counts across multiple requests.
- Run the full Rust suite after changing shared failover-loop inputs.
- Route-test managed and ordinary Codex requests together after changing
  provider selection, final wire-model tracking, or response observation.
