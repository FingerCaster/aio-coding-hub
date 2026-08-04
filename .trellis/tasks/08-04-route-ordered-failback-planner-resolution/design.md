# Planner 与 Resolution 有序目标链 - 技术设计

The planner owns target eligibility and ordered per-target dispatch mode. It
must scan the latest-route prefix before the effective bound provider and
return every eligible target plus structured not-triggered observations.

Resolution consumes that ordered target list through one stable reorder helper:
append each unique planned ID that exists, then append all remaining candidates
in their existing relative order. It also constructs the request dispatch
intent using the per-target modes without changing the serial failover loop.

No fixed `p1/p2` branches, second executor, or budget changes are permitted.
