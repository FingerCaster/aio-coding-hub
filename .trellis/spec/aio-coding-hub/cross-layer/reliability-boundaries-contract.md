# Reliability Boundary Contract

Contracts for route-draft initialization, startup recovery, diagnostic
redaction, task-complete notification, and upstream-sync review. These rules
apply when asynchronous state or untrusted diagnostic data crosses the Rust,
generated IPC, frontend service, or workflow boundary.

## 1. Scope / Trigger

Apply this contract when changing any of the following:

- Provider route-draft initialization or active sort-mode projection.
- `DbInitState`, startup retry, startup-status events, or the initial status GET.
- Console/IPC/frontend-error diagnostics or the Rust frontend-error receiver.
- Task-complete quiet-period timers or backend active-request confirmation.
- `.github/workflows/sync-upstream.yml` permissions, branch updates, or PR handling.

These paths are fail-closed reliability boundaries. They must not change
Provider enablement, gateway routing/retry semantics, or persisted formats.

## 2. Signatures

```rust
pub(crate) struct DbInitState(AsyncMutex<Option<db::Db>>);

pub(crate) async fn ensure_db_ready<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: &DbInitState,
) -> AppResult<db::Db>;

#[tauri::command]
pub(crate) fn app_startup_retry(app: tauri::AppHandle) -> AppStartupStatus;

#[tauri::command]
pub(crate) fn app_frontend_error_report(
    input: FrontendErrorReportInput,
) -> Result<bool, String>;
```

```typescript
export async function listenAndSyncAppStartupStatusSnapshot(): Promise<() => void>;
export async function retryAppStartupStatusSnapshot(): Promise<void>;

export function redactDiagnosticText(value: string, maxChars?: number): string;
export function redactDiagnosticValue(
  value: unknown,
  options?: DiagnosticRedactionOptions
): unknown;
export function redactDiagnosticJsonText(value: string, maxChars?: number): string;
export function sanitizeDiagnosticUrl(
  value: string | null | undefined,
  maxChars?: number
): string | null;

export async function activeRequestLogsSnapshot(): Promise<ActiveRequest[]>;
```

Route draft state is internal UI state:

```typescript
type ProviderUiState = {
  activeCli: CliKey;
  routeDraftInitialized: boolean;
  routeDraftSelection: RouteDraftSelection;
  // other view state omitted
};
```

## 3. Contracts

- Route draft:
  - Initialize once per active CLI after both sort-mode and active-mode queries resolve.
  - Select the persisted active mode only if that mode still exists; otherwise select default.
  - Any explicit selection sets `routeDraftInitialized = true` before late query results can apply.
- Startup:
  - `DbInitState` caches only `Ok(Db)`. An initialization error leaves the cache empty.
  - The async mutex covers the initialization attempt so concurrent successful callers initialize once.
  - Frontend startup bootstrap registers the event listener before starting the status GET.
  - Subscription identity plus update generation must reject an unmounted subscription or stale GET/retry result.
- Diagnostics:
  - Sensitive keys, authorization values, bearer tokens, secret assignments, and key-like tokens become `[REDACTED]`.
  - URLs retain only scheme/host/port/path; username, password, query, and fragment are removed.
  - Object traversal is bounded by depth, array items, object keys, nodes, per-string characters, and aggregate string characters.
  - Circular values and unreadable values become fixed markers. Redaction failure never falls back to the original value.
  - Frontend boundaries redact first; the Rust `app_frontend_error_report` receiver redacts again before tracing.
- Notification:
  - Every request start/complete increments the CLI session generation.
  - A quiet-period callback carries the generation captured when scheduled.
  - Before notifying, confirm the generation and frontend in-flight set, then query the backend active registry.
  - A same-CLI active row or snapshot error suppresses the notification. The default quiet period remains 30 seconds for every CLI.
- Upstream sync:
  - Workflow top-level permissions are `contents: read` and `pull-requests: write`.
  - Already-synchronized branches are a no-op. Fast-forward and diverged branches create or update a PR.
  - The workflow never pushes the target branch, locally merges upstream, or auto-merges a PR.
  - `DIRTY`, `UNKNOWN`, empty/unavailable merge state fails closed; review-blocked PRs remain open for manual review.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Persisted route mode exists | Initialize the draft to that mode exactly once |
| Persisted route mode was deleted | Initialize the draft to default |
| User selects while queries settle | Preserve the explicit selection |
| DB initialization returns `Err` | Return the error and leave `DbInitState` empty |
| Older startup GET/retry resolves late | Ignore the result |
| Startup listener registration fails | Reject setup; do not start the initial GET |
| Diagnostic key/text is sensitive | Emit `[REDACTED]`, never the original value |
| Diagnostic getter/proxy traversal fails | Emit `[REDACTION_FAILED]` or fail the whole projection closed |
| Diagnostic resource budget is exhausted | Emit a bounded truncation marker |
| Backend still has a same-CLI request | Do not notify |
| Backend active snapshot fails | Log a redacted warning and do not notify |
| Notification generation changes | Old timer/check returns without notifying |
| Upstream is already contained in origin | No-op |
| Upstream can fast-forward origin | Open/update PR; do not push |
| Upstream and origin diverged | Open/update PR; do not merge |
| Sync PR state is dirty/unknown/unavailable | Fail the workflow and require manual resolution |

## 5. Good / Base / Bad Cases

- Good: a transient DB failure reports a retryable startup failure; the next retry re-enters initialization, succeeds, and all later callers reuse the successful handle.
- Base: a normal startup listener is registered, its initial GET commits once, and later events replace the snapshot.
- Bad: cache a rejected initialization result or let a late initial GET overwrite a newer `ready` event.
- Good: diagnostic objects preserve safe categorical metadata while removing credentials and staying within budgets.
- Base: ordinary non-sensitive text remains readable at console/error-report boundaries.
- Bad: catch a redaction exception and call `String(originalValue)` because that can re-expose the secret.
- Good: a quiet-period callback confirms an empty same-CLI backend registry before notifying.
- Bad: notify solely because the frontend missed a request-start event.
- Good: a scheduled sync opens a PR even when a direct fast-forward is possible.
- Bad: grant `contents: write`, run `git push`, or call `gh pr merge` from the workflow.

## 6. Tests Required

- Route draft tests assert persisted-mode selection, deleted-mode fallback, explicit-selection race protection, and per-CLI reset.
- Rust startup tests assert failure then success performs two attempts, success is cached, and concurrent success initializes once.
- Frontend startup tests assert listener-before-GET ordering, event-before-GET protection, cleanup invalidation, subscription replacement, retry staleness, and GET failure logging.
- Diagnostic tests assert free-text secrets, structured sensitive keys, URL credentials/query/fragment, cycles, throwing getters, depth/items/keys/nodes/string budgets, IPC argument/error paths, frontend report payloads, and Rust defense in depth.
- Notification tests assert normal 30-second delivery, overlapping requests, same-CLI backend activity, snapshot failure, generation invalidation, disabled state, and Codex default timing.
- Sync policy self-tests mutate permissions and commands to prove direct push, local merge, auto-merge, missing PR creation, missing fail-closed states, and topology bypasses are rejected.
- Run focused tests plus `pnpm lint`, `pnpm typecheck`, `pnpm tauri:fmt`, `pnpm check:generated-bindings`, Rust library tests, and Clippy.

## 7. Wrong vs Correct

### Wrong

```typescript
const status = await appStartupStatusGet();
const unlisten = await listenAppStartupStatusEvents(setAppStartupStatusSnapshot);
setAppStartupStatusSnapshot(status);
```

This has an event-loss window and lets an older GET overwrite a newer event.

### Correct

```typescript
const unlisten = await listenAppStartupStatusEvents(setAppStartupStatusSnapshot);
const generation = statusUpdateGeneration;
const status = await appStartupStatusGet();
if (subscriptionIsCurrent() && statusUpdateGeneration === generation) {
  commitAppStartupStatusSnapshot(status);
}
```

### Wrong

```yaml
permissions:
  contents: write

# Fast-forward path
- run: git push origin HEAD:main
```

### Correct

```yaml
permissions:
  contents: read
  pull-requests: write

# Fast-forward and diverged paths both create/update a PR for manual review.
```
