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
  one TanStack Query owner for automatic, timed, and forced manual refreshes,
  plus the bounded, same-origin NewAPI model-token billing protocol.

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
2. Decide whether the change affects query ownership, the remote adapter
   protocol, or both; apply every relevant scenario in that contract.
3. For query changes, trace automatic, timed, and manual entry points through
   the same query key, options, cache owner, and component state.
4. Test uncancellable IPC Promises with deliberately reversed completion order.
5. For NewAPI changes, trace Base URL normalization, same-origin endpoints,
   redirect policy, authentication headers, bounded bodies, application-error
   ordering, field/unit validation, normalization, IPC, and display together.
6. Confirm account usage remains display-only and that fixtures/specs contain
   no upstream body/message, credential, PII, live host, token name, or actual
   account amount.

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
- When changing the NewAPI account-usage adapter, verify the public status plus
  two Bearer billing requests, trailing `/v1` normalization, same-origin and
  no-redirect rules, exact unit/formula/expiry parsing, per-response body caps,
  application-error precedence, all-or-nothing failure, and sub2api stability.
- Audit account-usage diffs for credential, PII, host, upstream-message/body,
  token-name, and actual-account-value leakage, and verify routing, circuit,
  availability, order, and enablement remain untouched.
