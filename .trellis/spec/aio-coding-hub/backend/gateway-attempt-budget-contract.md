# Gateway Attempt Budget Contract

## Scenario: Change Provider Retry Or Circuit Behavior

### 1. Scope / Trigger

Use this contract when changing provider attempt limits, OAuth reactive refresh,
Codex `previous_response_id` repair, transient upstream retries, Codex model
discovery, or circuit-breaker thresholds. These controls share the failover
loop, but they do not share one lifecycle.

### 2. Signatures

The persisted settings are:

```rust
pub struct Settings {
    pub failover_max_attempts_per_provider: u32, // default 5, valid 1..=20
    pub circuit_breaker_failure_threshold: u32, // default 5, valid 1..=50
}
```

The request-scoped calculation is owned by
`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs`:

```rust
fn provider_max_attempts_for_request(
    configured_max_attempts: u32,
    needs_oauth_reactive_refresh_retry: bool,
    needs_codex_previous_response_id_retry: bool,
    configured_transient_retries: u32,
    strict_configured_limit: bool,
) -> u32;
```

### 3. Contracts

- For a normal request, the effective per-provider budget is:

  ```text
  max(
    failover_max_attempts_per_provider,
    1 + oauth_refresh + previous_response_id_repair + enabled_transient_retries
  )
  ```

  Each boolean reservation contributes `1`. Transient retries contribute the
  effective policy's `max_retries` only when that policy is enabled.
- Resolve a provider's `upstream_retry_policy_override` before reserving
  transient retry capacity. A disabled effective policy reserves zero attempts,
  even if its stored `max_retries` is non-zero.
- Configured transient capacity is policy-scoped, not rule-scoped. An enabled
  HTTP rule match (status-only or status plus decoded-body content) and an
  enabled transport match share the same `max_retries` reservation, backoff,
  and circuit-accounting settings; rules do not add independent capacity.
- `max_retries` counts actual configured HTTP/transport retries for the current
  Provider, not the total attempt index. OAuth refresh, auth fallback,
  `previous_response_id` repair, thinking rectifiers, and generic baseline
  retries do not consume this counter; switching Provider resets it.
- The transient reservation is available only after an HTTP/transport matcher
  succeeds. An unmatched 5xx, 408/429, or other generic base decision is capped
  by the same calculation with `configured_transient_retries = 0`; it cannot
  consume the extra configured-retry slots.
- `circuit_breaker_failure_threshold` is not an input to the request budget.
  Circuit failures accumulate across requests; the threshold must never enlarge
  one request's configured attempt count.
- Codex model discovery is strict: its caller supplies a one-attempt provider
  limit and the strict path does not add OAuth, `previous_response_id`, or transient
  reservations. Discovery may still fail over and try another provider once.
- Grok and Codex Responses requests reserve the same single internal
  `previous_response_id` repair. A matching 400/404 removes the field and
  retries once; the repair cannot recurse or create a third request.
- The configured attempt limit is a user-facing baseline. Request-scoped internal
  recovery may raise the effective budget only through the explicit formula
  above; no other subsystem may add implicit capacity.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| `failover_max_attempts_per_provider == 0` | Reject with `SEC_INVALID_INPUT` |
| `failover_max_attempts_per_provider > 20` | Reject with `SEC_INVALID_INPUT` |
| attempts per provider x providers to try > 100 | Reject with `SEC_INVALID_INPUT` |
| effective transient policy disabled | Reserve zero transient attempts |
| OAuth and `previous_response_id` both applicable at configured limit 1 | Effective budget is 3 |
| circuit threshold greater than configured attempts | Do not change this request's budget |
| Codex model discovery | Exactly one attempt per provider |
| circuit threshold outside `1..=50` | Reject independently of attempt-limit validation |

### 5. Good / Base / Bad Cases

- Good: configured attempts `1`, OAuth enabled, a non-empty
  `previous_response_id`, and two enabled transient retries produce an effective
  per-provider budget of `5`.
- Base: configured attempts `5` with no retry reasons remain `5`.
- Good: five failures across separate one-attempt requests can open a circuit
  whose threshold is `5`.
- Bad: a circuit threshold of `5` silently changes a configured one-attempt
  request into five attempts.
- Bad: a disabled provider retry override still reserves its stored retry count.

### 6. Tests Required

- Unit-test `provider_max_attempts_for_request` for no-retry, OAuth-only,
  `previous_response_id`-only, combined internal retries, enabled/disabled transient
  retries, and strict-limit cases.
- Keep provider inheritance, complete override, and explicit disabled override
  tests. Provider HTTP rules replace the global rule set as part of the whole
  policy; they are never appended to it.
- Route-test a baseline of one attempt with a non-matching configured HTTP rule
  and prove the transient reservation does not create a second upstream call.
- Route-test an internal `previous_response_id` repair followed by a configured
  HTTP failure and prove the configured retry still runs within its independent
  policy counter.
- Keep an explicit regression proving configured `1` remains `1` when there is
  no retry reason, regardless of the circuit threshold.
- Route-test Codex model discovery for one call per provider, cross-provider
  failover, and health-neutral circuit snapshots.
- Route-test a real Grok Responses request against a two-response upstream:
  first continuation error, then success. Assert exactly two request bodies,
  removal on the second, final response id/usage, TTFB, and the existing 20 MiB
  non-SSE boundary.
- Persistence and frontend cross-layer tests must keep the attempt range
  `1..=20`, circuit range `1..=50`, and total-attempt cap `100` aligned.
- Run the full Rust suite after changing failover preparation. Focused budget
  tests do not expose every route helper's runtime settings or multi-request
  circuit behavior.

### 7. Wrong vs Correct

#### Wrong

```rust
let max_attempts = configured_max_attempts
    .max(circuit_failure_threshold)
    .max(1 + required_internal_retries);
```

This couples a cross-request health threshold to one request's retry budget.

#### Correct

```rust
let max_attempts = configured_max_attempts.max(1 + required_internal_retries);
```

Keep circuit accounting in the provider router and let failures accumulate
across requests.
