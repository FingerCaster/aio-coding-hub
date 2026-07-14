# Implementation plan: Codex approvals reviewer setting

## 1. Backend DTO And Parser

- Add `approvals_reviewer` to `CodexConfigState` and `CodexConfigPatch` in
  `src-tauri/src/infra/codex_config/types.rs`.
- Initialize and parse the root key in
  `src-tauri/src/infra/codex_config/parsing.rs`.
- Update full struct fixtures, especially `empty_patch()`.

## 2. Backend Validation And Patching

- Add strict `user | auto_review | empty` patch validation in
  `src-tauri/src/infra/codex_config/patching.rs`.
- Upsert or remove the root key through the existing root-key helper.
- Add strict raw TOML enum validation in
  `src-tauri/src/infra/codex_config/parsing.rs`.
- Confirm the field does not trigger provider synchronization.

## 3. Backend Tests

- Extend `src-tauri/src/infra/codex_config/tests.rs` for:
  - parsing `user`, `auto_review`, and an unknown string;
  - writing both supported values;
  - deleting on empty;
  - rejecting invalid structured values;
  - preserving an unknown existing value during unrelated patches;
  - raw validation for supported, empty, non-string, and unknown values.
- Extend `src-tauri/tests/codex_config_toml_raw.rs` to prove invalid reviewer
  input leaves existing file bytes unchanged.

## 4. Generated Types And Frontend Adapter

- Run `pnpm tauri:gen-types`, then
  `pnpm exec prettier --write src/generated/bindings.ts`.
- Add the null field to `DEFAULT_CODEX_CONFIG_PATCH` in
  `src/services/cli/cliManager.ts`.
- Update typed Codex state fixtures in service, query, model-migration, tab, and
  coverage tests.
- Run `pnpm check:generated-bindings` after generation.

## 5. Reviewer Presentation Model

- Add `src/components/cli-manager/tabs/codexApprovalReviewer.ts` with known-value
  detection and the agreed policy/reviewer notice matrix.
- Add focused unit tests under
  `src/components/cli-manager/tabs/__tests__/codexApprovalReviewer.test.ts` for
  every matrix branch, including unset and unknown values.

## 6. Codex Settings UI

- Add the independent selector after `approval_policy` in `CodexTab.tsx` using
  the approved Chinese copy and three normal options.
- Render a synthetic option for an unknown current value.
- Render neutral, inactive, or unsupported notices from the pure presentation
  model using existing semantic styles.
- Add the explicit `切换为 on-request` action; patch only `approval_policy`.
- Keep `auto_review` free of a confirmation modal and keep the row responsive.

## 7. Frontend Tests

- Extend `CodexTab.test.tsx` to cover:
  - approved labels/options and direct persistence of all three states;
  - no confirmation for `auto_review`;
  - unknown value display and preservation;
  - neutral unset-policy status;
  - warnings for `auto_review` with `never`, `untrusted`, and `on-failure`;
  - warning for `user + never`;
  - no warning for supported combinations;
  - switch action patches only `approval_policy` and respects saving state.
- Update other full-state fixtures required by the generated type change.

## 8. Validation

Run focused checks first:

```powershell
pnpm exec vitest run src/components/cli-manager/tabs/__tests__/codexApprovalReviewer.test.ts src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx
Push-Location src-tauri
cargo test --lib infra::codex_config::tests
cargo test --test codex_config_toml_raw
Pop-Location
pnpm check:generated-bindings
```

Then run the cross-layer quality gate:

```powershell
pnpm lint
pnpm typecheck
pnpm tauri:fmt
pnpm check:precommit:full
git diff --check
```

## Risk And Rollback Checkpoints

- Before UI work, verify backend unknown-value preservation with a unit test.
- After binding generation, inspect only the expected DTO field additions.
- Before completion, verify no test or implementation writes
  `approval_policy` as a side effect of rendering or changing reviewer values.
- Rollback is a code revert; no database or config migration is involved.
