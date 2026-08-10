# 传输错误重试退避

## Goal

让现有 `UpstreamRetryPolicy.backoff_ms` 对配置命中的连接、超时、读取及流式传输重试生效，避免传输失败后无间隔地立即再次请求，同时不改变重试预算、Provider 切换和熔断语义。

## Background

- `UpstreamRetryPolicy` 已持久化 `transport_errors`、`max_retries`、`backoff_ms` 和熔断计数设置，默认包含 connect/timeout/read 与 100ms 退避。
- 当前 HTTP 状态码配置重试在 `upstream_error.rs` 中直接 sleep；传输错误（reqwest 错误、请求超时、流式首字节/中途 read/idle timeout）调用共享记录函数后直接继续，没有统一应用 `backoff_ms`。
- 第一组任务明确将本项留作后续独立工作；本任务不新增配置字段或 UI。

## Requirements

- R1：配置命中的 transport retry 在进入下一次同 Provider 尝试前应用现有 `backoff_ms`；`backoff_ms = 0` 不等待。
- R2：覆盖 connect、request timeout、response read、stream first-byte/read/idle-timeout 等所有会返回 `RetrySameProvider` 的传输路径；未命中配置或仅发生 Provider 切换时不新增配置退避。
- R3：继续沿用现有 `max_retries`、Provider 最大尝试次数、count-tokens 特殊路径、Provider 覆盖替换和熔断计数语义，不重复累计配置重试。
- R4：退避等待必须可测试且不阻塞运行时线程；测试不得依赖真实长时间睡眠。
- R5：attempt reason/outcome 和现有日志语义保持兼容；必要时补充可定位的 retry/backoff 诊断，但不得泄露请求正文或凭据。

## Acceptance Criteria

- [ ] connect/timeout/read 的配置重试均在下一次尝试前应用策略退避，零退避立即继续。
- [ ] 流式首字节、流式中途读取和 idle timeout 的配置重试均覆盖；HTTP 规则既有退避逻辑不回归。
- [ ] 未命中 transport 配置、切换 Provider、count-tokens 和重试耗尽路径的行为与当前一致。
- [ ] 使用 paused time 或注入等待器等方式验证等待决策，不引入脆弱的实时 sleep 测试。
- [ ] 受影响 Rust 单元/集成测试、fmt/check/lint 通过，前端设置 round-trip 回归通过。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
