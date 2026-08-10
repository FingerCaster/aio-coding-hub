# Current main 落地证据

核查基线：`main` 当前头为 `444b92ac`。集成提交 `12e565c0` 可由当前
`main` 到达；含完整任务记录的分支提交 `51e3550f` 不在当前祖先链，仅作为来源对照，
本次没有 cherry-pick。

- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_record.rs`
  集中记录 transport retry/backoff 决策，沿用既有 attempt budget。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/upstream_retry_policy.rs`
  保留 `transport_errors`、`max_retries` 与 `backoff_ms` 校验和投影。
- `send_timeout.rs`、`success_event_stream.rs`、`success_non_stream.rs` 与
  `upstream_error.rs` 均接入同一 transport retry backoff，覆盖连接、超时、读取和流式终态路径。
- `.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md`
  记录 retry budget/backoff 的跨层约束；现有 Rust failover 测试保留 HTTP 退避边界回归。

该子任务已落地到当前 `main`；本记录只补齐归档证据，不重新实现或重测业务逻辑。
