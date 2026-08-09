# Settings Ownership And Rollback Contract

### 1. Scope / Trigger

Apply this contract whenever production code reads, mutates, imports, repairs, or rolls back
`AppSettings`. It covers settings UI, gateway effective-port repair, Grok preferences and CLI proxy,
config import, Image Gen storage roots, and every future `settings::write` call discovered under
`src-tauri/src`.

### 2. Signatures

```rust
pub fn update<R, F, T>(app: &AppHandle<R>, mutate: F) -> AppResult<(AppSettings, T)>
where
    R: Runtime,
    F: FnOnce(&mut AppSettings) -> AppResult<T>;

pub fn compare_and_swap<R: Runtime>(
    app: &AppHandle<R>,
    expected: &AppSettings,
    replacement: &AppSettings,
) -> AppResult<(AppSettings, bool)>;

pub struct SettingsUpdate {
    // Some(value) is explicit OS autostart intent; None preserves canonical state.
    pub auto_start: Option<bool>,
    // ...other ordinary settings fields...
}

#[derive(Default, serde::Deserialize, specta::Type)]
#[serde(default, rename_all = "camelCase")]
pub struct SettingsPatch {
    // Every ordinary settings field is Option<T>.
    // None means this writer does not own or change that field.
}

#[tauri::command]
pub async fn settings_patch(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    patch: SettingsPatch,
) -> Result<SettingsMutationResult, String>;

#[tauri::command]
pub async fn model_price_aliases_get(
    app: tauri::AppHandle,
) -> Result<ModelPriceAliasesV1, String>;

#[tauri::command]
pub async fn model_price_aliases_set(
    app: tauri::AppHandle,
    aliases: ModelPriceAliasesV1,
) -> Result<ModelPriceAliasesV1, String>;
```

The generated frontend contract is `autoStart: boolean | null`. Existing clients that send a boolean remain
compatible; new partial-save callers send `null` unless the source patch/changed-key set explicitly contains
`auto_start`.

`SettingsPatch` uses the same generated camelCase field names as `SettingsUpdate`, but every field is nullable.
The frontend entry point is:

```typescript
export async function settingsPatch(
  current: AppSettings,
  patch: AppSettingsPatch
): Promise<SettingsMutationResult | null>;
```

Model-price alias documents use schema version `2`, at most 512 rules, and the concrete rule fields
`cli_key`, `match_type` (`exact | prefix | wildcard`), `pattern`, `target_model`, and `enabled`.

Every field owner must also define an equality predicate or committed token containing only the fields it
owns. `settings::write(app, snapshot)` is a whole-snapshot primitive reserved for initialization and tests.

### 3. Contracts

- A production writer performs read, mutation, validation, serialization, and atomic replacement while
  holding the shared settings write lock through `settings::update`.
- Settings-page persistence sends only the keys changed from the last committed snapshot through
  `settings_patch`. Null/missing patch fields preserve the canonical value read under the backend write lock;
  they must never be expanded into a stale whole-settings snapshot.
- All ordinary settings mutations share one TanStack mutation scope. The Settings-page runner permits one
  in-flight save and coalesces later edits into the latest pending desired snapshot; after each settlement it
  recomputes changed keys against the returned canonical snapshot before issuing the next patch.
- A failed or unavailable settings read enables read-only protection, clears pending saves, and reverts only
  the affected local keys. Cached settings may remain visible but do not authorize writes.
- A writer changes only its owned fields. Ordinary `settings_set` applies an explicit field patch under
  `settings::update`; it never rebuilds a whole snapshot from a lock-out-of-date read. Image Gen owns
  `image_gen_storage_dir` / `image_gen_storage_roots`, Grok owns `grok_proxy_preferences`, circuit notice owns
  `enable_circuit_breaker_notice`, Codex completion owns `enable_codex_session_id_completion`, rectifier owns
  the 12 rectifier/response-fixer fields, and gateway repair conditionally owns only `preferred_port`.
- Ordinary `SettingsUpdate` / generated bindings / frontend ordinary payload must not include rectifier
  exclusive fields. Future `AppSettings` fields do not automatically become ordinary-owner fields.
- Complete config import may replace the whole snapshot only through `compare_and_swap` (or the shared
  autostart coordinator that wraps it); the canonical snapshot used for preparation is the expected value.
- A writer with external side effects records the exact owned-field value it committed. Rollback restores
  only those owned fields and only while they still equal that committed token.
- All production writers that change canonical `auto_start` share one autostart coordinator with a monotonic
  generation token. OS autostart side effects happen only after durable settings commit succeeds. Invalid
  candidates produce zero OS calls. Token losers never restore an older value over a newer winner; they only
  converge OS to the latest canonical value.
- Ordinary `settings_set` treats `SettingsUpdate.auto_start` as intent, not a required snapshot field.
  `Some(value)` enters the autostart coordinator and force-syncs the OS even when the value equals canonical;
  `None` preserves the latest lock-internal value, returns no autostart token, and performs no OS autostart call.
  Frontend patch builders must not infer intent merely because the current settings snapshot contains the field.
- A runtime-failure rollback with no autostart token is a settings-only owned CAS: it does not acquire the
  autostart owner, does not advance its generation, does not call OS autostart, and preserves a concurrent
  `auto_start` winner. A later effective preferred-port repair remains a separate writer that may advance the
  generation to invalidate stale tokens, but it must not call OS autostart.
- On Windows, disabling an already absent `Run` key/value is idempotent success (`ErrorKind::NotFound`). Registry
  permission, access, and all other I/O errors remain failures and must reach the coordinator's correction path.
- Lock order is `CONFIG_IMPORT_LOCK -> AUTO_START_LOCK -> SETTINGS_WRITE_LOCK`. Code holding the settings lock
  must never acquire the autostart lock.
- Losing rollback CAS preserves the newer settings and must not restore old gateway runtime, CLI proxy, or
  OS autostart state.
- Whole-import autostart reconciliation runs only inside the shared autostart coordinator after the settings
  CAS succeeds. Correction/rollback use the same generation token protocol and never restore a loser's value.
- Whole-import rollback treats generation ownership as authoritative for
  `auto_start`. If its token generation is stale, it must not restore
  `auto_start` even when the current value equals the import snapshot (a
  same-value ABA). It may still restore every other import-owned field whose
  value equals the committed snapshot, and OS autostart must converge to the
  resulting canonical winner.
- Settings-service owned rollback has an explicit `Restored` / concurrent-winner / failure result. Only
  `Restored` authorizes previous-runtime restoration. Other results keep or resynchronize runtime side effects
  from the current canonical snapshot.
- Searching production Rust sources for `settings::write(` must find no writer; fixture/seed calls are the
  only permitted exceptions.
- The model-price alias editor uses strict `model_price_aliases_get`; malformed, unreadable, oversized, or
  unsupported alias files keep the editor blocked until a retry succeeds. It must not replace failed reads with
  defaults and then overwrite the user's file. Runtime cost lookup may continue to use the explicitly named
  `read_fail_open` path because it does not authorize an edit.
- Alias reads and writes accept schema versions 1 and 2, migrate v1 to v2, cap the file at 1 MiB and rules at
  512, trim and validate non-empty fields up to 200 bytes, and write atomically. Wildcard patterns contain
  exactly one `*`; exact/prefix patterns and target models contain none.

### 4. Validation & Error Matrix

| Condition | Required result | Error / side effect |
| --- | --- | --- |
| Owned-field validation succeeds under lock | Commit latest snapshot plus owned delta | Return persisted snapshot |
| Unrelated owner commits before lock acquisition | Preserve unrelated fields | No error |
| Whole-import expected snapshot still matches | Replace atomically | CAS returns `true` |
| Whole-import snapshot drifted | Preserve latest snapshot | `SETTINGS_CONCURRENT_UPDATE` / CAS `false` |
| Whole-import CAS loses before autostart reconciliation | Preserve winner | No autostart side effect from loser |
| Later ordinary writer commits the import's same `auto_start` value and advances generation | Preserve that same-value winner | Roll back only other import-owned fields; sync OS to canonical winner |
| Ordinary patch omits / sends `null` for `auto_start` | Preserve the latest canonical value | No autostart owner/generation or OS call from direct commit |
| Settings-only runtime rollback races with explicit autostart writer | Restore ordinary owned fields only | Preserve concurrent `auto_start`; no OS call from the token-less rollback |
| Explicit Windows disable finds no Run key/value | Treat target state as already satisfied | Success; do not enter correction |
| Explicit Windows disable hits permission/access error | Keep canonical/OS recovery rules authoritative | Propagate the original non-`NotFound` error |
| External side effect fails and committed token still matches | Restore only owned fields | Report original operation failure |
| External side effect fails after newer owned-field commit | Skip rollback and old runtime restoration | Preserve newer value; safe warning allowed |
| Atomic settings persistence fails | Leave last durable snapshot authoritative | Return persistence error without partial file |
| Patch field is null or absent | Preserve the latest canonical field | No ownership or side effect for that field |
| One save is active and the user edits again | Retain the latest desired snapshot | Recompute a changed-key patch after settlement |
| Settings GET fails while cached data exists | Keep cached display read-only | Clear queued writes and surface the read error |
| Alias file is absent | Return the version-2 default document | Editor may load and save |
| Alias file is malformed, oversized, or unsupported | Fail the strict GET | Keep editor controls and save blocked |
| Alias v1 document is valid | Normalize to v2 and add the default Grok rule when missing | Return normalized document |
| Wildcard count is not exactly one | Reject the document | `SEC_INVALID_INPUT`; do not replace the file |

### 5. Good / Base / Bad Cases

- **Good:** `grok_config::set` preflights, then uses `settings::update` to replace only
  `grok_proxy_preferences`; a concurrent Image Gen root survives.
- **Good:** a retry-policy patch sends `autoStart: null`; Rust commits the policy under the settings lock and
  returns `None` for the autostart token, so an absent Windows startup entry is never inspected.
- **Base:** config import prepares from snapshot `S`, then CAS replaces `S` with imported `S2` when no writer
  intervenes.
- **Bad:** rebuilding a complete settings payload causes an unrelated retry-policy save to resend the current
  `autoStart` boolean; the backend then treats an unrelated save as explicit OS repair intent.
- **Bad:** code clones `settings::read`, changes one field, and later calls `settings::write`; it can overwrite
  every owner that committed in between.
- **Bad:** rollback writes an old whole snapshot or restores old runtime after its owned-field CAS loses.
- **Good:** while save A is in flight, edits B and C coalesce into the latest desired snapshot; after A returns,
  the runner sends only keys still different from A's canonical result.
- **Bad:** convert a failed alias read to an empty/default editable draft and let Save replace the unreadable
  file, or include unchanged settings keys in every queued request.

### 6. Tests Required

- Put a deterministic hook between a real production writer's preflight and locked mutation. Commit an Image
  Gen root through the production settings path and prove the real writer preserves it.
- Put a hook after a production Grok commit and before forced inspection failure. Commit a newer Grok value and
  prove rollback preserves it and does not restore stale runtime state.
- Cover whole-import CAS success and `SETTINGS_CONCURRENT_UPDATE` with deterministic interleaving.
- Force runtime sync failure, commit a newer owner value before rollback, and prove the service syncs the
  canonical winner rather than previous runtime. Count autostart calls in the real import CAS-loser path.
- Through the real config-import runtime-failure path, advance generation with
  an ordinary writer that commits the same `auto_start` value. Cover both a
  snapshot otherwise equal to the import and a partial ordinary-field winner;
  assert `auto_start` survives, other fields remain field-aware, and the last
  OS target is the canonical winner.
- Through the real settings service, save only `upstream_retry_policy` with `auto_start=None`; assert zero
  autostart lock attempts, zero generation-owned mutations, zero OS calls, and successful policy persistence.
- Force a token-less runtime rollback after a concurrent explicit autostart writer; assert ordinary fields are
  restored, the concurrent `auto_start` survives, and the rollback records zero autostart lock/OS calls.
- Test both frontend partial-save owners: CLI Manager source patches and Settings-page queued changed-key saves.
  After one explicit autostart save settles, the next unrelated queued request must encode `autoStart: null`.
- Unit-test the Windows adapter without real registry mutation: open/delete success, missing key, missing value,
  and non-`NotFound` open/delete failures.
- Search production Rust sources for `settings::write(` and allow only test fixtures/seeding.
- Test changed-key patch construction, no-op patches, rapid queued edits, settled `auto_start` omission,
  reverse/failed completion handling, pending-queue clearing on read failure, and backend merge after a
  deterministic concurrent writer.
- Test strict alias GET for invalid JSON, invalid UTF-8, oversized input, unsupported versions, rule-count and
  field bounds, invalid CLI/match types, wildcard shape, v1-to-v2 migration, default behavior only when absent,
  atomic write preservation, and UI save blocking until a successful retry.
- Run settings, gateway, Grok, CLI proxy, config-migration focused suites and the full Rust library suite.

### 7. Wrong vs Correct

```rust
// Wrong: mutation is based on a snapshot read before the serialization lock.
let mut next = settings::read(app)?;
next.grok_proxy_preferences = Some(preferences);
settings::write(app, &next)?;

// Correct: the owner mutates the latest value while holding the shared lock.
let committed = Some(preferences);
let (_, previous) = settings::update(app, |latest| {
    let previous = latest.grok_proxy_preferences.clone();
    latest.grok_proxy_preferences = committed.clone();
    Ok(previous)
})?;

// Correct rollback: restore only if this writer's committed token still owns the field.
settings::update(app, |latest| {
    if latest.grok_proxy_preferences == committed {
        latest.grok_proxy_preferences = previous;
    }
    Ok(())
})?;
```

```typescript
// Wrong: snapshot reconstruction invents auto-start intent for every patch.
const input = { ...current, ...patch, autoStart: current.auto_start };

// Correct: only the source patch owns intent; transport encodes omission as null.
const input = createSettingsSetInput(current, patch);
const update = { ...input, autoStart: input.autoStart ?? null };
```

```typescript
// Wrong: every queued save resends a snapshot captured before the prior save settled.
await settingsSet(createSettingsSetInput(staleSnapshot, desired));

// Correct: serialize ordinary mutations and send only keys that still differ.
const changedKeys = diffPersistedSettings(committed, desired);
await settingsPatch(committed, buildPersistedSettingsPatch(desired, changedKeys));
```

```typescript
// Wrong: a failed alias read silently becomes an editable empty document.
const aliases = query.data ?? { version: 2, rules: [] };

// Correct: cached data may render, but read error/null blocks edits and save.
const blocked = query.isError || query.data == null;
```

## Follow-up Findings F9 and F13

- An ordinary settings writer's previous and committed tokens must be built
  directly from that writer's locked durable settings::update result. A
  coordinator return or later canonical reread may update only the coordinator's
  own auto_start correction; it must not absorb a gateway preferred-port repair
  or another writer into the ordinary rollback token.
- The production regression for a post-coordinator preferred-port repair must
  pause between coordinator return and token construction, force the later
  runtime sync to fail, and prove rollback converges to the preferred-port
  winner without restoring the previous runtime.
- Settings persistence finalization must distinguish finalize failure from
  restore failure. If both fail, return SETTINGS_RECOVERY_REQUIRED, preserve
  the best available durable settings bytes (backup or retained writer temp),
  clean only writer-owned temporary output, and never claim that canonical
  settings are usable.

## Scenario: Commit Codex OAuth-Compatible Proxy Mode

### 1. Scope / Trigger

Apply this scenario when `enable_codex_oauth_compatible_proxy` changes. It is a
settings-owned value with a compensating Codex config/auth projection, not a
gateway-wide or model-catalog refresh.

### 2. Signatures

```rust
sync_codex_oauth_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    base_origin: &str,
    apply_live: bool,
) -> AppResult<CliProxyResult>
```

The frontend command awaits only the settings mutation. Successful mutation
invalidates settings, Codex structured/raw config, and CLI proxy status queries;
it must not await `refreshCodexModelCatalog`.

### 3. Contracts

- Commit the setting under settings ownership, then run OAuth-only projection.
- OAuth-only projection acquires the Codex lifecycle lock before reading the
  manifest. Missing or disabled manifest is a successful no-op.
- Enabled projection updates only the existing Codex config/auth/manifest
  targets. It preserves an active managed `model_catalog_json` binding and must
  not rebuild/discover the model catalog.
- Sync failure returns `CODEX_OAUTH_PROXY_SYNC_FAILED`, restores target/backup/
  manifest snapshots, then rolls back only the setting value still owned by the
  failed writer.
- If owned rollback loses to a newer settings writer, preserve that winner and
  converge Codex projection to the latest canonical value. If rollback or
  convergence fails, append `SETTINGS_RECOVERY_REQUIRED`.
- Never hold the gateway lifecycle lock while entering a helper that reacquires
  it; lock ordering must make forward progress in both gateway-running and
  gateway-stopped paths.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Proxy manifest absent or disabled | Persist setting; no config/auth/catalog write |
| Enabled proxy projection succeeds | Persist setting and coherent config/auth/manifest |
| Projection fails, token still owned | Restore previous setting and exact target snapshots |
| Projection fails after concurrent winner | Preserve winner; reproject to canonical winner |
| Snapshot restoration fails | `CODEX_OAUTH_PROXY_RECOVERY_REQUIRED` |
| Settings rollback/convergence fails | Include `SETTINGS_RECOVERY_REQUIRED` |

### 5. Good / Base / Bad Cases

- Good: toggle returns after the settings/projection transaction; query refresh
  happens asynchronously and a stalled model catalog cannot stall the switch.
- Base: routing is disabled; the preference persists and no proxy file is
  rewritten until a later explicit enable uses it.
- Bad: swallow projection failure after saving settings, or invoke full
  `sync_enabled` / bundled-catalog discovery for an OAuth-only toggle.

### 6. Tests Required

- Count catalog sync calls and assert zero for enabled, disabled, and managed
  profile OAuth-only cases; preserve `model_catalog_json` binding.
- Force projection failure and assert prior config, auth, backup, manifest, and
  owned setting are restored.
- Deterministically race manifest disable after lifecycle-lock contention; the
  locked reread must observe disabled and must not replay stale enabled state.
- Deterministically commit a concurrent settings winner during rollback; assert
  the winner survives and runtime converges to it.
- Frontend-test a never-resolving catalog refresh; the switch mutation still
  resolves and failures surface stable error text.

### 7. Wrong vs Correct

```rust
// Wrong: stale manifest read, full catalog sync, and ignored failure.
let manifest = read_manifest(app, "codex")?;
let _ = sync_enabled(app, base_origin, apply_live).await;

// Correct: the OAuth-only helper locks before reading and failure is transactional.
let result = sync_codex_oauth_enabled(app, base_origin, apply_live)?;
if !result.ok {
    rollback_owned_setting_and_converge_canonical(app, result)?;
}
```
