# Codex approvals reviewer setting

## Goal

Add first-class AIO support for Codex `approvals_reviewer` so users can choose
manual review (`user`) or App-style "Approve for me" automatic review
(`auto_review`) without editing raw `config.toml`, while preserving the real
sandbox and approval-policy boundaries.

## Background

- Codex uses independent controls: `approval_policy` decides when approval is
  needed, while `approvals_reviewer` decides who reviews eligible requests.
- App "Approve for me" / Auto-review maps to
  `approval_policy = "on-request"` plus
  `approvals_reviewer = "auto_review"`. It is not
  `approval_policy = "never"`; `never` produces no approval request to review.
- Current official Auto-review documentation explicitly supports interactive
  `on-request` or granular approval policies. AIO currently edits only string
  approval policies and does not support granular policy objects.
- AIO currently has no `approvals_reviewer` or `auto_review` runtime/config/UI
  implementation. The Rust state and patch DTOs lack the field
  (`src-tauri/src/infra/codex_config/types.rs:6`,
  `src-tauri/src/infra/codex_config/types.rs:52`), parsing ignores it
  (`src-tauri/src/infra/codex_config/parsing.rs:314`), patching has no
  validation/upsert path (`src-tauri/src/infra/codex_config/patching.rs:628`),
  generated bindings and frontend patch defaults lack it
  (`src/generated/bindings.ts:2419`, `src/services/cli/cliManager.ts:48`), and
  the UI only exposes `approval_policy`
  (`src/components/cli-manager/tabs/CodexTab.tsx:1595`).
- The raw TOML editor currently preserves otherwise unknown valid root keys, so
  users can manually add `approvals_reviewer = "auto_review"`; the structured
  UI cannot display, validate, or update it.
- AIO has no per-setting Codex CLI version gate. Recent config fields are
  exposed directly; detected CLI versions are used for display and update
  checks rather than capability gating.

## Requirements

### R1: Config Contract

- Add `approvals_reviewer` to `CodexConfigState` and `CodexConfigPatch` and
  round-trip it as a root `config.toml` key.
- Structured writes accept `user`, `auto_review`, or an empty string that
  deletes the key. No write occurs for an omitted patch field.
- When the key is present in raw TOML, validation strictly accepts only the
  documented `user` and `auto_review` strings; empty strings, non-strings, and
  unknown strings fail before a file write. In the structured patch API only,
  an empty string is the established delete-key control signal and is accepted.
- An externally written unknown reviewer string remains readable and is
  preserved by unrelated structured patches. AIO replaces or removes it only
  after the user explicitly changes the reviewer selector.
- Preserve unrelated keys, comments, tables, and current formatting behavior.
  Reviewer changes must not trigger provider sync or alter proxy state.

### R2: Selector And Copy

- Add an independent selector immediately beside the existing approval-policy
  control; do not replace the current controls with combined permission
  presets.
- Use label `审批者 (approvals_reviewer)` and subtitle
  `决定符合条件的审批请求由你处理，还是交给独立 reviewer 评估；不会扩大沙箱权限。`
- Expose exactly these normal choices:
  `默认（不设置）`, `由我审批（user）`, and
  `替我审批（auto_review）`.
- Unset deletes the key and preserves effective defaults/config layering;
  explicit `user` remains available as an override.
- Selecting `auto_review` persists directly without a confirmation modal. The
  existing confirmation remains specific to `danger-full-access`.
- If a structured read returns an unknown string, render a synthetic
  `不支持的当前值（<value>）` option and warning. Never display it as unset or
  silently clean it up.

### R3: Soft Linkage

- The two selectors remain independent. Changing one never silently rewrites or
  clears the other.
- With reviewer `auto_review`: explicit `on-request` is shown as active;
  explicit `never`, `untrusted`, and legacy `on-failure` show an
  inactive/unsupported warning and a `切换为 on-request` action; unset policy
  uses neutral copy because its effective inherited value is unknown.
- With reviewer `user`: explicit `never` shows that no user approval can occur
  and offers the same switch action. `on-request`, `untrusted`, and
  `on-failure` have no reviewer warning. An unset reviewer has no
  reviewer-specific warning.
- The switch action changes only `approval_policy` to `on-request`, and only
  after the user invokes it.
- Auto-review copy must describe reviewer evaluation, not promise unconditional
  approval, and must not imply broader filesystem or network access.

### R4: Compatibility And Scope Control

- Regenerate Specta TypeScript bindings and update the frontend patch-default
  adapter and typed fixtures.
- Follow the current AIO pattern without introducing a new Codex-version
  capability-gating framework.
- Preserve custom Codex Home behavior, raw editor behavior outside the new enum
  validation, WSL/full-file sync behavior, and CLI proxy backup/restore
  behavior.

## Acceptance Criteria

- **AC1 (R1):** Reading `user`, `auto_review`, or an unknown reviewer string
  returns the exact string through the structured API.
- **AC2 (R1):** Saving either supported value writes exactly one root key;
  choosing unset removes it; unrelated config remains unchanged.
- **AC3 (R1):** Structured patches reject unsupported non-empty reviewer
  strings, while an empty structured value removes the key. Raw full-file saves
  reject empty/non-string/unknown reviewer values without changing the existing
  file.
- **AC4 (R1, R2):** An unknown existing string is displayed verbatim as
  unsupported, survives unrelated patches, and changes only after an explicit
  reviewer selection.
- **AC5 (R2):** The selector renders the approved label, subtitle, and three
  normal options, persists each state, and opens no confirmation modal for
  `auto_review`.
- **AC6 (R3):** `auto_review + on-request` has no warning;
  `auto_review + never/untrusted/on-failure` warns and offers the switch action;
  `auto_review` with unset policy is neutral.
- **AC7 (R3):** `user + never` warns and offers the switch action; `user` with
  other current string policies and an unset reviewer do not show that warning.
- **AC8 (R3):** The switch action persists only
  `{ approval_policy: "on-request" }`; passive rendering never changes config.
- **AC9 (R4):** Generated bindings are current, focused Rust/React tests pass,
  and relevant lint, typecheck, formatting, and pre-commit checks pass.

## Out Of Scope

- Implementing the Codex reviewer agent or changing its reviewer policy.
- Editing `[auto_review]` policy instructions, `guardian_policy_config`, or
  managed requirements.
- Adding granular approval-policy object editing.
- Replacing the existing approval and sandbox controls with App-style combined
  presets.
- Automatically enabling full access or treating `never` as automatic review.
- Removing the legacy `on-failure` option.
- Introducing per-field Codex CLI version gating.

## Technical Notes

- The implementation spans Rust DTO/parsing/patching/validation, generated
  Specta bindings, the frontend IPC patch adapter, Codex settings UI, and focused
  tests.
- This task was initially drafted manually while the project-local Trellis
  runtime was absent. After restoring Trellis 0.6.6, the existing directory was
  registered through `task.py create --no-start`; the converged planning
  artifacts were preserved unchanged and the task remains in `planning`.
