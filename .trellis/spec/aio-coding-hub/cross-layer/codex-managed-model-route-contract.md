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
- Filesystem-safety fixtures that replace an active `$CODEX_HOME` preserve the
  active `config.toml` binding in the replacement when the intended variable is
  only the unsafe home layout. If they omit that binding, they are catalog-drift
  cases and must assert the catalog error instead of expecting
  `CODEX_MANAGED_PROFILE_HOME_UNSAFE` after an unrelated preflight already
  failed closed.
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

## Scenario: Device-Local Codex Model Context Rules

### 1. Scope / Trigger

Use this contract when changing exact-model context rules, settings migration
or import, the read-only base-catalog candidate API, Codex-home selection,
generated picker catalogs, raw or structured Codex config saves, CLI proxy
lifecycle, managed Profiles, provider capabilities, or startup reconciliation.

The feature owns only a device/Codex-home-local derived-catalog policy. It does
not own a root `model_context_window`, automatic compaction settings, the
installed Codex binary, a user's source catalog, or `aio/*` Profile context.

### 2. Signatures

The persisted and IPC boundary is:

```rust
pub struct CodexModelContextRule {
    pub model_id: String,
    pub context_window: i64,
    pub enabled: bool,
}

pub(crate) struct ManagedCatalogPolicy {
    pub(crate) model_context_rules: Vec<CodexModelContextRule>,
}

settings_codex_model_context_rules_set(
    rules: Vec<CodexModelContextRule>,
) -> SettingsView

cli_manager_codex_model_context_candidates_get()
    -> CodexModelContextCandidatesState

sync_current_locked(app: &AppHandle<R>) -> AppResult<()>
prepare_for_profiles_with_policy(
    app: &AppHandle<R>,
    profiles: &[ManagedCatalogProfile],
    policy: ManagedCatalogPolicy,
) -> AppResult<ManagedCatalogPlan>

ManagedCatalogPlan::apply(app) -> AppResult<AppliedManagedCatalog>
AppliedManagedCatalog::rollback() -> AppResult<()>
```

`AppSettings.codex_model_context_rules` is persisted with settings schema 65
and defaults to an empty vector. `SettingsView` exposes it read-only. Ordinary
`SettingsUpdate` and `SettingsPatch` carry it only in their compare token and
must never apply it; the whole-collection command is the sole writer.

### 3. Rule And Migration Contracts

- A rule matches one trimmed, case-sensitive, exact `models[].slug`. Prefix,
  glob, regex, alias, and model-family expansion are forbidden.
- The model ID must be 1..=256 UTF-8 bytes, contain no control characters, and
  must not start with the literal reserved prefix `aio/`. IDs are globally
  unique across enabled and disabled rules.
- The single decimal token value must be in `1_024..=10_000_000`, reusing the
  Provider model capability constants. `272K` and `372K` follow Codex's decimal
  convention: `272000` and `372000`, not binary-K conversions.
- A collection contains at most 128 rules and is stored deterministically by
  `model_id.as_bytes()`. Every writer, migration, policy constructor, and owner
  hash uses the same strict canonical normalizer.
- Disabled rules still pass all static validation, remain persisted, and do
  not require a current catalog target. Re-enabling performs full target
  validation; no rejected candidate is stored for future activation.
- Schema 64 legacy `false` or absent migrates to no rules. Legacy `true`
  migrates exactly to enabled `gpt-5.6-luna`, `gpt-5.6-sol`, and
  `gpt-5.6-terra` rules at decimal `372000`, then canonical sorting applies.
  A non-boolean schema 64 legacy field fails closed. The repair write must be
  durable before lifecycle reconciliation continues.
- Schema 65 no longer serializes the legacy bit. Export omits both the new and
  legacy keys; import removes both before typed decoding and restores the
  complete pre-import canonical rule set under lock.
- The GPT-5.6 372K shortcut exists only in frontend draft code. It adds the
  three ordinary exact rules when absent and never creates a backend special
  case or submits before the user applies the complete draft.

### 4. Base Candidates And Catalog Projection

- The candidate GET holds the Profile lifecycle lock and reads the original
  absolute user catalog or `codex debug models --bundled` directly. It must not
  call reconcile, create managed directories, or write config/catalog files.
- Candidates project exact slug, display-name fallback, conservative hidden
  state, and optional non-negative base/max windows. Literal `aio/` entries are
  excluded. Candidate failure degrades suggestions only; manual exact-ID entry
  remains available.
- Candidates are advisory. SET rereads and validates the authoritative base in
  the same transaction that prepares the candidate policy.
- A generated catalog is required when at least one rule is enabled OR managed
  Profiles exist. With neither owner, restore the original
  `model_catalog_json` binding or its absence and remove only the owned file.
- Every enabled rule must match exactly one base entry. Both base window fields
  must be non-negative JSON integers. Any missing, ambiguous, or structurally
  invalid target aborts the whole plan before the first write.
- Projection writes the rule's one value to both `context_window` and
  `max_context_window`. Values may increase, preserve, or lower the base value.
  All other root/model fields, `effective_context_window_percent`, automatic
  compaction, reasoning fields, and unknown fields remain base-owned.
- Apply ordinary exact rules before appending `aio/*` Profiles. Profile context
  remains exclusively owned by Provider model capabilities.
- Owner metadata schema v3 covers projection algorithm version, canonical full
  rule-set hash, enabled rule projection and hash, Profile hash, base-source
  fingerprint, normalized Codex-home identity, original binding, projection
  hash, and payload hash. v1/v2 metadata is validation/recovery-only and must
  not be treated as proof of v3 home ownership.
- Prepare and candidate reads are side-effect free. Apply rechecks owner, home,
  base source, generated bytes, live config, and proxy backup before writing.
  Activation applies backup repair, generated catalog, then live config;
  deactivation restores live config before deleting the generated file.

### 5. Transaction And Lifecycle Contracts

- The dedicated SET holds the Profile lifecycle lock and performs normalize ->
  prepare all targets -> commit complete canonical rules -> apply prepared
  files -> canonical/owner/projection confirmation. It returns only confirmed
  backend state.
- On failure, roll back committed files in reverse while each still equals this
  transaction's after-bytes, then CAS-restore rules only if they still equal
  this transaction's committed token. Preserve a newer winner and reconcile
  the catalog from that winner.
- A compensation or winner-reconciliation failure returns
  `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED` or the shared managed-catalog
  recovery code; it must not be hidden behind the original error.
- Startup, direct/proxy/offline transitions, proxy disable/exit, Codex config
  save, import, managed Profile changes, provider capability changes, and CLI
  fingerprint changes construct policy from the latest canonical rules and use
  the same reconciler.
- If a previously successful enabled target disappears after a CLI/base
  upgrade, preserve canonical enabled intent, the last-good generated catalog,
  live binding, and proxy backup. Startup fails at `ReadingSettings` with a
  retryable outer error and the inner typed catalog/rule error; it never
  partially rebuilds or silently disables the rule.
- Any enabled rule blocks an actual Codex-home change. Compare the effective
  homes resolved from candidate settings under the settings lock, not raw mode
  or override strings. Equivalent inputs such as a home directory and its
  `config.toml` path are allowed. Disabled-only collections may move home and
  remain unchanged.
- Any ordinary mutation carrying home intent still acquires the Profile
  lifecycle lock before `AUTO_START -> SETTINGS`, including semantic no-ops,
  so a home writer and rule writer cannot both commit against different homes.
- Config import lock order remains `config import -> Profile lifecycle ->
  update channel`. It preserves canonical rules, uses effective-home equality,
  prepares any inactive-policy rebind once, and rolls back catalog, rebind,
  settings/DB/Skill FS/runtime from independent committed tokens.
- Existing Codex processes retain their startup snapshot. UI success promises
  the new catalog only to newly launched Codex sessions.

### 6. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| More than 128 rules | `CODEX_MODEL_CONTEXT_RULE_LIMIT`; zero writes |
| Invalid ID, reserved `aio/`, or token outside shared bounds | `CODEX_MODEL_CONTEXT_RULE_INVALID`; zero writes |
| Duplicate trimmed exact ID | `CODEX_MODEL_CONTEXT_RULE_DUPLICATE`; zero writes |
| Enabled target absent from base | `CODEX_MODEL_CONTEXT_RULE_TARGET_MISSING`; no candidate intent or file commit |
| Enabled target duplicated or has unsafe window structure | `CODEX_MODEL_CONTEXT_RULE_TARGET_INVALID`; preserve source and live files |
| Disabled target absent | Valid persisted rule; no projection and no startup failure |
| User source/owner/home/config/generated/backup drifts after prepare | Fail closed; never overwrite drifted bytes |
| Settings commit succeeds but apply or confirmation fails | Roll back files, CAS rules, then reconcile a concurrent winner |
| Compensation cannot restore an owned target | `CODEX_MODEL_CONTEXT_RULES_RECOVERY_REQUIRED` or `CODEX_MANAGED_MODEL_RECOVERY_REQUIRED` |
| Effective Codex home changes while any rule is enabled | `CODEX_MODEL_CONTEXT_RULES_HOME_CHANGE_BLOCKED`; no home/catalog mutation |
| Raw home fields differ but resolve to the same home | Allow; do not prepare a rebind |
| Import carries absent, different, or malformed rule keys | Ignore both policy keys and preserve canonical local rules |
| Proxy stops while rules remain enabled | Keep a direct generated binding with every enabled exact projection |
| All rules disabled while Profiles remain | Keep Profile catalog; remove only rule projections |
| All rules disabled with zero Profiles | Restore original binding and remove the owned generated file |

### 7. Good / Base / Bad Cases

- Good: apply multiple exact enabled rules in one transaction, preserve unknown
  fields, write each value to both window fields, and later disable one without
  deleting its stored model ID or token value.
- Good: a base upgrade temporarily removes an enabled target; startup preserves
  last-good bytes and intent, then a later compatible base plus explicit retry
  converges automatically.
- Good: candidate GET returns base values without creating the managed catalog
  directory, and a candidate outage still allows manual draft repair.
- Base: no enabled rules and no Profiles leaves Codex on its user/bundled base.
  A managed `aio/*` Profile remains capability-owned regardless of rules.
- Bad: write top-level `model_context_window`, update only one window field,
  prefix-match a model family, retain a backend `372000` special case, or report
  settings success before catalog confirmation.
- Bad: accept an unavailable enabled target for future activation, trust a
  portable bundle's rules, compare raw home strings, or restore a whole
  settings snapshot over a concurrent rule winner.

### 8. Tests Required

- Normalizer/migration tests cover 1,024 and 10,000,000, 128/129 rules, UTF-8
  byte length, control characters, reserved prefix, normalized duplicates,
  deterministic sorting, schema 64 false/true/malformed, repair durability, and
  idempotent schema 65 rereads.
- Catalog tests cover one/many/disabled rules, exact case, high/equal/low
  values, dual-field projection, missing/duplicate/invalid targets, unknown
  fields, unchanged `aio/*`, v1/v2 recovery, v3 hashes/home identity, source
  drift, zero-write prepare, and every apply/rollback stage.
- Dedicated SET tests inject failure after settings commit and after catalog
  apply/confirmation; assert settings/config/generated/backup compensation,
  concurrent-winner preservation, and one collection commit per UI apply.
- Candidate tests assert original-base values, hidden filtering, malformed
  degradation, zero reconcile, zero directory creation, and no host-Codex
  dependency in success fixtures.
- Lifecycle tests cover direct, proxy, restored-direct, offline, startup,
  disable/exit, zero/nonzero Profiles, capability updates, CLI upgrades,
  last-good failure/recovery, and rules editing while Gateway is not ready.
- Import/export tests assert property omission, ignored malformed policy input,
  canonical rule preservation, effective same-home acceptance, active actual
  home-change rejection, disabled-only rebind, and recovery-code priority.
- Frontend tests cover draft CRUD, enable/disable, GPT-5.6 preset, strict local
  validation, searchable/manual ID input, base comparison and warning, one SET,
  pending suppression, canonical success, awaited failure reread, query
  invalidation, home guard, and startup-recovery navigation.

### 9. Wrong vs Correct

#### Wrong

```rust
settings.codex_model_context_rules = requested_rules;
settings::write(app, &settings)?;
let _ = sync_current_locked(app); // reports success before catalog convergence
```

#### Correct

```rust
let policy = ManagedCatalogPolicy::from_rules(requested_rules)?;
let canonical_rules = policy.model_context_rules.clone();
let plan = prepare_for_profiles_with_policy(app, &profiles, policy)?;

let (_, previous_rules) = settings::update(app, |latest| {
    let previous = latest.codex_model_context_rules.clone();
    latest.codex_model_context_rules = canonical_rules.clone();
    Ok(previous)
})?;

let applied = plan.apply(app).map_err(|error| {
    compensate_codex_model_context_rules_failure(
        app,
        &canonical_rules,
        previous_rules,
        None,
        error,
    )
})?;

let canonical = confirm_rules_owner_projection_or_compensate(
    app,
    &canonical_rules,
    applied,
)?;
Ok(SettingsView::from(&canonical))
```
