# 路由级回切集成回归

## Goal

新增三至五供应商、跳过、混合状态、预算及预约集成测试

## Requirements

- Add route-level regressions using the existing upstream/counting harness only
  in `gateway/routes.rs`.
- Cover three-provider ordered failback, five-provider dynamic ordering,
  skip-then-continue behavior, mixed direct/probe modes, success short-circuit,
  all-target failure fallback, and Ready-provider limits.
- Assert actual network call order/counts, persisted attempts/metadata, circuit
  states, final route/response, and session binding.
- Include reservation behavior where existing harnesses can observe it; do not
  weaken tests to implementation-only assertions.

## Acceptance Criteria

- [x] P1 failure then P2 success while current P3 makes exactly P1/P2 calls.
- [x] A five-provider case proves no hard-coded prefix length.
- [x] Cooldown/in-flight/not-triggered P1 does not block eligible P2.
- [x] Intermediate success prevents all later calls.
- [x] Exhausted higher targets return to current provider, and Ready cap holds.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
