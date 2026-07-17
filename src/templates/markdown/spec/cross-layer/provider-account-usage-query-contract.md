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
- Account usage is display-only. Refresh never tests availability, resets a
  circuit, mutates/reorders providers, or changes routing health.

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

### 5. Good / Base / Bad Cases

- Good: initial request A returns balance 0 late, manual request B returns
  balance 9 first, and both the observer and cache remain at 9 after A settles.
- Good: a fresh Infinity-stale cache still produces a new IPC when the user
  requests a manual refresh.
- Base: one configured enabled provider performs its initial query and optional
  timed refetch through the shared options.
- Bad: manual B writes balance 9 with `setQueryData`, then older automatic A
  completes through `useQuery` and restores balance 0.
- Bad: a successful availability test is required before account refresh, or a
  refresh invalidates circuit/provider-list state.

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

## Scenario: NewAPI Model-Token Billing Adapter

### 1. Scope / Trigger

Use this scenario when changing the NewAPI account-usage adapter, its endpoint
construction, authentication, response parsing, unit normalization, body
limits, or error presentation. The production IPC entry remains
`provider_account_usage_fetch`; this scenario owns the Rust boundary between a
configured model API key and the display-only `ProviderAccountUsageResult`.

This scenario does not change the query-owner rules above and does not apply
account-usage results to routing, circuit state, provider availability, order,
or enablement.

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
- Account usage is display-only. Fetching and parsing must not test or mutate
  routing, circuit/cooldown state, availability, provider order/enablement,
  OAuth quota, or local request usage.

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
- Base: USD status, a valid subscription, and valid usage produce one complete
  display snapshot with optional expiry.
- Bad: parse quota-like fields before checking `success=false`, causing an auth
  error to appear as a missing-field error.
- Bad: accept a partial snapshot when status or one billing endpoint fails, or
  hard-label an unknown unit as USD.
- Bad: feed account usage into availability, provider selection, circuit reset,
  or automatic provider disablement.

### 6. Tests Required

- Cover Base URL root and trailing `/v1` forms, exact derived paths, same-origin
  enforcement, disabled redirects, and rejection of credentialed/invalid URLs.
- Capture requests and assert status has no authentication, billing requests
  have Bearer authentication, no request has `New-Api-User`, and usage dates
  follow both payment-method branches.
- Assert the exact USD formula, finite overage/negative balance behavior,
  zero/expired/available status mapping, and unchanged positive expiry mapping.
- Assert `access_until` preserves exact integers above `2^53`, while floats,
  strings, and integers outside `i64` fail closed.
- Cover root `success=false`, root error, missing required fields, negative raw
  values, non-finite representations, unknown/non-USD units, invalid JSON,
  body caps, and each partial-endpoint failure.
- Assert errors contain no upstream message, body, key, host, PII, token name,
  or actual account amount; fixtures must remain wholly synthetic.
- Keep all sub2api fixtures green and prove its parser and redirect behavior do
  not change. Keep the query-owner, manual-refresh race, display rendering, and
  routing/circuit/availability isolation tests green.
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
