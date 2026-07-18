# Implementation Plan: 串行修复与父任务验收

## Planning Handoff (Historical Complete)

- [x] Planning artifacts and manifests were validated before the ordered execution began.
- [x] Preserve the actual historical implementation model as `gpt-5.6-luna / effort=max`.
- [x] The single Orca-managed Codex `gpt-5.6-terra / effort=max` execution terminal completed the remaining
      implementation and checks without a concurrent execution agent. The frozen-commit review used fresh,
      isolated Codex `gpt-5.6-sol / effort=max` and Claude `claude-opus-4-8 / effort=max` sessions; they did
      not exchange results or modify tracked files/task state/branch/remote. Do not reuse the Terra execution
      session for review. This is historical evidence only. From 2026-07-18 onward, every new frozen review
      uses only a fresh Codex `gpt-5.6-sol / effort=max` session; do not launch Claude, Pi, or any other
      review agent.
- [x] Round 7 implementation and checks used one fresh Orca-managed Codex `gpt-5.6-sol / effort=max` execution
      terminal under the approved PRD/design/implement scope. Implementation committed at `8bbc619a` and archived
      at `29133ac0`; its separate final review found the historical-user-decision P2 later corrected by Round 8.
- [x] Record that the already completed Round 6 independent read-only review producing F9-F15 used a
      new Codex `gpt-5.6-sol / effort=max` session; this is separate from both the historical Luna work and
      the remaining Terra execution terminal.
- [x] Confirm clean business-code state and exact base before starting child 1.

## Ordered Execution

### 1. Multi-provider failover

- [x] Run `task.py start 07-17-fix-multi-provider-failover-503` only.
- [x] Implement and check its reviewed artifacts without spawning parallel agents.
- [x] Run focused gateway/route/presentation tests and required full Rust checks.
- [x] Commit, finish and archive child 1; record its commit and validation evidence.

### 2. Manual account refresh

- [x] Assert child 1 is archived before starting child 2.
- [x] Implement one query-owned refresh path and deterministic deferred-response tests.
- [x] Verify no availability/circuit side effects, then commit and archive child 2.

### 3. NewAPI `muyuan`

- [x] Assert child 2 is archived before starting child 3.
- [x] Implement the evidence-backed NewAPI billing contract and fail-closed error handling.
- [x] Run fixture tests first, then the authorized minimal read-only `muyuan` validation with redacted
      output; commit and archive child 3.

### 4. Large Skill asset export

- [x] Assert child 3 is archived before starting child 4.
- [x] Align the per-file limit with the existing per-Skill budget and keep import symmetry.
- [x] Run binary round-trip and all security boundary tests; commit and archive child 4.

### 5. Upstream sync

- [x] Assert children 1-4 are all archived. Until this assertion passes, do not inspect or access
      project remote `upstream`.
- [x] Fetch `upstream/main`, record its immutable SHA, review drift and perform a real semantic merge.
- [x] Carry all non-conflicting changes. Pause on fork behavior conflicts and present evidence/options.
- [x] Keep child 5 integration-only: make only synchronization and minimal concrete textual/semantic
      conflict fixes. An upstream-origin defect that reproduces without the merge is recorded and assigned
      to a separately authorized follow-up task; it is not fixed in the merge task or merge commit.
- [x] Preserve the documented one-time Image Gen exception in this parent task; it does not establish a
      precedent for widening later upstream synchronization scope. Do not rewrite the archived upstream
      task history.
- [x] Rerun all child regressions and repository full gates; commit and archive child 5.

## Parent Integration Acceptance

- [x] Child 5 was archived before starting the first final-review security child.
- [x] Child 6 was implemented, checked, committed and archived before child 7 started.
- [x] Finish child 7, then verify commit/archive order is exactly 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7.
- [x] Finish child 8 after implementing explicit provider-selection decision A and full gates, then verify order
      is exactly 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8.
- [x] Finish child 9 after closing round-4 findings and full gates, then verify order is exactly
      1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9.
- [x] Finish child 10 after closing round-5 findings and full gates, then verify order is exactly
      1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10; archived at
      `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5`.
- [x] Finish child 11 after closing round-6 findings and full gates, then verify order is exactly
      1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11; it was archived at `a2abe128`.
- [x] Freeze commit `2a3788fc62db982737b9873757c354f89e198ce6` and collect independent read-only reports from
      Codex `gpt-5.6-sol / effort=max` and Claude `claude-opus-4-8 / effort=max`. The reports must still be
      aggregated and evidence-checked before passing; do not archive the parent before that summary passes.
- [x] The focused and full gates required by child 11, including `pnpm build`, `pnpm check:precommit:full`
      and `pnpm check:prepush`, passed and are recorded in the archived child evidence.
- [x] Complete child 12 at `8bbc619a`, archive it at `29133ac0`, and record that its fresh Sol review found the
      unsupported historical-user-decision P2.
- [x] Complete and archive child 13 as the sole Round 8 factual-record correction at `356fe32` while the parent
      remains `in_progress`.
- [x] After the parent fact-projection commits, the coordinator froze final parent tip
      `6de6ab8b20d2fa9dde00585e8a04b71405b85b9e` and ran a new independent Codex
      `gpt-5.6-sol / effort=max` read-only review over `29133ac0..6de6ab8`. It reported no P0-P2 and statically
      confirmed the Round 8 archive/manifests, parent projections, final-tip rule and task scope. Builds/tests were
      not rerun and remain the only residual risk; child archive `356fe32` was not used as the final anchor.
- [x] Read-only `git remote -v` confirmed `origin` as the normal fetch/push remote and `upstream` push as
      `DISABLED`; this session issued no push.
- [ ] Archive the parent only after every acceptance item is evidenced.

## Review children

### 6. First final-review security boundaries

- [x] Closed the first review's security findings, committed `7a668343`, archived in
      `.trellis/tasks/archive/2026-07/07-17-final-review-security-boundaries`.

### 7. Second final-review findings

- [x] Close F1-F8 under `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-2`, including post-fix
      `muyuan` live evidence and the upstream conflict decision audit.
- [x] Commit and archive child 7 while keeping this parent `in_progress`.

### 8. Third final-review findings

- [x] Close nine non-conflicting findings under `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-3`.
- [x] Record explicit user decision A: retain common-gate skipped/continue/full-503 behavior.
- [x] Rerun focused and full gates, then commit/archive child 8 while keeping this parent `in_progress`.

### 9. Fourth final-review findings

- [x] Implement all nine findings under `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-4`, including the
      explicit Skill root-authority-only product decision and template-synchronized executable specs.
- [x] Run focused tests plus `check:precommit:full` and `check:prepush` while keeping this parent `in_progress`.
- [x] Commit and archive child 9, record journal evidence, then return to the historical independent
      read-only review recorded at that point without inferring an unrecorded model/effort.

### 10. Fifth final-review findings

- [x] Close all six findings under `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5`, including top-level
      Skill handle authority, settings side-effect ownership, common-gate ordering, structured OAuth
      sanitization and the Grok production continuation regression.
- [x] Run focused and full gates, commit and archive only child 10, then return to the next independent
      Codex `gpt-5.6-sol / effort=max` read-only review in a new session while the parent remains
      `in_progress`.

### 11. Sixth final-review findings

- [x] Close historical F1-F8 and in-scope follow-up F9-F23 under
      `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-6`, including settings owned
      patch/autostart coordinator, exact preferred-port rollback ownership, config import serialization/Skill
      rollback lifecycle, hard-bounded reads, encoded export budget, Image Gen handle-relative storage stats,
      and evidence correction. F24 Trellis template-hash / safe-commit remains excluded and related dirty files
      remain preserved.
- [x] Run focused and full gates, commit `b430874d`, archive only child 11 in `a2abe128`, then freeze
      `2a3788fc` and collect isolated Codex Sol plus Claude Opus read-only reports while the parent remains
      `in_progress`.

### 12. Seventh final-review findings

- [x] Close the approved Round 7 runtime and archive-reconciliation findings, commit `8bbc619a`, and archive only
      child 12 in `29133ac0` while the parent remains `in_progress`.
- [x] Record the separate fresh Sol review's P2: the upstream archive claimed a historical user decision that the
      cited post-hoc audit does not prove.

### 13. Eighth final-review factual correction

- [x] Correct only the two unsupported historical-decision assertions and the parent Round 7/8 projection, then
      commit and archive only child 13 at `356fe32` while the parent remains `in_progress`.
- [x] Satisfy the final-parent-tip freeze rule with the independent Sol review of
      `6de6ab8b20d2fa9dde00585e8a04b71405b85b9e`; its no-P0-P2 conclusion permits parent archive while `main`
      remains unmerged.

## Stop And Rollback Rules

- Do not start the next child after any failed acceptance or dirty/uncommitted state.
- Do not weaken tests to advance the queue.
- Revert only the current child's coherent commit when rollback is required.
- During child 5, stop before resolution when a conflict changes fork product behavior.
