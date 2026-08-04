# Planner 与 Resolution 有序目标链

## Goal

修改 planner 和 provider resolution，支持任意长度前缀与逐候选触发

## Requirements

- Replace the single-provider planner result with an ordered list covering the
  complete higher-priority prefix before the effective session-bound provider.
- Evaluate natural timing independently per candidate; an earlier
  `not_triggered` candidate must not block a later due candidate.
- Preserve direct dispatch for `CLOSED` candidates and probe dispatch with the
  exact trigger for `OPEN`/`HALF_OPEN` candidates.
- Reorder the session-rotated provider list stably so all planned targets run
  in planner order before the current provider, with no fixed provider count.
- Preserve the existing effective-binding and all-open-unbound behavior.

## Acceptance Criteria

- [x] `[p3,p1,p2]` plus planned `[p1,p2]` becomes `[p1,p2,p3]`.
- [x] Five-provider inputs prove the target list is dynamic.
- [x] An earlier not-due candidate is observed but a later due candidate is
      still planned.
- [x] Explicit route-change/compaction/aggressive triggers cover the complete
      prefix, and mixed `CLOSED`/`OPEN` modes remain distinct.
- [x] Focused planner and resolution tests pass.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
