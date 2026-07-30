# Codex Config Contract

## Scenario: Add Or Change A Structured Codex Config Field

### 1. Scope / Trigger

Use this contract when a root `config.toml` field is exposed through AIO's
structured Codex settings. The field crosses these owners:

```text
config.toml
  -> src-tauri/src/infra/codex_config
  -> src/generated/bindings.ts
  -> src/services/cli/cliManager.ts
  -> src/components/cli-manager/tabs/CodexTab.tsx
```

This contract prevents a field from being readable in one layer but silently
cleared, rejected, or misrepresented in another.

### 2. Signatures

The Rust source of truth is in
`src-tauri/src/infra/codex_config/types.rs`:

```rust
pub struct CodexConfigState {
    pub approvals_reviewer: Option<String>,
}

pub struct CodexConfigPatch {
    pub approvals_reviewer: Option<String>,
}
```

Specta generates both TypeScript fields as `string | null` in
`src/generated/bindings.ts`. The public frontend patch type is
`Partial<GeneratedCodexConfigPatch>`; `DEFAULT_CODEX_CONFIG_PATCH` in
`src/services/cli/cliManager.ts` supplies `null` for omitted generated fields.

### 3. Contracts

- Read: `make_state_from_bytes` reads root string values verbatim. Supported
  and future strings remain observable through `CodexConfigState`.
- Structured patch omitted / `null`: do not modify the existing TOML key.
- Structured patch empty string: delete the root key.
- Structured patch supported string: upsert exactly one root key through the
  existing patch helpers.
- Structured patch unsupported string: fail before output bytes are produced.
- Full raw TOML save: validate the complete file before atomic write. Fields
  with exact enums must reject empty, non-string, padded, and unknown values.
- Generated boundary: edit Rust types, run `pnpm tauri:gen-types`, and format
  the generated file. Do not hand-maintain generated types as the source of
  truth.
- Frontend adapter: a `null` default must deserialize to Rust `None`; it is not
  the same as the empty-string deletion signal.
- UI: preserve unknown current values with a synthetic option. Passive render
  and changes to a companion setting must never clean up that value.

For `approvals_reviewer`, `approval_policy` decides whether a request exists
and the reviewer decides who evaluates an eligible request. The UI may warn
about an ineffective combination, but only an explicit user action may patch
the companion field.

### 4. Validation & Error Matrix

| Boundary | Input | Result |
| --- | --- | --- |
| Structured patch | field omitted / `null` | Preserve current key |
| Structured patch | `""` | Delete current key |
| Structured patch | `user` / `auto_review` | Upsert root string |
| Structured patch | other non-empty string | Return validation error |
| Raw TOML | exact `"user"` / `"auto_review"` | Accept |
| Raw TOML | empty or padded string | Reject before write |
| Raw TOML | non-string or unknown string | Reject before write |
| Structured read | unknown string already on disk | Return it verbatim |
| Unrelated structured patch | unknown reviewer on disk | Preserve it verbatim |

The raw reviewer enum uses exact comparison in
`validate_root_exact_string_enum`. Do not use the trim-tolerant generic enum
validator for a field whose raw contract requires exact values.

### 5. Good / Base / Bad Cases

- Good: select `auto_review`; write one
  `approvals_reviewer = "auto_review"` root key and preserve comments/tables.
- Base: patch only `model`; an existing future reviewer value remains intact.
- Good: render `auto_review + never` as ineffective and offer an explicit
  `approval_policy = "on-request"` action.
- Bad: map an unknown reviewer to the unset option; this hides external state
  and encourages accidental cleanup.
- Bad: silently rewrite `approval_policy` when the reviewer selector changes.
- Bad: validate a full raw save after writing; invalid input must leave the
  previous file bytes unchanged.

### 6. Tests Required

- `src-tauri/src/infra/codex_config/tests.rs`
  - Parse supported and unknown strings exactly.
  - Write supported values once, delete on empty, and reject unsupported
    structured values.
  - Preserve unknown values during unrelated patches.
  - Cover empty, padded, non-string, and unknown raw values.
- `src-tauri/tests/codex_config_toml_raw.rs`
  - Assert each invalid full-file save leaves existing bytes unchanged.
- `src/services/cli/__tests__/cliManager.service.test.ts`
  - Assert omitted frontend fields normalize to `null` and explicit values
    cross the generated command boundary.
- `src/components/cli-manager/tabs/__tests__/`
  - Exhaust the pure policy/reviewer matrix.
  - Assert unknown-value display, direct selector patches, and companion-field
    changes only from the explicit action.

Focused verification:

```powershell
pnpm exec vitest run src/components/cli-manager/tabs/__tests__/codexApprovalReviewer.test.ts src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx
Push-Location src-tauri
cargo test --lib infra::codex_config::tests
cargo test --test codex_config_toml_raw
Pop-Location
pnpm check:generated-bindings
```

### 7. Wrong vs Correct

#### Wrong

```typescript
// Changing reviewer silently changes when approvals are requested.
persistCodexConfig({
  approvals_reviewer: "auto_review",
  approval_policy: "on-request",
});
```

#### Correct

```typescript
// Selector changes only its own field.
persistCodexConfig({ approvals_reviewer: "auto_review" });

// A separate user-invoked action changes only the policy.
persistCodexConfig({ approval_policy: "on-request" });
```

## Scenario: Project Codex Proxy Provider Identity

### 1. Scope / Trigger

Use this contract whenever CLI proxy enable/disable, raw or structured Codex
config save, OAuth-compatible proxy mode, startup repair, or sidebar repair
status can change or inspect `model_provider` / `model_providers`.

### 2. Signatures

The shared implementation boundary is
`src-tauri/src/infra/codex_config/provider_projection.rs`:

```rust
desired_provider_key_from_config(config: &[u8]) -> AppResult<CodexManagedProviderKey>
project_active_provider(config: &[u8], base_url: &str, previous_managed_base_url: Option<&str>) -> AppResult<Vec<u8>>
merge_raw_user_changes(baseline: &[u8], expected_projection: &[u8], current_live: &[u8], submitted: &[u8]) -> AppResult<Vec<u8>>
restore_managed_provider_projection(current: &[u8], baseline: &[u8]) -> AppResult<Vec<u8>>
```

### 3. Contracts

- Exact TOML boolean `[features].remote_compaction = true` selects provider
  key/name `OpenAI`; every other value or absence selects `aio`. Comments,
  quoted text, or same-named keys in other tables do not count.
- Projection owns only `model_provider` and provider fields `name`, `base_url`,
  `wire_api`, and `requires_openai_auth`. Unknown provider fields, comments,
  tables, and unrelated root keys remain user-owned.
- The pre-enable backup is the canonical user baseline. While routing is on,
  saves merge user-owned deltas into that baseline, then derive a fresh live
  projection. Never persist the projected gateway URL as the baseline.
- A single existing `aio` or `OpenAI` provider may be reused and overlaid even
  when it has a direct URL; backup ownership makes that reversible. If both
  identities exist and cannot be proven equivalent, fail before any config,
  backup, manifest, auth, or catalog write.
- Changing `remote_compaction` while routing is on must atomically switch the
  active provider identity and reproject the gateway. Routing off restores the
  original direct provider/URL and preserves user-owned edits made while active.
- Raw saves must reject edits to proxy-owned fields relative to current live
  bytes with `CODEX_PROXY_OWNED_FIELD_EDIT`; they may not bypass projection.
- Repair status uses the same provider selector and complete owned-field check
  as projection. `OpenAI` is healthy, not repairable drift, when exact remote
  compaction is enabled and the gateway projection matches.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Remote compaction true, only `aio` exists | Reconcile to one `OpenAI` projection |
| Remote compaction false/absent, only `OpenAI` exists | Reconcile to one `aio` projection |
| Equivalent source and target identities exist | Deduplicate to the selected identity |
| Conflicting `aio` and `OpenAI` identities exist | `CODEX_REMOTE_COMPACTION_PROVIDER_CONFLICT`; zero writes |
| Raw save edits an owned field | `CODEX_PROXY_OWNED_FIELD_EDIT`; preserve all bytes |
| Raw save changes remote compaction only | Update baseline and live provider coherently |
| Disable after direct-provider overlay | Restore direct URL/provider plus user-owned fields |

### 5. Good / Base / Bad Cases

- Good: direct `OpenAI` with custom headers is overlaid while active and restored
  byte-semantically on disable; custom headers survive.
- Base: routing is off and raw config changes remote compaction; the saved config
  remains canonical and is not rewritten by background proxy sync.
- Bad: rename tables with string replacement, infer the provider from display
  text, or treat current projected live bytes as the next backup.

### 6. Tests Required

- Unit-test exact boolean selection, dotted/inline tables, quoted text, comments,
  equivalent dedupe, one-sided managed fields, conflict preflight, raw three-way
  merge, owned-field rejection, and restore.
- Integration-test direct URL -> route-off toggle -> enable -> disable; assert
  the original URL and custom fields return.
- With routing on, test structured and raw user-owned edits, remote provider
  switching, current-live drift healing, explicit-history process-running
  zero-write rejection, and byte-identical provider conflicts.
- UI-test that `OpenAI` under remote compaction does not show repair and that
  provider/config/settings mutations invalidate config, raw config, and proxy
  status queries.

### 7. Wrong vs Correct

```rust
// Wrong: live output becomes canonical and provider identity is patched ad hoc.
write_backup(&current_live)?;
rename_provider_with_string_replace(&mut config, "aio", "OpenAI");

// Correct: merge only user-owned deltas into baseline, then project once.
let baseline = merge_raw_user_changes(&backup, &expected, &current_live, &submitted)?;
let live = project_active_provider(&baseline, gateway_url, previous_managed_url)?;
```

## Scenario: Scope And Bound Codex Provider History Sync

### 1. Scope / Trigger

Use this contract whenever a Codex config mutation can change the managed
provider identity, or whenever rollout, SQLite session, or global-state provider
metadata is migrated.

### 2. Signatures

```rust
cli_manager_codex_config_set(
    patch: CodexConfigPatch,
    sync_history: Option<bool>,
) -> Result<CodexConfigState, String>

CodexProviderSyncContext {
    trigger: String,
    target_provider: String,
    config_bytes: Option<Vec<u8>>,
    sync_history: bool,
}
```

Frontend services expose the transient option as
`{ syncHistory?: boolean }`. It is command input, never a persisted
`config.toml` field.

### 3. Contracts

- Enabling or disabling `remote_compaction` from the settings UI must offer
  cancel, config-only, and config-plus-history actions. Cancel performs zero
  mutation, and the dialog title/patch value must match the requested direction.
- The dialog is controlled: while either action is pending, its switch, all
  three buttons, Escape, and outside-click close are disabled. Close it only
  after a non-null successful result; on a null result or rejected promise,
  preserve the requested direction, clear pending, and allow retry.
- Config-only is the default for structured/raw saves and callers that omit the
  option. Manual Provider Sync is always full history sync.
- `sync_history = false` must branch before the Codex App process check and
  before enumerating `sessions`,
  `archived_sessions`, Codex SQLite databases, or
  `.codex-global-state.json`. Provider/config projection still commits even
  while Codex App is running; config symlink, size, TOML, and atomic-write
  safety checks still apply.
- `sync_history = true` must run the process check before acquiring or writing
  any history-sync target. Manual Provider Sync remains in this scope.
- Full history discovery retains rollout paths only. Read and rewrite JSONL one
  line at a time, atomically replace one file at a time, and keep rollback bytes
  on disk. Never retain every original rollout, rewritten rollout, or SQLite
  sidecar in memory.
- A rollback entry is `{ target_path, backup_path, existed }`. Restore existing
  files atomically from the disk backup; remove sidecars that did not exist
  before the transaction. Attempt every rollback entry before reporting a
  combined `CODEX_PROVIDER_SYNC_ROLLBACK_FAILED`.
- On Windows, the rollout reader must be dropped before `MoveFileExW` replaces
  the target. A streaming closure must own the reader rather than borrow a
  reader that remains live across finalization.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| UI enable or disable is cancelled | Zero config/history writes |
| `sync_history = false` with invalid/unreadable history | Config update succeeds without touching history |
| `sync_history = true` with invalid rollout UTF-8 | Fail before commit, or restore every started write |
| `sync_history = false`, Codex App is running | Skip process/history preflight and commit config projection |
| `sync_history = true`, Codex App is running | Preserve the existing process-closed error and zero writes |
| SQLite creates a new WAL/SHM sidecar before failure | Rollback removes the previously absent sidecar |
| Any restore entry fails | Continue other restores, then return `CODEX_PROVIDER_SYNC_ROLLBACK_FAILED` |

### 5. Good / Base / Bad Cases

- Good: a user with many gigabytes of sessions selects config-only and the
  command never opens a history directory.
- Base: manual Provider Sync migrates rollout/SQLite/global state and returns a
  managed disk-backup path.
- Bad: scan history before checking the scope, store `Vec<u8>` originals and
  rewritten bytes in every change-set entry, or keep a Windows source handle
  open during atomic replacement.

### 6. Tests Required

- UI-test all three actions for both enable and disable, including the target
  boolean, `syncHistory` option, direction-specific title, controlled close,
  pending state, and retry after null/rejected async results.
- Service/query tests must assert `syncHistory` reaches the generated command.
- Integration-test config-only against deliberately invalid rollout UTF-8 and
  a forced running-process result; assert unchanged history plus updated
  provider config without a process-closed error.
- Integration-test explicit history sync against the same running-process
  result; assert `CODEX_PROVIDER_SYNC_PROCESS_RUNNING` and zero writes.
- Test explicit full migration, multi-file/large-row streaming, CRLF
  preservation, idempotence, SQLite/global updates, and byte-exact rollback.
- On Windows, execute the real atomic replacement path; a mock rename is not
  sufficient to prove source handles are released.

### 7. Wrong vs Correct

```rust
// Wrong: scope is checked after an aggregate in-memory history scan.
let changes = collect_all_history_with_original_and_next_bytes(home)?;
if !sync_history { return write_config_only(); }

// Correct: exclude history at the boundary and retain paths only.
let history = sync_history.then(|| collect_history_paths(home)).transpose()?;
for path in history.into_iter().flatten() {
    stream_rewrite_to_atomic_temp(path)?;
}
```
