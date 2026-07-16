# AIO Coding Hub Cross-Layer Specs

Rules for contracts that cross the root application's Rust backend, generated
TypeScript bindings, frontend adapters, and React UI.

## Topics

- [Codex config contract](./codex-config-contract.md): typed config fields,
  patch semantics, raw TOML validation, generated bindings, and UI behavior.
- [Gateway failover route contract](./gateway-failover-route-contract.md):
  common provider-gate ownership, Ready-provider limits, persisted attempts,
  route hops, and UI count semantics.
- [Provider account-usage query contract](./provider-account-usage-query-contract.md):
  one TanStack Query owner for automatic, timed, and forced manual refreshes.

## Pre-Development Checklist

When changing a Codex `config.toml` field:

1. Read [Codex config contract](./codex-config-contract.md).
2. Trace both read and write paths through Rust, generated bindings, the
   frontend adapter, and the consuming UI.
3. Decide separately how structured patches and full raw TOML saves handle
   unset, invalid, and future values.
4. Search for every complete `CodexConfigState` fixture before regenerating
   bindings.

When changing provider account-usage fetching:

1. Read [Provider account-usage query contract](./provider-account-usage-query-contract.md).
2. Trace automatic, timed, and manual entry points through the same query key,
   options, cache owner, and component state.
3. Test uncancellable IPC Promises with deliberately reversed completion order.

## Quality Check

- Regenerate and verify `src/generated/bindings.ts` from Rust source.
- Test Rust parsing, structured patching, and full-file write safety.
- Test frontend adapter defaults and the UI's null/unknown-value behavior.
- Verify unrelated patches preserve fields that they do not own.
- Run focused tests, `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`, and
  `pnpm check:generated-bindings`.
- When changing gateway selection or failover, verify skipped candidates,
  Ready-provider limits, route projection, and attempt/transition labels together.
- When changing account-usage refresh, verify forced fetches, late-result
  suppression, loading/error state, and provider/cache isolation together.
