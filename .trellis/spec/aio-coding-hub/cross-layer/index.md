# AIO Coding Hub Cross-Layer Specs

Rules for contracts that cross the root application's Rust backend, generated
TypeScript bindings, frontend adapters, and React UI.

## Topics

- [Codex config contract](./codex-config-contract.md): typed config fields,
  patch semantics, raw TOML validation, generated bindings, UI behavior, and
  chain-managed route transactions.
- [Codex retry gateway contract](./codex-retry-gateway-contract.md): external
  process ownership, route recovery, Provider Sync integrity, bridge sessions,
  and generated lifecycle results.

## Pre-Development Checklist

When changing a Codex `config.toml` field:

1. Read [Codex config contract](./codex-config-contract.md).
2. Trace both read and write paths through Rust, generated bindings, the
   frontend adapter, and the consuming UI.
3. Decide separately how structured patches and full raw TOML saves handle
   unset, invalid, and future values.
4. Search for every complete `CodexConfigState` fixture before regenerating
   bindings.

When changing the external Codex retry gateway:

1. Read [Codex retry gateway contract](./codex-retry-gateway-contract.md).
2. Trace the shared lifecycle lock and startup reconciliation order.
3. Verify route and Provider Sync rollback preflight every snapshot before any
   target mutation.
4. Trace Rust DTOs through generated bindings, service/query adapters, iframe
   lifecycle, and browser entry independently.

## Quality Check

- Regenerate and verify `src/generated/bindings.ts` from Rust source.
- Test Rust parsing, structured patching, and full-file write safety.
- Test frontend adapter defaults and the UI's null/unknown-value behavior.
- Verify unrelated patches preserve fields that they do not own.
- With a managed Codex route, verify health failure restores config, Provider
  Sync data, route metadata, runtime projection, and backup directory shape.
- Verify details-view cleanup revokes only iframe access and never disables or
  stops the external gateway.
- Run focused tests, `pnpm typecheck`, `pnpm lint`, `pnpm tauri:fmt`, and
  `pnpm check:generated-bindings`.
