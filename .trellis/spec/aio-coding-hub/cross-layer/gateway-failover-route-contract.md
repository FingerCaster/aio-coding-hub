# Gateway Failover Route Contract

## Scenario: Change Provider Selection, Gates, Or Route Presentation

### 1. Scope / Trigger

Use this contract when changing session-bound provider selection, ordered
failback planning, circuit, rate-limit, or account-usage gates, request-scoped trigger
reservations, `failover_max_providers_to_try`, persisted request attempts,
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
not the provider ID retained in the session snapshot. Its logical result must
carry an arbitrary-length ordered target list, one dispatch mode per target,
and every natural-mode observation that was not eligible:

```rust
ProbePlannerInput {
    bound_provider_id: ctx.session_bound_provider_id,
    session_recovery_epoch_baseline: session_snapshot.recovery_epoch_baseline,
    // ordered candidates, strategy, triggers, and request eligibility omitted
}

AccountUsageRecoveryInput {
    provider_recovery_epochs: &[(provider_id, epoch)],
    session_recovery_epoch_baseline:
        session_snapshot.account_usage_recovery_epoch_baseline,
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
```

Provider recovery and live session convergence use a schema-free process-local
epoch. `CircuitSnapshot.recovery_epoch` is the Provider's latest applied probe
success epoch; `SessionRoutingSnapshot.recovery_epoch_baseline` is captured
when that live session binding is first created. Neither field is persisted.
Account-usage recovery uses a separate process-local Provider/global epoch and
`SessionRoutingSnapshot.account_usage_recovery_epoch_baseline`; it is never
folded into the circuit epoch or globally consumed by one Session.

Resolution projects that plan into one request-scoped intent. The target list
is arbitrary in length and each provider independently carries either no probe
trigger (direct dispatch) or its exact probe trigger:

```rust
struct DispatchTarget {
    provider_id: i64,
    probe_trigger: Option<ProbeTrigger>,
}

intent.targets_provider(provider_id);
intent.probe_trigger_for(provider_id);
intent.claim_for_provider(provider_id, probe_guard);
```

`new_all_open_recovery` may remain as a compatibility constructor, but it must
lower every routed provider into this same target-list model. Claiming a
provider lease and consuming a session trigger reservation are separate
operations; the latter occurs only at the existing transport-send boundary.

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
- For the latest effective route
  `p1 -> ... -> p(X-1) -> current pX -> ... -> pN`, ordinary failback planning
  scans the complete prefix `p1 -> ... -> p(X-1)`. The prefix length is dynamic;
  no planner, resolution helper, dispatch intent, or test may special-case P1,
  P2, or any fixed number of providers. If the request is ineligible or the
  prefix is empty, return `Stay` without creating a dispatch intent.
- Route-change, successful-compaction, and aggressive failback are explicit
  triggers. Scan the entire prefix in latest-route order: a candidate whose
  planning snapshot is `CLOSED` becomes `Direct`, while an `OPEN` or
  `HALF_OPEN` candidate becomes `Probe` with that exact explicit trigger.
- Natural failback eligibility is evaluated independently for every prefix
  candidate from its own circuit snapshot. A `CLOSED` candidate whose
  `recovery_epoch` is newer than the live session's captured baseline becomes
  `Direct`: this is how every single-flight follower observes the winner's
  success without waiting for another timer. Otherwise, if
  `natural_probe_due_at <= now`, a `CLOSED` candidate becomes `Direct` and an
  `OPEN`/`HALF_OPEN` candidate becomes `Probe(NaturalMaxWait)`. An
  `OPEN`/`HALF_OPEN` candidate whose
  existing `open_until <= now` becomes `Probe(MaxOpenWait)`. Every other
  candidate produces the exact structured `probe_result="not_triggered"`
  observation and scanning continues; an earlier not-due provider must never
  block a later due provider. An already active Provider probe lease is the
  exception to the re-read deadline check: dispatch rearms that deadline, so a
  concurrent follower must still target the Provider and let the common gate
  report `in_flight` with zero network calls instead of misreporting
  `not_triggered`.
- A counted failure arms or rearms that provider's `natural_probe_due_at` even
  while its circuit remains `CLOSED`. Rearm from the latest counted failure,
  clear the deadline on complete success, preserve it across persisted-state
  reload, and recompute it from `probe_reference_at` when
  `natural_probe_max_wait_secs` changes. For a legacy `CLOSED` row with failures
  but no reference, recover the reference from its latest persisted failure
  timestamp, not a later unrelated `updated_at`. A healthy `CLOSED` candidate
  with no pending deadline and no recovery epoch newer than the session
  baseline remains `not_triggered`; natural mode must not become aggressive
  failback.
- Resolution performs a stable target-first reorder. Given session-rotated
  input `[pX, p1, p2, ..., p(X-1), tail...]` and planned targets
  `[p1, p2, ..., p(X-1)]`, dispatch order is
  `[p1, p2, ..., p(X-1), pX, tail...]`. Append each unique planned ID that
  exists, then append non-targets in their existing relative order; ignore
  missing or duplicate target IDs. A natural `not_triggered` observation is
  not a target and must not be moved ahead of `pX` merely for observation.
- Every planned target reaches the same
  `failover_loop/prepare/provider_checks::run_gates`. `Direct` targets use the
  ordinary gate and never receive probe selection metadata. `Probe` targets use
  their own trigger and independently acquire a Provider-global single-flight
  lease. A circuit, cooldown, in-flight, or provider-limit denial creates one
  stable `outcome="skipped"` attempt and makes zero upstream calls.
- The account-usage gate is the first common pre-send gate, before
  circuit/cooldown/probe-lease acquisition, OAuth/local-spend checks,
  credential/Base-URL preparation, and Ready-provider counting. Only a current
  trusted `Blocked(ZeroBalance|Expired)` projection denies. Missing, stale,
  failed, conflicting, unconfigured, or gate-off state allows. Denial records
  `GW_PROVIDER_ACCOUNT_USAGE_BLOCKED`, category `account_usage`, and reason
  `account_usage_zero_balance` or `account_usage_expired`; provider/retry
  indices and every circuit/probe field stay empty. It changes no circuit,
  health, Session, retry budget, or Ready count.
- The existing failover loop remains serial. A target that exhausts its normal
  retry chain, is gate-skipped, loses a probe race, or aborts before transport
  send advances to the next planned target. The first complete success stops
  immediately, completes/closes its probe circuit when applicable, binds the
  session to the provider that actually succeeded, and makes zero calls to all
  later targets and fallbacks.
- Provider-global single-flight remains probe-only. Exactly one applied,
  dispatched, trusted `complete_probe_success` publishes a new monotonic
  recovery epoch after the Provider has become `CLOSED`; failure, stale,
  abandon, pre-send drop, and ordinary `CLOSED` success publish none. The
  session baseline is captured at new-binding creation and is not advanced by
  sliding-TTL refresh, same-Provider success, an in-flight loser request, or a
  different session's convergence. It is not a globally consumed cursor, so
  any number of follower sessions can observe the same Provider recovery.
- Account-usage recovery is independently observable in natural mode. For a
  higher-priority candidate whose circuit is `CLOSED`, a current fresh
  ConfirmedAvailable Provider epoch newer than that Session's account baseline
  produces the existing `Direct` dispatch. Equal/older/zero/hidden/reblocked
  epochs do not. Account recovery never turns `OPEN` or `HALF_OPEN` into Direct,
  never closes a circuit, and never creates a new probe trigger; existing
  circuit due/cooldown/single-flight planning remains authoritative.
- A request already planned while another owner still holds the probe lease may
  record `in_flight` and complete on current `pX`. After the winner succeeds,
  that follower's next eligible request sees the newer `CLOSED` recovery epoch,
  dispatches it directly in latest-route order, carries no probe metadata, and
  binds on complete success. It does not wait another
  `natural_probe_max_wait_secs` interval and does not acquire another probe
  lease.
- Session binding commits are monotonic across concurrent requests and response
  modes. Assign every incoming request a strictly increasing binding request
  token in the proxy entrypoint before its first asynchronous middleware, retain
  it only when session routing resolves a session, carry it through non-stream
  and stream finalization, and require it when the request creates a live session
  binding. Each binding incarnation stores that creation token as its floor.
  Token-aware success may update only an existing, unexpired binding whose floor
  and last successful token are not newer than the request; it must never insert
  or recreate a missing or expired binding. Clearing or expiring a binding ends
  that incarnation, and a later request creates the replacement with a new floor,
  so an older in-flight response cannot repopulate the cleared session or write
  into the replacement. A later-started success may replace an earlier success
  regardless of completion order; an older P2 response or stream that completes
  after a newer P1 convergence must not reverse the P1 binding. Equal-token
  replay may refresh only the same Provider, and a request that never succeeds
  does not advance the applied token. If token allocation is unavailable,
  including monotonic counter exhaustion, skip both binding creation and success
  publication; never fall back to an unversioned write.
- New binding creation captures both circuit and account-usage global baselines
  with their required Acquire reads. Sliding TTL, same-Provider success, route
  confirmation, and old responses preserve both baselines. Clear or expiry
  creates a new incarnation with fresh baselines. An account refresh never
  calls a binding API; only a complete real non-stream or stream model response
  can commit the recovery target.
- Only after every planned target fails or is skipped may the loop continue to
  current `pX`, followed by the existing remaining fallback order. A complete
  `pX` success keeps or reconfirms its binding. Preserve the existing terminal
  rule that a committed streaming response cannot be spliced with another
  provider.
- `RequestDispatchIntent` owns any route-change/compaction session trigger
  reservation for the whole request. Gate denial, cooldown, in-flight,
  Ready-limit evaluation, or any other pre-send exit neither consumes nor
  releases it. The first planned target that actually crosses the transport
  send boundary atomically commits it exactly once; later target sends cannot
  commit it again. Dropping pre-send ownership leaves the reservation available
  to later planned targets. If no planned target sends, dropping the intent
  releases the reservation so a later request can retry, even if the request
  subsequently succeeds through `pX` or another non-target fallback. Preserve
  fail-closed trigger persistence and rollback on commit failure.
- Provider lease ownership is independent from the session reservation.
  `claimed_provider_ids` prevents one request from claiming a target twice,
  and the serial loop releases/completes the current provider lease before
  preparing another target, so one request holds at most one executing probe
  lease at a time. Provider-level global single-flight remains authoritative.
- A persisted binding whose circuit denies reuse is not a stable planner
  anchor. Pass circuit-validated `session_bound_provider_id`; do not re-read
  `routing_snapshot.provider_id`. If there is no effective binding and every
  eligible routed provider is `OPEN`/`HALF_OPEN`, preserve the ordered
  `new_unbound_session` all-open recovery intent containing the entire route.
  This compatibility branch uses the same per-target intent and common gate;
  it is not an ordinary-prefix length restriction. If any eligible routed
  provider is `CLOSED`, do not misclassify the route as all-open recovery.
- In all-open recovery, cooldown and in-flight candidates make zero calls and
  advance to the next routed provider. A dispatched probe that exhausts its
  retry chain also advances. First complete success closes and binds the actual
  provider. If every candidate is denied, return
  `GW_ALL_PROVIDERS_UNAVAILABLE` / HTTP 503 with all skips intact; if probes
  dispatch but all fail, preserve the existing terminal upstream error.
- `providers_tried` increments only after common gates and preparation produce
  `Ready`. Therefore `failover_max_providers_to_try` caps Ready providers, not
  inspected targets, `not_triggered` observations, or gate-skipped rows.
  Reaching the cap still evaluates authoritative gates for later candidates so
  later denials remain observable; stop only when a later candidate itself is
  `Ready` beyond the cap. Ordered failback adds no hidden attempt, no parallel
  send, no same-provider retry, and no increase to the per-provider or total
  attempt budget.
- `IterationCounters.skipped_account_usage` is separate from circuit/cooldown/
  limit counts. Account refresh cadence is not an authoritative recovery time,
  so account skips never update `earliest_available_unix` and pure account-only
  503 responses have no `Retry-After`. A mixed terminal response may retain
  another gate's trusted `Retry-After`, but any terminal containing an account
  skip must not enter the recent-error cache. Otherwise recovery, failure, or
  staleness could be hidden by an old cached 503.
- Forced-provider requests, single-candidate routes, model discovery,
  health-neutral requests, warmup, token accounting, the compaction request
  itself, and managed-model route eligibility retain their existing behavior.
  Add no setting, database field, background synthetic probe, or parallel
  probe execution.
- Recovery epochs and session baselines are process-local only. A Gateway
  restart also clears every live session binding, so persisted circuits load
  with epoch zero and new sessions start from the latest route without a schema
  migration or a stale follower cursor.
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
- Gate-only terminal classification must explicitly include both account-usage
  reason codes. A route containing only account skips, or account skips mixed
  with circuit/cooldown/limit skips, is unavailable (503), not
  `GW_UPSTREAM_ALL_FAILED` (502). Ordinary account skips remain route attempts;
  they are not planner `not_triggered` observations.
- Explicitly closing an active route gate or its adapter clears that CLI's live
  Session bindings and recent-error cache after the Provider mutation, then
  reconciles account targets. This is configuration invalidation, not recovery:
  neither circuit nor account recovery epoch advances.
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
| Latest route is `p1 -> ... -> p(X-1) -> current pX` under an explicit failback trigger | Plan the complete ordered prefix `p1 -> ... -> p(X-1)` with no fixed-length truncation |
| Session-rotated input is `[pX, p1, p2, ..., p(X-1), tail...]` | Dispatch planned targets in latest-route order before `pX`, then preserve non-target relative order |
| Explicit prefix contains `CLOSED` P1, `OPEN` P2, and `HALF_OPEN` P3 | Dispatch P1 direct, then P2/P3 as independent probes with the exact trigger; never mark P1 as a probe |
| Natural P1 is not due while later P2 is due | Persist P1 `probe_result="not_triggered"`, continue scanning, and plan P2 |
| Natural P1/P2 are both due | Plan both in route order using each candidate's own direct/probe mode and trigger |
| N sessions hit due OPEN P1 while bound to P2 | One winner sends the P1 probe; every loser records `in_flight`, makes zero P1 calls in that request, and may continue to P2 |
| The single P1 winner completes successfully | Publish one recovery epoch; every still-bound follower's next eligible request dispatches P1 direct with no probe metadata and no new 60-second wait |
| A P1 winner fails, is stale, or is abandoned | Publish no recovery epoch; followers retain the original natural/gate behavior and cannot treat P1 as healthy |
| One follower has already converged after P1 recovery | Other follower sessions still observe the same epoch independently; there is no first-consumer-wins flag |
| An older same-session P2 request/stream completes after a newer request converges to P1 | Reject the older binding commit by request token and retain P1 |
| A first request resolves a new session with a binding token | Create the route binding with that token as its incarnation floor, then allow complete success from the same token |
| Token-aware success finds no binding or an expired binding | Reject without inserting or recreating a binding |
| A binding is cleared or expires, then a newer request recreates it | Reject every older-incarnation token below the new creation floor; allow the recreating request to commit |
| The same token publishes success more than once | Refresh only when the Provider is unchanged; reject an equal-token Provider change |
| Binding token allocation is unavailable or exhausted | Create and publish no session binding; never use an unversioned fallback |
| P1 completes successfully before planned P2 and current P3 | Stop after P1, bind P1, and make zero P2/P3 calls |
| Every planned higher-priority target fails or is skipped | Only then continue to current P3 and retain/reconfirm P3 when it succeeds |
| A planned target is gate/cooldown/in-flight/pre-send skipped | Make zero target calls, retain the session reservation for the next planned target, and continue |
| First planned target crosses transport send | Atomically consume the session reservation exactly once; later target sends do not consume it again |
| No planned target crosses transport send | Release the unconsumed reservation on intent drop so a later request can retry |
| Persisted P1 binding is `OPEN`, every routed provider is `OPEN`, and P1 probe cooldown is due | Treat the binding as ineffective, create an ordered `new_unbound_session` recovery intent, and let one lease winner call P1 |
| P1 probe fails and due P2 is also `OPEN` | Complete P1 failure, then serially acquire and probe P2; stop and bind P2 on complete success |
| P1 cooldown is not due but P2 is due | Record P1 `probe_result="cooldown"` with zero P1 calls, then probe P2 |
| Another request owns P1 probe but P2 is due | Record P1 `probe_result="in_flight"` with zero P1 calls from this request, then probe P2 under P2's own single-flight lease |
| Every all-open candidate is in cooldown/in-flight | HTTP 503 with structured skips and zero calls to denied candidates |
| Effective-unbound route has any `CLOSED` provider | Do not use the all-open recovery branch; preserve ordinary route behavior |
| Higher-priority `CLOSED` candidate has a pending natural deadline that is not due | Record `not_triggered`, make zero calls to it, and continue checking later prefix candidates |
| Higher-priority `CLOSED` candidate has an expired natural deadline | Plan a direct call; on complete success clear its deadline and bind it |
| Expired direct candidate fails while a later prefix candidate is eligible | Rearm the failed candidate from its failure and continue to the later planned target before current `pX` |
| Candidate is gate-skipped | Zero upstream calls and no Ready-provider budget consumed |
| All candidates are gate-skipped | HTTP 503 with every candidate in attempts and route |
| Trusted zero-balance/expired projection reaches common gate | Account skip with stable code/reason, empty circuit/probe/index fields, zero upstream and Ready use |
| Account projection is absent/stale/failed/conflicting or gate is off | Allow through this gate; preserve existing later gates |
| Every candidate is account blocked | HTTP 503 `GW_ALL_PROVIDERS_UNAVAILABLE`, all account skips retained, no `Retry-After` |
| Account block plus circuit/limit denial | HTTP 503; retain the other gate's trusted `Retry-After`, but write no recent-error cache entry |
| A previously blocked Provider becomes fresh available after Session baseline | If circuit is CLOSED, natural planner emits Direct in route order; bind only on real success |
| Account recovery epoch is equal/older/zero or Provider reblocks | Do not plan Direct from account recovery |
| Account recovery exists while circuit is OPEN/HALF_OPEN | Do not bypass circuit; use existing probe/cooldown rules |
| Active gate/adapter is explicitly closed | Clear live route runtime and reconcile targets; publish no recovery epoch |
| Ready-provider cap is reached | Stop before the next Ready provider |
| Two Ready providers consume cap 2, then a circuit-open candidate follows | Record the third skipped attempt/route; make no third upstream call |
| Route has 3 hops and 4 attempt rows | 3 providers, 2 transitions, 4 attempts |
| P1 has `probe_result="not_triggered"`, then P2 succeeds | Persist both rows for detail; summary/live route is only P2, `attempt_count=1`, `has_failover=false` |
| P1 has an ordinary gate-skipped row, then P2 succeeds | Route is P1 -> P2, `attempt_count=2`, `has_failover=true` |
| Upstream 401/403 body contains a credential-like value | Keep status and safe reason, but persist/log none of the body |
| Gzip body exceeds the decoded scan prefix | Match only decoded bytes within the first 64 KiB; never scan compressed fallback bytes |

### 5. Good / Base / Bad Cases

- Good: current P5 has explicit failback targets P1 through P4; P1, P2, and P3
  fail in order, P4 succeeds, and P5 is never called. The four-target prefix is
  data-driven rather than a special three- or five-provider branch.
- Good: current P3 plans P1 then P2; P1 fails, P2 succeeds, the loop binds P2,
  and neither P3 nor any later fallback is called.
- Good: natural P1 is not due while P2 is due. P1 persists a structured
  `not_triggered` observation with zero calls, P2 gets the first network call,
  and P1 does not block it.
- Good: eight sessions are bound to P2 when due OPEN P1 is probed. One request
  calls P1 while seven record `in_flight` and finish on P2; after the winner
  closes P1, each of those seven sessions' next requests call P1 directly and
  bind P1 without another probe or timer.
- Good: one follower's old P2 request remains in flight while P1 recovers. A
  newer request for that same session completes on P1 and binds it; the old P2
  response then finishes but its lower request token cannot overwrite P1. The
  same ordering holds when either completion is finalized from a stream.
- Good: an ordered prefix contains direct `CLOSED` P1 followed by probe
  `OPEN` P2. P1 uses ordinary selection metadata; if it fails, P2 acquires its
  own lease and trigger. A P2 success prevents every later call.
- Base: every planned target fails or is skipped, then current P3 succeeds and
  keeps/reconfirms the P3 binding.
- Base: a healthy `CLOSED` P1 has no pending natural deadline, so it is observed
  as `not_triggered` when it has no recovery epoch newer than this session's
  baseline; a later due P2 can still run, otherwise the session stays on
  current P3.
- Good: the first two planned targets are pre-send skipped and the third sends;
  the third consumes the route-change/compaction reservation once. If all three
  skip, intent drop releases it for a later request.
- Good: two circuit-open candidates are skipped, then a third Ready candidate
  succeeds with `failover_max_providers_to_try = 2`; the skips do not consume
  either Ready slot.
- Good: three gate-skipped candidates return 503, produce three route hops and
  three attempt rows, and call no upstream.
- Good: P1 is account-blocked and P2 succeeds. P1 records a normal filtered
  account skip before any probe lease or Ready slot, P1 makes zero calls, and
  the Session binds P2 only after P2's real success.
- Good: P1 account-blocked plus P2 circuit-open returns a mixed 503 with P2's
  trusted `Retry-After`; after P1 becomes available, the identical next request
  reaches P1 because the mixed response was not cached.
- Good: two live Sessions are bound to P2 before P1's blocked-to-available
  transition. Each retains its own baseline and independently direct-dispatches
  P1 on its next eligible request; one Session's success consumes nothing for
  the other.
- Base: one Ready provider and one attempt render as a direct request with zero
  provider transitions.
- Good: an effective-unbound all-open route serially probes every routed target
  under `new_unbound_session`; a failed P1 does not starve recovered P2, and P2
  success closes and binds P2.
- Base: every all-open candidate is cooling down or already has a probe in
  flight; the request returns 503 with complete skip evidence and no denied
  candidate network call.
- Bad: selecting only the first eligible prefix provider. P1 failure then sends
  current P3 before higher-priority P2, so P2 is starved by P3 success.
- Bad: stopping natural scanning when P1 is not due. This loses P1's structured
  observation and can indefinitely starve a due P2.
- Bad: calling a one-target move-to-front helper repeatedly on session-rotated
  input. It can place current P3 between P1 and P2 instead of producing the
  stable target-first order.
- Bad: attaching a probe trigger to a planned `CLOSED` target. This invents
  probe metadata and Provider lease semantics for an ordinary direct attempt.
- Bad: releasing the session reservation when P1 is gate-denied. A later real
  P2 send then loses the trigger opportunity that the request reserved.
- Bad: treating the persisted `OPEN` P1 binding as the planner's stable index;
  because P1 is already first, all-open recovery may find no target and remain
  stuck on 503 after cooldown expires.
- Bad: requiring a failed provider to become `OPEN` before arming its natural
  deadline; a below-threshold `CLOSED` failure can otherwise leave the session
  stuck on a lower-priority provider.
- Bad: removing a temporarily denied session-bound provider before
  `run_gates`; the request loses the provider and skip reason from its audit
  trail.
- Bad: rendering four attempt rows as "switched 4 times" when they represent
  three providers, two transitions, and one retry.
- Bad: treating a `not_triggered` planner observation as a provider attempt and
  reporting a false failover hop.
- Bad: clearing one global `provider_recovered` boolean after the first follower
  reads it. Every other live session then misses the same successful probe.
- Bad: using `state_revision` as recovery evidence. Dispatch, failure, expiry,
  reset, and other transitions also advance that revision, so it is not a
  success-only recovery signal.
- Bad: classify an account-only skipped chain as upstream failure, derive a
  `Retry-After` from the refresh interval, cache a mixed account 503, or reuse
  the circuit recovery epoch for balance recovery.

### 6. Tests Required

- Planner-test latest route `p1 -> p2 -> p3` with current P3 and an eligible
  prefix: assert ordered targets `[p1, p2]`, not one target. Add a five-provider
  case asserting `[p1, p2, p3, p4]` before current P5 so no fixed length passes.
- Planner-test explicit route-change, successful-compaction, and aggressive
  triggers across the full prefix. In a mixed snapshot, assert every `CLOSED`
  target is `Direct` and every `OPEN`/`HALF_OPEN` target is `Probe` with its
  exact trigger.
- Planner-test natural eligibility independently: P1 not due plus P2 due must
  return P1 in `not_triggered_provider_ids` and P2 in `targets`. Cover the
  inverse, multiple due targets, `NaturalMaxWait`, `MaxOpenWait`, and a healthy
  `CLOSED` candidate with no pending deadline.
- Planner-test a `CLOSED` candidate with recovery epoch newer than the session
  baseline as `Direct`, equal/older epochs as `not_triggered`, and a mixed
  arbitrary-length prefix containing both recovered direct and due probe
  targets in route order.
- Planner-test account recovery separately: current fresh Provider epoch newer
  than the Session account baseline produces Direct only for `CLOSED`; equal,
  older, zero, stale, failed, reblocked, `OPEN`, and `HALF_OPEN` never gain an
  account-derived Direct. Mix circuit and account recoveries across an
  arbitrary prefix and preserve route order.
- Circuit-test that only an applied dispatched probe success publishes a
  monotonic recovery epoch. Failure, stale generation, abandon, and ordinary
  success must leave it unchanged, and publication must not expose the global
  epoch before the Provider snapshot carries the same epoch.
- Session-test that a new binding captures the current global recovery baseline
  exactly once. Sliding TTL, same-Provider success, route confirmation, and an
  in-flight loser completion preserve it; expiry/clear plus recreation captures
  the then-current baseline.
- Session-test the independent account baseline under the same lifecycle, then
  route-test at least two live Sessions observing one account recovery without
  global consumption. Refresh alone must not bind either Session.
- Session- and stream-finalizer-test both completion orders for two same-session
  request tokens. The later-started successful request must own the final
  binding, and an older late stream completion must be rejected.
- Resolution-test `[p3, p1, p2]` plus targets `[p1, p2]` becomes
  `[p1, p2, p3]`. Add target IDs that are duplicated or absent and assert they
  are ignored without disturbing non-target relative order. A not-triggered ID
  must not be moved merely for observation.
- Route-test current P3 with eligible P1/P2: P1 failure then P2 success makes
  exactly one P1 and one P2 upstream chain, makes zero P3 calls, stops after P2,
  returns/routes to P2, and binds P2.
- Route-test current P5 with four planned higher-priority targets and assert the
  exact P1 -> P2 -> P3 -> P4 call order before P5 is allowed. Success at any
  intermediate target must make all later target and fallback call counts zero.
- Route-test cooldown, in-flight lease, natural not-triggered, common-gate
  denial, and local pre-send failure on an earlier target. Assert zero calls and
  stable structured evidence for that target, then successful dispatch of a
  later eligible target.
- Route-test mixed direct/probe targets. Assert direct attempts do not carry
  `selection_method="circuit_probe"` or probe metadata, each probe uses its own
  trigger/generation/lease, and the serial request never holds two executing
  probe leases.
- Route-test all planned targets failing or skipping before current P3. Assert
  P3 is called only after them, its complete success supplies the response and
  final route, and the session remains/rebinds to P3. Preserve the committed
  streaming-response terminal behavior.
- Route-test one natural-mode winner plus a data-driven set of at least three
  follower sessions against a gated real TCP upstream. Before winner release,
  assert exactly one P1 network call and follower `in_flight -> P2`; after
  trusted success, assert each follower's next request is direct P1, carries no
  probe metadata, does not increase P2 calls, and binds P1. Use an explicit
  accept/release barrier and bounded timeout, not a sleep-based race.
- Route-test a failed/stale winner publishes no recovery epoch and therefore
  cannot make followers direct-dispatch a still-unhealthy Provider.
- Route-test with explicit barriers that an old same-session request reaches and
  blocks on P2, a P1 winner publishes recovery, and a newer request binds P1
  direct before the old P2 response is released. After releasing P2, assert the
  old response still returns normally while the session binding remains P1.
- Dispatch-intent-test that gate/cooldown/in-flight/Ready-limit/pre-send exits
  leave the reservation available; the first planned transport send consumes
  it once; later sends cannot consume it; and an all-zero-send target chain
  releases it on drop. Keep persistence-failure rollback and fail-closed tests.
- Route-test that skipped candidates consume no Ready-provider slot, while each
  Ready candidate consumes the existing cap. Include the boundary before the
  next Ready candidate and `Ready, Ready, circuit-open/cooldown` at cap 2 so the
  third denial remains visible without a third network call.
- Route-test account denial before circuit/probe/limits/Ready and assert stable
  account code/reason, empty circuit/probe/index metadata, zero upstream calls,
  unchanged circuit snapshot, and fallback success. Cover zero balance and
  expiry through the shared normalized projection.
- Route-test an account-only all-unavailable 503 with no `Retry-After`, then a
  mixed account plus circuit/limit 503 with the other gate's trusted header.
  Make the blocked snapshot available and repeat the exact request fingerprint;
  assert it reaches upstream, proving no recent-error cache write.
- Unit-test gate-only terminal classification with both account reason codes so
  they cannot regress to `GW_UPSTREAM_ALL_FAILED`.
- Unit-test selection so a temporarily denied bound provider stays in the
  candidate list while reuse selection returns no effective bound provider.
- Preserve all-open recovery tests: a persisted ineffective binding, P1 probe
  failure then P2 probe success, P1 cooldown, P1 in-flight, and all-cooldown.
  Assert ordered `new_unbound_session` metadata, per-provider single-flight,
  correct binding/closure, zero denied calls, and 503 only when all are denied.
- Circuit-test that a counted `CLOSED` failure arms the natural deadline, a
  later failure rearms it from the newer timestamp, complete success clears it,
  reload preserves it, and a max-wait configuration update recomputes it.
- Use `SYNTHETIC_SECRET` in 401 and 403 bodies; assert console output, attempt
  serialization, and error details omit it without changing failover/auth
  classification or recorded status.
- Keep forced-provider, single-candidate, model-discovery strict-attempt,
  health-neutral, warmup, token-accounting, compaction-request, and managed
  model-route regressions passing; shared planning/gate changes must not broaden
  their eligibility or budgets.
- Frontend-test provider, transition, and attempt counts with skips and retries.
  Persist and project P1 `probe_result="not_triggered"` plus P2 success: detail
  metadata remains, while summary/live route is `[P2]`, `attempt_count=1`, and
  `has_failover=false`. Keep an ordinary-skipped case proving it still counts.
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

Do not collapse account recovery into circuit state or cache its uncertain
eligibility:

```rust
// Wrong: refresh time is not a recovery deadline and circuit state is not an
// account-usage signal.
earliest_available_unix = next_account_refresh;
circuit.record_success(provider_id);

// Correct: common gate reads the current projection, while natural planning
// compares a separate Provider epoch to this Session's account baseline.
let route = account_runtime.route_read(target, monotonic_now, wall_now);
let recovered = circuit.state == Closed
    && route.recovery_epoch > session.account_usage_recovery_epoch_baseline;
```

Do not use a persisted but circuit-denied binding as the probe planner anchor:

```rust
// Wrong: an OPEN first-provider binding produces stable_index == 0 and no probe.
let bound_provider_id = session_snapshot.and_then(|snapshot| snapshot.provider_id);

// Correct: reuse resolution returns None while retaining the candidate for gate evidence.
let bound_provider_id = ctx.session_bound_provider_id;
```

Do not truncate an ordinary higher-priority prefix or let session rotation put
the current provider between planned targets:

```rust
// Wrong: P1 failure falls through to current P3 before eligible P2.
if let Some(target) = higher_priority_prefix.iter().find_map(plan_candidate) {
    move_provider_to_front(&mut providers, target.provider_id);
}

// Correct: preserve every eligible target and restore them as one stable prefix.
let targets = higher_priority_prefix
    .iter()
    .filter_map(plan_candidate)
    .collect::<Vec<_>>();
providers = stable_targets_first(providers, &targets);
```

Natural eligibility is per candidate, so a not-due provider records an
observation and scanning continues:

```rust
// Wrong: P1 suppresses a due P2.
if !candidate_is_due(candidate, now) {
    return stay_decision(not_triggered_provider_ids);
}

// Correct: keep exact observation data and continue through the prefix.
if !candidate_is_due(candidate, now) {
    not_triggered_provider_ids.push(candidate.provider_id);
    continue;
}
targets.push(plan_due_candidate(candidate));
```

Do not let a pre-send skip decide the request-scoped reservation lifecycle:

```rust
// Wrong: P1 gate denial discards the trigger before P2 can really send.
if gate_denied {
    intent.release_unclaimed_reservation();
    continue;
}

// Correct: gate/preparation skips leave ownership on the request intent. At the
// send boundary, fail closed unless the first planned sender commits it once.
intent.commit_reservation_at_transport_send(provider_id)?;
transport.send(request).await?;
```

All-open unbound recovery remains an ordered compatibility branch using the
same target-list execution model:

```rust
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
