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
- A writer changes only its owned fields. Settings UI preserves `image_gen_storage_dir`,
  `image_gen_storage_roots`, and `grok_proxy_preferences`; Grok writers own only
  `grok_proxy_preferences`; gateway repair conditionally owns only `preferred_port`.
- Complete config import may replace the whole snapshot only through `compare_and_swap`; the canonical
  snapshot used for preparation is the expected value.
- A writer with external side effects records the exact owned-field value it committed. Rollback restores
  only those owned fields and only while they still equal that committed token.
- Losing rollback CAS preserves the newer settings and must not restore old gateway runtime, CLI proxy, or
  OS autostart state.
- Searching production Rust sources for `settings::write(` must find no writer; fixture/seed calls are the
  only permitted exceptions.

### 4. Validation & Error Matrix

| Condition | Required result | Error / side effect |
| --- | --- | --- |
| Owned-field validation succeeds under lock | Commit latest snapshot plus owned delta | Return persisted snapshot |
| Unrelated owner commits before lock acquisition | Preserve unrelated fields | No error |
| Whole-import expected snapshot still matches | Replace atomically | CAS returns `true` |
| Whole-import snapshot drifted | Preserve latest snapshot | `SETTINGS_CONCURRENT_UPDATE` / CAS `false` |
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
