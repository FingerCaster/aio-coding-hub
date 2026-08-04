# Bug Analysis: Ordered Failback And Session Convergence

## 1. Root Cause Category

- **Primary category**: B - Cross-Layer Contract
- **Secondary categories**: D - Test Coverage Gap; E - Implicit Assumption
- **Specific cause**: Failback eligibility, Provider single-flight ownership,
  recovery publication, per-session convergence, and response-finalizer binding
  commits were implemented in different layers without one lifecycle contract.
  The original code assumed one preferred Provider, one winning session, and a
  stable binding object whose completion order matched request causality.

Evidence changed the diagnosis in stages:

| Hypothesis | Initial prior | Discriminating evidence | Final assessment |
| --- | ---: | --- | --- |
| Fixed-size planner/dispatch | 40% | P1 was attempted but due P2 was never reached | Confirmed, but incomplete |
| Provider gate/single-flight | 30% | Followers recorded `in_flight`, then stayed on P2 after winner success | Confirmed cross-layer gap |
| Session completion race | 30% | Gated P2 response completed after a newer P1 success and rewrote the binding | Confirmed release blocker |

The deterministic route, stream-finalizer, clear, and TTL tests raise confidence
above 95% that the final cause and prevention boundaries are correctly modeled.

## 2. Why Earlier Fixes Failed

1. **Single preferred target**: The planner fixed the immediate P1 case but did
   not represent an arbitrary ordered prefix, so P2 and later candidates were
   structurally unreachable.
2. **Ordered probes only**: Provider single-flight prevented duplicate network
   probes, but winner success updated only the winner session. Followers saw a
   closed circuit with no due deadline and could remain on the old Provider.
3. **Recovery epoch only**: A success-only Provider epoch made followers converge
   on their next request, but old same-session P2 completions could still overwrite
   a newer P1 binding because finalizers published unversioned state.
4. **Last-success token only**: Monotonic request tokens fixed completion-order
   inversion inside one live binding, but clear or TTL recreation reset the last
   token and reopened an ABA write path.
5. **Deadline-only active-lease check**: An attempted planner tightening passed an
   artificial snapshot test, but the real transport boundary moves deadlines.
   Followers then skipped the common gate instead of observing `in_flight`.

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | Model an arbitrary ordered dispatch target list with per-Provider direct/probe intent | DONE |
| P0 | Architecture | Publish a success-only Provider recovery epoch and capture a per-binding baseline | DONE |
| P0 | Compile-time API | Allocate one checked request token before any `await` and require it at every production binding commit | DONE |
| P0 | Runtime fence | Store a binding creation token; reject missing, expired, pre-creation, overflowed, or causally older commits | DONE |
| P0 | Test coverage | Use deterministic TCP/barrier tests for winner/followers, both completion orders, stream finalization, clear, and TTL recreation | DONE |
| P1 | Documentation | Record active-lease gate visibility, marker invalidation, token propagation, and incarnation fencing in the Gateway contract | DONE |
| P1 | Review checklist | Add long-lived binding and single-flight questions to the cross-layer thinking guide and template | DONE |

## 4. Systematic Expansion

- **Similar issues**: Any state updated after async work can suffer the same ABA
  problem if clear, expiry, eviction, or recreation resets only the last-write
  version. Audit recent-error caches, route caches, reservations, and stream
  finalizers with the same lifecycle questions when they gain late writes.
- **Design improvement**: Version comparisons must include object incarnation.
  A last-event counter is not sufficient when the versioned object can disappear
  and be recreated.
- **Process improvement**: Concurrency tests must drive the real state transition
  that changes eligibility, not construct only the desired final snapshot.
  Every async binding change needs both request-order permutations and at least
  one destruction/recreation permutation.
- **Knowledge gap closed**: Provider health recovery is global, but session
  convergence is lazy and per session. Winner success publishes the fact;
  followers consume it on their next eligible request without another timeout.

## 5. Knowledge Capture

- [x] Updated the executable Gateway failover route contract.
- [x] Updated the cross-layer thinking guide.
- [x] Updated the corresponding template guidance.
- [x] Added focused and real-route regression coverage.
- [x] Kept the root fix in this task; no separate issue remains.
