# Dispatch Reservation 与 Provider Gate - 执行计划

1. Trace every `RequestDispatchIntent`, ownership, reservation, and send-boundary
   call site before changing types.
2. Implement per-target triggers and request-scoped reservation state in
   `dispatch.rs`; add focused unit tests.
3. Update `provider_gate.rs` to query the target-specific trigger and remove
   whole-request reservation release on gate denial.
4. Run focused dispatch/gate tests and Rust format for touched files.
5. Do not edit planner, resolution, route integration tests, or specs.
