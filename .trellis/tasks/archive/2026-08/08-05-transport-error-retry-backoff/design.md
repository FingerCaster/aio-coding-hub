# 传输错误重试退避设计

## Boundary

本任务不改 `UpstreamRetryPolicy` schema、默认值、Provider 覆盖或 UI。只修复 transport retry 已决策为 `RetrySameProvider` 后缺少等待的问题。

## Current Gap

- HTTP 状态错误分支在返回 `ContinueRetry` 前显式应用 `retry_policy_backoff_delay`。
- reqwest connect/read、send timeout、非流式 body read、流式首字节/read/idle timeout 统一经 `record_system_failure_and_decide_impl` 返回 `ContinueRetry`，该共享路径没有等待。
- 这些 transport 路径只有策略匹配并且预算未耗尽时才得到 `RetrySameProvider`；未配置时直接 SwitchProvider。

## Design

在 `record_system_failure_and_decide_impl` 完成熔断状态修正、attempt 记录和日志写入后，以最终 `decision` 为准：

- `RetrySameProvider`：读取当前 Provider 的有效 `upstream_retry_policy.backoff_ms`，大于零时使用 `tokio::time::sleep` 后返回 `ContinueRetry`。
- `SwitchProvider` / `Abort`：不等待。

等待必须发生在熔断可能将 retry 改写为 switch 之后，避免已经开路时仍多等一次。HTTP 状态码路径继续保留现有专用等待，不进入该共享 system-error 路径，因此不会双重退避。

## Coverage

共享函数的调用面覆盖：

- `send_timeout.rs` 请求/首字节超时。
- `upstream_error.rs` reqwest connect/timeout/read。
- `success_non_stream.rs` response body timeout/read。
- `success_event_stream.rs` 首 chunk、prefix 读取、idle timeout 和读取错误。

## Testability

抽取纯函数，根据最终 decision 与 policy 返回可选 Duration；测试所有 decision、零值和非零值。运行时只在 pure helper 返回 Some 时 await sleep，避免单元测试依赖真实墙钟。

## Compatibility And Rollback

- 配置、序列化、UI、attempt 文本和熔断计数均无变化。
- 回滚只移除共享 system-error retry 等待及纯决策 helper。
