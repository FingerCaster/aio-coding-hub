# Codex Managed Model Route Contract

## Scenario: Provider-Scoped Model Discovery And Managed Codex Profiles

### 1. Scope / Trigger

Use this contract when changing any of the following:

- provider identity, provider-model catalog schema, or config-bundle import;
- OpenAI-compatible provider model discovery or manual model entries;
- managed Codex profile files under `$CODEX_HOME`;
- Codex picker `model_catalog_json` generation, proxy-time activation, or
  bundled catalog process launch;
- `aio/<profile_name_key>` / legacy `aio/<model_uuid>` parsing, provider
  selection, wire-model rewriting, cost attribution, or model-route diagnostics;
- provider-model/profile IPC, generated bindings, TanStack Query keys, or the
  provider model catalog UI.

The complete flow is:

```text
provider_uuid + provider-scoped catalog
  -> model_uuid + explicit reasoning/context capabilities
  -> profile_name_key + $CODEX_HOME/<name>.config.toml
  -> model = "aio/<profile_name_key>" + model_provider = "aio"
  -> complete merged model_catalog_json picker entry
  -> exact profile lookup (or exact legacy model UUID lookup)
  -> one bound provider + remote_model_id
  -> wire-vs-observed route evidence
```

### 2. Signatures

Schema v40 adds stable identities and local catalog/profile state. Schema v41
adds explicit provider-model capabilities:

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
  capabilities_configured INTEGER NOT NULL DEFAULT 0,
  supported_reasoning_efforts_json TEXT NOT NULL DEFAULT '[]',
  default_reasoning_effort TEXT,
  context_window INTEGER,
  UNIQUE(provider_id, remote_model_id)
)

codex_managed_profiles(
  profile_uuid TEXT PRIMARY KEY,
  profile_name TEXT NOT NULL,
  profile_name_key TEXT NOT NULL UNIQUE,
  model_uuid TEXT NOT NULL,
  content_sha256 TEXT NOT NULL,
  codex_home_path TEXT NOT NULL,
  FOREIGN KEY(model_uuid) REFERENCES provider_models(model_uuid) ON DELETE RESTRICT
)
```

Rust IPC commands are generated into `src/generated/bindings.ts`:

```rust
provider_models_get(provider_id: i64, provider_uuid: String)
provider_models_refresh(provider_id: i64, provider_uuid: String)
provider_model_manual_upsert(provider_id: i64, provider_uuid: String, remote_model_id: String)
provider_model_manual_delete(provider_id: i64, provider_uuid: String, model_uuid: String)
provider_model_capabilities_update(
    provider_id: i64,
    provider_uuid: String,
    model_uuid: String,
    capabilities: ProviderModelCapabilitiesInput,
)
codex_managed_profiles_list()
codex_managed_profile_create(profile_name: String, model_uuid: String)
codex_managed_profile_delete(profile_uuid: String)

struct ProviderModelCapabilitiesInput {
    supported_reasoning_efforts: Vec<ProviderModelReasoningEffort>,
    default_reasoning_effort: Option<ProviderModelReasoningEffort>,
    context_window: Option<i64>,
}
```

The gateway resolves a server-owned route context:

```rust
pub struct ManagedModelRoute {
    canonical_model: String, // aio/<profile_name_key> or legacy aio/<model_uuid>
    model_uuid: String,
    provider_id: i64,
    provider_uuid: String,
    remote_model_id: String,
}
```

The picker integration resolves and runs the installed Codex executable with
structured arguments:

```rust
fetch_bundled_catalog(launch, codex_home) // debug models --bundled
sync_current_locked(app)                  // rebuild/apply/restore catalog
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
- `profile_name_key` is the lowercase, case-insensitive product key used by new
  picker aliases. UUID-shaped profile names are reserved so
  `aio/<profile_name_key>` can never be ambiguous with the legacy UUID form.

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

#### Provider-model capabilities

- Capabilities belong to the provider-scoped `provider_models` row, not to a
  managed Profile. Multiple Profiles that reference one `model_uuid` share the
  same capability configuration.
- Newly discovered and manually added models start with
  `capabilities_configured = 0`; neither provider name nor model ID may infer
  capabilities. A v40 -> v41 migration is the only compatibility exception: it
  marks existing rows configured with `low / medium / high`, default `medium`,
  and unknown context so already-created Profiles remain valid.
- Supported efforts are the canonical ordered subset of `none`, `minimal`,
  `low`, `medium`, `high`, `xhigh`, `max`, and `ultra`. Duplicates are invalid.
  A non-empty set requires a default from that set. An empty set plus a null
  default explicitly means “do not send `reasoning.effort`”; it is distinct
  from an unconfigured model.
- `context_window` is either null (explicitly unknown) or an integer from 1,024
  through 10,000,000 tokens. The backend remains authoritative even though the
  frontend mirrors the bounds for immediate feedback.
- Manual upsert and discovery refresh preserve existing capability columns on
  conflict. Config v4 same-machine local-state capture/restore preserves them
  byte-for-byte while still marking retained discovered rows stale.
- Profile creation must reject an unconfigured model before any profile,
  catalog, or root-config mutation. Capability updates use the same global
  managed-profile lifecycle lock as create/delete and validate
  `provider_id + provider_uuid + model_uuid` again inside the write transaction.

#### Managed profile ownership

- AIO writes `$CODEX_HOME/<profile>.config.toml` with top-level keys only:

  ```toml
  model = "aio/<profile_name_key>"
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

#### Codex picker catalog lifecycle

- Profile files do not populate `/model` by themselves. While the Codex CLI
  proxy is enabled, AIO owns one complete merged `model_catalog_json` containing
  the current Codex base catalog plus one visible `aio/<profile_name_key>` entry
  per managed profile.
- Managed picker entries project the bound provider-model's configured effort
  set and default. A non-empty set enables `supports_reasoning_summaries`; an
  empty set writes no default and disables that flag. Known context is written
  to both `context_window` and `max_context_window`; unknown context writes null
  to both. `auto_compact_token_limit` remains null so Codex derives compaction.
- The Profile-set ownership hash includes effort set, default effort, and
  context. Updating a model referenced by any Profile therefore rebuilds the
  complete managed catalog. Catalog/config ownership drift fails before the DB
  update; a DB commit failure after file application restores the exact prior
  catalog and root config bytes.
- If the pre-proxy root config contains an absolute user `model_catalog_json`,
  preserve that document, every existing model, and unknown fields as the base.
  Otherwise run the currently installed Codex executable with
  `debug models --bundled`; never substitute an AIO compile-time snapshot.
- Tests that create Profiles or otherwise require a generated catalog must
  write a deterministic complete user catalog and bind its absolute path in
  the fixture's `config.toml`. Such fixtures must not fall through to the
  installed-Codex branch: a developer machine with Codex installed can hide a
  Linux/clean-host failure. This is test setup only; production keeps the
  installed-Codex fallback and fails closed when neither source exists.
- If an enabled proxy backup from an older/failed flow points
  `model_catalog_json` exactly (or canonically) at AIO's current generated
  catalog, treat only that binding as provable baseline pollution. Prepare a
  sanitized backup with the binding removed and use the installed bundled
  catalog as the base. Arbitrary user catalog paths are never sanitized.
- Baseline repair, generated catalog update, and live config projection are one
  ordered transaction. Snapshot and drift-check all three before writing;
  apply backup -> generated -> live, roll back in reverse, attempt every
  rollback, and return `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` if compensation
  cannot restore every owned file.
- Generated catalog bytes carry owner, payload/profile/base hashes. Every
  update verifies those hashes and the root-config snapshot; external edits or
  concurrent drift fail closed instead of being overwritten.
- Profile create/delete, DB mutation, profile file activation, generated
  catalog activation, and root config patching share the managed-profile
  lifecycle lock and compensate only bytes still owned by that operation.
- Enabling/syncing the CLI proxy rebuilds the catalog. Disabling/restoring the
  proxy restores the original `model_catalog_json` value or its absence. With
  zero managed profiles, no generated picker catalog remains active.
- On Windows, pass the resolved `.cmd` / `.bat` executable and each fixed
  argument separately to `std::process::Command`. Do not rebuild the command as
  a quoted `cmd.exe /S /C` string: Rust's quote escaping becomes literal to
  `cmd.exe` and can turn `\"codex.cmd\"` into an unknown command.

#### Exact managed routing

- New `aio/<profile_name_key>` values must resolve by exact managed-profile
  lookup. Legacy exact canonical `aio/<uuidv4>` values resolve by model UUID.
  Prefix resemblance is not an authorization boundary; either form must reach
  an existing server-owned binding and its exact provider identity.
- The bound provider must be an enabled direct Codex provider. It is the only
  candidate, session reuse is disabled, forced-provider conflicts fail closed,
  and cross-provider failover is forbidden. Common circuit/cooldown/limit/auth
  gates and same-provider retries remain active.
- Request plugins run before send, but a managed route must still have the same
  provider and exact `remote_model_id` immediately before network I/O.
  Mutation fails with `GW_MANAGED_MODEL_INVALID` and sends zero upstream calls.

#### Canonical, wire, and observed models

- `request_logs.requested_model` keeps the actual canonical alias selected by
  Codex: new `aio/<profile_name_key>` or legacy `aio/<model_uuid>`.
- Each attempt records `requested_upstream_model`, the final model actually
  selected for upstream transmission. Final wire-model synchronization is
  Codex/managed-route scoped and must not alter ordinary Claude/Grok logging.
- Route detection reads the raw upstream response before bridge, response
  fixer, or response plugin changes. It compares final wire model with observed
  model, never canonical alias with remote model.
- A matching expected response produces no `model_route_mapping`. A different
  model or conflicting models produce the severe mapping. Missing, truncated,
  or unparsable evidence is `unobserved`: no alert and no verified-match claim.
- A selected managed-model effort does not change canonical/wire/observed model
  identity. If the upstream omits effort evidence, do not report an effort
  mismatch; an explicitly different returned effort remains a real mismatch.
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
| Create Profile for a model with `capabilities_configured = 0` | `PROVIDER_MODEL_CAPABILITIES_REQUIRED`; no file/catalog/DB mutation |
| Duplicate effort, missing/out-of-set default, or context outside 1,024..10,000,000 | `SEC_INVALID_INPUT`; no mutation |
| Empty effort set with null default | Valid explicit no-reasoning configuration |
| Capability update sees managed catalog/config ownership drift | Fail closed; capability row remains unchanged |
| Capability DB commit fails after catalog application | Restore prior catalog/config bytes and roll back capability row |
| Unknown same-name profile file | `CODEX_MANAGED_PROFILE_FILE_EXISTS`; do not overwrite |
| Managed file hash differs on delete | Preserve file, remove metadata, return `filePreserved=true` |
| Unsafe Codex home | `CODEX_MANAGED_PROFILE_HOME_UNSAFE`; no filesystem mutation |
| UUID-shaped profile name | `SEC_INVALID_INPUT`; avoid new/legacy alias ambiguity |
| `aio/` alias missing/invalid/not bound | `GW_MANAGED_MODEL_INVALID` before provider use |
| Bundled Codex command cannot spawn or exits non-zero | `CODEX_MANAGED_MODEL_BUNDLED_UNAVAILABLE`; no partial profile/catalog/config commit |
| Bundled Codex command times out | `CODEX_MANAGED_MODEL_BUNDLED_TIMEOUT`; terminate the process tree and leave state unchanged |
| Bundled Codex output is empty, invalid, or oversized | `CODEX_MANAGED_MODEL_BUNDLED_INVALID`; no partial state |
| No user catalog is bound and no Codex CLI is installed | `CODEX_MANAGED_MODEL_CLI_NOT_FOUND`; no partial profile/catalog/config commit |
| Generated catalog owner/hash or root-config snapshot changed externally | Fail closed; preserve external bytes and roll back this lifecycle action |
| Enabled backup catalog equals current AIO generated catalog | Remove only that backup binding, use bundled base, and transact backup/generated/live |
| Enabled backup catalog is any other user path | Preserve it and apply ordinary user-catalog validation; never auto-clean |
| Baseline/generated/live compensation fails | `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` after attempting all remaining rollbacks |
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
  `model_uuid`; profiles `grok-primary` and `grok-backup` expose distinct
  readable aliases, and each request calls only its bound provider.
- Good: an installed Windows Codex resolved as `C:\Program Files\...\codex.cmd`
  is launched with separate `debug`, `models`, and `--bundled` arguments and its
  complete bundled catalog becomes the merge base.
- Good: a failed refresh leaves the previous discovered models visible as
  stale and leaves manual models unchanged.
- Good: a model is explicitly configured with no reasoning and unknown context;
  Profile creation becomes available and its picker row carries an empty effort
  list, null context fields, and no reasoning-summary capability.
- Good: changing a model with existing Profiles to `minimal / max`, default
  `max`, and a 1,000,000-token context rebuilds every affected picker row and
  prompts the user to start or restart a Codex session.
- Good: an early retry observes the wrong model, then the terminal retry is
  matched or unobserved; the terminal log contains no stale severe warning.
- Base: a normal non-`aio/` Codex request keeps existing sorting, session
  binding, retry, and cross-provider failover behavior.
- Base: ordinary Claude/Grok plugin mutation does not opt into Codex final-wire
  audit synchronization.
- Base: an old `aio/<model_uuid>` profile continues resolving to the same
  provider-scoped model after readable picker aliases are introduced.
- Base: production with neither an absolute user catalog nor an installed
  Codex CLI fails closed with `CODEX_MANAGED_MODEL_CLI_NOT_FOUND`; success-path
  fixtures bind their own user catalog instead of depending on the test host.
- Bad: derive provider ownership from model prefix, `owned_by`, display name,
  numeric provider ID, or provider ordering.
- Bad: rewrite `request_logs.requested_model` to the remote ID merely to avoid
  an alias mismatch warning.
- Bad: treat `aio/anything` as trusted, or hide all mismatches whenever an AIO
  managed marker exists.
- Bad: generate only AIO picker rows without a valid complete base catalog, or
  wrap a Windows `.cmd` invocation into one manually escaped command string.
- Bad: overwrite/delete a profile file because its filename appears in AIO's
  database without verifying the generated content hash.
- Bad: infer effort or context from provider/model names, copy capability values
  into each Profile, or reset them during a later refresh/manual upsert.
- Bad: let a managed-profile/config-import test fixture inherit the host's
  Codex installation instead of binding an explicit temporary user catalog.

### 6. Tests Required

- Migration/fresh-schema tests: v39 -> v40 UUID backfill, uniqueness,
  immutability triggers and FK/delete protection; v40 -> v41 existing-row
  compatibility backfill, new-row unconfigured defaults, context bounds, and
  idempotent upgrade.
- Provider lifecycle tests: create/edit/copy/share/config-v4 UUID semantics and
  destructive-import preflight for local profile rebinding.
- Discovery tests: exact provider isolation, Base URL joining, no redirects,
  bounded body/count/ID, typed errors, stale preservation, connection-change
  races, credential redaction, and capability preservation across refresh and
  repeated manual upsert.
- Profile tests: current Codex file format, case-insensitive name collision,
  UUID-shaped-name rejection, readable alias, no-clobber create,
  managed/missing/modified projection, hash compensation, unsafe home, and
  metadata/file/catalog/config partial-failure recovery.
- Picker tests: user-base unknown-field preservation, installed bundled-base
  fallback, alias collision, owner/hash drift, zero-profile restore, proxy
  enable/sync/disable rollback, Windows `.cmd` path-with-spaces launch, and a
  real installed-Codex `model/list` smoke test for `aio/<profile_name_key>`.
  Assert configured efforts/default/context, explicit no-reasoning/unknown
  context, Profile-set hash invalidation, drift-before-write failure, and exact
  catalog/config/DB restoration after a forced commit failure.
- Managed-profile and config-import fixtures that can build a generated
  catalog bind an absolute temporary user catalog in `config.toml`. Run their
  profile create/delete cases without an installed Codex CLI (Linux CI is the
  release gate) and assert they never return
  `CODEX_MANAGED_MODEL_CLI_NOT_FOUND` merely because of host setup.
- Catalog recovery tests: a proxy backup bound to the exact AIO generated path
  is sanitized before base selection; a different absolute user path is
  unchanged; forced generated/live write failures restore the original backup,
  generated catalog, and live config bytes or surface recovery-required.
- Gateway route tests: exact alias validation, disabled/replaced provider,
  forced-provider conflict, one-candidate routing, no cross-provider failover,
  same-provider retry, plugin mutation fail-closed, and ordinary-route
  regression coverage.
- Route-evidence tests across complete JSON, body-buffer JSON, complete SSE,
  and early/incomplete SSE: matched, mismatch, unobserved, conflict, and
  later-terminal-evidence clearing of stale mappings.
- Cross-layer tests: generated bindings, service decoders, provider-scoped
  query keys/generation guards, late-result suppression, save-then-refresh UI,
  manual fallback, capability-required Profile gating, effort/context saves,
  existing-Profile restart messaging, profile preserved messaging, and neutral
  versus severe log presentation.
- Run full Rust tests after shared gateway/config migration changes, plus unit
  tests, typecheck, lint, Rust fmt/Clippy/check, generated-binding checks, and
  `git diff --check`.

### 7. Wrong vs Correct

#### Wrong

```rust
// Prefix grants trust, numeric IDs leak into long-lived client config,
// ordinary failover can route elsewhere, and manual cmd.exe quoting breaks
// Windows npm wrappers.
let capabilities = infer_capabilities_from_model_name(remote_model_id);
create_profile_without_capability_confirmation(model_uuid, capabilities);

if requested_model.starts_with("aio/") {
    suppress_model_route_warning();
}
let provider_id = parse_provider_id(requested_model);
route_with_normal_failover(provider_id, remote_model_id);

Command::new("cmd.exe")
    .args(["/D", "/S", "/C"])
    .arg(format!("\\\"{}\\\" debug models --bundled", executable.display()));

// A generated catalog cannot safely become its own base.
let base = read_catalog(baseline.model_catalog_json)?;
generate_catalog(&base, profiles)?;
```

#### Correct

```rust
let binding = resolve_managed_model_alias(&db, &canonical_alias)?
    .ok_or_else(managed_model_invalid)?;
validate_canonical_uuid_v4(&binding.model_uuid)?;
validate_exact_provider_identity(binding.provider_id, &binding.provider_uuid)?;

// Capabilities are explicit model-owned data. Updating them and any active
// picker catalog is one lifecycle-locked, compensating operation.
let capabilities = normalize_explicit_capabilities(input)?;
update_capabilities_and_rebuild_catalog_locked(binding.model_uuid, capabilities)?;

let providers = vec![load_enabled_direct_codex_provider(&binding)?];
rewrite_wire_model_exact(&binding.remote_model_id)?;
validate_again_immediately_before_send(&binding)?;

// Keep canonical audit identity; compare only what was sent with raw response
// evidence. A later non-mismatch terminal observation clears stale mismatch.
request_log.requested_model = Some(canonical_alias);
attempt.requested_upstream_model = Some(binding.remote_model_id.clone());
apply_final_route_evidence(wire_model, observed_model);

let mut command = Command::new(&launch.executable);
command.args(["debug", "models", "--bundled"]);

let baseline = prepare_catalog_baseline(proxy_backup, generated_path)?;
let source = base_catalog_source(baseline.catalog_path.as_deref())?;
apply_backup_generated_and_live_transaction(baseline, source, profiles)?;
```

Test fixtures follow the same source contract explicitly:

```rust
// Wrong: success now depends on whether `codex` happens to be installed on
// the developer or CI host.
let app = tauri::test::mock_app();
codex_managed_profiles::create(app.handle(), &db, "fixture-source", &model_uuid)?;

// Correct: bind deterministic source bytes before exercising Profile/catalog
// lifecycle code. Production behavior is unchanged.
let codex_home = codex_home_dir(app.handle())?;
let fixture = test_support::install_codex_model_catalog_fixture(&codex_home);
assert!(fixture.catalog_path.is_absolute());
codex_managed_profiles::create(app.handle(), &db, "fixture-source", &model_uuid)?;
```

## Scenario: Dedicated GPT-5.6 372K Catalog Policy

### 1. Scope / Trigger

Use this contract when changing the device-local GPT-5.6 372K switch, settings
migration/import, Codex-home selection, generated picker catalog, raw or
structured Codex config saves, CLI proxy lifecycle, or startup reconciliation.

The feature owns a derived-catalog policy. It does not own a root
`model_context_window`, `model_auto_compact_token_limit`, an installed Codex
binary, or a user's source catalog.

### 2. Signatures

The backend and generated IPC boundary are:

```rust
pub(crate) const GPT56_372K_CONTEXT_TOKENS: u64 = 372_000;

pub(crate) struct ManagedCatalogPolicy {
    pub(crate) gpt56_372k_context_enabled: bool,
}

settings_codex_gpt56_372k_context_set(enabled: bool) -> SettingsView
sync_current_locked(app: &AppHandle<R>) -> AppResult<()>
prepare_for_profiles_with_policy(
    app: &AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
) -> AppResult<ManagedCatalogPlan>

pub(crate) struct ManagedCatalogPlan { /* prepared snapshots and guards */ }
pub(crate) struct AppliedManagedCatalog { /* committed file tokens */ }

ManagedCatalogPlan::apply(app) -> AppResult<AppliedManagedCatalog>
AppliedManagedCatalog::rollback() -> AppResult<()>
```

`AppSettings.codex_gpt56_372k_context_enabled` is persisted with settings
schema 64 and defaults to `false`. It is visible in `SettingsView`, but is not
an owned field of ordinary `SettingsUpdate`, `SettingsPatch`, config export, or
config import.

### 3. Contracts

- The exact nominal value is decimal `372000`, following the bundled Codex
  decimal `272000` convention. `380928` is not an enabled value.
- The only target slugs are `gpt-5.6-sol`, `gpt-5.6-terra`, and
  `gpt-5.6-luna`. Match exact slugs; never prefix-match aliases or future
  models.
- Enabling requires one valid occurrence of every target. Rewrite both
  `context_window` and `max_context_window` to `372000` in the derived copy.
  Preserve all other model/root fields, `effective_context_window_percent`,
  `auto_compact_token_limit`, and every `aio/*` projection.
- A generated catalog is needed when managed Profiles exist OR the policy is
  enabled. With neither owner, restore the original `model_catalog_json`
  binding (or its absence) and remove only the owned generated file.
- The base is the validated absolute user catalog recorded by the original
  binding, otherwise `codex debug models --bundled` from the installed Codex.
  A generated AIO catalog must never become its own base.
- Owner metadata schema v2 hashes the base/source binding, managed Profiles,
  policy version, enabled bit, token count, and exact slug list. Unknown,
  malformed, or externally modified ownership data fails closed.
- Prepare is side-effect free. Apply rechecks ownership, the base-source guard,
  live config, generated file, and proxy backup before its first write.
- Activation writes proxy backup repair (when required), generated catalog,
  then live config. Deactivation restores live config before removing the
  generated catalog. Compensation runs in reverse and attempts every committed
  file independently.
- Every rollback compares the file with this transaction's committed bytes.
  Config drift must not suppress an otherwise-owned backup rollback, and
  backup drift must not suppress an otherwise-owned config rollback.
- The dedicated settings transaction persists intent under the shared
  lifecycle/settings locks, applies the prepared catalog, and returns only a
  canonical reread. Apply failure or post-apply confirmation failure rolls back
  `AppliedManagedCatalog` and CAS-restores only the still-owned settings bit.
  A concurrent winner is preserved and the catalog is reconciled to that
  canonical winner.
- Structured and raw config saves treat submitted bytes as the proposed base,
  then prepare backup/generated/live output in that order. While enabled, a
  user may change the original catalog binding, but the live config remains
  bound to the current AIO catalog until disable restores the new source.
- Portable config export serializes the policy as disabled, and config import
  replaces any incoming policy value with the canonical pre-import bit. An
  imported bundle can neither enable nor disable this device/home-owned policy.
- Config import holds `config import -> managed Profile lifecycle` for the
  whole operation. When the canonical policy is enabled, an imported Codex-home
  change fails before DB or file mutation. When the policy is disabled but the
  old home still has an enabled proxy projection, import prepares a compensating
  home rebind and applies it after the durable settings commit through the
  `_locked` import path.
- The `_locked` import rebind restores/moves the proxy projection without
  applying a live gateway config and without synchronizing the managed catalog.
  It must not reacquire the public lifecycle lock or call public catalog sync;
  the outer import transaction independently prepares and applies the catalog
  from the staged Profiles and preserved canonical policy.
- Config import stages DB and Skill FS state, commits settings/autostart, applies
  the prepared home rebind, applies the separately prepared catalog, converges
  CLI runtime, and commits the DB transaction last. A later failure rolls back
  catalog and rebind first, independently from their own committed tokens, then
  aborts/restores DB and restores settings/autostart, Skill FS, and runtime.
  Every compensation is attempted even if another target drifted or failed.
- Rebind, catalog, settings, or runtime compensation failure must replace the
  ordinary primary error with the owning recovery-required result:
  `CLI_PROXY_REBIND_RECOVERY_REQUIRED`,
  `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`, or
  `CONFIG_IMPORT_RECOVERY_REQUIRED`. Recovery failure must never be hidden by
  returning only the original import error.
- Startup, direct/proxy/offline transitions, proxy disable/exit, managed
  Profile changes, provider capability changes, and Codex CLI fingerprint
  changes call the same policy-aware reconciliation path.
- While enabled, changing `codex_home_mode` or `codex_home_override` is
  forbidden. The user must disable successfully, completing source restoration,
  before selecting another home.
- Any ordinary settings mutation that carries Codex-home intent acquires the
  managed-profile lifecycle lock before `AUTO_START -> SETTINGS`, even when the
  requested home equals the current value. This serializes the home commit with
  the entire dedicated enable/disable transaction; the temporary persisted
  intent inside a compensating catalog update is never authorization to move
  homes.
- Existing Codex processes retain their startup snapshot. UI success promises
  the new catalog only to newly launched Codex sessions.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Policy absent, migrated, imported, or defaulted | Persist/return `false`; no policy-only catalog binding |
| All three exact targets are valid | Rewrite both window fields to `372000` |
| Any target is missing or duplicated | `CODEX_GPT56_372K_MODELS_MISSING`; zero committed intent/catalog changes |
| A target window field is structurally invalid | `CODEX_GPT56_372K_CATALOG_INVALID`; preserve source and live files |
| User source/owner/config/generated/backup drifts after prepare | Fail closed; never overwrite the drifted bytes |
| Settings commit succeeds but catalog apply fails | Roll back the owned setting and every committed file |
| Catalog apply succeeds but confirmation reread fails | Roll back catalog, CAS the setting, then reconcile any concurrent winner |
| Any compensation target no longer equals its committed token | Preserve the winner; reconcile from canonical settings |
| Compensation or winner reconciliation fails | `CODEX_GPT56_372K_CONTEXT_RECOVERY_REQUIRED` or `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` |
| Codex home changes while enabled | `CODEX_GPT56_372K_CONTEXT_HOME_CHANGE_BLOCKED`; no home/catalog mutation |
| Config import carries a different 372K bit | Ignore it and preserve the canonical pre-import bit |
| Disabled policy plus enabled proxy changes home during import | Rebind under the already-held lifecycle lock; apply catalog separately; acquire the lifecycle lock once |
| Import fails after home rebind or catalog application | Roll back catalog, rebind, DB/settings/Skill FS/runtime from independent committed tokens |
| Import compensation cannot restore an owned target | Promote to the applicable `CLI_PROXY_REBIND_RECOVERY_REQUIRED`, `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED`, or `CONFIG_IMPORT_RECOVERY_REQUIRED` code |
| Proxy stops while policy remains enabled | Keep a direct generated binding with `372000` targets |
| Policy disables while Profiles remain | Keep generated catalog/Profile rows; remove only the GPT-5.6 rewrite |
| Policy disables with zero Profiles | Restore the original binding/base and remove the owned generated file |

### 5. Good / Base / Bad Cases

- Good: enable against a complete user catalog, preserve its unknown fields,
  bind an AIO-derived catalog containing three `372000` pairs, then disable and
  restore the exact user path without modifying the source bytes.
- Good: leave the policy enabled while the proxy stops; a new direct Codex
  process still loads the derived catalog. Restart and CLI-update sync remain
  byte-stable when the base fingerprint has not changed.
- Good: with the policy disabled and an enabled proxy on the old Codex home,
  config import changes home under one lifecycle-lock acquisition, restores the
  old projection, rebinds the proxy baseline to the new home, and lets the outer
  import transaction apply the catalog exactly once.
- Base: a default installation with no managed Profiles keeps bundled behavior
  and no AIO catalog binding; the three bundled entries remain `272000`.
- Base: a managed `aio/*` Profile keeps its explicit provider-model context and
  effort capabilities regardless of this policy.
- Bad: write `model_context_window = 372000`, modify only
  `max_context_window`, treat `380928` as enabled, prefix-match `gpt-5.6*`, or
  regenerate asynchronously after reporting settings success.
- Bad: restore a whole settings snapshot or skip backup compensation because
  the live config concurrently drifted.
- Bad: trust the bundle's 372K bit, invoke the public home-rebind path from
  inside config import, recursively acquire the lifecycle lock, or let rebind
  call catalog sync before the outer import has prepared the staged catalog.

### 6. Tests Required

- Catalog unit tests assert exact three-slug/two-field rewrite, `380928`
  negative ownership, missing/duplicate/invalid fields, unknown-field
  preservation, unchanged `aio/*`, v2 metadata/hash, and stable regeneration.
- Dedicated settings tests inject failure after settings commit and after
  catalog apply/confirmation; assert exact settings, config, generated, and
  backup compensation plus concurrent-winner preservation.
- Config tests cover structured and raw proposed-source saves, active-binding
  preservation, changed-source restoration, source drift, and independent
  committed-token rollback for config and proxy backup.
- Lifecycle tests cover direct, proxy, restored-direct, offline, startup,
  proxy disable/exit, zero/nonzero Profiles, capability updates, and Codex
  executable fingerprint changes.
- Migration/import tests assert schema 63 -> 64 defaults false, ordinary
  writers cannot own the bit, export/import do not transfer it, and active
  policy blocks an imported Codex-home change. With the policy disabled and an
  enabled old-home proxy projection, assert successful home rebind, exact old
  and new baseline bytes, exactly one lifecycle-lock attempt, runtime-failure
  rollback of catalog/rebind/settings/DB state, and typed recovery-code
  promotion for failed rebind, catalog, or import compensation.
- Frontend tests assert exact `372,000` copy, canonical success/error rollback,
  duplicate-save blocking, query invalidation, and disabling every Codex-home
  control during the active policy or another Codex config write.
- In an isolated `CODEX_HOME`, run real `codex debug models` without
  `--bundled` after enable and assert all three pairs are `372000`; after
  disable assert the source/bundled `272000` values return.

### 7. Wrong vs Correct

#### Wrong

```rust
settings.codex_gpt56_372k_context_enabled = enabled;
settings::write(app, &settings)?;
let _ = sync_current_locked(app); // reports success before catalog convergence
```

#### Correct

```rust
let policy = ManagedCatalogPolicy {
    gpt56_372k_context_enabled: enabled,
};
let plan = prepare_for_profiles_with_policy(app, &profiles, policy)?;
let (_, previous_enabled) = settings::update(app, |latest| {
    let previous = latest.codex_gpt56_372k_context_enabled;
    latest.codex_gpt56_372k_context_enabled = enabled;
    Ok(previous)
})?;
let applied = match plan.apply(app) {
    Ok(applied) => applied,
    Err(error) => return Err(compensate_codex_gpt56_372k_context_failure(
        app, enabled, previous_enabled, None, error,
    )),
};
let canonical = match read_codex_gpt56_372k_confirmation(app) {
    Ok(canonical) => canonical,
    Err(error) => return Err(compensate_codex_gpt56_372k_context_failure(
        app, enabled, previous_enabled, Some(applied), error,
    )),
};
if canonical.codex_gpt56_372k_context_enabled != enabled {
    sync_current_locked(app)?;
}
Ok(canonical)
```
