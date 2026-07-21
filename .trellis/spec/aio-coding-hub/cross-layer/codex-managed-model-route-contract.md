# Codex Managed Model Route Contract

## Scenario: Provider-Scoped Model Discovery And Managed Codex Profiles

### 1. Scope / Trigger

Use this contract when changing any of the following:

- provider identity, provider-model catalog schema, or config-bundle import;
- OpenAI-compatible provider model discovery or manual model entries;
- managed Codex profile files under `$CODEX_HOME`;
- `aio/<model_uuid>` parsing, provider selection, wire-model rewriting, cost
  attribution, or model-route diagnostics;
- provider-model/profile IPC, generated bindings, TanStack Query keys, or the
  provider model catalog UI.

The complete flow is:

```text
provider_uuid + provider-scoped catalog
  -> model_uuid
  -> $CODEX_HOME/<name>.config.toml
  -> model = "aio/<model_uuid>" + model_provider = "aio"
  -> exact DB lookup
  -> one bound provider + remote_model_id
  -> wire-vs-observed route evidence
```

### 2. Signatures

Schema v40 adds stable identities and local catalog/profile state:

```sql
providers.provider_uuid TEXT NOT NULL UNIQUE

provider_model_catalogs(
  provider_id INTEGER PRIMARY KEY,
  protocol TEXT NOT NULL,
  stale INTEGER NOT NULL,
  last_attempt_at INTEGER,
  last_success_at INTEGER,
  last_error_code TEXT
)

provider_models(
  model_uuid TEXT PRIMARY KEY,
  provider_id INTEGER NOT NULL,
  remote_model_id TEXT NOT NULL,
  source TEXT NOT NULL, -- discovered | manual
  stale INTEGER NOT NULL,
  UNIQUE(provider_id, remote_model_id)
)

codex_managed_profiles(
  profile_uuid TEXT PRIMARY KEY,
  profile_name TEXT NOT NULL UNIQUE COLLATE NOCASE,
  model_uuid TEXT NOT NULL,
  content_sha256 TEXT NOT NULL,
  FOREIGN KEY(model_uuid) REFERENCES provider_models(model_uuid) ON DELETE RESTRICT
)
```

Rust IPC commands are generated into `src/generated/bindings.ts`:

```rust
provider_models_get(provider_id: i64, provider_uuid: String)
provider_models_refresh(provider_id: i64, provider_uuid: String)
provider_model_manual_upsert(provider_id: i64, provider_uuid: String, remote_model_id: String)
provider_model_manual_delete(provider_id: i64, provider_uuid: String, model_uuid: String)
codex_managed_profiles_list()
codex_managed_profile_create(profile_name: String, model_uuid: String)
codex_managed_profile_delete(profile_uuid: String)
```

The gateway resolves a server-owned route context:

```rust
pub struct ManagedModelRoute {
    canonical_model: String, // aio/<model_uuid>
    model_uuid: String,
    provider_id: i64,
    provider_uuid: String,
    remote_model_id: String,
}
```

Frontend catalog keys include both identities:

```ts
providerModelsKeys.catalog(providerId, providerUuid)
codexManagedProfilesKeys.list()
```

### 3. Contracts

#### Stable identity

- `provider_uuid` and `model_uuid` are canonical lowercase UUIDv4 values.
- Normal provider edits preserve `provider_uuid`. Provider copy and
  single-provider share import create a new UUID. Config bundle v4 preserves
  UUIDs and validates all UUID/reference conflicts before destructive import.
- Numeric `provider_id` remains a local database key. It must never be embedded
  in or trusted from the Codex model alias.
- Model identity is `(provider_uuid, model_uuid, remote_model_id)`. Equal remote
  IDs on two providers remain distinct entries and distinct aliases.

#### Provider-scoped discovery

- Discovery accepts a saved `provider_id + provider_uuid`; a stale frontend
  identity fails closed instead of refreshing a replacement row with the same
  numeric ID.
- Automatic discovery is `openai_compatible` only and is available only for
  direct enabled/disabled Codex providers with no source/bridge relation.
- The backend owns credentials and performs a no-redirect, bounded
  `GET /v1/models`. A failure records a typed catalog error and preserves the
  last successful discovered rows plus all manual rows.
- Connection changes mark discovered data stale. Name, note, priority, and
  other non-connection edits do not.
- Provider refresh locks are keyed by stable provider UUID. The first version
  keeps lock entries for the process lifetime; this avoids overlapping refresh
  races at the cost of a small map entry per UUID. Reclamation is a later
  optimization and must not weaken identity isolation.

#### Managed profile ownership

- AIO writes `$CODEX_HOME/<profile>.config.toml` with top-level keys only:

  ```toml
  model = "aio/<model_uuid>"
  model_provider = "aio"
  ```

- Codex always has one AIO provider. Never generate per-upstream
  `model_providers` entries such as `aio-provider-<id>`.
- Database metadata plus `content_sha256` is the ownership manifest. Creation
  uses no-clobber atomic I/O; an unknown same-name file is never overwritten.
- File status is `managed`, `missing`, or `modified`. Deleting a modified file
  removes only AIO metadata and preserves the external file. Compensation may
  remove only bytes whose hash still matches the file created by this action.
- Codex-home resolution must fail closed on unsafe symlink/reparse layouts.

#### Exact managed routing

- Only exact canonical `aio/<uuidv4>` values are considered managed aliases.
  Prefix resemblance is not an authorization boundary; the UUID must resolve
  to an existing model row and its exact provider identity.
- The bound provider must be an enabled direct Codex provider. It is the only
  candidate, session reuse is disabled, forced-provider conflicts fail closed,
  and cross-provider failover is forbidden. Common circuit/cooldown/limit/auth
  gates and same-provider retries remain active.
- Request plugins run before send, but a managed route must still have the same
  provider and exact `remote_model_id` immediately before network I/O.
  Mutation fails with `GW_MANAGED_MODEL_INVALID` and sends zero upstream calls.

#### Canonical, wire, and observed models

- `request_logs.requested_model` keeps the canonical `aio/<model_uuid>`.
- Each attempt records `requested_upstream_model`, the final model actually
  selected for upstream transmission. Final wire-model synchronization is
  Codex/managed-route scoped and must not alter ordinary Claude/Grok logging.
- Route detection reads the raw upstream response before bridge, response
  fixer, or response plugin changes. It compares final wire model with observed
  model, never canonical alias with remote model.
- A matching expected response produces no `model_route_mapping`. A different
  model or conflicting models produce the severe mapping. Missing, truncated,
  or unparsable evidence is `unobserved`: no alert and no verified-match claim.
- Later terminal evidence replaces earlier attempt evidence. A final `matched`
  or `unobserved` observation clears a stale mismatch; a later mismatch replaces
  the earlier mismatch. This prevents retry/failover history from becoming a
  false final warning.
- `aio_managed_model_route` is a neutral provider-scoped audit marker containing
  canonical, provider, remote, wire/priced model, applied state, and observation.
  It never suppresses a real wire-vs-observed mismatch.

#### Query/cache ownership

- Catalog queries and mutations are keyed by `provider_id + provider_uuid`.
  Provider replacement, config import, and data reset advance generation
  counters before cancellation/invalidation so a late IPC result cannot write
  into a new provider identity.
- Profile mutations invalidate the global profile list and only the matching
  provider catalog identity. No provider list DTO carries an unbounded model
  array.

### 4. Validation & Error Matrix

| Boundary / condition | Required result |
| --- | --- |
| Non-canonical provider/model/profile UUID | `SEC_INVALID_INPUT`; no DB/network/file mutation |
| Stale `provider_id + provider_uuid` | `PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED` |
| Non-Codex or bridge/source provider discovery | `PROVIDER_MODELS_UNSUPPORTED_PROVIDER` |
| Discovery 401 / 403 / 404-405 | `unauthorized` / `forbidden` / `not_supported`; preserve catalog |
| Timeout/network/invalid JSON/empty/limit | Typed catalog error; preserve successful/manual rows |
| Manual model over 256 bytes, padded, empty, or control-containing | `SEC_INVALID_INPUT` |
| Delete model referenced by a managed profile | `PROVIDER_MODEL_MANAGED_PROFILE_REFERENCED` |
| Unknown same-name profile file | `CODEX_MANAGED_PROFILE_FILE_EXISTS`; do not overwrite |
| Managed file hash differs on delete | Preserve file, remove metadata, return `filePreserved=true` |
| Unsafe Codex home | `CODEX_MANAGED_PROFILE_HOME_UNSAFE`; no filesystem mutation |
| `aio/` alias missing/invalid/not bound | `GW_MANAGED_MODEL_INVALID` before provider use |
| Bound provider disabled, replaced, bridged, or UUID-mismatched | Fail closed; zero calls to another provider |
| Forced provider differs from binding | `GW_MANAGED_MODEL_INVALID` |
| Request plugin changes bound model/provider | `GW_MANAGED_MODEL_INVALID`; zero upstream calls |
| Wire equals observed model | No severe mapping; observation `matched` |
| Wire differs from observed model | Persist provider-scoped `model_route_mapping` |
| No reliable observed model | Observation `unobserved`; clear stale terminal mismatch |
| Multiple conflicting observed models | Observation `conflict`; severe mapping remains |
| Config v1-v3 with local managed profiles that cannot rebind | Reject before replacing providers |
| Config v4 duplicate/invalid/missing UUID references | Reject before destructive import |

### 5. Good / Base / Bad Cases

- Good: two providers both expose `grok-4.5`; each receives a distinct
  `model_uuid`, profile creation selects one, and only its provider is called.
- Good: a failed refresh leaves the previous discovered models visible as
  stale and leaves manual models unchanged.
- Good: an early retry observes the wrong model, then the terminal retry is
  matched or unobserved; the terminal log contains no stale severe warning.
- Base: a normal non-`aio/` Codex request keeps existing sorting, session
  binding, retry, and cross-provider failover behavior.
- Base: ordinary Claude/Grok plugin mutation does not opt into Codex final-wire
  audit synchronization.
- Bad: derive provider ownership from model prefix, `owned_by`, display name,
  numeric provider ID, or provider ordering.
- Bad: rewrite `request_logs.requested_model` to the remote ID merely to avoid
  an alias mismatch warning.
- Bad: treat `aio/anything` as trusted, or hide all mismatches whenever an AIO
  managed marker exists.
- Bad: overwrite/delete a profile file because its filename appears in AIO's
  database without verifying the generated content hash.

### 6. Tests Required

- Migration/fresh-schema tests: v39 -> v40 UUID backfill, uniqueness,
  immutability triggers, FK/delete protection, and idempotent upgrade.
- Provider lifecycle tests: create/edit/copy/share/config-v4 UUID semantics and
  destructive-import preflight for local profile rebinding.
- Discovery tests: exact provider isolation, Base URL joining, no redirects,
  bounded body/count/ID, typed errors, stale preservation, connection-change
  races, and credential redaction.
- Profile tests: current Codex file format, case-insensitive name collision,
  no-clobber create, managed/missing/modified projection, hash compensation,
  unsafe home, and metadata/file partial-failure recovery.
- Gateway route tests: exact alias validation, disabled/replaced provider,
  forced-provider conflict, one-candidate routing, no cross-provider failover,
  same-provider retry, plugin mutation fail-closed, and ordinary-route
  regression coverage.
- Route-evidence tests across complete JSON, body-buffer JSON, complete SSE,
  and early/incomplete SSE: matched, mismatch, unobserved, conflict, and
  later-terminal-evidence clearing of stale mappings.
- Cross-layer tests: generated bindings, service decoders, provider-scoped
  query keys/generation guards, late-result suppression, save-then-refresh UI,
  manual fallback, profile preserved messaging, and neutral versus severe log
  presentation.
- Run full Rust tests after shared gateway/config migration changes, plus unit
  tests, typecheck, lint, Rust fmt/Clippy/check, generated-binding checks, and
  `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
// Prefix grants trust, numeric IDs leak into long-lived client config, and
// ordinary failover can route the same model name to another provider.
if requested_model.starts_with("aio/") {
    suppress_model_route_warning();
}
let provider_id = parse_provider_id(requested_model);
route_with_normal_failover(provider_id, remote_model_id);
```

#### Correct

```rust
let binding = resolve_managed_model_alias_exact(&canonical_alias)?;
validate_canonical_uuid_v4(&binding.model_uuid)?;
validate_exact_provider_identity(binding.provider_id, &binding.provider_uuid)?;

let providers = vec![load_enabled_direct_codex_provider(&binding)?];
rewrite_wire_model_exact(&binding.remote_model_id)?;
validate_again_immediately_before_send(&binding)?;

// Keep canonical audit identity; compare only what was sent with raw response
// evidence. A later non-mismatch terminal observation clears stale mismatch.
request_log.requested_model = Some(canonical_alias);
attempt.requested_upstream_model = Some(binding.remote_model_id.clone());
apply_final_route_evidence(wire_model, observed_model);
```
