# 传输错误重试退避执行计划

- [ ] 在独立 worktree 中记录基线并确认所有 transport `RetrySameProvider` 调用面。
- [ ] 抽取“最终决策是否应用策略退避”的纯函数并补单元测试。
- [ ] 在 `record_system_failure_and_decide_impl` 最终 RetrySameProvider 分支应用异步等待。
- [ ] 为 connect/timeout/read 与流式/非流式调用路径补行为测试，确认配置预算与熔断改写不变。
- [ ] 运行目标 Rust 测试、fmt/check/Clippy，确认前端设置 round-trip 不回归。
- [ ] 执行 Trellis quality check，修复发现并提交子任务。

## Validation Commands

```powershell
cargo test --manifest-path src-tauri/Cargo.toml upstream_retry_policy
cargo test --manifest-path src-tauri/Cargo.toml send_timeout
cargo test --manifest-path src-tauri/Cargo.toml success_event_stream
cargo test --manifest-path src-tauri/Cargo.toml success_non_stream
cargo test --manifest-path src-tauri/Cargo.toml upstream_error
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Review Gates

- 只对最终仍为 RetrySameProvider 的 transport system error 等待。
- HTTP 状态码配置重试不重复等待。
- count-tokens、Provider switch、Abort 和 circuit-open 改写不等待。
- backoff_ms=0 无额外延迟。
