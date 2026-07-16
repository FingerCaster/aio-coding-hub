# Manual account-balance refresh root cause

## User observation

An upstream balance changed from zero to a positive value, but manual refresh kept showing zero. A
successful provider availability test followed by another manual refresh, or waiting for timed refresh,
eventually showed the new value.

## Verified code path

1. `src/query/providers.ts:156-163` implements manual refresh as an independent
   `providerAccountUsageFetch` followed by `queryClient.setQueryData`.
2. `src/query/providers.ts:443-461` configures the same provider key as an enabled `useQuery`, with an
   optional interval and `staleTime: Infinity`.
3. `src/components/providers/ProviderAccountUsageInline.tsx:143-169` tracks only the manual Promise in
   local `refreshing` state; it does not coordinate with the query's initial/interval fetch state.
4. `src/query/providers.ts:464-472` shows provider availability success only invalidates gateway
   circuit data. It does not refetch or mutate account usage.
5. `src-tauri/src/commands/providers/account_usage.rs:99-114` builds a fresh reqwest client and sends a
   new GET on every IPC invocation; there is no local HTTP response cache.

The two request producers write the same query key through different lifecycles. If automatic A begins
before manual B, B can set the new balance and then A can commit the older balance afterwards. This is
a deterministic cache ownership bug, not a requirement to run availability testing.

## History and coverage gap

- Commit `6396d35a8` introduced manual account usage and the direct `setQueryData` helper.
- Commit `2fa0a4826` later enabled initial and timed automatic fetch but left the manual helper
  independent, creating the concurrency window.
- `src/query/__tests__/providers.test.tsx:287-350` waits for automatic fetch to complete before invoking
  manual refresh. It verifies only serial success and cannot detect response reordering.

## Cache boundary

- Confirmed cache: TanStack Query in process, keyed by provider ID.
- Not present: a Rust client cache; a new client is built for each command call.
- Not observed during authorized NewAPI inspection: `Cache-Control`, `Age`, `ETag`, `Last-Modified`
  or `Expires` headers.
- The originally reported provider was not probed, so an upstream intermediary cache is not globally
  disproven. It is unnecessary to reproduce or explain the local stale overwrite and is not part of
  the minimal fix.

## Falsifiable conclusion

Use deferred Promises:

1. start automatic A returning old balance 0;
2. start manual B returning new balance 9;
3. resolve B, then resolve A;
4. final cache must remain 9.

The root conclusion is falsified if current direct-write code passes that ordering without additional
coordination. The fix is incomplete if it merely makes the button show 9 temporarily before A restores 0.

## Regression matrix

| Case | Expected |
| --- | --- |
| Initial A old, manual B new, B resolves first | final data B |
| Timed A old, manual B new, B resolves first | final data B |
| Manual while cache is fresh/Infinity | a new IPC call occurs |
| Two manual entry points | one controlled query lifecycle; deterministic result |
| Disabled provider | no automatic fetch; existing behavior preserved |
| Provider edited/deleted | exact account key removed |
| Availability test success | no account query mutation |
| Manual success/failure | no circuit, route, enable, sort or health side effect |

## Risk and rollback

Tauri IPC may not physically abort when TanStack cancels it; the critical guarantee is that the old
completion is ignored by query state. Roll back the frontend query/component commit if cancellation or
loading regressions occur; no persisted data changes are involved.
