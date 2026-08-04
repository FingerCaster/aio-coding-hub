# 路由级回切集成回归 - 技术设计

Extend the existing route-test fixtures and counting upstreams. Tests must
exercise the production handler and serial failover loop, not call planner
helpers directly. Use table-driven helpers only when they already match local
style and reduce meaningful duplication.

Assertions must distinguish planner `not_triggered` observations from ordinary
gate-skipped attempts and verify both zero-call skips and actual ordered sends.
