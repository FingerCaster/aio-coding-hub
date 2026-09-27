# Research: downstream commit 边界与判定位置

- **Query**: 「downstream commit」的准确定义、判定代码位置；响应头何时发给客户端；commit 之后哪些东西不可再改；判定 commit 状态的变量/字段叫什么，能否在 `streams/finalize.rs` 里读到
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 结论速览

| 问题 | 结论 | 性质 |
|---|---|---|
| 是否存在名为 `committed` / `downstream_commit` 的布尔量控制流式提交？ | **不存在** | 代码已证实 |
| commit 的判定点在哪 | `success_event_stream.rs` 的缓冲前缀循环退出（`StartStreaming` / guard 超时 / 上游 EOF）→ 构造并返回 `LoopControl::Return(response)` | 代码已证实 |
| `streams/finalize.rs` 能读到 commit 状态吗 | **不能**。`StreamFinalizeCtx` 没有该字段；但**按构造**只有 commit 决策做完后才会创建 `StreamFinalizeCtx`，所以该模块内的所有终态天然都是「已提交」侧 | 代码已证实（构造点唯一） |
| 唯一被持久化的 pre/post-commit 标记 | `StreamInternalErrorEvidence::disposition`，取值 `"buffered_before_commit"` / `"forwarded_after_commit"` | 代码已证实 |

### Files Found

| File Path | Description |
|---|---|
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs` | commit 决策与响应构造的唯一位置 |
| `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs` | `LoopControl::Return` 上抛路径 |
| `src-tauri/src/gateway/proxy/handler/failover_loop/context.rs` | `LoopControl` 定义、`build_stream_finalize_ctx` |
| `src-tauri/src/gateway/streams/types.rs` | `StreamFinalizeCtx` 字段全集（无 commit 标记） |
| `src-tauri/src/gateway/streams/request_end.rs` | 请求日志/事件的状态码投影（合成 502 的真正来源） |
| `src-tauri/src/gateway/streams/relay.rs` | `FirstChunkStream`：把缓冲前缀重新拼回下游流 |

### commit 的准确定义（代码语义）

仓库里没有 commit 这个显式状态量。**「commit」= 带有流式 `Body` 的 `Response` 被交还给 axum**，
此后 hyper 把状态行 + 响应头写入客户端 socket。三条路径都在 `success_event_stream.rs`
的缓冲前缀循环里 `break`，然后走同一段响应构造：

1. `BufferedStreamPrefixDecision::StartStreaming` → `success_event_stream.rs:1307-1329`
   （含 `guard_cap_reached` 的诊断分支）
2. guard 窗口到期（`Err(_) if guard_timeout`）→ `success_event_stream.rs:1365-1369`
3. 上游在窗口内直接 EOF（`next_chunk` 为 `None`）→ `success_event_stream.rs:1483-1486`

三条路径统一把 `buffered_prefix` 装回 `first_chunk`，然后：

- `success_event_stream.rs:1584-1588` — `prepend_and_decode_event_stream(first_chunk, upstream, ...)`
  把缓冲前缀通过 `FirstChunkStream`（`streams/relay.rs:32-62`）重放到下游，所以
  **缓冲的字节不会丢，只是延迟发送**。
- `success_event_stream.rs:1590-1671` — 组装 body（response_fixer / bridge / plugin /
  `spawn_usage_sse_relay_body` 或 `UsageSseTeeStream`）。
- `success_event_stream.rs:1673-1677` — `Response::builder().status(status)` + 逐个拷贝
  `response_headers` + `x-trace-id`。**`status` 是上游真实状态（本案例 200）**。
- `success_event_stream.rs:1679-1695` — `abort_guard.disarm()` 后 `return LoopControl::Return(...)`。

上抛链：`retry_engine.rs:82-85`（`LoopControl::Return(resp) => { finalize_current_probe(...); return Some(resp); }`）
→ `failover_loop::run` → axum handler 返回。**沿途没有任何环节会再改写这个 200 流式响应**
（`match_response_rule` 的两个调用点都在错误路径，见 `retry-keyword-mechanism.md`）。

### commit 之后不可再改的东西

代码已证实：

1. **HTTP 状态码** — 在 `success_event_stream.rs:1673` 就固化进 `Response`，且值来自上游
   （200）。之后再无写入点。
2. **响应头** — 同上，`1674-1677` 拷贝完成即定型。
3. **已发出的 SSE 字节** — 缓冲前缀在 `1584-1588` 交给 `FirstChunkStream`，后续 chunk 由
   axum 逐个拉取写出。已写出的 chunk 无法撤回或修改。
4. **唯一可行动作 = 往 body 流尾部继续 append 字节**。`usage_tee.rs` 里已有一个先例：
   `usage_tee.rs:465-468`，当终态帧本身是插件错误块时，`stop_after_terminal_error = true`
   并且**仍然把该 chunk 下发**（`return Poll::Ready(Some(Ok(chunk)))`），即「先发再停」。

### 那个 502 到底发给谁了

**代码已证实：502 从未进入客户端 HTTP 响应。** 它只出现在两个投影里：

- 请求日志的 `status`：`streams/request_end.rs:175-177`
  `status_for_stream_request_log(status, error_code) = status_override::effective_status(...)`，
  调用点 `request_end.rs:310`。`GW_STREAM_ERROR → 502` 的映射表在
  `proxy/status_override.rs:11-17`。
- attempt 行的 `status`：`request_end.rs:208-234` 的
  `mark_last_stream_attempt_terminal_failure`，第 218 行同样用
  `status_for_stream_request_log`。

`ctx.status` 本身在 `build_stream_finalize_ctx` 时传入的是上游真实状态
（`success_event_stream.rs:1544-1556` 传 `status.as_u16()`），即 200。

### 错误类别为什么是 SYSTEM_ERROR（与用户报告一致）

`streams/finalize.rs:49-66` `stream_terminal_error_category`：`GW_STREAM_ERROR` 既不是
abort 类也不是 `GW_FAKE_200`/`GW_EMPTY_RESPONSE`，于是落到
`configured_category.or(Some(ErrorCategory::SystemError.as_str()))`；而
`ctx.error_category` 在 `context.rs:314` 恒为 `None`。→ `SYSTEM_ERROR`。

连带后果（代码已证实）：

- `finalize.rs:149-162` — 会触发 provider cooldown（`error_code.is_some()` 且类别非
  ClientAbort）。
- `finalize.rs:256-263` — **不会**记熔断失败（该分支要求类别 == `PROVIDER_ERROR`）。

### commit 状态在 `streams/finalize.rs` 的可见性

- `StreamFinalizeCtx` 字段全集见 `streams/types.rs:153-203`：有 `status`、`error_code`、
  `error_category`、`detect_stream_internal_errors`、`fake_200_detected` 等，**没有任何
  commit / committed / phase 字段**。
- `build_stream_finalize_ctx`（`context.rs:278-355`）在整个仓库只有一个 SSE 调用点：
  `success_event_stream.rs:1544`，位于缓冲前缀循环 `break` **之后**。因此
  「拿到 `StreamFinalizeCtx` ⇒ commit 决策已完成」在当前代码里是构造保证，不是约定。
- 唯一可读的 pre/post-commit 语义标记是 evidence 的 `disposition` 字符串：
  - pre-commit 分类：`success_event_stream.rs:345` 传入 `"buffered_before_commit"`
  - post-commit 分类：`domain/usage.rs:1241` 传入 `"forwarded_after_commit"`
  - 字段定义 `domain/usage.rs:36`，`set_disposition` 在 `usage.rs:45-50`（会被
    `record_buffered_provider_failure` 覆写成 `retry_same_provider` /
    `switch_provider` / `retry_exhausted`，见 `success_event_stream.rs:621-627`）
  - 落库路径：`request_end.rs:267-269` 写入 `last.stream_internal_error`

### 命名线索（供实现取名参考）

- 注释里出现 "commit" 的地方：`success_event_stream.rs:347-349`
  （"Capacity failures must never be committed to Codex"）、
  `success_event_stream.rs:1314`（"buffer cap reached; committing response"）。
- `error_code.rs:2` 的 `precommit:full` 指的是 git 钩子脚本，与本主题无关。
- `session_manager.rs:134/678/837/844` 的 `committed: bool` 是 session 绑定请求的提交状态，
  **与下游响应提交无关**，勿混用。
- `dispatch.rs:192` 注释里的 "committed durably" 指 probe 派发状态落库。

## Caveats / Not Found

- **推断（非代码断言）**：`Response::builder().body(body)` 在
  `success_event_stream.rs:1680-1695` 失败时会走 500 fallback，此时刚建好的 tee body 被
  drop，触发 `Drop for UsageSseTeeStream`（`usage_tee.rs:661-711`）→
  `GW_STREAM_ABORTED` / `DirectDrop`。这是「`StreamFinalizeCtx` 已存在但响应从未提交」的
  唯一窄窗口。属于 builder 错误路径，正常流量不会命中。
- 未查：`spawn_usage_sse_relay_body` 返回的 `Body` 被 axum 丢弃时（客户端连接在写头前就断）
  的精确时序。相关 drop 语义见 `usage_tee.rs:661-711` 与 relay 任务的
  `client_abort_detected_by` 分支（`usage_tee.rs:971-1055`）。
- 相关文件：[[pre-commit-guard-window]]、[[stream-terminal-origins]]、[[retry-keyword-mechanism]]
