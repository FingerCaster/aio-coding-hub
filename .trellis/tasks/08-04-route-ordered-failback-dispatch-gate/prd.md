# Dispatch Reservation 与 Provider Gate

## Goal

修改 dispatch intent、预约生命周期和 provider gate

## Requirements

- Represent request dispatch intent as an arbitrary ordered set of targets with
  an optional probe trigger per provider.
- Acquire provider-global probe leases independently and at most once per
  provider while preserving direct dispatch for non-probe targets.
- Keep the session trigger reservation request-scoped through every gate or
  pre-send skip; consume it once at the first real transport send boundary.
- If no planned target sends, drop/release the reservation so a later request
  can retry. Preserve fail-closed persistence and rollback behavior.
- Do not alter Ready-provider, retry, or total attempt budgets.

## Acceptance Criteria

- [x] Gate/cooldown/in-flight/pre-send skips leave the reservation available to
      later targets.
- [x] The first real send consumes once; later sends cannot consume again.
- [x] A zero-send chain releases the reservation on intent drop.
- [x] Each target returns its own trigger and cannot claim twice.
- [x] Focused dispatch and provider-gate tests pass.

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
