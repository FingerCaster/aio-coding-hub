# Design: External gateway frontend

## Foundation Contract

Consume generated status, enable-plan, trust/update/Node DTOs and narrow command
signatures from the foundation commit. Do not locally redefine backend payloads
or edit `src/generated/bindings.ts`. React Query/service adapters own decoding
and generation ordering; components receive presentation state.

## State Flow

```text
generated commands + status event
  -> service adapter
  -> React Query cache keyed by feature status
  -> generation-aware projection
  -> compact Codex settings section / details route / dialogs
```

One pure presentation module maps runtime phase, route mode, trust, Node,
update, and WSL fields to labels/actions. Event data with a lower generation
than cached state is ignored.

## Enable Flow

1. Request a read-only enable plan.
2. Render one confirmation with conditional rows for download, trust,
   CLI-proxy enable, Provider Sync, and WSL.
3. Provider Sync row shows exact current/target provider, session/provider-state
   synchronization, backup, and Codex-closed requirement.
4. Cancel closes only. Confirm sends plan generation plus explicit accepted
   flags; backend errors refresh status and reuse existing safe guidance.

Do not stack several modal dialogs for one enable operation.

## Main Codex Surface

Replace the old guard panel with a compact unframed settings section containing
switch, actual route/phase, commit trust, port, Node, WSL warning, details,
update check, retry, and data-removal actions. Use existing button, switch,
select, dialog, tooltip, and semantic notice primitives.

Keep display text concise and operational. Stable widths/wrapping prevent SHA,
path, status, and warning content from overlapping at desktop/mobile test
viewports.

## Details Route

Add a lazy application route. AIO-owned controls remain outside the iframe.
Request a new bridge session when entering/refreshing; never construct a raw
external URL. Sandbox allows the minimum scripts/same-origin/forms/modals/
downloads contract and no Tauri IPC/top navigation/unrestricted popup.

Route exit only disposes frontend session/view state. External lifecycle calls
occur only from explicit buttons or the intercepted external restore response.

## Legacy Frontend Removal

This worker owns all `src/**` removal listed in parent `inventory.md`, including
CodexTab controls, settings adapters/fixtures, request-log statistics and
presentation. It keeps generic special-settings parsing and route/system/
service-tier projections that are not local-guard-owned.

## Owned and Forbidden Paths

Allowed: `src/**` except `src/generated/bindings.ts`, frontend tests/fixtures,
and the narrow CSP value in `src-tauri/tauri.conf.json`.

Forbidden: all other `src-tauri/src/**`, Rust tests, generated bindings, scripts/
docs/spec/AGENTS removal, parent/other child tasks, main, remote operations,
push, and release. Backend contract defects are reported to the coordinator.

## Integration Handoff

Return commit SHA, modified path audit, focused Vitest/typecheck results, mock
contract assumptions, screenshot/layout observations if performed, and any
binding/glue changes required after backend merges.
