# Design: 串行任务门控与最终集成验收

## Architecture

父任务只负责顺序、跨任务约束和最终验收；十一个子任务分别拥有独立的实现、测试、提交与
归档边界。主会话是唯一协调者，不派生并发执行代理；两个明确的只读审核 gate 例外地各启动
一对彼此隔离的 reviewer。

```text
planning validated
  -> child 1 start / implement / check / commit / archive
  -> child 2 start / implement / check / commit / archive
  -> child 3 start / implement / check / commit / archive
  -> child 4 start / implement / check / commit / archive
  -> child 5 fetch+merge upstream / check / commit / archive
  -> child 6 first final-review security regression / archive
  -> child 7 second final-review findings / archive
  -> child 8 third final-review findings + explicit product decision / archive
  -> child 9 fourth final-review security/concurrency findings / archive
  -> child 10 fifth final-review trust/CAS/gate/log/router findings / archive
  -> child 11 sixth final-review ownership/budget/stats findings / archive
  -> child 11 frozen-commit dual read-only review / aggregate
  -> parent frozen-commit dual read-only review / archive only after approval
```

## Model And Session Boundary

- 已发生的 Round 6 实现保留真实 `gpt-5.6-luna / effort=max` 模型记录。
- 剩余实现与检查只使用一个 Orca 管理的 Codex `gpt-5.6-terra / effort=max` 终端；主会话负责串行
  协调，不创建第二个并发执行终端。
- 产生当前 F9-F15 findings 的已发生 Round 6 独立只读终审使用新开独立 Codex
  `gpt-5.6-sol / effort=max` 会话。
- 每次后续独立只读终审，包括子任务 11 完成后的下一轮终审和父任务最终审核，必须针对同一冻结
  提交新开彼此隔离的一对 reviewer：Codex `gpt-5.6-sol / effort=max` 与 Pi（`grok-cpa / grok-4.5`）。
  两者可并行但不得交换结果，不能修改 tracked 文件、任务状态、分支或 remote；主会话统一去重、
  核实证据并汇总结论。
- Terra 执行会话不得复用于审核；已发生轮次的历史记录按已记录的具体模型保留。

## Task Ownership

| Order | Task | Owned boundary |
| --- | --- | --- |
| 1 | Multi-provider failover 503 | Candidate visibility, common gate accounting, route label semantics |
| 2 | Manual balance refresh | TanStack Query request ownership and last-visible result |
| 3 | NewAPI response | NewAPI endpoint/auth/field normalization and safe errors |
| 4 | Config export | Skill file budgets and export/import symmetry |
| 5 | Upstream sync | Exact upstream SHA, minimal conflict resolution, full merge validation; no upstream-origin defect fixes |
| 6 | Final review security boundaries | First review's filesystem/network/config hardening |
| 7 | Final review findings round 2 | Eight findings, evidence closure and stable pagination |
| 8 | Final review findings round 3 | Ten blocking findings, including one explicit user product-decision gate |
| 9 | Final review findings round 4 | Nine filesystem, settings, IPC-budget, logging and archive-integrity findings |
| 10 | Final review findings round 5 | Six top-level trust, CAS/runtime, gate-order, OAuth log and Grok router findings |
| 11 | Final review findings round 6 | Historical F1-F8 plus in-scope follow-up F9-F23 settings/autostart, import rollback, bounded read, Image Gen stats and evidence findings; F24 Trellis work excluded by user decision |
| 12 | Parent | Frozen-commit dual read-only review by Codex `gpt-5.6-sol / effort=max` and Pi `grok-cpa / grok-4.5`, then aggregate merge-readiness decision |

## Cross-Task Contracts

1. Child 1 may touch generic gateway selection/log presentation, but not response reconstruction,
   usage extraction, response IDs, TTFB, body buffering or deleted reasoning-guard surfaces.
2. Children 2 and 3 share an account-usage query key and DTO, but are separate commits: child 2 owns
   concurrency/cache state; child 3 owns backend protocol semantics.
3. Account usage remains a one-way display flow:
   `remote read -> Rust normalization -> IPC DTO -> Query cache -> UI`.
   There is no edge back into provider routing or health.
4. Child 4 changes only bounded artifact transport. Export and import limits remain symmetric, and no
   file is silently omitted.
5. Child 5 is the only stage authorized to access `upstream`; it must preserve all committed results
   from children 1-4 and stop on fork behavior conflicts. Defects reproducible on the pinned upstream
   revision without this merge are recorded for a separate task and are not repaired by child 5.

## Validation Gates

- Each child starts from a clean, committed predecessor state and ends with focused tests plus its
  documented full-scope checks.
- Archival is the dependency signal; directory tree order alone is insufficient.
- Child 5 reruns all child-focused regressions after merge, then repository aggregate gates.
- Each parent review freezes one commit, obtains the two independent reviewer reports, and has the
  coordinator deduplicate and verify evidence before deciding pass/fix; it also checks history/order,
  task archive evidence, no upstream push, and all cross-layer contracts before completion.

## Compatibility And Rollback

- Each child is one coherent rollback unit; do not combine unrelated child changes in one commit.
- A failed child stays active until repaired or its own commit is reverted. Do not advance the queue.
- Before child 5 commit, the merge may be aborted or left paused without rewriting completed child
  commits. After merge, revert the merge as one unit rather than selectively discarding upstream files.
- The parent has no direct product code, so rollback is entirely through child commits.
