# Implementation Plan

## 1. Settings And Runtime Mapping

- [x] Add typed effort mapping, defaults, bounds and schema-63 migration.
- [x] Thread the field through settings update/patch/view ownership and persistence normalization.
- [x] Load the mapping into `Cx2ccSettings` and apply it only to explicit IR effort variants.
- [x] Add settings migration, validation, bridge matrix and config round-trip tests.

## 2. Local Gateway Delegation

- [x] Detect typed CX2CC current-gateway delegation without using hostname alone.
- [x] Clamp outer Provider attempt budgets to one.
- [x] Disable only the authorized outer first-byte timeout; preserve abort cancellation and all inner timeouts.
- [x] Add focused budget/timeout/replay/ordinary-self-loop tests and, where practical, a route-level regression.

## 3. Frontend And Bindings

- [x] Regenerate bindings for the new setting type and field.
- [x] Extend settings adapters and validation with exhaustive field ownership checks.
- [x] Build the CX2CC effort mapping editor with add/edit/delete/restore-default behavior.
- [x] Add component and service tests, including save failure rollback and invalid rows.

## 4. Verification

- [x] Run targeted Rust settings/protocol/reentry/route tests.
- [x] Run targeted frontend settings/CX2CC tests, typecheck and lint.
- [x] Run generated-binding verification, Rust fmt/check/Clippy and full Rust library suite.
- [x] Dispatch independent `trellis-check`, resolve findings, and rerun affected gates.

## 5. Delivery

- [x] Update the CX2CC routing contract with configurable mapping and delegated-timeout ownership.
- [x] Commit with hooks able to resolve `node` and `pnpm`.
- [x] Push the feature branch and create an explicit origin PR for `FingerCaster/aio-coding-hub`.

## Rollback Points

- Settings/schema and mapping behavior form one atomic commit boundary.
- Local reentry timeout/attempt ownership is independently revertible but must retain existing nonce security tests.
- UI must not ship before generated binding and backend validation are present.
