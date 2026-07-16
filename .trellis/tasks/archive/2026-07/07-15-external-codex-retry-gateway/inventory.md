# Removal inventory

## Removed managed integration

- `src-tauri/src/infra/codex_retry_gateway/**`
- `src-tauri/src/app/codex_retry_gateway_service.rs`
- `src-tauri/src/commands/codex_retry_gateway.rs`
- managed Codex route coordination and startup reconciliation
- external gateway settings and generated IPC commands
- manager/details UI, query, service, hook, event, fixtures, and MSW handlers
- iframe CSP allowance and installer inventory gate dedicated to bundled
  external source/runtime checks

## Retained

- repository recommendation card and fixed official URL
- generic AIO gateway retry/failover/logging
- ordinary CLI proxy and Provider Sync
- Codex `approvals_reviewer`
- `codex-auto-review*` route-neutral presentation
- schema 49 legacy guard-removal marker

## Forbidden active-source markers

- managed `codex_retry_gateway` Rust/IPC/settings identifiers
- gateway desired/runtime/guarded route state
- launch tokens, management bridge sessions, source commit selection, Node
  override, update, retry, uninstall, and embedded details behavior
- local `CodexReasoningGuard` runtime/config/statistics types
