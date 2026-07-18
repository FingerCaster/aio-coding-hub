# Round 7 Implementation Plan

## Preconditions

- [ ] Confirm the branch is still based on `35db0f32` plus task-planning-only changes and preserve all unrelated
      dirty files listed in the parent PRD.
- [ ] Confirm no Claude, Pi, or other review terminal is running; future review ownership is Sol max only.
- [ ] Launch exactly one fresh Orca Codex `gpt-5.6-sol / effort=max` execution terminal with a closed execution
      prompt: follow this checklist only; do not explore unrelated code, hunt additional findings, refactor,
      compare alternate architectures, or expand scope. Stop and report if a checklist item cannot be completed
      without a scope decision.
- [ ] Read the curated config-migration, Image Gen, settings rollback, task archive, and cross-layer thinking specs.

## Ordered Work

1. [ ] Add Image Gen controller regressions for a successful retry of persisted done and error rows, a failed retry,
       reference rehydration, and an in-flight first persistence race. Make the tests assert distinct IDs and no
       overwrite/delete of the source row.
2. [ ] Implement fresh immutable retry attempts in the frontend, correct upsert language, and update the Image Gen
       trust-boundary contract to match the new attempt lifecycle.
3. [ ] Add Rust config-export tests for aggregate installed/local payload and file-count exhaustion, including a
       sentinel target file and a legal 1-8 MiB round trip.
4. [ ] Implement the shared export aggregate budget before Base64 allocation, retaining all existing per-Skill and
       final bundle checks. Update the Skill bundle contract.
5. [ ] Add the same-value `auto_start` ABA regression(s) through the real config-import runtime-failure path.
6. [ ] Implement generation-gated whole rollback so stale imports cannot restore `auto_start`; update the settings
       ownership contract with the explicit ABA invariant.
7. [ ] Audit archive evidence for child tasks 1-5 and truthfully mark completed PRD/implement entries. Run a narrow
       unchecked-marker audit plus `task.py validate --all`.
8. [ ] Run focused frontend/Rust tests, then the required full quality gates and Linux/Docker watchdog where the
       repository requires it. Record failures and retries honestly.
9. [ ] Run `trellis-check`, update applicable specs, commit, archive this child, then freeze the commit for one
       fresh Sol max read-only final review.

## Validation

```powershell
pnpm exec vitest run src/pages/image-gen/__tests__/useImageGenController.test.tsx
pnpm check:precommit:full
pnpm check:prepush
python .trellis/scripts/task.py validate --all
git diff --check
```

Run the focused Rust config-migration, autostart/settings, and Image Gen suites selected by the changed modules
before broader Rust validation. If Unix-only watchdog coverage cannot run locally, use the authorized Docker Linux
environment and record the exact command/result.

## Review Gate

Do not start a second review agent. After all checks pass, create one fresh independent Orca worktree/terminal for
Codex `gpt-5.6-sol / effort=max`, give it a frozen SHA and a read-only review prompt, then aggregate only its report.
