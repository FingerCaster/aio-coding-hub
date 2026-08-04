# 回切路由契约更新

## Goal

更新 Gateway failover route contract

## Requirements

- Update the Gateway failover route contract from ordinary single-target
  failback to an arbitrary-length ordered higher-priority prefix.
- Define per-candidate natural eligibility, mixed direct/probe dispatch,
  target-first ordering, success/failure continuation, and reservation
  consumption at the first real transport send.
- Preserve all-open unbound recovery, common gate, Provider single-flight,
  skip observability, Ready-provider cap, and attempt budgets.
- Update validation matrix, examples, required tests, and wrong/correct code.

## Acceptance Criteria

- [x] Contract explicitly describes `p1 -> ... -> p(X-1) -> current pX`.
- [x] No statement retains the obsolete ordinary single-target restriction.
- [x] Natural, explicit, reservation, skip, budget, and compatibility rules are
      internally consistent with the parent design.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
