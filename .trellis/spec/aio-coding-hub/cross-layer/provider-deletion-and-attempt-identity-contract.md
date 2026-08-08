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

## Scenario: Retire built-in Provider bridge types

### 1. Scope / Trigger

Apply this contract when removing an active Provider bridge type or a
Provider-owned mapping field across SQLite, Rust DTOs, generated bindings,
gateway execution, config migration, single-Provider share, and React UI.
The retirement must not turn a bridge into a direct Provider or copy transport
credentials from its source.

### 2. Signatures

```rust
const LATEST_SCHEMA_VERSION: i64 = 44;

fn migrate_v43_to_v44(conn: &mut Connection) -> Result<(), String>;
fn is_supported_bridge_type(bridge_type: &str) -> bool; // built-in: cx2cc only

pub(crate) fn prepare_config_import(
    bundle: ConfigBundle,
) -> AppResult<PreparedConfigImport>;

pub(crate) fn parse_provider_share(
    bytes: &[u8],
) -> AppResult<ProviderShareEnvelopeV2>;
```

The retired bridge identifiers are
`codex_to_openai_chat`, `codex_to_openai_responses`, and
`codex_to_anthropic_messages`. They are private compatibility data, not
exported domain constants or generated DTO values.

### 3. Contracts

- SQLite 43 -> 44 deletes Providers whose `bridge_type` is one of the three
  retired values inside one transaction. Existing foreign-key dependents are
  removed, surviving `source_provider_id` references to a deleted row become
  `NULL`, and all surviving `model_mapping_json` values become `{}`.
- Before deleting anything, the migration rejects a managed Codex Profile that
  reaches a retired Provider through `provider_models`. The stable error must
  contain no model UUID, Profile name, path, credential, or Provider payload.
- `request_logs` and their `attempts_json` snapshots are historical evidence.
  They are neither deleted nor rewritten even when `final_provider_id` names a
  removed Provider.
- Active `ProviderUpsertParams`, `ProviderSummary`,
  `ProviderForGateway`, TypeScript bindings, editor state, and gateway bridge
  context expose no generic Provider `ModelMapping`. The physical SQLite
  `model_mapping_json` column and Provider-share v1/v2 wire member are
  compatibility shells only and always normalize to an empty object.
- Runtime registration and Provider upsert accept only the still-supported
  built-in `cx2cc` bridge. Retired identifiers may occur only in migration,
  import/share rejection classifiers, and negative regression fixtures.
- `prepare_config_import` rejects an entire bundle containing a retired bridge
  with `CODEX_PROVIDER_TRANSLATION_UNSUPPORTED` before taking the import lock,
  clearing current rows, writing settings, or touching Skill files.
- Provider-share v1/v2 rejects a retired bridge with the same stable code.
  A legacy non-bridge `model_mapping` member parses strictly but is discarded
  before preview, export, or import.
- Ordinary Codex Responses/compact handling, Claude CX2CC, plugin protocol
  contributions, account-usage routing, and historical attempt display remain
  independent and must not be removed as collateral.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Upsert uses a retired or unknown bridge type | `SEC_INVALID_INPUT: unsupported bridge_type`; write nothing |
| SQLite has ordinary Codex, source, or CX2CC Providers | Preserve the rows and normalize only the compatibility mapping column |
| SQLite has a managed Profile referring to a retired Provider model | Abort and roll back the complete 43 -> 44 migration |
| Full config bundle contains any retired bridge | `CODEX_PROVIDER_TRANSLATION_UNSUPPORTED` before destructive import |
| Provider share v1/v2 contains a retired bridge | Same stable error; create no preview/import row |
| Old share/config mapping shell is non-empty without a retired bridge | Accept the compatible shape, normalize to `{}`, expose no active mapping DTO |
| Registry lookup uses a retired bridge | Return no bridge factory |

### 5. Good / Base / Bad Cases

- Good: upgrade deletes three retired bridge rows and their active route/model
  references while preserving source Providers, CX2CC, direct Codex Providers,
  request logs, and request-time Provider names.
- Base: an old direct-Provider share contains a non-empty `model_mapping`
  member; parsing succeeds, reserialization emits the empty compatibility
  shape, and the imported database column is `{}`.
- Bad: copy a source Provider URL or credential into a retired bridge row and
  silently convert it to a direct Provider.
- Bad: clear current config, then discover a retired bridge in the backup.
- Bad: leave a disabled registry/test branch that can still construct the
  retired request, response, or stream translators.

### 6. Tests Required

- Migration: cover all three retired values, dependents, nested source
  references, mapping normalization, historical logs, idempotence, and
  `PRAGMA foreign_key_check`.
- Migration failure: seed a managed Profile reference and assert schema version,
  Providers, models, and Profile rows are unchanged and diagnostics are
  sensitive-data free.
- Domain/registry: reject all retired upserts, accept explicit Claude CX2CC,
  and assert no retired factory is registered.
- Config/share: assert stable preflight rejection preserves the current
  Provider and that legacy mapping shells normalize to `{}`.
- Cross-layer: regenerate bindings and assert the generic Provider
  `ModelMapping` and editor fields are absent.
- Regression: run complete Provider, failover, ordinary Codex, CX2CC,
  account-usage, frontend, and Rust suites.

### 7. Wrong vs Correct

#### Wrong

```rust
if retired_bridge {
    provider.bridge_type = None;
    provider.base_urls = source.base_urls;
    provider.api_key = source.api_key;
}
clear_existing_config_data(&tx)?;
validate_imported_bridges(&bundle.providers)?;
```

This changes credential ownership and can destroy current configuration before
reporting that the backup is unsupported.

#### Correct

```rust
reject_retired_bridges(&bundle.providers)?;

let tx = conn.transaction()?;
reject_managed_profile_references(&tx)?;
delete_retired_provider_dependents(&tx)?;
delete_retired_providers(&tx)?;
tx.execute("UPDATE providers SET model_mapping_json = '{}'", [])?;
tx.commit()?;
```

Compatibility validation is pre-destructive, while database cleanup is
bounded, transactional, and leaves historical log snapshots intact.
