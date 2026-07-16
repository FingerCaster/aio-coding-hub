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
