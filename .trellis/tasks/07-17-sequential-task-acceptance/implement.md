# Implementation Plan: 串行修复与父任务验收

## Planning Handoff

- [ ] Confirm all six tasks pass `task.py validate` and remain `planning`.
- [ ] Main session switches from the max research terminal to a `gpt-5.6-sol` medium implementation
      terminal. User approval to start after validation is already recorded; do not ask again.
- [ ] Confirm clean business-code state and exact base before starting child 1.

## Ordered Execution

### 1. Multi-provider failover

- [ ] Run `task.py start 07-17-fix-multi-provider-failover-503` only.
- [ ] Implement and check its reviewed artifacts without spawning parallel agents.
- [ ] Run focused gateway/route/presentation tests and required full Rust checks.
- [ ] Commit, finish and archive child 1; record its commit and validation evidence.

### 2. Manual account refresh

- [ ] Assert child 1 is archived before starting child 2.
- [ ] Implement one query-owned refresh path and deterministic deferred-response tests.
- [ ] Verify no availability/circuit side effects, then commit and archive child 2.

### 3. NewAPI `muyuan`

- [ ] Assert child 2 is archived before starting child 3.
- [ ] Implement the evidence-backed NewAPI billing contract and fail-closed error handling.
- [ ] Run fixture tests first, then the authorized minimal read-only `muyuan` validation with redacted
      output; commit and archive child 3.

### 4. Large Skill asset export

- [ ] Assert child 3 is archived before starting child 4.
- [ ] Align the per-file limit with the existing per-Skill budget and keep import symmetry.
- [ ] Run binary round-trip and all security boundary tests; commit and archive child 4.

### 5. Upstream sync

- [ ] Assert children 1-4 are all archived. Until this assertion passes, do not inspect or access
      project remote `upstream`.
- [ ] Fetch `upstream/main`, record its immutable SHA, review drift and perform a real semantic merge.
- [ ] Carry all non-conflicting changes. Pause on fork behavior conflicts and present evidence/options.
- [ ] Rerun all child regressions and repository full gates; commit and archive child 5.

## Parent Integration Acceptance

- [x] Child 5 was archived before starting the first final-review security child.
- [x] Child 6 was implemented, checked, committed and archived before child 7 started.
- [ ] Finish child 7, then verify commit/archive order is exactly 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7.
- [ ] Run the independent max read-only final review; do not archive the parent before it passes.
- [ ] Run `pnpm build`, `pnpm check:precommit:full`, `pnpm check:prepush` and any focused commands
      listed by child tasks that are not included in aggregate gates.
- [ ] Verify `origin` remains the normal GitHub target, `upstream` remains fetch-only, and no push was
      made.
- [ ] Archive the parent only after every acceptance item is evidenced.

## Review children

### 6. First final-review security boundaries

- [x] Closed the first review's security findings, committed `7a668343`, archived in
      `.trellis/tasks/archive/2026-07/07-17-final-review-security-boundaries`.

### 7. Second final-review findings

- [ ] Close F1-F8 under `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2`, including post-fix
      `muyuan` live evidence and the upstream conflict decision audit.
- [ ] Commit and archive child 7 while keeping this parent `in_progress`.

## Stop And Rollback Rules

- Do not start the next child after any failed acceptance or dirty/uncommitted state.
- Do not weaken tests to advance the queue.
- Revert only the current child's coherent commit when rollback is required.
- During child 5, stop before resolution when a conflict changes fork product behavior.
