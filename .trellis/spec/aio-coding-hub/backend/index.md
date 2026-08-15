# AIO Coding Hub Backend Specs

Rules for the root application's Rust backend and local gateway runtime.

## Topics

- [Gateway attempt budget contract](./gateway-attempt-budget-contract.md):
  per-request provider attempts, reserved internal retries, typed nested
  first-byte ownership, strict model discovery, and cross-request circuit-
  breaker accounting.
- [Codex request content-encoding contract](./codex-request-content-encoding-contract.md):
  bounded decoding at the gateway boundary, supported HTTP encodings, identity
  forwarding, and local failure classification.
- [Upstream error handling contract](../cross-layer/upstream-error-handling-contract.md):
  configured HTTP/transport/stream retries, terminal HTTP rewrites, and bounded
  request-log evidence.
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
5. Derive response-header and SSE first-chunk budgets from the same confirmed
   attempt target; local reentry must not disable other timeout families.

When changing managed Codex alias routing or model-route detection:

1. Read [Codex managed model route contract](../cross-layer/codex-managed-model-route-contract.md).
2. Keep the managed provider as the only candidate while preserving common
   gates and same-provider retry.
3. Prove later terminal matched/unobserved evidence cannot leave a stale severe
   mapping from an earlier attempt.

When changing Codex request-body encoding:

1. Read [Codex request content-encoding contract](./codex-request-content-encoding-contract.md).
2. Keep semantic context compaction separate from HTTP request-body encoding.
3. Bound every decoded layer and preserve non-Codex transport behavior.
4. Keep decoding failures before provider selection and circuit accounting.

When changing upstream error retry or terminal response behavior:

1. Read [Upstream error handling contract](../cross-layer/upstream-error-handling-contract.md).
2. Preserve real upstream facts through retry, failover, and circuit accounting.
3. Keep stream recovery pre-commit and final HTTP rewriting terminal-only.
4. Reuse the shared configured-retry budget and backoff helper.

## Quality Check

- Unit-test the attempt-budget calculation at its boundary values.
- Run route-level tests that exercise real provider retries and failover.
- For local Gateway reentry, verify both first-byte projections are delegated
  only after typed intent plus `SelfLoop` confirmation; ordinary targets retain
  their configured budgets.
- Verify circuit failure counts across multiple requests.
- Run the full Rust suite after changing shared failover-loop inputs.
- Route-test managed and ordinary Codex requests together after changing
  provider selection, final wire-model tracking, or response observation.
- Verify supported Codex encodings arrive upstream as identity JSON, while
  invalid or oversized encoded bodies make zero upstream attempts.
- Verify Codex stream guard/backoff with paused time and final rewrite across
  success-after-failure, all-Provider failure, fake-200, and probe paths.
