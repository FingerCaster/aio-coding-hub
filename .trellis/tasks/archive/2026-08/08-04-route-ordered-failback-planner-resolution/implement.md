# Planner 与 Resolution 有序目标链 - 执行计划

1. Inspect all planner decision consumers and constructors before changing the
   shared decision type.
2. Implement ordered targets and per-candidate natural/explicit eligibility in
   `probe_planner.rs`; update its unit tests.
3. Implement stable target-first ordering and intent construction in
   `provider_resolution.rs`; update local tests.
4. Run focused planner/resolution tests and Rust format for touched files.
5. Do not edit dispatch, provider gate, route integration tests, or specs.
