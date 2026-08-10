# Bug Analysis: Stable account block replays failback every request

## 1. Root Cause Category

- **Category**: B + D - Cross-layer contract and test coverage gap
- **Specific Cause**: The account-usage gate returns before
  `claim_for_provider`. The successful fallback is not a planned target, so it
  cannot commit the request-scoped compaction/route reservation. Intent drop
  releases the reservation and the next request plans the same blocked target.
  The original contract specified each layer independently but did not specify
  the steady multi-request behavior for a durable pre-claim denial.

## 2. Why Earlier Coverage Missed It

1. The recovery route test exercised only the first request of each Session,
   then immediately changed the snapshot to Available.
2. Planner tests covered account recovery epochs but not a fresh Blocked input
   during natural or explicit failback.
3. Summary projections intentionally hide `not_triggered` observations, so
   only the raw request attempts and local database distinguished a real account
   skip from a display-count issue.

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | Feed fresh Blocked IDs and recovery epochs from the same route read into the planner | DONE |
| P0 | Test coverage | Exercise initial skip, two steady same-Session compaction turns, and recovery | DONE |
| P1 | Executable spec | Define stable-bound suppression without deleting ordinary candidates or consuming the trigger | DONE |
| P1 | Review checklist | Require consecutive-request analysis for durable pre-send gates and reservations | DONE |

## 4. Systematic Expansion

- **Similar Issues**: Any future durable eligibility gate that rejects before
  dispatch ownership can create the same replay. Circuit cooldown/in-flight
  must not be changed mechanically because their common-gate observations and
  single-flight ownership are intentional.
- **Design Improvement**: Keep the common gate authoritative for send-time
  races, but allow a typed, current, fail-open projection to guide only the
  stable-bound higher-priority failback prefix.
- **Process Improvement**: For a new gate, test the first request, at least two
  unchanged-state requests, an uncertain-state request, and confirmed recovery.

## 5. Knowledge Capture

- [x] Update the Gateway failover route contract.
- [x] Update the provider account-usage route contract.
- [x] Update the long-lived binding cross-layer checklist.
- [x] Add planner and real route regressions.
