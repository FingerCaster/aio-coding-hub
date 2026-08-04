# Gateway Failover Route Contract

## Scenario: Change Provider Selection, Gates, Or Route Presentation

### 1. Scope / Trigger

Use this contract when changing session-bound provider selection, circuit or
rate-limit gates, `failover_max_providers_to_try`, persisted request attempts,
route projection, probe-planning observations, or the Home request-log route
label. These layers share one observable failover chain, but their counters
have different meanings.

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

Probe planning receives the effective binding produced by reuse resolution,
not the provider ID retained in the session snapshot:

```rust
ProbePlannerInput {
    bound_provider_id: ctx.session_bound_provider_id,
    // ordered candidates, strategy, triggers, and request eligibility omitted
}
```

An all-open route uses one request-scoped multi-target intent while ordinary
failback keeps the single-target constructor:

```rust
RequestDispatchIntent::new_all_open_recovery(
    first_provider_id,
    remaining_provider_ids,
    ProbeTrigger::NewUnboundSession,
);

intent.targets_provider(provider_id);
intent.claim_for_provider(provider_id, probe_guard);
```

The only persisted row that is not a provider attempt has this exact structured
shape; consumers must not infer it from `reason` text:

```rust
FailoverAttempt {
    outcome: "skipped".to_string(),
    provider_index: None,
    retry_index: None,
    probe_result: Some("not_triggered"),
    // provider_id/provider_name identify the observed higher-priority provider.
    ..
}
```

### 3. Contracts

- Session binding owns reuse preference and ordering only. If the bound
  provider remains in the eligible candidate set but its circuit currently
  denies reuse, keep it in the list and let the later common gate decide. Clear
  the binding only when the provider is no longer eligible for that candidate
  set.
- A persisted binding whose circuit denies reuse is not a stable binding for
  probe planning. Pass the circuit-validated `session_bound_provider_id` into
  `ProbePlannerInput`; do not re-read `routing_snapshot.provider_id` as the
  planner anchor. When every routed provider is `OPEN`, this makes the request
  an effective unbound request and creates one ordered
  `new_unbound_session` recovery intent containing every routed provider.
- The recovery intent does not bypass the common gate. The outer failover loop
  remains serial: each candidate calls `try_acquire_probe`, owns at most one
  provider lease for its complete retry chain, and releases/completes that
  lease before the next provider is prepared. Provider-level global
  single-flight remains authoritative.
- In an all-open recovery, cooldown and in-flight candidates make zero calls
  and advance to the next planned provider. A dispatched probe that exhausts
  its normal retry chain also advances. The first complete success stops the
  loop, closes that provider circuit, and binds the actual provider.
- If all planned providers are denied, return
  `GW_ALL_PROVIDERS_UNAVAILABLE` / HTTP 503 with every skip intact. If probes
  were dispatched but all failed, preserve the existing terminal upstream
  error contract. Never add hidden retries or exceed the configured Ready
  provider and total attempt budgets.
- The multi-target exception is valid only when every eligible routed provider
  is `OPEN`/`HALF_OPEN` at planning time. If any `CLOSED` fallback exists,
  ordinary failback still targets at most one `OPEN` provider and then uses the
  `CLOSED` fallback.
- In natural failback mode, a counted failure arms or rearms the provider-level
  `natural_probe_due_at` deadline even while the circuit remains `CLOSED`.
  Rearm from the latest counted failure, clear the deadline on complete
  success, preserve it across persisted-state reload, and recompute it from
  `probe_reference_at` when `natural_probe_max_wait_secs` changes. When loading
  a legacy `CLOSED` row with failures but no reference, recover the reference
  from its latest persisted failure timestamp rather than a later unrelated
  `updated_at` value.
- A higher-priority `CLOSED` provider participates in natural deadline planning
  only while that pending deadline exists. Before it is due, retain the current
  session binding. Once due, the next eligible real request directly targets
  that provider with `ProbeTrigger::NaturalMaxWait`; `OPEN`/`HALF_OPEN`
  candidates continue through the existing single-flight probe gate. A healthy
  `CLOSED` provider with no pending deadline must not turn natural mode into
  aggressive failback.
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
- `attempt_count` is the number of persisted provider-attempt rows. It may
  include retries and ordinary skipped rows, so it is not a provider count or
  switch count. The sole exception is
  `probe_result="not_triggered"`: that row is a planner observation retained in
  `attempts_json` for detail views, but it is not a provider attempt.
- The projected `route` is the source of provider-hop display. Derive
  `providerCount = route.length` and
  `transitionCount = max(providerCount - 1, 0)`; display `attempt_count`
  separately.
- Backend request-log summaries and live frontend projections must exclude
  `probe_result="not_triggered"` before deriving route hops, failover state,
  start/final provider presentation, and provider-attempt count. Keep the row
  available to structured probe detail UI. Ordinary `outcome="skipped"` rows
  without that exact probe result still count as attempts and route hops.
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
| Persisted P1 binding is `OPEN`, every routed provider is `OPEN`, and P1 probe cooldown is due | Treat the binding as ineffective, create an ordered `new_unbound_session` recovery intent, and let one lease winner call P1 |
| P1 probe fails and due P2 is also `OPEN` | Complete P1 failure, then serially acquire and probe P2; stop and bind P2 on complete success |
| P1 cooldown is not due but P2 is due | Record P1 `probe_result="cooldown"` with zero P1 calls, then probe P2 |
| Another request owns P1 probe but P2 is due | Record P1 `probe_result="in_flight"` with zero P1 calls from this request, then probe P2 under P2's own single-flight lease |
| Every all-open candidate is in cooldown/in-flight | HTTP 503 with structured skips and zero calls to denied candidates |
| Route has any `CLOSED` fallback | Do not create multi-target recovery; retain ordinary one-probe failback behavior |
| Higher-priority P1 is `CLOSED` after a counted failure and its natural deadline is not due | Keep the current P2 binding and make zero P1 calls |
| Higher-priority P1 is `CLOSED` with an expired natural deadline | Directly call P1 on the next eligible request; on complete success clear the deadline and bind P1 |
| Expired direct P1 failback fails while P2 is `CLOSED` | Rearm P1 from that failure, continue to P2, and retain/bind P2 when it succeeds |
| Candidate is gate-skipped | Zero upstream calls and no Ready-provider budget consumed |
| All candidates are gate-skipped | HTTP 503 with every candidate in attempts and route |
| Ready-provider cap is reached | Stop before the next Ready provider |
| Two Ready providers consume cap 2, then a circuit-open candidate follows | Record the third skipped attempt/route; make no third upstream call |
| Route has 3 hops and 4 attempt rows | 3 providers, 2 transitions, 4 attempts |
| P1 has `probe_result="not_triggered"`, then P2 succeeds | Persist both rows for detail; summary/live route is only P2, `attempt_count=1`, `has_failover=false` |
| P1 has an ordinary gate-skipped row, then P2 succeeds | Route is P1 -> P2, `attempt_count=2`, `has_failover=true` |
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
- Good: a session still stores P1 after P1 and P2 both opened; P1's due probe
  fails, the same request serially probes recovered P2, closes P2, returns its
  complete response, and binds the session to P2.
- Base: P1 is still cooling down while P2 is due; P1 records a zero-call skip
  and P2 gets the only network call.
- Base: every candidate is still cooling down or already has a probe in flight;
  the request returns 503 with complete skip evidence and no denied-candidate
  network call.
- Bad: treating the persisted `OPEN` P1 binding as the planner's stable index;
  because P1 is already first, the planner finds no higher candidate and the
  route remains stuck on 503 even after the probe cooldown expires.
- Bad: creating a single-target intent for an all-open route; repeated P1
  failures starve a recovered P2 forever.
- Bad: enabling multi-target probing when a `CLOSED` fallback exists; this adds
  avoidable latency and changes ordinary failback behavior.
- Good: P1 has one counted failure but remains `CLOSED`; after the natural
  deadline expires, the next eligible P2-bound request tries P1 directly and a
  complete success clears the deadline and rebinds the session to P1.
- Base: a healthy `CLOSED` P1 has no pending natural deadline, so a P2-bound
  natural session remains on P2 until a real recovery signal or failure-armed
  deadline applies.
- Bad: requiring P1 to become `OPEN` before arming the natural deadline; a P1
  that failed below the circuit threshold remains `CLOSED` and can otherwise
  leave the session stuck on P2 forever.
- Good: an ineligible P1 probe opportunity persists a structured
  `not_triggered` observation, while the real P2 success remains the only route
  hop and the only counted provider attempt.
- Bad: removing a temporarily denied session-bound provider before
  `run_gates`; the request still fails quickly but loses the provider and skip
  reason from its audit trail.
- Bad: rendering four attempt rows as "switched 4 times" when they represent
  three providers, two transitions, and one retry.
- Bad: treating a `not_triggered` planner observation as a P1 attempt and then
  reporting a false P1 -> P2 failover.

### 6. Tests Required

- Unit-test selection so a temporarily denied bound provider stays in the
  candidate list while reuse selection returns no bound provider.
- Route-test all-gate-skip behavior: 503, one skipped row and route hop per
  candidate, preserved session binding, and zero upstream calls.
- Route-test a persisted first-provider binding with every provider `OPEN` and
  an expired probe cooldown: assert HTTP 200, one P1 network call, zero P2
  calls, `selection_method="circuit_probe"`,
  `probe_trigger="new_unbound_session"`, `probe_result="success"`, P1
  `CLOSED`, and the session bound to P1.
- Route-test P1 probe failure followed by P2 probe success: assert both attempts
  use `selection_method="circuit_probe"`, both carry
  `probe_trigger="new_unbound_session"`, P1 records failure, P2 records success,
  each upstream is called once, P2 closes, and the session binds P2.
- Pair the all-open success test with P1 cooldown and P1 in-flight cases while
  P2 is due: assert the structured P1 skip, zero P1 calls from the request, one
  P2 probe call, and P2 success. Keep an all-cooldown case asserting 503 and
  zero calls.
- Assert the Ready-provider cap still stops the sequence and that a mixed route
  containing a `CLOSED` fallback does not create a multi-target intent.
- Circuit-test that a counted `CLOSED` failure arms the natural deadline, a
  later failure rearms it from the newer timestamp, complete success clears it,
  reload preserves it, and a max-wait configuration update recomputes it.
- Route-test an expired pending deadline on a higher-priority `CLOSED` P1:
  assert one direct P1 call, zero P2 calls, complete success, cleared deadline,
  and session binding to P1. Pair it with a failed direct P1 call that continues
  to successful P2 and rearms P1's deadline from the new failure.
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
- Persist and project P1 `probe_result="not_triggered"` plus P2 success; assert
  detail metadata remains present, while backend summary and live frontend both
  report route `[P2]`, one provider attempt, and no failover. Keep a paired
  legacy ordinary-skipped test proving it still counts.
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

Do not use a persisted but circuit-denied binding as the probe planner anchor:

```rust
// Wrong: an OPEN first-provider binding produces stable_index == 0 and no probe.
let bound_provider_id = session_snapshot.and_then(|snapshot| snapshot.provider_id);

// Correct: reuse resolution returns None while retaining the candidate for gate evidence.
let bound_provider_id = ctx.session_bound_provider_id;
```

Do not reuse the ordinary single-target intent for an all-open recovery:

```rust
// Wrong: a failed P1 consumes the only target and a recovered P2 is starved.
RequestDispatchIntent::new(p1_id, Some(trigger), None);

// Correct: the existing failover loop serially gates P1, then P2, then P3.
RequestDispatchIntent::new_all_open_recovery(
    p1_id,
    vec![p2_id, p3_id],
    ProbeTrigger::NewUnboundSession,
);
```

For route/count projection, do not drop every skipped row:

```rust
// Wrong: hides real circuit/cooldown/limit denials.
let attempts = attempts.iter().filter(|attempt| attempt.outcome != "skipped");

// Correct: only the exact planner observation is not a provider attempt.
let attempts = attempts
    .iter()
    .filter(|attempt| attempt.probe_result.as_deref() != Some("not_triggered"));
```
