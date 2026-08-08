# Gateway Failover Route Contract

## Scenario: Change Provider Selection, Gates, Or Route Presentation

### 1. Scope / Trigger

Use this contract when changing session-bound provider selection, ordered
failback planning, circuit or rate-limit gates, request-scoped trigger
reservations, Provider recovery publication, concurrent session binding
commits, `failover_max_providers_to_try`, persisted request attempts, route
projection, or the Home request-log route label. These layers share one
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

Probe planning receives the effective circuit-validated session binding and
the binding's recovery baseline. Its logical result carries an arbitrary-length
ordered target list, one dispatch mode per target, and every ineligible natural
observation:

```rust
struct AccountUsageRecoveryInput<'a> {
    provider_recovery_epochs: &'a [(i64, u64)],
    blocked_provider_ids: &'a [i64],
    session_recovery_epoch_baseline: u64,
}

struct PlannedFailbackTarget {
    provider_id: i64,
    dispatch: PlannedDispatch,
}

enum PlannedDispatch {
    Direct,
    Probe(ProbeTrigger),
}

enum ProbePlannerDecision {
    Dispatch {
        targets: Vec<PlannedFailbackTarget>,
        reservation_trigger: ProbeTrigger,
        not_triggered_provider_ids: Vec<i64>,
    },
    // Stay fields omitted here.
}
```

The target list lowers into one request-scoped dispatch intent. Each target has
its own optional probe trigger; Provider lease ownership and the session trigger
reservation are separate. `CircuitSnapshot.recovery_epoch` publishes a trusted
probe success, `probe_in_flight` exposes active lease ownership to the common
gate, and `SessionRoutingSnapshot.recovery_epoch_baseline` is captured once per
live binding incarnation. Binding creation and completion carry a checked
monotonic `SessionBindingRequest`; the binding stores `creation_request` as its
incarnation floor and `last_binding_request` for CAS-style success publication.

### 3. Contracts

- Session binding owns reuse preference and ordering only. If the bound
  provider remains in the eligible candidate set but its circuit currently
  denies reuse, keep it in the list and let the later common gate decide. Clear
  the binding only when the provider is no longer eligible for that candidate
  set.
- For latest route `p1 -> ... -> p(X-1) -> current pX`, scan the entire dynamic
  prefix before `pX`; do not special-case P1, P2, or any fixed length. Explicit
  route-change, compaction, and aggressive triggers plan every prefix candidate
  in route order: `CLOSED` is direct and `OPEN`/`HALF_OPEN` is a probe with its
  exact trigger.
- Natural eligibility is per candidate. A `CLOSED` candidate with a recovery
  epoch newer than the session baseline is direct; otherwise evaluate that
  candidate's own natural/open deadline. Record exact
  `probe_result="not_triggered"` observations and continue scanning, so an
  earlier not-due provider never blocks a later due provider.
- An active Provider probe lease stays visible even after transport dispatch
  rearms deadlines. Keep that Provider as a planned probe target and let the
  common gate record `in_flight` with zero network calls; do not downgrade it to
  a planner-only `not_triggered` observation.
- Resolution performs one stable target-first reorder. Given session-rotated
  `[pX, p1, p2, ..., tail]` and targets `[p1, p2, ...]`, dispatch targets in the
  planned order, then append non-targets in their existing relative order.
  Ignore duplicate or missing target IDs; observation-only IDs are not targets.
- The failover loop stays serial. A failed, skipped, lease-losing, or pre-send
  target advances to the next planned target; the first complete success stops,
  binds the actual Provider, and makes zero later calls. Only after all planned
  targets fail or skip may current `pX` and existing fallbacks run.
- Every eligible candidate reaches
  `failover_loop/prepare/provider_checks::run_gates`. A circuit, cooldown, or
  provider-limit denial creates one `outcome="skipped"` attempt with its stable
  error/reason data and makes zero upstream calls.
- An explicitly enabled account-usage route gate runs first in that common
  pre-send gate. Only a fresh trusted zero-balance/expired projection denies;
  missing, stale, failed, conflicting, or unconfigured state fails open. The
  denial is an ordinary account skip with zero circuit, retry, Ready, Session,
  or upstream side effects.
- Direct targets use ordinary gate metadata. Probe targets use their own trigger
  and Provider-global single-flight lease; a serial request holds at most one
  executing probe lease. The common gate, not selection or the planner, owns
  cooldown/in-flight/limit denial evidence.
- Exactly one applied, dispatched, trusted probe success publishes a monotonic
  Provider recovery epoch. Followers that previously recorded `in_flight` may
  finish on their old Provider, then each independently sees the same newer
  epoch on its next eligible request and dispatches the recovered Provider
  directly without another lease, timer, or probe metadata.
- Recovery markers are success-only and process-local. Counted failure,
  probe failure/reopen, and explicit reset invalidate the Provider marker;
  stale, abandon, pre-send drop, and ordinary success publish none, and epoch
  overflow fails closed without reuse. Restart resets Provider epochs and live
  session bindings together; no epoch or baseline is persisted.
- Account recovery uses a separate Provider/global epoch and per-Session
  baseline. Resolution derives that epoch and `blocked_provider_ids` from one
  route read per candidate. Only fresh trusted Blocked enters the set;
  UnknownAllow never suppresses planning or fabricates recovery.
- For an effectively bound fallback Session, omit blocked higher-priority
  candidates before natural/compaction/route-change/aggressive target,
  observation, or reservation creation. This is a failback hint, not a second
  provider list: new/unbound Sessions, the current bound Provider, forced
  requests, and normal fallback retain ordinary candidates and the common gate
  remains authoritative for send-time races.
- If all higher targets are blocked, create no synthetic attempt or reservation.
  Compaction remains pending; route change may confirm its new fingerprint.
  Fresh recovery removes the hint and re-enables Direct failback, whose real
  transport send alone consumes any pending Session trigger.
- Allocate one binding request token at proxy entry before the first `await`,
  retain it only for resolved session routing, and carry it through non-stream
  and stream finalizers. Binding creation installs the token as the creation
  floor. Success uses CAS-style checks: reject missing/expired bindings, tokens
  below the floor or last success, and equal-token Provider changes; equal-token
  same-Provider replay is idempotent. Clear/expiry/recreation creates a new
  floor, so an old response cannot recreate or overwrite the new incarnation.
  Token allocation overflow skips binding creation and publication rather than
  falling back to an unversioned write.
- `RequestDispatchIntent` owns a route-change/compaction reservation for the
  whole target chain. Gate, cooldown, in-flight, Ready-limit, and other pre-send
  exits neither consume nor release it. The first planned transport send commits
  it exactly once; later sends cannot. If no planned target sends, intent drop
  releases it for a later request. Preserve commit rollback and fail-closed
  persistence behavior.
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
  existing failover/auth classification. Serialization defensively strips a
  supplied 401/403 preview even when an earlier layer accidentally included it.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| `failover_max_providers_to_try == 0` | Reject with `SEC_INVALID_INPUT` |
| `failover_max_providers_to_try > 20` | Reject with `SEC_INVALID_INPUT` |
| attempts per provider x providers to try > 100 | Reject with `SEC_INVALID_INPUT` |
| Eligible session-bound provider is circuit-open | Keep candidate; common gate records one skipped row |
| Latest route is `p1 -> ... -> current pX` | Plan and stably dispatch the complete eligible prefix before `pX` |
| Natural P1 is not due while later P2 is due | Persist P1 `not_triggered`, continue scanning, and plan P2 |
| Another request owns P1's active probe lease | Target P1 so the common gate records `in_flight`; make zero P1 calls |
| N sessions hit due OPEN P1 while bound to P2 | One winner probes P1; followers record `in_flight` and may continue to P2 |
| The winner completes a trusted probe success | Publish one epoch; every follower's next eligible request tries P1 direct |
| P1 has counted failure, probe failure/reopen, or reset | Clear P1's recovery marker without reusing or rewinding the global epoch |
| An old P2 request completes after a newer P1 request | Request-token CAS rejects P2 and preserves the P1 binding |
| Binding clears/expires and a newer request recreates it | Reject old tokens below the new creation floor |
| First planned target crosses transport send | Commit the session reservation once; later sends cannot commit it again |
| Every planned target exits before send | Drop releases the reservation for a later request |
| Candidate is gate-skipped | Zero upstream calls and no Ready-provider budget consumed |
| All candidates are gate-skipped | HTTP 503 with every candidate in attempts and route |
| Bound P2 repeatedly sees fresh account-blocked higher P1 | Omit P1 target/observation/reservation; send and log only P2 |
| The same route has no effective binding | Keep P1 for common-gate account skip audit before fallback |
| Blocked P1 becomes fresh available | Expose a newer account epoch; next eligible request sends P1 Direct and binds only on success |
| Ready-provider cap is reached | Stop before the next Ready provider |
| Two Ready providers consume cap 2, then a circuit-open candidate follows | Record the third skipped attempt/route; make no third upstream call |
| Route has 3 hops and 4 attempt rows | 3 providers, 2 transitions, 4 attempts |
| Upstream 401/403 body contains a credential-like value | Keep status and safe reason, but persist/log none of the body |

### 5. Good / Base / Bad Cases

- Good: current P5 has planned targets P1 through P4; they run strictly in
  route order, an intermediate success stops the chain, and no fixed-length
  planner or reorder helper can pass accidentally.
- Good: P1 is not due while P2 is due; P1 records `not_triggered` with zero
  calls, then P2 runs. If P1 instead has an active lease, the common gate
  records `in_flight` before the chain continues.
- Good: one natural-mode winner probes P1 while any number of follower sessions
  continue on P2. After trusted success, every follower independently tries P1
  direct on its next eligible request and binds it without another probe.
- Good: an older same-session P2 response finishes after a newer request binds
  P1; its lower request token cannot overwrite P1. Clear/TTL recreation is also
  protected by the new binding's creation floor.
- Good: two planned targets skip before send and the third sends. The third
  consumes the reservation once; an all-zero-send chain releases it on drop.
- Good: initial P1 account skip binds P2; two identical post-compaction turns
  contain only P2 while P1 stays blocked, then the first P1 send after recovery
  consumes the still-pending fingerprint.
- Base: all planned targets fail or skip, then current P3 succeeds and keeps or
  reconfirms the P3 binding.
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
- Bad: selecting only the first prefix target, stopping at a not-due P1, or
  repeatedly moving one target to the front. Each can place current P3 before
  eligible P2 and starve ordered failback.
- Bad: recomputing only deadlines after dispatch and skipping an active lease
  before the common gate; followers lose their structured `in_flight` evidence.
- Bad: clearing a global `provider_recovered` boolean after one follower reads
  it, or using general `state_revision` as recovery evidence. Recovery must be
  a success-only per-Provider epoch observed against each session baseline.
- Bad: an unversioned stream/non-stream finalizer recreates or rewrites a
  binding after clear, expiry, or a newer request completion.
- Bad: a gate denial releases the request reservation, so a later target that
  really sends loses the reserved route-change/compaction trigger.
- Bad: repeatedly create an intent for a durable account-blocked target that
  returns before claim, then assume a non-target fallback success consumed it.
- Bad: rendering four attempt rows as "switched 4 times" when they represent
  three providers, two transitions, and one retry.

### 6. Tests Required

- Planner-test `[p1, p2, p3]` with current P3 and a five-Provider case with
  targets `[p1, p2, p3, p4]` before P5. Cover explicit triggers, mixed
  direct/probe targets, per-candidate natural deadlines, later-due scanning,
  and active-lease targeting.
- Planner-test fresh account-blocked IDs across natural and explicit triggers:
  no target/observation/reservation, no starvation of later due candidates,
  all-blocked Stay, unbound common-gate audit, and route-change confirmation.
- Resolution-test session-rotated input plus multiple targets becomes one stable
  target-first order; duplicate/missing targets and observation-only IDs do not
  disturb non-target order.
- Circuit-test that only applied dispatched probe success publishes an epoch;
  failure, stale, abandon, ordinary success, reset/reopen invalidation, and
  overflow behave as contracted. Snapshot publication must precede the global
  epoch, and reload/restart must not persist recovery markers.
- Session-test baseline capture once per incarnation, checked request-token
  allocation, both completion orders, equal-token behavior, missing/expired
  rejection, clear/recreate, TTL/recreate, and token overflow. Cover both
  non-stream and stream finalization.
- Route-test current P3 with P1 failure then P2 success, plus current P5 with
  four ordered targets. Assert exact calls, success short-circuiting, final
  route, and binding.
- Route-test cooldown, in-flight, not-triggered, common-gate, and local pre-send
  exits before a later successful target. Cover mixed direct/probe metadata,
  one live lease at a time, all targets failing before current P3, and committed
  streaming terminal behavior.
- Use a real gated upstream and explicit barriers for one winner plus at least
  three follower sessions. Assert one first-wave P1 call, follower
  `in_flight -> P2`, direct P1 convergence after success, and no convergence
  after failed/stale winner; do not use sleep-based races.
- Barrier-test an old P2 response against newer same-session P1 convergence,
  then repeat through stream finalization and clear/TTL recreation.
- Dispatch-intent-test that every pre-send exit leaves the reservation, the
  first transport send commits once, later sends cannot, and an all-zero-send
  chain releases on drop. Keep commit rollback and fail-closed tests.
- Unit-test selection so a temporarily denied bound provider stays in the
  candidate list while reuse selection returns no bound provider.
- Route-test all-gate-skip behavior: 503, one skipped row and route hop per
  candidate, preserved session binding, and zero upstream calls.
- Route-test initial P1 account skip plus P2 success, two same-Session
  post-compaction requests with only P2 in raw attempts, then fresh P1 recovery
  and real fingerprint consumption at transport send.
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

Do not truncate or reorder the dynamic prefix one target at a time:

```rust
// Wrong: P1 failure falls through to current P3 before eligible P2.
if let Some(target) = prefix.iter().find_map(plan_candidate) {
    move_provider_to_front(&mut providers, target.provider_id);
}

// Correct: plan every target, then restore the whole stable prefix once.
let targets = prefix.iter().filter_map(plan_candidate).collect::<Vec<_>>();
providers = stable_targets_first(providers, &targets);
```

Do not hide active single-flight ownership behind a rearmed deadline:

```rust
// Wrong: the follower reports not_triggered without reaching the gate.
if !candidate_is_due(candidate, now) {
    not_triggered_provider_ids.push(candidate.provider_id);
    continue;
}

// Correct: an active lease remains a target; the common gate reports in_flight.
if snapshot.probe_in_flight {
    targets.push(plan_probe(candidate));
    continue;
}
```

Do not publish session bindings without request and incarnation ordering:

```rust
// Wrong: a late finalizer inserts or overwrites without an incarnation token.
session.publish_binding_unversioned(session_id, provider_id);

// Correct: pass the entry token; the manager CASes existing state without insert.
let committed = session.bind_success_for_request(
    cli_key,
    session_id,
    provider_id,
    sort_mode_id,
    binding_request,
    now_unix,
);
```

Do not let pre-send skips own the request-scoped reservation lifecycle:

```rust
// Wrong: P1 gate denial discards the trigger needed by a later real send.
intent.release_unclaimed_reservation();

// Correct: the first planned sender commits exactly once at transport send.
intent.commit_reservation_at_transport_send(provider_id)?;
transport.send(request).await?;
```

Do not globally remove a trusted account-blocked Provider or repeatedly plan it
for a stable fallback Session:

```rust
// Wrong: hides unbound/forced audit evidence.
providers.retain(|provider| !blocked_provider_ids.contains(&provider.id));

// Correct: suppress only the stable-bound higher-priority failback prefix.
let targets = higher_prefix
    .iter()
    .filter(|provider| !blocked_provider_ids.contains(&provider.id));
```

Use a success-only per-Provider recovery epoch, not a one-shot global boolean or
general state revision. Compare it with each binding's captured baseline,
invalidate the Provider marker on failure/reopen/reset, and keep epochs and
baselines process-local.
