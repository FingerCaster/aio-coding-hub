# Gateway Failover Route Contract

## Scenario: Change Provider Selection, Gates, Or Route Presentation

### 1. Scope / Trigger

Use this contract when changing session-bound provider selection, circuit or
rate-limit gates, `failover_max_providers_to_try`, persisted request attempts,
route projection, or the Home request-log route label. These layers share one
observable failover chain, but their counters have different meanings.

### 2. Signatures

The persisted provider limits are:

```rust
pub struct Settings {
    pub failover_max_attempts_per_provider: u32, // default 5, valid 1..=20
    pub failover_max_providers_to_try: u32,      // default 5, valid 1..=20
}
```

The frontend presentation receives the projected route and persisted attempt
count separately:

```ts
buildRequestRouteMeta({
  route: RequestLogRouteHop[] | null | undefined,
  status: number | null,
  hasFailover: boolean,
  attemptCount: number,
});
```

`RequestLogRouteHop` exposes `provider_id`, `provider_name`, `ok`, `attempts`,
`skipped`, and optional status/error/decision/reason fields.

### 3. Contracts

- Session binding owns reuse preference and ordering only. If the bound
  provider remains in the eligible candidate set but its circuit currently
  denies reuse, keep it in the list and let the later common gate decide. Clear
  the binding only when the provider is no longer eligible for that candidate
  set.
- Every eligible candidate reaches
  `failover_loop/prepare/provider_checks::run_gates`. A circuit, cooldown, or
  provider-limit denial creates one `outcome="skipped"` attempt with its stable
  error/reason data and makes zero upstream calls.
- `providers_tried` increments only after the common gates and preparation
  produce `Ready`. Therefore `failover_max_providers_to_try` caps Ready
  providers, not inspected candidates or skipped rows.
- Reaching the Ready-provider cap does not bypass the authoritative gate for
  later candidates. Later gate denials still emit skipped attempts/routes; the
  loop stops only when a later candidate itself becomes `Ready` beyond the cap.
- `attempt_count` is the number of persisted attempt rows. It may include
  retries and skipped rows, so it is not a provider count or switch count.
- The projected `route` is the source of provider-hop display. Derive
  `providerCount = route.length` and
  `transitionCount = max(providerCount - 1, 0)`; display `attempt_count`
  separately.
- When all candidates are denied by gates, return
  `GW_ALL_PROVIDERS_UNAVAILABLE` / HTTP 503 and preserve every denied provider
  in both attempts and route. Do not manufacture an upstream call to make the
  failure observable.
- Upstream 401 and 403 bodies are authentication material and must never enter
  console diagnostics, persisted attempt reasons, `attempts_json`, or
  `error_details_json`. The bounded body may remain in memory only as needed by
  existing failover/auth classification or an explicit configured HTTP retry
  rule. Serialization defensively strips a supplied 401/403 preview even when
  an earlier layer accidentally included it.
- HTTP retry content matching joins the existing error-body inspection path:
  consume the network body once, scan at most the decoded first 64 KiB, and use
  a separately bounded encoded input for gzip. A decode/read failure is an
  unmatched rule and compressed bytes must never be treated as text.
- Only an actual configured HTTP retry adds `retry_rule=<1-based index>` and an
  optional bounded single-line description to the attempt reason. Matcher
  contents, hit fragments, and response bodies are never added by this feature.
  Description `%`, `,`, and `=` delimiters are percent-escaped before joining
  the attempt-reason field format so they cannot impersonate another field.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| `failover_max_providers_to_try == 0` | Reject with `SEC_INVALID_INPUT` |
| `failover_max_providers_to_try > 20` | Reject with `SEC_INVALID_INPUT` |
| attempts per provider x providers to try > 100 | Reject with `SEC_INVALID_INPUT` |
| Eligible session-bound provider is circuit-open | Keep candidate; common gate records one skipped row |
| Candidate is gate-skipped | Zero upstream calls and no Ready-provider budget consumed |
| All candidates are gate-skipped | HTTP 503 with every candidate in attempts and route |
| Ready-provider cap is reached | Stop before the next Ready provider |
| Two Ready providers consume cap 2, then a circuit-open candidate follows | Record the third skipped attempt/route; make no third upstream call |
| Route has 3 hops and 4 attempt rows | 3 providers, 2 transitions, 4 attempts |
| Upstream 401/403 body contains a credential-like value | Keep status and safe reason, but persist/log none of the body |
| Gzip body exceeds the decoded scan prefix | Match only decoded bytes within the first 64 KiB; never scan compressed fallback bytes |

### 5. Good / Base / Bad Cases

- Good: two circuit-open candidates are skipped, then a third Ready candidate
  succeeds with `failover_max_providers_to_try = 2`; the skips do not consume
  either Ready slot.
- Base: one Ready provider and one attempt render as a direct request with zero
  provider transitions.
- Good: three gate-skipped candidates return 503, produce three route hops and
  three attempt rows, and call no upstream.
- Bad: removing a temporarily denied session-bound provider before
  `run_gates`; the request still fails quickly but loses the provider and skip
  reason from its audit trail.
- Bad: rendering four attempt rows as "switched 4 times" when they represent
  three providers, two transitions, and one retry.

### 6. Tests Required

- Unit-test selection so a temporarily denied bound provider stays in the
  candidate list while reuse selection returns no bound provider.
- Route-test all-gate-skip behavior: 503, one skipped row and route hop per
  candidate, preserved session binding, and zero upstream calls.
- Route-test that skipped candidates do not consume the Ready-provider cap,
  plus a boundary where the cap stops before the next Ready provider.
- Route-test the reverse boundary `Ready, Ready, circuit-open/cooldown` at cap
  2; the third candidate must remain visible as skipped.
- Use `SYNTHETIC_SECRET` in 401 and 403 bodies; assert console output, attempt
  serialization, and error details omit it without changing failover/auth
  classification or the recorded status.
- Keep model-discovery strict-attempt and health-neutral circuit tests passing;
  shared gate changes must not broaden those requests.
- Frontend-test provider, transition, and attempt counts with skips and retries.
- Run the full Rust library suite after shared failover selection or gate
  changes, then generated bindings, typecheck, lint, and Rust format checks.

### 7. Wrong vs Correct

#### Wrong

```rust
if !circuit.should_allow(bound_provider_id, created_at).allow {
    providers.retain(|provider| provider.id != bound_provider_id);
}
```

This makes session selection a second gate and silently drops observable
failover evidence.

#### Correct

```rust
if !circuit.should_allow(bound_provider_id, created_at).allow {
    return None; // retain the candidate; the common gate records the skip
}
```

Keep selection responsible for preference and make the common gate the single
authoritative owner of deny decisions and skipped attempts.
