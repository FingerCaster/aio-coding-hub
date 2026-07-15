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

## Scenario: Mutate Config While the Codex Route Is Chain-Managed

### 1. Scope / Trigger

Use this contract when structured patches or raw TOML writes run while the
Codex CLI proxy manifest is enabled or its route is not `Unproxied`. It is
mandatory for `features.remote_compaction`, `model_provider`, managed provider
tables, or any write that must preserve a live external-gateway projection.

### 2. Signatures

```rust
async fn run_codex_config_stage<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    label: &'static str,
    operation: impl FnOnce() -> AppResult<CodexConfigMutationStage> + Send + 'static,
) -> Result<CodexConfigState, String>;

struct CodexConfigMutationTransaction;
impl CodexConfigMutationTransaction {
    fn verification_record(&self) -> Option<&CodexRetryGatewayManagedProcessRecord>;
    fn commit(self);
    fn rollback(self) -> AppResult<()>;
}

fn codex_provider_sync_transaction_reversible(...) -> AppResult<(
    CodexProviderSyncResult,
    T,
    Option<CodexProviderSyncRollback>,
)>;
```

The Tauri structured and raw commands use `run_codex_config_stage`. Direct
synchronous helpers may commit only when no asynchronous managed-process health
verification is required.

### 3. Contracts

- Acquire `gateway_lifecycle_lock` before the blocking mutation stage and hold
  it through managed health verification and the blocking commit or rollback.
- Read and patch the canonical unproxied bytes, then project the live bytes for
  the current `Unproxied | DirectAio | Guarded` route. Never learn an external
  listener as canonical state.
- Derive managed provider identity from the projected config:
  `remote_compaction=true -> OpenAI`; otherwise `aio`.
- Before the first write, snapshot every owned surface that may change: live
  config, Provider Sync session/SQLite/global files, CLI manifest, canonical
  backup, gateway manager state, runtime state, and Provider Sync backup-root
  shape.
- If an owned external process exists, update both runtime state and its manager
  record, then verify full ownership and health including `provider_name`
  before commit.
- Reversible Provider Sync must defer backup pruning until commit. Rollback
  removes the new uncommitted managed backup and removes its root only when this
  transaction created that root and it is still empty.
- Emit the public gateway status only after commit. A failed verification must
  not expose the staged generation.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| No managed route | Preserve the ordinary validated atomic config write |
| Provider identity changes while Codex is running | Fail before mutation with the existing Provider Sync process error |
| Managed process record changes before verification | `CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED` and full rollback |
| Health does not report the staged provider | `CODEX_RETRY_GATEWAY_PROVIDER_VERIFY_FAILED` and full rollback |
| Any rollback surface cannot be restored | Return `CODEX_CONFIG_MANAGED_ROLLBACK_FAILED` with the original cause |
| Synchronous caller reaches an async verification requirement | Roll back and return `CODEX_CONFIG_MANAGED_ASYNC_VERIFY_REQUIRED` |

### 5. Good / Base / Bad Cases

- Good: toggle remote compaction while guarded; rename `aio -> OpenAI`, keep the
  external listener route, synchronize provider/session state, verify external
  status, then commit one coherent generation.
- Base: edit an unrelated field while direct AIO with no owned external process;
  update canonical/live bytes and manifest atomically without an HTTP probe.
- Bad: write live config and prune Provider Sync backups before health
  verification; a later provider mismatch cannot restore the prior filesystem.

### 6. Tests Required

- Unit-test rollback token commit, explicit rollback, drop rollback, managed
  backup removal, and five-backup pruning.
- Integration-test guarded and direct-AIO remote-compaction toggles, asserting
  provider name, route mode, effective origin, manager record, and runtime state.
- Inject a stale health provider and assert byte-for-byte restoration of live
  config, rollout/session data, CLI manifest, canonical backup, manager state,
  runtime state, plus identical backup-root existence and directory entries.
- Assert structured writes, raw writes, and manual Provider Sync wait for the
  shared lifecycle lock.

### 7. Wrong vs Correct

#### Wrong

```rust
write_live_config(next)?;
provider_sync(next_provider)?; // commits and prunes before external health check
verify_external_provider(next_provider).await?;
```

#### Correct

```rust
let mut staged = blocking::run(label, operation).await?;
let transaction = staged.transaction.take().expect("managed transaction");
if let Some(record) = transaction.verification_record() {
    if let Err(error) = verify_managed_provider_projection(&paths, record).await {
        return rollback_failed_config_stage(transaction, error).await;
    }
}
commit_config_stage(transaction).await?;
```
