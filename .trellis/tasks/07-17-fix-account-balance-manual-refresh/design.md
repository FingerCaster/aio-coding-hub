# Design: query-owned forced refresh

## Current race

```text
auto useQuery A starts -> manual IPC B starts -> B setQueryData(new)
                                           -> A query completion setData(old)
                                           -> UI shows old
```

The provider availability test does not update the account query. Its duration merely allows A to
settle before the next manual click, which explains the observed correlation.

## Target design

Create one account-usage query options factory that owns:

- `queryKey = providerAccountUsageKeys.detail(providerId)`
- `queryFn = providerAccountUsageFetch(providerId)`
- common retry/stale metadata used by automatic and manual paths

`useProviderAccountUsageQuery` consumes those options and adds enable/interval behavior. Manual
refresh performs:

1. exact `cancelQueries` for the provider key;
2. a forced `fetchQuery` using the same options with a stale time that requires network fetch;
3. no manual `setQueryData`, because query completion owns the write.

TanStack cancellation must prevent an ignored/uncancellable Tauri Promise from committing late data;
the regression test verifies behavior rather than assuming physical IPC cancellation.

## UI state

Prefer query `fetchStatus/isFetching` as the shared loading truth. A small local state may remain only
for button-specific error presentation, not as request ownership. The button remains disabled while
the authoritative query fetch is running.

## Cache and state boundaries

- Cache identity is provider ID. Provider edit/delete continues to remove that exact key.
- A failed refresh may expose the query error according to existing UI policy; it must not restore a
  result from a different provider or configuration.
- No HTTP cache layer is added. Rust still performs a fresh GET per IPC call.
- Gateway circuit and availability queries are separate namespaces and remain untouched.

## Risks And Rollback

- **Cancellation semantics:** the Tauri call may continue physically. The query cancellation token
  must suppress its cache commit; deferred tests cover this.
- **Infinity stale time:** a forced fetch must explicitly avoid a cache hit.
- **UI churn:** moving loading truth to query state can change button timing; component tests cover it.
- Frontend-only rollback restores the previous helper; there is no DB/schema migration.
