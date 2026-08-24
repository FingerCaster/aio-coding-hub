# Research: 流尾追加自定义 SSE 事件的注入点与可复用机制

- **Query**: 要在流末尾追加自定义 SSE 事件，技术上的注入点在哪？现有代码里有没有类似能力可复用（response_fixer、fake200、usage_tee 的事件改写/补发）？
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 结论速览

仓库里**已有两处成熟的「合成 SSE error 帧并塞进下游流」实现**可直接复用其模式：

1. `plugin_chunk.rs:121-127` `plugin_stream_error_chunk` —— 带 marker 的注入 + tee 侧配合停流。
2. `protocol_bridge/stream.rs:167-212` 的三个 `terminate_*` —— 入队 error 帧 + `terminated` 标志 → 下一次 poll 返回 `None`（干净 EOF）。

两者都不带「规则文案」能力，但注入的**机制**完全可搬。

### 可复用机制清单（全部已证实）

#### M1. `plugin_stream_error_chunk` —— 最贴近本任务需求

`src-tauri/src/gateway/streams/plugin_chunk.rs:121-127`：

```rust
fn plugin_stream_error_chunk(error: &str, reason: &str) -> Bytes {
    Bytes::from(format!(
        "{PLUGIN_STREAM_ERROR_MARKER}event: error\ndata: {{\"error\":\"{error}\",\"reason\":{}}}\n\n",
        serde_json::to_string(reason)
            .unwrap_or_else(|_| "\"Plugin stream hook failed\"".to_string())
    ))
}
```

- marker 常量：`plugin_chunk.rs:15` `PLUGIN_STREAM_ERROR_MARKER: &str = ": aio-plugin-error\n"`
  （SSE 注释行，客户端会忽略，只用于网关内部识别）。
- tee 侧配合：`usage_tee.rs:239-243` `is_plugin_stream_error_chunk` 用滑动窗口匹配 marker；
  `usage_tee.rs:465-468` 在识别出该 chunk 时 `stop_after_terminal_error = true` 并**先把 chunk 吐给客户端**，
  下一次 poll 由 `usage_tee.rs:356-358` 返回 `Poll::Ready(None)`。
- 端到端已被测试覆盖：`routes.rs:7233-7241` 断言客户端 body 含 `event: error` 与 `plugin_blocked`。
- 单元覆盖：`usage_tee.rs:1547-1556`（`plugin_stream_error_chunk_is_still_detected_without_rewriting_marker`）。

**这是「注入一帧 + 干净收尾 + 客户端确实收到」在本仓库的完整先例。**

#### M2. `BridgeStream::terminate_*` —— 队列 + terminated 标志模式

`src-tauri/src/gateway/proxy/protocol_bridge/stream.rs`：

- 静态字节常量：`:17-21` `BRIDGE_SSE_FRAME_TOO_LARGE`
  ```
  event: error\n
  data: {"type":"error","error":{"type":"invalid_request_error","message":"bridge_sse_frame_too_large"}}\n\n
  ```
- `terminate_oversized_frame` `:167-176`、`terminate_registry_miss` `:178-194`、
  `terminate_translation_error` `:196-212`：统一做法是
  `line_buf.clear()` → `buffer.push_back(frame)` → `terminated = true`。
- `poll_next` `:288-296`：先吐 `buffer` 里的帧，`buffer` 空且 `terminated` 时返回 `Poll::Ready(None)`。
- 测试：`:339-363`、`:365-391` 断言先收到 error 帧、随后 `Poll::Ready(None)`。

**这是「不产生 `Err`、以合法 EOF 结束」的干净收尾模式**，正是本任务需要的（避免 chunked 截断）。

#### M3. `sse_frame` 帧序列化 helper

`src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:355-358`：

```rust
fn sse_frame(event_type: &str, payload: Value) -> Bytes {
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_type}\ndata: {data}\n\n"))
}
```

私有于 anthropic inbound 模块，需要复制或上提为共享 helper（建议放 `gateway/proxy/sse.rs`，
那里已有 `find_sse_event_end` / `parse_sse_frame`，见 `proxy/sse.rs:7-66`）。

#### M4. `queued: VecDeque<Bytes>` + `pending_error` 模式

`src-tauri/src/gateway/response_fixer/stream.rs`：

- 字段 `:214-215` `queued: VecDeque<Bytes>` / `pending_error: Option<reqwest::Error>`；
- `poll_next` `:315-322` 先吐 `queued`，再吐 `pending_error`；
- 上游出错时 `:346-361` 先把缓冲区排空成一个 `Ok` chunk 入队，**再**把 `Err` 存进 `pending_error`。

`GeminiOAuthSseStream` 用同一套字段（`gemini_oauth.rs:145-146`）。
这说明「先补发若干 `Ok` 帧、然后才终止」在本仓库是既有惯例。
本任务只需把「然后吐 `Err`」改成「然后吐 `None`」。

#### M5. `response_fixer::push_special_setting` —— 审计元数据通道

改写审计写入的既有通道（R8 要求的有界元数据）：

- `usage_tee.rs:1019-1039`：client_abort 场景 push 结构化 JSON。
- `success_event_stream.rs:1316-1324`：`stream_internal_error_guard` buffer-cap 诊断。
- `upstream_error_response_rules.rs:70-83` `UpstreamErrorResponseRewrite::special_setting()`
  已经产出规则审计对象（`type: "upstream_error_response_rule"`），可直接沿用同一形状。

**关键时序约束**：`special_settings` 在 finalize 内被序列化进请求日志
（`streams/request_end.rs:309` `response_fixer::special_settings_json(&ctx.special_settings)`）。
所以审计 push 必须发生在 `tee.finalize()` **之前**，否则元数据不会进日志。

#### M6. `fake200` / pre-commit 拦截（对比参考，不可复用于已 commit 场景）

- `success_event_stream.rs:294-410` `inspect_buffered_event_stream_prefix`：pre-commit 缓冲窗口内检查。
- `success_event_stream.rs:701-937` `finalize_buffered_stream_error_response`：
  **保留原状态码（200）但把 body 换成空** —— 注意它做的是「整体替换 body」，靠的是还没发头。
- `success_event_stream.rs:1330-1350` 是它的调用点。

这条路径证明了 pre-commit 可以完整改写；**已 commit 后不可用**（前缀已发出）。

### 注入点评估

下面 4 个候选点，按可行性排序。

#### P1（推荐主方案）：`UsageSseTeeStream::poll_next_inner` 的 `Err` 分支

位置：`src-tauri/src/gateway/streams/usage_tee.rs:474-519`。

改法：在 `finalize(Some(GW_STREAM_ERROR), ...)` **之前** push 审计 special setting，
然后不返回 `Poll::Ready(Some(Err(err)))`，而是返回合成的 `Ok(frame)`，
并置一个类似 `stop_after_terminal_error`（`usage_tee.rs:301`、`356-358`）的标志让下一次 poll 返回 `None`。

优点：

- **一处改动同时覆盖 relay 分支和非 relay 分支**（两者都以此 tee 为核心）。
- 天然位于 `SseUsageTracker` 之后 —— 注入的字节**不会**被 `tracker.ingest_chunk` 吃到，
  不会污染 `terminal_error_seen` / `fake_200_detected`（见 [[logging-stats-impact]]）。
- 可以精确控制 finalize 与 audit push 的先后顺序。
- 可同样覆盖 idle-timeout 分支（`usage_tee.rs:363-383`），把静默截断也变成可读错误。

需解决的问题：

- `poll_next_inner` 的返回类型是 `Poll<Option<Result<B, reqwest::Error>>>`，
  `B: AsRef<[u8]>` 是泛型（`usage_tee.rs:286-302`、`355`）。无法从字节直接构造 `B`。
  → 要么给 `UsageSseTeeStream` 加 `B: From<Bytes>` 之类的 bound（所有真实调用点 `B = Bytes`），
  要么把注入逻辑限定在一个 `Bytes` 特化的包装层。
- `UpstreamErrorResponseRewrite` 与 `match_response_rule` 的可见性是 `pub(super)`，
  且 `mod upstream_error_response_rules;` 在 `proxy/mod.rs:30` 是**私有模块**。
  `crate::gateway::streams` 访问不到，需要放宽为 `pub(in crate::gateway)`。
- `StreamFinalizeCtx`（`streams/types.rs:153-203`）目前没有规则字段，需新增
  （构造点 `failover_loop/context.rs:278-355`）。
- `CommonCtxOwned`（`failover_loop/context.rs:157-187`）**不携带** `upstream_error_response_rules`；
  但 `handle_success_event_stream` 的借用参数 `ctx: CommonCtx<'_, R>` 有
  （`failover_loop/context.rs:55`），在 `success_event_stream.rs:1544` 调用
  `build_stream_finalize_ctx` 时可以取到。

#### P2（codex-only 备选）：relay 任务的 `Err` 分支

位置：`src-tauri/src/gateway/streams/usage_tee.rs:894-903`。

改法：把 `tx.send(Err(err))` 换成 `tx.send(Ok(合成帧))`，然后 `break`（tx drop → 下游干净 EOF）。

优点：类型明确（`DownstreamRelayItem.item: Result<Bytes, reqwest::Error>`，`usage_tee.rs:715-718`），无泛型问题。

缺点：

- 只覆盖 codex `/v1/responses`（`use_sse_relay` 为 true 的分支），
  claude / gemini / grok / codex chat-completions 走不到。
- **finalize 早于此处已经执行完**（在 `poll_next_inner` 内，`usage_tee.rs:506-517`），
  所以审计 special setting 与规则匹配结果无法进入请求日志，除非提前决定并塞进 ctx。
- 注意 `completion_seen` 必须传 `false`，否则 `DownstreamRelayBodyStream::poll_next`
  （`usage_tee.rs:743-745`）会误置 `completion_delivered`，进而影响
  `is_codex_client_abort_successish`（`usage_tee.rs:158-176`）的判定。

#### P3：在 tee 之外新增一层「尾部改写」Stream

位置：非 relay 分支包在 `success_event_stream.rs:1625-1631` / `1663-1669` 的 tee 外面；
relay 分支包在 `usage_tee.rs:1058-1061` 的 `DownstreamRelayBodyStream` 外面。

优点：改动隔离，`Bytes` 类型明确，可写成独立可测的小 Stream（照抄 M2 模式）。

缺点：位于 finalize **之后**，无法参与 finalize 期的日志/审计。
若采用，必须把规则匹配结果在 commit 时就算好并透传（用 `Arc<Mutex<...>>` 之类共享给 finalize 侧），
时序更绕。

#### P4：`Drop` 实现 —— 不可行

`usage_tee.rs:661-711`。`Drop` 触发时下游 body 已经被消费方丢弃或流已结束，无处可写字节。仅能做日志兜底。

### 追加事件后必须保持的既有不变量

| 不变量 | 代码位置 | 风险 |
|---|---|---|
| 注入字节不得被 `SseUsageTracker` 摄入 | `usage_tee.rs:426` `self.tracker.ingest_chunk(chunk.as_ref())` | 若在 tracker 上游注入，`event: error` 会置 `terminal_error_seen`+`fake_200_detected`（`usage.rs:1259-1269`），错误码从 `GW_STREAM_ERROR` 变成 `GW_FAKE_200` |
| 不得置 `completion_delivered` | `usage_tee.rs:743-745` | 影响探针成功判定 |
| 已发出的前缀事件逐字节不变 | `success_event_stream.rs:1673-1695` 之后不可回退 | 只能追加，不能改写 |
| `plugin` marker 语义不冲突 | `plugin_chunk.rs:15`、`usage_tee.rs:239-243` | 新 marker 请另取常量，不要复用 `: aio-plugin-error\n` |
| `stop_after_terminal_error` 的既有语义 | `usage_tee.rs:301`、`356-358`、`466` | 若复用该字段，需确认不影响 plugin 路径 |

### Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md`
  第 3 节明确「HTTP 200 stream errors and transport errors never enter rewrite matching」，
  本任务 R1 要改这条契约，需同步更新（第 3 节 + 第 4 节矩阵行
  `Transport failure or HTTP 200 SSE error | Never evaluate final HTTP rewrite rules`）。

## Caveats / Not Found

- 仓库中**没有**任何「在流末尾按配置文案追加事件」的现成能力，只有上述硬编码文案的注入先例。
- `match_response_rule`（`upstream_error_response_rules.rs:170-241`）在 `body = None` 时，
  对配置了 keyword 的规则返回 `ConditionResult::Unknown` → 整体 `None`（`:194-198`、`:326-333`）。
  传输层断裂没有 body，因此**只有纯 status_codes 规则能命中**；混了关键词的规则会静默不生效。
  这一点 PRD 未提及，属实现前必须决策的点（是否给流式路径传一个合成 body 供关键词匹配）。
- 上述 P1 的泛型 bound 方案未经编译验证，属**推断**。
