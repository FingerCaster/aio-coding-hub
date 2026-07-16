# Codex Retry Gateway Contract

## Scenario: Manage the External Codex Retry Gateway

### 1. Scope / Trigger

Use this contract when changing the managed external gateway lifecycle, Codex
route projection, Provider Sync integration, management bridge, process
identity, or their generated frontend boundary. The protected topology is:

```text
host Codex -> external gateway -> AIO gateway -> provider
```

AIO owns source trust, Node/process ownership, route transactions, recovery,
and bridge access. The external repository owns interception behavior and its
management page.

### 2. Signatures

The command and generated TypeScript boundary includes:

```rust
pub struct CodexRetryGatewaySetEnabledResult {
    pub status: CodexRetryGatewayStatus,
    pub provider_sync: Option<CodexProviderSyncResult>,
}

pub struct CodexRetryGatewayDetailsSession {
    pub generation: u64,
    pub iframe_url: String,
    pub browser_url: String,
    pub iframe_view_id: String,
    pub expires_at_ms: u64,
}

#[serde(rename_all = "camelCase")]
pub struct CodexRetryGatewayRevokeDetailsSessionRequest {
    pub view_id: String,
}

pub struct CodexRouteReconcileResult {
    pub route: CodexRouteVerifyResult,
    pub pending_transition_reconciled: Option<bool>,
    pub recovery_warning: Option<String>,
}
```

`codex_retry_gateway_revoke_details_session` returns Rust `()` and therefore a
successful Tauri payload of `null`. The frontend adapter must explicitly accept
that `null`; it is not a missing-result error.

### 3. Contracts

- Every chain-affecting operation uses `gateway_lifecycle_lock`. Enable,
  disable, CLI-proxy changes, external restore, update, startup recovery, and
  provider-mode changes must not use an independent lifecycle lock.
- Starting and health-checking the external process does not commit enablement.
  The enable transaction captures the complete prior settings and manager
  state before runtime mutation. If guarded-route activation fails, it stops
  the candidate process and restores desired state, commit metadata, effective
  port, process record, and recovery metadata while advancing generation
  monotonically.
- Startup order is fixed: reconcile the persisted Codex route, take over any
  pending managed-process launch, then reconcile desired runtime state. A
  quarantined route journal becomes a public warning without skipping the
  remaining startup steps.
- Enable returns the exact `CodexProviderSyncResult` produced by guarded route
  activation. The frontend shows its real counts and backup/warning state;
  it must not replace this with a generic submitted/success message.
- Public `cli_proxy_applied` and guarded status come from `verify_route`,
  including live TOML, auth, AIO origin, guarded origin, and desired state.
  Commented text or a manifest alone cannot prove the route is applied.
- Route rollback reads and validates every snapshot before changing any target.
  Corrupt or oversized journals/snapshots are moved under the owned quarantine
  root and retained for diagnosis; they must not permanently block direct-AIO
  fail-open.
- Corrupt Provider Sync journals or snapshots follow the same fail-open rule.
  Move the complete transaction under the owned Provider Sync quarantine root,
  clear its stale lock, surface a recovery warning, and continue the pending
  route journal. Never partially restore Provider Sync targets before deciding
  to quarantine.
- Provider Sync snapshots are bounded before persistence and recovery: at most
  4096 entries, 128 MiB per file, and 256 MiB aggregate. Schema 2 records byte
  length and SHA-256; recovery preflights every backup before the first target
  write. Only parsed `session_meta` JSONL rows are rewritten; unknown, malformed,
  and non-session rows plus their original CRLF/LF endings remain unchanged.
- Bridge launch tokens are one-time and expire after 60 seconds. Iframe and
  system-browser entry use independent tokens and view identities. Refresh or
  leaving the AIO details view revokes only its iframe view; this must not stop
  the gateway or revoke an already-open browser session.
- Bridge API requests require a verified same-origin browser context. Status
  overlays may buffer up to 8 MiB; other allowlisted responses stream through a
  cumulative 8 MiB bound. Every request revalidates the managed generation and
  process identity.
- Automatic Node discovery may canonicalize a package-manager symlink to a
  concrete executable. A manual override rejects symlink/reparse input before
  probing and never replaces a prior valid override on failure.
- Node selection is not a hot runtime switch. Reject manual/automatic Node
  changes while external desired state is enabled. A successful disabled-state
  change advances manager generation so old enable plans and late status
  responses become stale before the next launch.
- Official gateway health and status must agree on the same non-zero top-level
  `process_id`; any nested state PID, when present, must also agree. Spawned
  children are asynchronously reaped so an exited PID cannot retain a stale
  Windows process identity.
- ZIP extraction enforces its single-file and aggregate byte budgets against
  bytes actually read and written, not only entry-declared uncompressed sizes.
  Archive paths reject non-portable Windows alternate-data-stream segments.
- Release jobs build locally first, unpack each installer and portable artifact
  with platform-native tools, and reject external gateway source paths, its
  entrypoint/dependency tree, or a bundled Node runtime before the first
  workflow-artifact or GitHub Release upload.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Enable plan generation changed | Reject before route/process mutation |
| Guarded config, auth, or origin drift | `cli_proxy_applied=false`; never report guarded protection |
| Corrupt route journal | Quarantine it, surface `TRANSITION_CORRUPT`, continue safe reconciliation |
| Any rollback snapshot is missing, oversized, or hash-invalid | Fail before writing any target |
| Provider Sync recovery material is corrupt during route recovery | Quarantine it, warn, and continue the route journal |
| Bridge view ID is not exactly 32 hex characters | `CODEX_RETRY_GATEWAY_BRIDGE_SESSION_INVALID` |
| Revoke succeeds with Tauri `null` | Resolve successfully in service/query layers |
| Launch token is missing, expired, or reused | HTTP 410; do not create a cookie session |
| API request is cross-site or a mutation lacks exact origin | HTTP 403 |
| Health/status process IDs differ | Treat the process as unowned/unhealthy |
| Manual Node override is a link, directory, relative path, or Node < 18 | Reject and preserve prior selection |
| Node selection changes while desired state is enabled | Reject without changing settings, process, or generation |

### 5. Good / Base / Bad Cases

- Good: enabling changes `OpenAI -> aio`; the route commits only after Provider
  Sync succeeds, and the UI reports the actual session/SQLite/workspace counts.
- Base: entering details creates an iframe view; leaving details revokes that
  view while desired state, process state, and an open browser view remain.
- Good: startup finds a malformed route journal, quarantines it, records a
  warning, then still reconciles pending launch and desired-on runtime state.
- Bad: restore the first route or Provider Sync snapshot before discovering
  that a later backup is missing. This creates an unrecoverable mixed state.
- Bad: infer an applied route from commented TOML text, the manifest, or a
  healthy listener without verifying auth and the effective origin.
- Bad: reuse one bridge launch URL for iframe and browser, or revoke the browser
  session when the AIO frame unmounts.

### 6. Tests Required

- Rust lifecycle tests assert route -> pending launch -> desired runtime order,
  enable result propagation, complete runtime metadata rollback after guarded
  route failure, monotonic rollback generation, and direct-route-before-stop
  ordering.
- CLI-proxy and Provider Sync tests corrupt the last snapshot and assert every
  target remains byte-for-byte unchanged; cover count, size, hash, duplicate,
  and quarantine bounds.
- A cross-transaction test keeps both Provider Sync and route journals pending,
  corrupts Provider Sync recovery material, and proves quarantine does not
  prevent verified route recovery or its public warning.
- Provider Sync JSONL tests cover provider names that grow/shrink, CRLF, unknown
  rows, malformed rows, and a final line without a newline.
- Bridge tests cover single-use launch tokens, origin/fetch-site/referrer rules,
  view revocation, awaited restore, stream limits, stale generation, and
  process disappearance.
- Frontend service/query/component tests cover generated DTOs, successful
  `null` revocation, stale async session cleanup, independent browser entry,
  unmount revocation, and Provider Sync result presentation.
- Process tests require matching health/status PIDs and prove exited children
  are reaped. Run generated bindings, full Rust, precommit, prepush, build, and
  installer-content gates before release.
- Node tests cover enabled-state rejection, disabled-state generation advance,
  stale plan invalidation, and UI controls that explain the disable-first rule.
  The installer-content gate has a self-test and workflow-order contract that
  prevents any upload-capable build step from preceding it.

### 7. Wrong vs Correct

#### Wrong

```rust
for snapshot in journal.snapshots.iter().rev() {
    let bytes = read_snapshot(snapshot)?;
    write_target(snapshot, &bytes)?;
}
```

If a later snapshot is corrupt, earlier targets have already changed.

#### Correct

```rust
let prepared = preflight_all_snapshots(journal)?;
for snapshot in prepared.into_iter().rev() {
    restore_prevalidated_snapshot(snapshot)?;
}
```

Read, bound, and authenticate the complete recovery set before the first
mutation. Apply the same rule to route and Provider Sync rollback stores.
