# Design: 串行任务门控与最终集成验收

## Architecture

父任务只负责顺序、跨任务约束和最终验收；八个子任务分别拥有独立的实现、测试、提交与
归档边界。主会话是唯一协调者，不派生并发代理。

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
  -> parent independent max review / archive only after approval
```

## Task Ownership

| Order | Task | Owned boundary |
| --- | --- | --- |
| 1 | Multi-provider failover 503 | Candidate visibility, common gate accounting, route label semantics |
| 2 | Manual balance refresh | TanStack Query request ownership and last-visible result |
| 3 | NewAPI response | NewAPI endpoint/auth/field normalization and safe errors |
| 4 | Config export | Skill file budgets and export/import symmetry |
| 5 | Upstream sync | Exact upstream SHA, semantic conflict handling, full merge validation |
| 6 | Final review security boundaries | First review's filesystem/network/config hardening |
| 7 | Final review findings round 2 | Eight findings, evidence closure and stable pagination |
| 8 | Final review findings round 3 | Ten blocking findings, including one explicit user product-decision gate |
| 9 | Parent | Independent max review and merge-readiness decision |

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
   from children 1-4 and stop on fork behavior conflicts.

## Validation Gates

- Each child starts from a clean, committed predecessor state and ends with focused tests plus its
  documented full-scope checks.
- Archival is the dependency signal; directory tree order alone is insufficient.
- Child 5 reruns all child-focused regressions after merge, then repository aggregate gates.
- Parent review checks history/order, task archive evidence, no upstream push, and all cross-layer
  contracts before completion.

## Compatibility And Rollback

- Each child is one coherent rollback unit; do not combine unrelated child changes in one commit.
- A failed child stays active until repaired or its own commit is reverted. Do not advance the queue.
- Before child 5 commit, the merge may be aborted or left paused without rewriting completed child
  commits. After merge, revert the merge as one unit rather than selectively discarding upstream files.
- The parent has no direct product code, so rollback is entirely through child commits.
