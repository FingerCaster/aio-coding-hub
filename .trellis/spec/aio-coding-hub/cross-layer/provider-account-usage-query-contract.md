# Provider Account-Usage Query Contract

## Scenario: Change Account-Usage Fetching Or Refresh

### 1. Scope / Trigger

Use this contract when changing provider account-usage query keys, automatic or
timed refresh, the manual refresh button, provider edit/delete cache cleanup, or
the account-usage IPC adapter. A Tauri IPC Promise may continue after logical
cancellation, so cache ownership and completion ordering are part of the
contract.

### 2. Signatures

The shared query owner is defined in `src/query/providers.ts`:

```ts
providerAccountUsageKeys.detail(providerId: number);

providerAccountUsageQueryOptions(providerId: number);

refreshProviderAccountUsage(
  queryClient: QueryClient,
  providerId: number,
): Promise<ProviderAccountUsageResult | null>;

useProviderAccountUsageQuery(provider: ProviderSummary, enabled?: boolean);
```

The query function calls `providerAccountUsageFetch(providerId)` and returns
`ProviderAccountUsageResult | null`.

The shared remote-request cache boundary is:

```rust
pub(crate) fn apply_account_usage_cache_busting(
    request: reqwest::RequestBuilder,
) -> reqwest::RequestBuilder;

pub(crate) fn apply_account_usage_cache_busting_to_request(
    request: &mut reqwest::Request,
);
```

### 3. Contracts

- Automatic initial fetch, timed refetch, and manual refresh use the exact same
  provider-scoped key, top-level query function, and base query options.
- Base options keep `staleTime: Infinity` and `retry: false`. Manual refresh
  must first cancel that exact key, then call `fetchQuery` with the shared
  options and `staleTime: 0` so a fresh cache entry cannot suppress the IPC.
- Manual refresh must not call the IPC directly and then use `setQueryData`.
  Query completion is the only owner allowed to commit a fetched result.
- Logical TanStack cancellation must prevent an older automatic, timed, or
  manual Promise from committing after a newer manual result, even if the
  underlying Tauri IPC cannot be physically aborted.
- The account component derives loading and error presentation from the query's
  `isFetching` and `error`. Local component state must not become a second
  request lifecycle or cache owner.
- Cache identity is provider ID. Editing or deleting a provider removes only
  that provider's account-usage key; refresh does not invalidate another
  provider, provider lists, OAuth quota, availability, or gateway circuit data.
- The TanStack query is the display-cache owner; it never directly tests
  availability, resets a circuit, mutates/reorders providers, or writes route
  state. When a Provider explicitly enables `routeGateEnabled`, the same
  backend refresh completion may also update the process-owned route
  projection described below. React remains neither a route owner nor a
  second account-usage cache.
- Every remote account-usage request must pass through the shared cache boundary
  immediately before sending. It sets `Cache-Control: no-cache, no-store` and
  `Pragma: no-cache` on both `RequestBuilder` and already-built `Request`
  values. This applies to sub2api, NewAPI billing/account, and custom adapter
  requests; a Provider availability test is never a cache-refresh prerequisite.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Provider ID is not a positive safe integer | Reject with `SEC_INVALID_INPUT` |
| Cached data is fresh under `staleTime: Infinity` | Manual refresh still starts one new IPC |
| Older initial/timed Promise finishes after manual result | Ignore old completion; cache remains manual result |
| Second manual refresh starts before the first finishes | Cancel first lifecycle; latest result owns cache |
| Manual request is pending | Query reports fetching; refresh button remains disabled |
| Manual request fails | Query exposes the error; unrelated cached data is untouched |
| Provider is disabled | No automatic fetch; existing manual/configured behavior is preserved |
| Another provider has cached usage | Refresh leaves that exact key unchanged |
| Remote account-usage request is built | Both cache-control directives are present before send |

### 5. Good / Base / Bad Cases

- Good: initial request A returns balance 0 late, manual request B returns
  balance 9 first, and both the observer and cache remain at 9 after A settles.
- Good: a fresh Infinity-stale cache still produces a new IPC when the user
  requests a manual refresh.
- Base: one configured enabled provider performs its initial query and optional
  timed refetch through the shared options.
- Good: a fresh local query forces a remote request with cache-bypass headers,
  so an intermediary cannot reuse a stale zero-balance response.
- Bad: manual B writes balance 9 with `setQueryData`, then older automatic A
  completes through `useQuery` and restores balance 0.
- Bad: a successful availability test is required before account refresh, or a
  refresh invalidates circuit/provider-list state.
- Bad: add cache headers only to one built-in adapter, or let a custom script's
  caller-controlled headers omit them before executing its Request.

### 6. Tests Required

- Assert hook and manual paths use the same key and query function; manual
  refresh must call exact `cancelQueries` then forced `fetchQuery`.
- Use deferred Promises for initial-old/manual-new and timed-old/manual-new
  reversed completion. Assert both observer data and query cache after the old
  Promise settles.
- Cover repeated manual entry points and prove the older lifecycle is canceled
  while the latest result remains cached.
- Component-test query-owned loading, disabled-button timing, success display,
  and error display.
- Assert refresh does not call availability, circuit reset, provider mutation,
  reorder, duplicate, or OAuth quota operations and does not invalidate other
  caches.
- Capture at least one request from each built-in family and a custom Request;
  assert both `Cache-Control: no-cache, no-store` and `Pragma: no-cache` are
  present at the final send boundary.
- Keep disabled-provider, interval, and provider edit/delete cleanup regressions
  passing, followed by typecheck, lint, format, and diff checks.

### 7. Wrong vs Correct

#### Wrong

```ts
const next = await providerAccountUsageFetch(providerId);
queryClient.setQueryData(providerAccountUsageKeys.detail(providerId), next);
```

This creates a second writer outside the query fetch lifecycle, so an older
automatic completion can overwrite the manual result.

#### Correct

```ts
const options = providerAccountUsageQueryOptions(providerId);
await queryClient.cancelQueries({ queryKey: options.queryKey, exact: true });
return queryClient.fetchQuery({ ...options, staleTime: 0 });
```

Cancellation establishes the new ordering boundary, and the forced shared
query remains the only cache writer.

For the remote request boundary:

```rust
let request = apply_account_usage_cache_busting(client.get(url));
let custom_request = validate_and_build_request(...)?;
client.execute(custom_request).await?;
```

The first line is required for builder-based built-in calls. The custom request
constructor applies the Request-level helper before returning, so callers
cannot forget it at execution time. A new IPC request without these headers is
not an authoritative refresh guarantee.

## Scenario: Explicit Account-Usage Route Gate And Recovery

### 1. Scope / Trigger

Use this scenario when changing `routeGateEnabled`, account-usage runtime
targets or leases, route snapshot projection, background Gateway refresh,
Provider mutation invalidation, or the account-usage recovery epoch. The
feature is opt-in and fail-open: missing or uncertain account data must never
remove an otherwise eligible Provider.

### 2. Signatures

The canonical extension adds one independent field:

```text
routeGateEnabled: boolean, default false
```

The backend target and synchronous request boundary are:

```rust
struct ProviderAccountUsageTarget {
    provider_id: i64,
    schedule: ProviderAccountUsageRefreshSchedule,
    adapter_kind: ProviderAccountUsageAdapterKind,
    config_token: [u8; 32],
    custom_permission_ready: bool,
}

enum ProviderAccountUsageRouteProjection {
    ConfirmedAvailable,
    Blocked(ProviderAccountUsageBlockReason),
    UnknownAllow,
}

enum ProviderAccountUsageBlockReason { ZeroBalance, Expired }

struct ProviderAccountUsageRouteRead {
    projection: ProviderAccountUsageRouteProjection,
    recovery_epoch: u64,
}

struct AccountUsageRecoveryInput<'a> {
    provider_recovery_epochs: &'a [(i64, u64)],
    blocked_provider_ids: &'a [i64],
    session_recovery_epoch_baseline: u64,
}

ProviderAccountUsageTarget::from_gateway_fetch_context(provider_id, context);
ProviderAccountUsageRuntimeState::route_read(target, monotonic_now, wall_now_unix);
ProviderAccountUsageRuntimeState::global_recovery_epoch();
```

`ProviderForGateway.account_usage_route_target` is computed after extension
values are loaded from SQLite. Planner and common-gate reads reuse that target;
they do not parse extension JSON or recompute its token.

### 3. Contracts

- Missing or non-boolean `routeGateEnabled` normalizes to `false`. It is
  independent from `timedRefreshEnabled`; the latter remains a display
  scheduling preference.
- A Gateway target requires an enabled direct API-key Provider, a configured
  adapter, `routeGateEnabled=true`, and, for custom scripts, a current local
  permission confirmation. Missing config, gate off, OAuth, source Provider,
  disabled Provider, invalid adapter, or unconfirmed custom config creates no
  Gateway consumer and leaves routing unchanged.
- Gateway startup reconciles immediately, then every 5 seconds renews a
  15-second lease and exactly replaces the target set. Successful Provider
  create/update/duplicate/enable/delete and successful full-config import also
  request immediate reconciliation. Gateway stop releases its leases; an
  independent desktop lease may continue display refresh.
- Gateway targets refresh at the saved normalized 60..=300 second interval
  even when display timed refresh is off. Manual, desktop, and Gateway demand
  share one Provider generation, snapshot, in-flight request, force-tail rule,
  and global four-Provider concurrency cap. A request never waits for or starts
  a remote account query.
- Each coalesced manual refresh owns a checked, monotonic force epoch. Calls
  arriving while the same force is pending share that epoch; a force arriving
  during any in-flight fetch queues at most one immediate tail epoch. Ordinary
  background completions and lifecycle notifications may wake waiters but must
  not satisfy a force epoch. Pending/in-flight force work keeps the scheduler
  active independently of desktop/Gateway leases and timed refresh settings.
- A force epoch is complete only after its current-generation result is stored
  in the display snapshot and the matching route projection is published.
  The waiter validates config token and generation and clones that snapshot
  under the same lock; target replacement wakes it with an unavailable result
  instead of returning either the old or a replacement target's snapshot.
- The coordinator query loads only Provider identity, transport shape, and
  canonical extension config. API keys and private NewAPI credentials are
  loaded only by the existing uncached fetch boundary after a target is due.
- A route snapshot is trusted only when generation, token, adapter, and saved
  interval match; freshness is `Fresh`; display time exists and is not in the
  future; all numeric fields are finite; and monotonic age is strictly less
  than `2 * refreshIntervalSeconds`. Age equal to the boundary is stale. The
  one-hour display cache never extends route trust.
- Fresh consistent `ZeroBalance` blocks only when neither `balance` nor
  `plan_remaining` is a finite positive value. Fresh consistent `Expired`
  blocks unless `expires_at` is in the future. An explicit amount-free custom
  zero/expired status may block. Positive-amount zero, future-expiry expired,
  invalid numbers, past-expiry available, missing/stale/future snapshots, and
  unsupported/config/auth/query failures return `UnknownAllow`.
- Fresh Blocked stores `last_confirmed=Blocked` and clears the Provider epoch.
  In the same generation, only the first later fresh ConfirmedAvailable
  publishes a recovery epoch. Initial/continuous available, continuous
  blocked, contradictory output, and failures publish none. Failure or staleness
  hides any Provider epoch from readers but retains transition memory; a later
  fresh available may publish once after a blocked state or re-expose the
  already published epoch without incrementing it.
- Provider resolution reads each candidate once and projects that same result
  into either a nonzero recovery epoch or `blocked_provider_ids`. It must never
  combine an epoch from one read with a blocked classification from another.
- For an effectively bound fallback Session, `blocked_provider_ids`
  suppresses natural/compaction/route-change/aggressive higher-prefix targets,
  observations, and reservations while fresh Blocked remains trusted. For an
  effective-unbound Session, it may classify which candidates can actually
  recover: preserve the original route prefix through the selected unblocked
  target, so blocked entries still reach the common account gate before a
  later circuit probe. It never deletes ordinary candidates or bypasses the
  common send-time gate for unbound, current-bound, forced, or normal fallback
  selection.
- An all-blocked compaction prefix creates no reservation and leaves the
  fingerprint pending. Fresh ConfirmedAvailable removes the hint and exposes a
  newer recovery epoch; UnknownAllow removes the hint without fabricating a
  recovery signal, and the common gate still handles Available-to-Blocked races.
- Epoch increment is checked. While holding the route-state write lock, write
  snapshot, Provider epoch, and confirmed state first, then Release-store the
  process-global epoch. Session creation Acquire-loads the global baseline.
  Overflow leaves the Provider routable but publishes no recovery marker and
  never wraps.
- Config-token/generation changes, permission revocation, gate/adapter close,
  deletion, and runtime reset reject stale completion and remove old route
  state. Name, note, and display-only timed-refresh changes preserve query and
  transition state. Query semantics and route semantics are compared
  separately so changing only the route switch need not evict the display
  snapshot.
- Route state and diagnostics contain only stable adapter/status/reason,
  generation, epoch, and timing metadata. Amounts, messages, scripts, Origins,
  credentials, and upstream bodies never enter Gateway DTOs or logs.

### 4. Validation & Error Matrix

| Input / state | Route projection / action |
| --- | --- |
| Field missing, malformed, or false | No Gateway target; existing route behavior |
| Adapter disabled/invalid, Provider OAuth/source/disabled, or custom permission stale | No Gateway target; allow |
| Gate true and display timed refresh false | Gateway lease still refreshes at saved interval |
| Snapshot missing, lock contended, token/generation mismatch, or age `>= 2x` | `UnknownAllow`, recovery epoch hidden |
| Fresh `zero_balance` with no positive spendable amount | `Blocked(ZeroBalance)` |
| Fresh `zero_balance` with positive balance or plan remaining | `UnknownAllow`; publish no recovery |
| Fresh `expired` with future expiry | `UnknownAllow`; publish no recovery |
| Fresh consistent `expired` | `Blocked(Expired)` |
| Auth/config/query/unsupported result | `UnknownAllow`; retain same-generation transition memory |
| Bound fallback Session repeatedly sees fresh Blocked higher-priority P1 | Suppress P1 failback target/observation/reservation; log only the actual fallback request |
| New/effectively-unbound Session sees blocked P1 before an available P2 | Preserve both in route order; common gate records P1's account skip before P2 sends |
| Effective-unbound P1/P2 are blocked `CLOSED` and final P3 is `OPEN` | Preserve P1/P2 account skips, then allow the same request's `new_unbound_session` probe for P3 |
| Every effective-unbound candidate is blocked | Plan no recovery target; ordinary selection still records every account skip |
| Planner sees Available but common gate later sees Blocked | Record one account skip and fallback in that request; suppress P1 on the next request |
| Blocked then fresh available in same generation | Publish exactly one Provider/global recovery epoch |
| Reblocked after recovery | Clear Provider epoch immediately |
| Epoch is `u64::MAX` | Allow available; publish zero Provider marker; do not wrap |

### 5. Good / Base / Bad Cases

- Good: timed display refresh is off, route gating is on, and the Gateway lease
  continues one bounded shared refresh cadence without any UI mounted.
- Good: a blocked Provider produces a query failure and immediately becomes
  fail-open; the next fresh available result publishes one recovery signal,
  while repeated available results publish none.
- Good: a Session initially logs P1 account skip and binds P2; repeated
  post-compaction requests while P1 remains fresh Blocked contain only P2, and
  the first real P1 send after recovery consumes the pending fingerprint.
- Base: an upgraded Provider lacks the new field, creates no Gateway target,
  and follows the exact pre-feature route.
- Bad: reuse the one-hour display TTL for routing, retain a stale zero-balance
  block, or synchronously fetch account state from a model request.
- Bad: treat query failure as recovery, use a global consumed boolean, or
  publish the global epoch before the Provider snapshot carries it.
- Bad: consume the Session trigger when the account gate skips, or repeatedly
  create an intent for a target that is known to return before `claim_for_provider`.

### 6. Tests Required

- Rust/TypeScript config tests must cover missing/invalid false defaults, field
  independence, and disabled/unsupported Provider target exclusion.
- Projection table tests must cover intervals 60 and 300 at `2x-1` and exact
  `2x`, future display time, config/generation/adapter mismatch, all statuses,
  absent/negative/positive amounts, future/past expiry, non-finite fields, and
  sub2api/NewAPI/custom normalized results.
- Scheduler tests must cover timed-off Gateway demand, manual force-tail
  coalescing, event-driven tail dispatch below the periodic scheduler tick,
  force-epoch identity, simultaneous manual waiters, exact target replacement,
  5/15-second lease lifecycle, hard display expiry, four-Provider nonwaiting
  concurrency, and Gateway stop. The zero-to-positive case must run through the
  shared Sub2API/NewAPI/custom runtime without a Provider availability test.
- Recovery tests must cover blocked-to-available exactly once,
  blocked-to-error-to-available, temporary epoch hiding/re-exposure, reblock,
  stale generation, token/permission changes, Release/Acquire publication, and
  checked overflow.
- Planner tests must cover stable-bound blocked suppression for natural and
  explicit triggers, later-due candidates, all-blocked Stay, unbound audit, and
  route-change confirmation. Cover an effective-unbound blocked `CLOSED`
  prefix followed by an `OPEN` candidate, including the original-prefix target
  order and the all-blocked Stay case. Route tests must cover same-request final
  probing plus initial skip, two repeated same-Session compaction requests with
  one fallback attempt each, then recovery and real fingerprint consumption.
- Integration tests must prove DB-loaded `ProviderForGateway` stores the
  precomputed target, request reads perform no account network wait, and
  successful mutation/import reconciliation removes obsolete targets.
- Keep full Rust, generated bindings, frontend tests, typecheck, lint, format,
  Clippy, secret audit, and diff checks green.

### 7. Wrong vs Correct

#### Wrong

```rust
// Reparse config and wait for a balance request in every model request.
let config = config_from_extension_values(&provider.extension_values);
let latest = fetch_account_usage(provider.id).await?;
if latest.balance == Some(0.0) { deny(); }
```

#### Correct

```rust
let Some(target) = provider.account_usage_route_target.as_ref() else {
    return UnknownAllow;
};
match runtime.route_read(target, Instant::now(), now_unix).projection {
    Blocked(reason) => deny_with_stable_reason(reason),
    ConfirmedAvailable | UnknownAllow => allow(),
}
```

Provider loading owns canonical target construction; the request path is a
bounded synchronous memory read and every uncertain state fails open.

Do not turn the planning hint into a second eligibility list:

```rust
// Wrong: hides the first skip and changes unbound/forced behavior.
providers.retain(|provider| !blocked_provider_ids.contains(&provider.id));

// Correct: only stable-bound higher-priority failback candidates are omitted;
// every actually selected candidate still reaches the common gate.
let failback_targets = higher_prefix
    .iter()
    .filter(|provider| !blocked_provider_ids.contains(&provider.id));
```

## Scenario: NewAPI Model-Token Billing Adapter

### 1. Scope / Trigger

Use this scenario when changing the NewAPI account-usage adapter, its endpoint
construction, authentication, response parsing, unit normalization, body
limits, or error presentation. The production IPC entry remains
`provider_account_usage_fetch`; this scenario owns the Rust boundary between a
configured model API key and the normalized `ProviderAccountUsageResult`.

This scenario does not change the query-owner rules above. The adapter and
parser have no direct routing, circuit, availability, ordering, or enablement
side effects; only the explicit runtime route-gate scenario above may project
their normalized result into a route decision.

### 2. Signatures

The production entry and NewAPI protocol helpers are:

```rust
pub(crate) async fn provider_account_usage_fetch(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    provider_id: i64,
) -> Result<ProviderAccountUsageResult, String>;

pub(crate) fn build_newapi_billing_urls(base_url: &str)
    -> Result<NewapiBillingUrls, String>;

pub(crate) async fn fetch_newapi_account_usage(
    base_url: &str,
    api_key: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult;

pub(crate) fn parse_newapi_billing_responses(
    status_body: &Value,
    subscription_body: &Value,
    usage_body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult;
```

`build_newapi_billing_urls` produces one same-origin public status URL and two
same-origin billing URLs. `fetch_newapi_account_usage` constructs its own
no-redirect client and owns the bounded GET sequence, so a caller cannot weaken
redirect policy. `parse_newapi_billing_responses` owns all-or-nothing validation
and normalization. The existing generated IPC DTO remains the frontend boundary.

### 3. Contracts

- Normalize a configured Base URL to its deployment root, accepting both a
  root URL and a URL with a trailing `/v1`. Derive only these same-origin paths:
  `/api/status`, `/v1/dashboard/billing/subscription`, and
  `/v1/dashboard/billing/usage`.
- The status GET is unauthenticated. The subscription and usage GETs use only
  the configured model key as Bearer authentication. Never send
  `New-Api-User` on this path.
- Disable redirects for the NewAPI client so credentials cannot follow a
  redirect target. Keep sub2api's existing redirect behavior unchanged.
- Read `quota_display_type` from status before interpreting billing values.
  The implemented unit is exactly `USD`; unknown and non-USD values fail
  closed and must not inherit a USD label.
- For USD, require finite, non-negative numeric `hard_limit_usd` and
  `total_usage`, then normalize `total = hard_limit_usd`,
  `used = total_usage / 100`, and `balance = total - used`. A finite negative
  derived balance is valid overage data and is passed to the shared status
  mapping; it is not a cross-endpoint inconsistency.
- Treat `hard_limit_usd == 100_000_000.0` as the one exact NewAPI
  model-token unlimited sentinel. Return `Available` with the local
  "model token unlimited" message and leave `total`, `used`, `balance`, unit,
  and expiry empty. Do not use a greater-than threshold: adjacent finite
  values remain ordinary model-token limits.
- `access_until` must be an exact JSON integer representable as `i64`.
  Floating-point values such as `1.0`, strings, and out-of-range integers fail
  closed. Values `<= 0` mean no expiry; values `> 0` map unchanged to
  `expires_at`.
- Select the usage date window from subscription payment-method semantics:
  month start through today when a payment method exists, otherwise the
  established inclusive 100-day window through today.
- On every response, recognize root `success=false` and a root `error` before
  required-field parsing. An authenticated `success=false` is an auth failure;
  other application errors are query failures. Never expose the upstream
  message.
- The snapshot is all-or-nothing. A transport, HTTP, application, JSON, size,
  type, unit, or required-field failure at any endpoint returns no partial
  total, used amount, balance, unit, or expiry.
- Enforce explicit response limits: status 16 KiB, subscription 8 KiB, usage
  8 KiB, and sub2api 32 KiB.
- Upstream bodies/messages, API keys, PII, hosts, token names, and actual
  account amounts must not enter logs, IPC errors, fixtures, research output,
  or this spec. Fixtures use synthetic values and reserved test hosts only.
- Fetching and parsing must not directly test or mutate circuit/cooldown state,
  availability, provider order/enablement, OAuth quota, or local request usage.
  An explicitly enabled route consumer may read the completed normalized
  snapshot only through the shared runtime projection.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Base URL is the deployment root | Derive the three documented same-origin paths |
| Base URL ends in `/v1` | Remove only that trailing API segment before deriving paths |
| Base URL contains credentials, lacks an HTTP(S) origin, or cannot be parsed | Configuration-required result; send no request |
| Status request | No Authorization or `New-Api-User` header |
| Subscription or usage request | Bearer model key only; no `New-Api-User` |
| NewAPI response redirects | Do not follow; fail the snapshot without forwarding credentials |
| `quota_display_type` is `USD` | Apply the documented billing formulas and label USD |
| Unit is missing, unknown, non-USD, or differently cased | Query failure with no unit or amounts |
| Raw total/usage is missing, negative, non-numeric, NaN, or infinite | Query failure with no partial snapshot |
| `hard_limit_usd` equals the exact unlimited sentinel | `Available`; no amount, unit, or expiry fields |
| `hard_limit_usd` is merely near or above the sentinel | Parse as an ordinary finite model-token limit |
| Used amount exceeds total | Preserve the finite negative derived balance and use shared status mapping |
| `access_until` is an exact in-range JSON integer | Preserve it exactly; map positive values to `expires_at` |
| `access_until` is `<= 0` | Successful snapshot with no expiry |
| `access_until` is a float, string, or outside `i64` | Query failure with no partial snapshot |
| Root `success=false` on an authenticated response | `AuthFailed` before required-field validation |
| Root `error` or unauthenticated application error | `QueryFailed` before required-field validation |
| Any one endpoint returns non-success HTTP, invalid JSON, or exceeds its cap | Fail the whole snapshot; never reuse values from another endpoint |
| Account usage fetch completes | No routing, circuit, availability, or provider mutation side effect |

### 5. Good / Base / Bad Cases

- Good: a Base URL ending in `/v1` yields one public status request and two
  same-origin Bearer billing requests; the model key never reaches status or a
  redirect target.
- Good: finite non-negative raw values produce total, used, and their exact
  finite difference; an overage produces a negative balance rather than a
  fabricated zero or query failure.
- Good: a large exact `i64` JSON expiry is preserved without passing through
  floating-point conversion.
- Good: the exact official unlimited sentinel produces an amount-free
  available result, while adjacent finite totals still use the normal formula.
- Base: USD status, a valid subscription, and valid usage produce one complete
  display snapshot with optional expiry.
- Bad: parse quota-like fields before checking `success=false`, causing an auth
  error to appear as a missing-field error.
- Bad: accept a partial snapshot when status or one billing endpoint fails, or
  hard-label an unknown unit as USD.
- Bad: let the adapter directly mutate availability, selection, circuit state,
  or Provider enablement instead of publishing one normalized snapshot.

### 6. Tests Required

- Cover Base URL root and trailing `/v1` forms, exact derived paths, same-origin
  enforcement, disabled redirects, and rejection of credentialed/invalid URLs.
- Capture requests and assert status has no authentication, billing requests
  have Bearer authentication, no request has `New-Api-User`, and usage dates
  follow both payment-method branches.
- Assert the exact USD formula, finite overage/negative balance behavior,
  zero/expired/available status mapping, and unchanged positive expiry mapping.
- Assert exact-sentinel equality, empty amount/unit/expiry fields for unlimited
  model tokens, and ordinary finite parsing immediately below and above it.
- Assert `access_until` preserves exact integers above `2^53`, while floats,
  strings, and integers outside `i64` fail closed.
- Cover root `success=false`, root error, missing required fields, negative raw
  values, non-finite representations, unknown/non-USD units, invalid JSON,
  body caps, and each partial-endpoint failure.
- Assert errors contain no upstream message, body, key, host, PII, token name,
  or actual account amount; fixtures must remain wholly synthetic.
- Keep all sub2api fixtures green and prove its parser and redirect behavior do
  not change. Keep the query-owner, manual-refresh race, display rendering,
  explicit route-projection, and circuit/availability side-effect tests green.
- Run focused Rust and frontend tests, generated-binding validation, typecheck,
  lint, Rust format/check/Clippy, secret/PII diff audit, and `git diff --check`
  in proportion to the change.

### 7. Wrong vs Correct

#### Wrong

```rust
// A model key is not a user access token.
client
    .get(format!("{origin}/api/user/self"))
    .bearer_auth(model_key)
    .header("New-Api-User", user_id);

let balance_usd = quota / 500_000.0;
```

This combines incompatible authentication contracts, may mask
`success=false` as a missing quota field, and assumes a deployment-specific
divisor and USD unit.

#### Correct

```text
GET {origin}/api/status
GET {origin}/v1/dashboard/billing/subscription  Authorization: Bearer <model-key>
GET {origin}/v1/dashboard/billing/usage         Authorization: Bearer <model-key>
```

```rust
// Internally normalizes three same-origin URLs, performs bounded requests,
// and returns a snapshot only after complete response validation.
fetch_newapi_account_usage(base_url, model_key, fetched_at, now).await
```

The NewAPI client refuses redirects, every URL remains on the normalized
origin, status selects the display unit, and the billing parser applies the
documented formula only after application-error and field validation.

## Scenario: NewAPI User-Account Mode And Private Credentials

### 1. Scope / Trigger

Use this scenario when changing the NewAPI billing/account mode selector,
account credentials, provider summaries or save patches, the v38 private
credential table, the status plus user-account HTTP contract, or the editor's
configured/missing/clear behavior.

The account path is an explicitly selected alternative to model-token billing.
It is never inferred from a provider name, host, response amount, or a legacy
User ID.

### 2. Signatures

```rust
pub(crate) enum NewapiQueryMode { Billing, Account }

pub(crate) struct ProviderAccountUsageCredentialsPatch {
    pub new_api_user_id: Option<String>,
    pub new_api_access_token: Option<String>,
    pub clear_new_api_access_token: bool,
}

pub(crate) async fn fetch_newapi_user_account_usage(
    base_url: &str,
    access_token: &str,
    user_id: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult;

pub(crate) fn parse_newapi_account_responses(
    status_body: &Value,
    account_body: &Value,
    expected_user_id: &str,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult;
```

The private SQLite owner is:

```sql
CREATE TABLE provider_account_usage_credentials (
  provider_id INTEGER PRIMARY KEY,
  newapi_user_id TEXT,
  newapi_access_token_plaintext TEXT,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
);
```

`ProviderSummary` may expose the normalized User ID and
`newapi_account_access_token_configured: bool`; it must never expose the
access token.

### 3. Contracts

- The built-in extension stores only `adapterKind`, `newApiQueryMode`,
  timed-refresh enablement, and refresh interval. Missing or unknown
  `newApiQueryMode` defaults to `billing`.
- v37-to-v38 migrates only a valid legacy `newApiUserId` into the private
  table, removes every private account field from extension JSON through the
  shared sanitizer, and cascades credential deletion with the provider.
- `None` for the entire credential patch preserves private credentials. In a
  supplied patch, a normalized positive User ID replaces the stored ID;
  absent/blank User ID clears it. A non-empty token replaces the token,
  absent/blank token preserves it, and the explicit clear flag removes it.
  Setting and clearing a token in one patch is invalid.
- User IDs contain ASCII digits only, canonicalize without leading zeroes, and
  must be in `1..=i64::MAX`, matching the signed Go `int` compatibility
  boundary used by supported NewAPI deployments. Tokens are at most 64 KiB
  and must form a valid Bearer header value.
- Switching query mode or adapter only changes extension config. It does not
  send a credential-clear patch. The explicit clear action removes both the
  User ID and token.
- Account mode loads only the private User ID/token. Missing either returns
  `ConfigurationRequired` before client creation or network I/O. It never
  reads or sends the provider model API key.
- Billing and sub2api paths load only the model API key and never read or send
  private account credentials.
- Normalize the Base URL exactly as the billing scenario does. Account mode
  derives only same-origin `/api/status` and `/api/user/self`, disables
  redirects, caps status and account bodies at 16 KiB, and uses a 15-second
  client timeout.
- Status is public. User-account GET authentication is exactly Bearer system
  access token plus `New-Api-User: <canonical-id>`.
- Account success must be the exact root boolean `true`; missing, false, or
  non-boolean success fails closed. Status requires exact `USD` and a finite
  positive `quota_per_unit`. Account identity must be a positive JSON
  integer representable as `i64` and exactly match the configured identity.
- `quota` may be any finite number and maps to `balance = quota /
  quota_per_unit`. `used_quota` must be finite and non-negative and maps to
  historical `used`. Account mode never fabricates `total`.
- Upstream messages, bodies, hosts, token names, credentials, identities, PII,
  and live amounts do not enter logs or IPC errors. Save diagnostics redact
  both nested token and User ID fields before logging.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Mode missing or unknown | Use billing; do not load account credentials |
| Account mode has only one credential | `ConfigurationRequired`; send no request |
| User ID is zero, signed, non-digit, or above `i64::MAX` | `SEC_INVALID_INPUT`; do not persist or import |
| Token is empty during an ordinary edit | Preserve the stored token |
| Explicit clear is selected | Delete User ID and token together |
| Set and clear token are both requested | `SEC_INVALID_INPUT`; preserve stored credentials |
| Status request | No Authorization or `New-Api-User` |
| Account request | Bearer account token and matching canonical `New-Api-User` |
| Redirect, non-success HTTP, invalid JSON, or body cap failure | Fail all-or-nothing; forward no credential |
| Account root `success` is not exactly `true` | `QueryFailed`; expose no amounts |
| Account ID is invalid or does not match | `QueryFailed` or `AuthFailed`; expose no amounts |
| Unit/divisor/value is missing, non-finite, or unsupported | `QueryFailed`; expose no partial result |
| Valid negative quota and non-negative historical use | Preserve negative balance; leave total empty |

### 5. Good / Base / Bad Cases

- Good: explicit account mode with complete private credentials performs one
  public status request and one private user request, then returns balance and
  historical use without a total.
- Good: mode and adapter switches preserve an unsaved secret draft and stored
  credentials; returning to account mode restores the same draft.
- Base: legacy NewAPI config without a mode continues through model-token
  billing even if a migrated User ID exists.
- Bad: infer account mode because a User ID exists, use the model API key as a
  system token, or accept an identity above the signed range.
- Bad: log a failed upsert payload before replacing both account identity and
  token fields with a redacted placeholder.

### 6. Tests Required

- Migration: valid/invalid legacy User IDs, sanitized extension JSON, v38
  idempotence, private table creation, and delete cascade.
- Persistence: whole-patch preserve, User ID set/clear, token preserve/set/
  clear, set-plus-clear rejection, local copy, and summaries without token.
- Protocol: root and trailing-`/v1` URLs, public/private headers, no redirects,
  both 16 KiB caps, exact success boolean, signed identity bounds and match,
  unit/divisor/value matrices, and all-or-nothing failures.
- Isolation: prove account mode never loads the model key and billing/sub2api
  never load private credentials; prove missing credentials send no request.
- Frontend: explicit selector, legacy billing default, partial-save allowed,
  configured/missing state, masked token, explicit clear, mode/adapter draft
  preservation, and nested diagnostic redaction.
- Run generated-binding validation, focused and full frontend/Rust tests,
  typecheck, lint, format, Clippy, secret/PII diff audit, and diff check.

### 7. Wrong vs Correct

#### Wrong

```rust
// Credential presence must not select the protocol.
if credentials.new_api_user_id.is_some() {
    fetch_user_self(base_url, model_api_key).await
}
```

#### Correct

```text
mode=billing -> model key -> status + subscription + usage
mode=account -> private User ID/token -> status + user/self
missing account credential -> local ConfigurationRequired, zero requests
```

Selection is explicit, each branch loads only its own credential class, and
every account response is identity- and unit-validated before projection.

## Scenario: sub2api Daily Rate-Limit Projection

### 1. Scope / Trigger

Use this scenario when changing sub2api `rate_limits` parsing or the daily
usage fields shown by `ProviderAccountUsageResult`. The parser may project a
proved daily window; it must not reinterpret periodic limits as wallet balance.

### 2. Signatures

```rust
pub(crate) fn parse_account_usage_response(
    adapter_kind: ProviderAccountUsageAdapterKind,
    body: &Value,
    fetched_at: i64,
    now_unix: i64,
) -> ProviderAccountUsageResult;
```

The existing result fields are `daily_used` and `daily_total`. No new
balance field or window-specific credential is introduced.

### 3. Contracts

- If `rate_limits` is present, it must be an array. Recognize only an exact
  string `window == "1d"`; unknown windows are ignored rather than guessed as
  day, week, or month.
- At most one `1d` entry is valid. Its `limit`, `used`, and `remaining`
  are finite and non-negative, `used <= limit`, and `remaining` equals
  `limit - used` within `max(1e-9, abs(limit) * 1e-9)`.
- `window_start` and `reset_at` must parse as timestamps and
  `reset_at > window_start`.
- A valid entry maps only `limit -> daily_total` and `used -> daily_used`.
  It does not populate `balance`, `plan_remaining`, weekly, or monthly
  fields.
- Existing root balance/remaining, plan remaining, subscription, expiry, and
  validity behavior remains unchanged. A successful validity-only payload with
  no recognized period remains `Available`.
- A malformed or duplicate known `1d` entry fails the result closed; the
  parser must not silently drop it and show a partially valid snapshot.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| No `rate_limits` | Preserve legacy sub2api parsing |
| `rate_limits` is not an array | `QueryFailed`; no partial daily values |
| Only unknown windows exist | Ignore them; do not invent periodic fields |
| One valid `1d` entry | Populate daily total/used only |
| Duplicate `1d` entries | `QueryFailed` |
| Negative/non-finite amount or `used > limit` | `QueryFailed` |
| Remaining arithmetic exceeds tolerance | `QueryFailed` |
| Missing/invalid timestamps or reset not after start | `QueryFailed` |

### 5. Good / Base / Bad Cases

- Good: an otherwise validity-only response with one consistent `1d` entry
  displays daily used/total and no wallet balance.
- Base: a legacy root balance or subscription payload parses exactly as before.
- Base: an unknown periodic window is ignored and does not become daily data.
- Bad: map `remaining` from a `1d` window into account `balance`.
- Bad: accept the first of two `1d` entries or silently ignore malformed
  known-window arithmetic.

### 6. Tests Required

- Cover one valid `1d` entry, absent limits, unknown-only windows, duplicate
  known windows, non-array input, every amount validation, tolerance boundary,
  timestamp parsing, and reset ordering.
- Assert daily projection leaves balance, plan remaining, weekly, and monthly
  fields untouched.
- Keep all legacy root-balance, plan, subscription, expiry, validity-only,
  authentication, body-cap, and display regressions green.
- Assert the parser has no direct Provider/circuit/order side effects; when the
  explicit route gate is enabled, only its normalized result may be consumed by
  the shared projection contract above.

### 7. Wrong vs Correct

#### Wrong

```rust
result.balance = rate_limit["remaining"].as_f64();
```

#### Correct

```rust
result.daily_total = Some(validated_limit);
result.daily_used = Some(validated_used);
// Account balance remains absent unless a legacy balance contract proves it.
```

Window semantics stay in their matching DTO fields; field-name similarity does
not create a wallet-balance contract.

## Scenario: Sub2API Subscription Window Maintenance Before Manual Refresh

### 1. Scope / Trigger

Use this scenario when a Sub2API Plus/订阅供应商跨日、周或月后，手动账户用量刷新仍
显示零额度。它只改变 Sub2API 的显式手动适配器请求；NewAPI、Custom、自动/定时/
Gateway 查询和 Provider 可用性测试保持原协议。

### 2. Signatures

```rust
enum ProviderAccountUsageFetchIntent {
    Background,
    Manual,
}

fn sub2api_usage_requires_window_maintenance(body: &serde_json::Value) -> bool;
fn build_sub2api_window_maintenance_url(usage_url: &str) -> Result<String, String>;
```

`ProviderAccountUsageRuntimeState` maps ordinary scheduling to `Background` and a
force epoch (including an immediate tail) to `Manual`. The intent is internal and
is not serialized through IPC.

### 3. Contracts

- The first request is always the existing bounded, cache-busting `GET /v1/usage`.
- A preflight is permitted only for `Manual` when the **raw first JSON** has exact
  `mode == "unrestricted"`, `isValid == true`, an object `subscription`, finite
  `remaining <= 0`, and at least one positive daily/weekly/monthly limit whose
  matching usage is finite, non-negative, and `used >= limit`. Numeric strings use
  the existing finite numeric parser; missing, malformed, negative-limit, and
  non-finite values fail closed.
- The preflight derives a same-origin `/v1/chat/completions` URL from the validated
  usage URL, preserving any path prefix and removing query/fragment. It sends the
  same Bearer credential, cache-bypass headers, `Content-Type: application/json`,
  and the exact body `{}` with no `model` field.
- Only HTTP 400 means the upstream maintenance path ran and the request reached
  model-parameter validation. Only then is a second bounded `GET /v1/usage` sent;
  its complete result is authoritative. Network errors and every other preflight
  status retain the first result and do not issue a second GET. A second GET error
  is returned as the normal query failure.
- The raw JSON predicate is the sole trigger source; a `QueryFailed`/other display
  DTO status caused by an unrelated field must not suppress a proven subscription
  maintenance attempt. Preflight responses are not read or logged.
- Background, timed, Gateway, wallet-zero, `quota_limited` API-key, invalid,
  incomplete, or authentication responses never send the preflight. No request
  mutates circuit, availability, Provider order, Session, route state, or cache
  ownership.

### 4. Validation & Error Matrix

| Input / condition | Required result |
| --- | --- |
| Manual + strict exhausted subscription JSON | `GET -> POST {} -> GET` |
| Manual + positive remaining or no complete positive period | One GET |
| Background/timed/Gateway intent | One GET |
| Wallet response without object subscription | One GET |
| `mode == quota_limited` or invalid/incomplete JSON | One GET |
| Preflight network failure or non-400 status | First result; no second GET |
| Preflight HTTP 400 | Second GET result, including normal query failure mapping |
| URL has credentials or invalid origin/path | No preflight; first result |
| URL has query/fragment | Strip them while deriving the same-origin maintenance URL |
| Any preflight or usage response contains secrets/body/PII | Nothing copied to logs or DTO diagnostics |

### 5. Good / Base / Bad Cases

- Good: a strict exhausted raw subscription response followed by `POST {}` 400 and
  a positive second usage response returns the second positive snapshot.
- Good: a malformed unrelated `rate_limits` field still permits the raw subscription
  maintenance predicate; the display parser does not become a second trigger owner.
- Base: a normal positive Sub2API response performs exactly one GET.
- Bad: treat a new GET, cache-busting header, or successful Provider test as proof that
  the upstream usage window was maintained.
- Bad: send a model, real prompt, or non-empty body; accept non-400 as recovery; or
  unconditionally preflight automatic refreshes.

### 6. Tests Required

- Domain table tests cover every predicate field, all three periods, aliases, finite
  numeric strings, malformed/negative values, and same-origin URL derivation.
- HTTP sequence tests capture exact method/path, Bearer and cache headers, `{}` body,
  absent `model`, prefix preservation, 400-only second GET, non-400 fail-closed,
  second-GET failure, raw-predicate-only triggering, and no-trigger payloads.
- Runtime tests assert `Background` for ordinary scheduling and `Manual` for force,
  including an automatic in-flight request followed by one coalesced manual tail.
- Keep NewAPI/Custom adapter isolation, route/circuit/availability isolation, query
  ordering, full Rust, generated binding, frontend typecheck/lint, Clippy, format,
  and diff checks green.

### 7. Wrong vs Correct

#### Wrong

```rust
let result = parse_account_usage_response(Sub2api, &body, fetched_at, now);
if result.status == ProviderAccountUsageStatus::ZeroBalance {
    // Reuse the display DTO or run an availability test to "refresh" it.
}
```

This confuses a local projection with the upstream state transition and can either
miss a real subscription window or create an unrelated provider side effect.

#### Correct

```rust
let (first, raw) = fetch_sub2api_account_usage_once(...).await;
if intent == Manual && raw.is_some_and(sub2api_usage_requires_window_maintenance) {
    let status = post_empty_chat_completions(...).await?;
    if status == HTTP_400 {
        return fetch_sub2api_account_usage_once(...).await.result;
    }
}
return first;
```

The adapter owns the explicitly bounded upstream maintenance handshake, while the
query/runtime layers remain cache owners without Provider-health side effects.
