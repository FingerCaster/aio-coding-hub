# Provider Deletion and Attempt Identity Contract

## Scenario: Delete a provider without stale route projections

### 1. Scope / Trigger

Apply this contract when changing provider deletion, provider route query keys,
Default or sort-mode provider projections, or provider identity in request-log
attempts. The database, frontend cache, and historical log UI must agree on
one stable provider ID while retaining the request-time display snapshot.

### 2. Signatures

```ts
providerDelete(
  providerId: number,
  options: { clearUsageStats: boolean }
): Promise<boolean>

useProviderDeleteMutation(): mutation<{
  cliKey: CliKey;
  providerId: number;
  clearUsageStats?: boolean;
}>

providersKeys.list(cliKey)
providersKeys.defaultRoute(cliKey)
sortModeProvidersQueryPrefix(cliKey)
sortModeProvidersQueryKey(modeId, cliKey)
```

Persisted route references are owned by these foreign keys:

```text
provider_pool_order.provider_id     -> providers.id ON DELETE CASCADE
default_route_providers.provider_id -> providers.id ON DELETE CASCADE
sort_mode_providers.provider_id     -> providers.id ON DELETE CASCADE
```

Historical attempts expose the request-time identity tuple:

```ts
type AttemptProviderIdentity = {
  provider_id: number;
  provider_name: string;
  base_url: string;
};
```

### 3. Contracts

- SQLite foreign-key cascade is the persisted source of truth. Do not add a
  second product deletion loop for route rows.
- After `providerDelete` returns `true`, the frontend owns one continuous route
  cache commit boundary: cancel the provider list, Default route, and every
  sort-mode provider query for the normalized CLI; filter existing caches by
  `providerId`; then immediately invalidate the same query family.
- Cache filtering must not create missing data. Preserve `null` and absent
  cache entries, other CLIs, and all rows whose stable ID differs.
- Await route invalidation together with any independent provider-model
  cancellation. Do not insert an unrelated `await` between route filtering and
  route invalidation.
- Request-log details render `provider_name` and `provider_id` from the attempt
  snapshot. They must not resolve the ID against the current provider table,
  because the provider may have been renamed, deleted, or replaced.
- Use the same identity label in start/final summaries and in collapsed and
  expanded attempt rows: `Name (#ID)`. The URL is secondary context and never
  an identity or deletion key.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Delete service returns `true` | Cancel, filter by stable ID, invalidate, and reconcile with SQLite |
| Delete service returns `false` | Do not mutate or invalidate provider route caches |
| Delete service rejects | Propagate the error and preserve all provider route caches |
| Same name, URL, or type with a different ID | Preserve the other provider and every route entry that references it |
| Name is empty, whitespace, `Unknown` (case-insensitive), or `未知` | Render `未知供应商 (#ID)` |
| ID is not a positive safe integer | Render `Name (ID 不可用)` or the unknown-name equivalent |
| Historical provider no longer exists | Continue rendering the request-time name and ID snapshot |

### 5. Good / Base / Bad Cases

- Good: deleting provider `#7` removes only `#7` from the list, Default route,
  and every cached mode for that CLI; a late pre-delete query cannot restore it.
- Base: a deleted historical provider still renders as `Provider A (#7)` in
  every decision-chain surface without a current-provider lookup.
- Bad: filtering by name or URL removes a same-domain sibling configuration.
- Bad: invalidating only the currently visible mode leaves stale IDs in another
  cached sort mode.
- Bad: rendering only `#7`, or resolving `#7` to the current provider name,
  loses readable historical identity.

### 6. Tests Required

- Query regression: seed provider list, Default route, at least two sort modes,
  and another CLI; assert only the target ID is filtered and the target query
  family is invalidated.
- Race regression: start uncancellable queries for all route projections,
  complete them after deletion in reverse order, and assert the deleted ID does
  not return.
- Failure regressions: service `false`, rejection, `null`, and absent cache
  cases must not fabricate or partially mutate state.
- Rust regression: persist pool, Default, and multiple sort-mode references;
  delete through the public domain entry point, reopen the database, verify all
  target references are gone, sibling ordering/enabled state is unchanged, and
  `PRAGMA foreign_key_check` is empty.
- UI regression: cover same-name/same-URL providers with different IDs,
  unknown names, invalid IDs, collapsed attempts, empty URLs, and damaged raw
  JSON with the compatible structured attempt source still available.

### 7. Wrong vs Correct

#### Wrong

```ts
queryClient.setQueryData(providersKeys.list(cliKey), filterDeleted);
queryClient.invalidateQueries({ queryKey: currentModeKey });
renderProvider(providerId);
```

This leaves Default and other cached modes stale, permits an older query to
restore the row, and gives the user no readable historical identity.

#### Correct

```ts
await cancelRouteQueryFamily(cliKey);
filterExistingRouteCachesByProviderId(cliKey, providerId);
const reconciliation = invalidateRouteQueryFamily(cliKey);
await Promise.all([reconciliation, cancelProviderModelQueries(providerId)]);

renderProvider(`${requestTimeName} (#${providerId})`);
```

The exact helpers may differ, but cancellation, ID-based filtering, immediate
family invalidation, and snapshot-based rendering are mandatory.
