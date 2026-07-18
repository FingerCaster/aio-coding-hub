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
```

Every field owner must also define an equality predicate or committed token containing only the fields it
owns. `settings::write(app, snapshot)` is a whole-snapshot primitive reserved for initialization and tests.

### 3. Contracts

- A production writer performs read, mutation, validation, serialization, and atomic replacement while
  holding the shared settings write lock through `settings::update`.
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

### 4. Validation & Error Matrix

| Condition | Required result | Error / side effect |
| --- | --- | --- |
| Owned-field validation succeeds under lock | Commit latest snapshot plus owned delta | Return persisted snapshot |
| Unrelated owner commits before lock acquisition | Preserve unrelated fields | No error |
| Whole-import expected snapshot still matches | Replace atomically | CAS returns `true` |
| Whole-import snapshot drifted | Preserve latest snapshot | `SETTINGS_CONCURRENT_UPDATE` / CAS `false` |
| Whole-import CAS loses before autostart reconciliation | Preserve winner | No autostart side effect from loser |
| Later ordinary writer commits the import's same `auto_start` value and advances generation | Preserve that same-value winner | Roll back only other import-owned fields; sync OS to canonical winner |
| External side effect fails and committed token still matches | Restore only owned fields | Report original operation failure |
| External side effect fails after newer owned-field commit | Skip rollback and old runtime restoration | Preserve newer value; safe warning allowed |
| Atomic settings persistence fails | Leave last durable snapshot authoritative | Return persistence error without partial file |

### 5. Good / Base / Bad Cases

- **Good:** `grok_config::set` preflights, then uses `settings::update` to replace only
  `grok_proxy_preferences`; a concurrent Image Gen root survives.
- **Base:** config import prepares from snapshot `S`, then CAS replaces `S` with imported `S2` when no writer
  intervenes.
- **Bad:** code clones `settings::read`, changes one field, and later calls `settings::write`; it can overwrite
  every owner that committed in between.
- **Bad:** rollback writes an old whole snapshot or restores old runtime after its owned-field CAS loses.

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
- Search production Rust sources for `settings::write(` and allow only test fixtures/seeding.
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
