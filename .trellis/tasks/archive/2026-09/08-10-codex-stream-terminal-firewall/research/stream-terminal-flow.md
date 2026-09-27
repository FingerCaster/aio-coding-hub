# 流终态错误现状研究

## 现有入口

- `src-tauri/src/domain/usage.rs`
  - `is_codex_capacity_stream_internal_error` 只识别终态 Codex 事件中的内置容量码和
    `selected model is at capacity` 文案。
  - `classify_codex_stream_internal_error` 的优先级是旧 `retry_keywords`、内置容量、
    旧 `non_retry_keywords`、unknown。
  - `SseUsageTracker` 在消费完整 SSE 事件时保存有界的原始证据，并在启用 classifier 时
    标记 `terminal_error_seen` / `fake_200_detected`。
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs`
  - `inspect_buffered_event_stream_prefix` 只在未桥接原生 Codex Responses 路径调用旧分类器。
  - 命中 retryable 或内置容量时返回 `ProviderFailure`，进入共享 retry/failover loop。
  - 原生路径的未知/非重试终态当前返回 `StartStreaming`，因此提交后会被
    `forwarded_after_commit` 透传。
  - 非原生或桥接路径遇到终态错误时返回 `FinalizeAsEmptyBody`，不使用两个关键词列表。
- `src-tauri/src/gateway/streams/usage_tee.rs`
  - `UsageSseTeeStream` 先把原始 chunk 交给 tracker，再将 chunk 交给 relay。
  - Codex Responses 使用 `spawn_usage_sse_relay_body` 和 `with_defer_terminal_error`，
    当前 relay 对任意 chunk 透传并在终态后继续 drain。
  - 因此客户端投影必须放在 tracker 之后、relay 发送之前，并具备跨 chunk 的有界 SSE 帧缓冲。
- `src-tauri/src/gateway/proxy/sse.rs`
  - 已有 `find_sse_event_end` / `parse_sse_frame`，支持 `event:`、`data:`、CRLF，优先复用。

## 配置与兼容

- Rust 权威类型为 `UpstreamStreamInternalErrorPolicy`，当前字段是
  `enabled`、`retry_keywords`、`non_retry_keywords`。
- 前端投影在 `src/services/gateway/upstreamRetryPolicy.ts` 和
  `src/components/gateway/RetryPolicyFields.tsx`；生成绑定为
  `src/generated/bindings.ts`。
- Provider override/share 复用同一 Rust 类型，相关代码在
  `src-tauri/src/domain/providers/share.rs`。
- 设置 schema 当前为 58。新字段迁移必须同步全局设置、Provider override、share/import、
  TypeScript 默认值/校验和生成绑定。

## 必须改写的行为测试

- `src-tauri/src/gateway/routes.rs:13231` 的未知终态转发测试必须改为断言客户端不见原文、
  内部 evidence 仍存在。
- `src-tauri/src/gateway/routes.rs:13060` 附近的容量重试测试保留，并增加当前
  `service_unavailable_error + server_error + overloaded` 复现帧。
- `src-tauri/src/gateway/routes.rs:13300` 附近的“关闭重试仍拦截容量”测试必须按新产品决定
  改写：`stream_internal_errors.enabled=false` 时 capacity 与其他终态均不拦截、不触发
  stream-internal retry/failover，只保留被动 evidence。
- `src-tauri/src/domain/usage/tests.rs` 和 `success_event_stream.rs` 的分类优先级、跨 chunk、
  CRLF、同 chunk 多帧测试需要扩展。

## 约束

- 共享 retry budget/backoff/circuit 只能由提交前的瞬态分类消耗。
- 提交后不得拼接另一 Provider 的可见输出；默认丢弃终态错误帧并结束流。
- firewall 开启时，客户端诊断不得包含供应商 message/code/type 或容量词；关闭时保持当前
  原流行为，内部日志仍保留限长、脱敏 evidence。
- 具体 Codex 兜底 SSE payload 在实施前要由当前客户端/协议 fixture 验证；不能凭经验伪造事件。
