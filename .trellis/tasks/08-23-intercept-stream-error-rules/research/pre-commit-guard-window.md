# Research: pre-commit 守护窗口完整实现

- **Query**: pre-commit 守护窗口的配置项名称、默认值 500ms、上限 5000ms 分别在哪定义（settings types + 前端）；窗口内缓冲前缀存在哪、上限 1MiB 在哪限制；窗口内如何丢弃缓冲并重试
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 配置项：`stream_internal_error_guard_ms`

| 关注点 | 位置 | 值 |
|---|---|---|
| AppSettings 字段声明 | `src-tauri/src/infra/settings/types.rs:444` | `pub stream_internal_error_guard_ms: u32` |
| 默认值常量 | `src-tauri/src/infra/settings/defaults.rs:14` | `DEFAULT_STREAM_INTERNAL_ERROR_GUARD_MS: u32 = 500` |
| 默认值应用 | `src-tauri/src/infra/settings/types.rs:544` | `impl Default for AppSettings` |
| 上限常量 | `src-tauri/src/infra/settings/defaults.rs:15` | `MAX_STREAM_INTERNAL_ERROR_GUARD_MS: u32 = 5_000` |
| 保存校验（fail closed） | `src-tauri/src/infra/settings/persistence.rs:434-437` | 超限报 `SEC_INVALID_INPUT` |
| 读取修复（clamp） | `src-tauri/src/infra/settings/migration.rs:847-850` | 夹到上限；调用点 `migration.rs:1549` |
| 常量再导出 | `src-tauri/src/infra/settings/mod.rs:32` | — |
| 运行时兜底 | `src-tauri/src/gateway/proxy/handler/runtime_settings.rs:151-153` | 缺配置时取 `DEFAULT_...` |

前端镜像：

| 关注点 | 位置 |
|---|---|
| 上限常量（TS 侧） | `src/services/settings/settingsValidation.ts:57` `MAX_STREAM_INTERNAL_ERROR_GUARD_MS = 5_000` |
| 范围校验 `0..MAX` | `src/services/settings/settingsValidation.ts:473`（标签「流内部错误保护窗」） |
| General Tab 校验 + 保存 | `src/components/cli-manager/tabs/GeneralTab.tsx:847-864` |
| Codex Tab 校验 + 保存 | `src/components/cli-manager/tabs/CodexTab.tsx:1907-1965`，输入渲染 `602`、`2317-2322` |
| 字段组件 | `src/components/gateway/CodexStreamInternalErrorFields.tsx:129-148` |
| 跨层常量一致性测试 | `src/constants/__tests__/crossLayerContracts.test.ts:177-178`（默认值）、`226`（上限名） |

契约描述：`.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:56-59`
（"The setting is `0..=5000` ms with default `500`; the buffered prefix is capped at 1 MiB per request."）

### 运行时管线（settings → guard Duration）

```
AppSettings.stream_internal_error_guard_ms                 (types.rs:444)
  → runtime_settings.rs:32 / 151-153                       (RuntimeSettings 字段)
  → handler/middleware/mod.rs:183
  → proxy/request_context.rs:125, 337                      (字段透传)
  → proxy/request_context.rs:58, 201-202                   Duration::from_millis(u64::from(...))
  → failover_loop/mod.rs:296
  → failover_loop/context.rs:61 / 96 / 140 / 181 / 215     (CommonCtx / CommonCtxOwned)
  → success_event_stream.rs:1275                           BufferedStreamPrefixConfig { guard: ... }
```

注意：**guard 只有全局配置，没有 Provider override**（契约 `:36-37` 明确说明
"Provider overrides do not expose the global observation window"）。Provider 级 override 只覆盖
`upstream_retry_policy`（`ProviderCtxOwned.upstream_retry_policy`，`context.rs:272`）。

### 窗口启动条件（不是从流开始就计时）

`BufferedStreamPrefixState`（`success_event_stream.rs:270-282`）：

```rust
struct BufferedStreamPrefixState {
    cursor: usize,
    meaningful_output_started_at: Option<Instant>,
    completion_seen: bool,
}
```

- 起点写入：`success_event_stream.rs:362-366` — 仅当
  `usage::has_codex_meaningful_output(&data)` 为真且尚未设置时。
- 判定函数：`src-tauri/src/domain/usage.rs:974-1012`。识别范围：
  - `response.output_text.delta` / `response.refusal.delta` /
    `response.reasoning_summary_text.delta`（`delta` 非空）
  - `response.function_call_arguments.delta`（`delta` 非空）
  - `response.output_text.done` / `response.refusal.done` /
    `response.reasoning_summary_text.done`（`text` 或 `refusal` 非空）
  - `data.item` 是有意义输出项（`is_meaningful_output_item`，`usage.rs:940-972`：
    `function_call` / `function_call_output` / `tool_call` / `tool_result` /
    非空 `output_text` / `text` / `refusal` / 递归 `content` / `summary`）
  - `data.output` 或 `data.response.output` 数组里任一有意义项
- 因此 `response.created`、`response.in_progress`、`: keepalive` 等元数据帧**不启动窗口**，
  与契约 `:56-58` 一致。测试证据：`success_event_stream.rs:2104-2163`
  （`buffered_native_stream_waits_through_preamble_then_commits_at_guard_or_cap`，用
  `tokio::time::advance` 在 499ms → NeedMore、500ms → StartStreaming）。
- 剩余时间：`BufferedStreamPrefixState::guard_remaining`（`success_event_stream.rs:278-282`），
  `guard.saturating_sub(started.elapsed())`。

### 窗口到期的两条判定

1. **同步判定**（已有足够字节时）：`success_event_stream.rs:394-402` —
   `meaningful_output_started_at.is_some_and(|s| s.elapsed() >= config.guard)` →
   `StartStreaming { guard_cap_reached: false }`。
2. **异步等待判定**：`success_event_stream.rs:1354-1369` — 把 guard 剩余时间与
   `upstream_stream_idle_timeout` 取 `min`，并用 `guard <= idle` 标记 `guard_timeout`；
   `tokio::time::timeout` 超时且 `guard_timeout` 为真 → `break`（提交），否则视为
   idle timeout 走 `record_system_failure_and_decide`（`1370-1423`，
   `GW_UPSTREAM_TIMEOUT`）。

另有提前提交分支：`success_event_stream.rs:379-392` — 一旦看到 completion 帧就立刻结算
（`is_empty_success` → `GW_EMPTY_RESPONSE` 的 ProviderFailure，否则 StartStreaming），
不再等 guard。测试：`success_event_stream.rs:2082-2102`。

### 缓冲前缀存放位置与 1 MiB 上限

- 缓冲变量：`let mut buffered_prefix = ...` `success_event_stream.rs:1263-1266`
  （`Vec<u8>`，初值为 first-chunk probe 拿到的 chunk）；追加在
  `success_event_stream.rs:1490` `buffered_prefix.extend_from_slice(chunk.as_ref())`。
- 1 MiB 常量：`success_event_stream.rs:22`
  `const MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES: usize = 1024 * 1024;`
- 上限检查：`success_event_stream.rs:305-314`

  ```rust
  let buffer_cap_reached = if inspect_empty_success {
      raw.len() >= MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES   // 1 MiB，原生 codex 路径
  } else {
      raw.len() > MAX_NON_SSE_BODY_BYTES                   // 20 MiB，非原生路径
  };
  ```

  `MAX_NON_SSE_BODY_BYTES = 20 * 1024 * 1024` 定义在
  `failover_loop/context.rs:17`。
- 命中 1 MiB 上限 → `StartStreaming { guard_cap_reached: true }` →
  `success_event_stream.rs:1307-1325`：`tracing::warn!` +
  `push_special_setting({"type":"stream_internal_error_guard","reason":"buffer_cap_reached",
  "buffered_bytes":...,"cap_bytes":...})`。契约 `:70` 明确 "Buffer-cap release is
  diagnostic and is not a Provider failure"。
- **缓冲不会丢**：三条 break 路径都执行
  `first_chunk = (!buffered_prefix.is_empty()).then(|| Bytes::from(buffered_prefix))`
  （`1327` / `1367` / `1484`），再经 `prepend_and_decode_event_stream`
  （`38-44`）→ `FirstChunkStream`（`streams/relay.rs:32-62`）重放给下游。

### 窗口只对原生 Codex Responses 生效

`inspect_empty_success = is_native_codex_responses_event_stream_path(...)`
（`success_event_stream.rs:299-304`；定义 `209-218`；路径白名单 `201-207`：
`/v1/responses`、`/responses`、`/v1/codex/responses`，且要求
`active_bridge_type.is_none() && !provider_bridged`）。

非原生路径走另一套：`success_event_stream.rs:368-374` —
`is_terminal_error_sse_frame` 命中即
`FinalizeAsEmptyBody(GW_FAKE_200)`，不看关键词、不看 guard。

### 窗口内「丢弃缓冲并重试」的完整路径

`BufferedStreamPrefixDecision::ProviderFailure`（`success_event_stream.rs:262-267` 定义，
`283-306` 分派）→ `record_buffered_provider_failure`（`413-698`）：

1. **缓冲前缀不下发**：`raw` 只被用于
   - `model_route_mapping::observe_model_route_from_bytes`（`431-442`）
   - `upstream_client_error_rules::match_quota_exhausted(raw)`（`505-507`）
   函数没有任何把 `raw` 写入下游 body 的路径；返回值是
   `LoopControl::ContinueRetry` / `BreakRetry`（`685-697`），不是 `Return(Response)`。
2. **error_code**：`GW_FAKE_200`（`success_event_stream.rs:354`，来自终态错误帧）或
   `GW_EMPTY_RESPONSE`（`385`，completion 但 `is_empty_success`）。
3. **决策**：`482-496`
   - evidence `is_retryable()` → `transient_failure_decision(false,
     RetryPolicyMatch::StreamInternalError, policy, configured_transient_retries_used,
     retry_index, provider_max_attempts)`（共享预算，不新增独立预算）
   - 否则 → `(FailoverDecision::SwitchProvider, false)`
   - 预算计数递增 `497-501`
   - 熔断打开时把 `RetrySameProvider` 降级为 `SwitchProvider`（`555-565`）
4. **cooldown**：`567-582`（非 probe、非 oauth 配额耗尽、决策为 Switch/Abort 时触发）
5. **退避**：`685-691` `apply_configured_retry_backoff(decision,
   configured_retry_backoff_delay(retry_policy, configured_retry))` —— 只有
   `RetrySameProvider` 等待一次
6. **evidence disposition 覆写**：`621-627` → `retry_same_provider` /
   `switch_provider` / `retry_exhausted`
7. **attempt 落库**：`629-667`（含 `stream_internal_error: evidence`）

另一条 pre-commit 终结路径（**会提交一个空 body**，注意与上面不同）：
`FinalizeAsEmptyBody` → `finalize_buffered_stream_error_response`（`701-937`）。
它在 `913-936` 用 `Response::builder().status(status)`（**上游原状态**，不是 502）+
`Body::from(Bytes::new())` 返回空体，同时把请求日志记成
`effective_terminal_failure_status`（`58-62`，即 502）+ `GW_FAKE_200`。

## Caveats / Not Found

- guard 的时间基准是 `tokio::time::Instant`（`success_event_stream.rs:20` 引入），
  测试用 `#[tokio::test(start_paused = true)]` + `tokio::time::advance` 控制。
- 未查：`stream_internal_error_guard_ms == 0` 时的实际行为链路（`guard_remaining` 返回
  `Some(0)` → `tokio::time::timeout(Duration::ZERO, ...)` 的语义）。若实现需要依赖
  「0 = 关闭窗口」，需另行验证。
- 相关文件：[[downstream-commit-boundary]]、[[retry-keyword-mechanism]]、[[stream-terminal-origins]]
