# Research: 上游 SSE 流转发给客户端的完整数据通路

- **Query**: 从 reqwest 响应体到 axum Body 的每一层包装（tee、relay、drain 等），代码位置与结构；流是如何被「切断」的
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### Files Found

| File Path | Description |
|---|---|
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs` | 流式成功路径唯一入口，装配整条包装链并构建 `Response` |
| `src-tauri/src/gateway/streams/usage_tee.rs` | `UsageSseTeeStream`（统计 tee）+ `spawn_usage_sse_relay_body`（codex 专用 relay）+ `DownstreamRelayBodyStream` |
| `src-tauri/src/gateway/streams/relay.rs` | `FirstChunkStream`（把预读的首块重新贴回流头）、测试用 `RelayBodyStream` |
| `src-tauri/src/gateway/streams/gunzip.rs` | `GunzipStream`，gzip 解码层 |
| `src-tauri/src/gateway/streams/plugin_chunk.rs` | `MaybePluginChunkStream`，插件 chunk hook 层；含**唯一已存在的「向流内注入 error 事件」实现** |
| `src-tauri/src/gateway/response_fixer/stream.rs` | `ResponseFixerStreamInner`，按行缓冲 + 编码/SSE/JSON 修复层 |
| `src-tauri/src/gateway/proxy/protocol_bridge/stream.rs` | `BridgeStream`，协议桥（cx2cc）翻译层；含第二处「注入 error 帧 + 干净 EOF」实现 |
| `src-tauri/src/gateway/proxy/gemini_oauth.rs` | `GeminiOAuthSseStream`，Gemini OAuth SSE 改写层 |
| `src-tauri/src/gateway/streams/types.rs` | `StreamFinalizeCtx` / `StreamTerminalOrigin` / `StreamTerminalEvidence` |
| `src-tauri/src/gateway/streams/request_end.rs` | `emit_request_event_and_spawn_request_log`，终态请求日志投影 |
| `src-tauri/src/gateway/streams/finalize.rs` | 熔断/会话终态收敛。**已确认：无任何向流注入事件的能力**（全文仅 `finalize_circuit_and_session` 一个 pub 出口，见 `finalize.rs:115`） |

### 完整包装链（已证实，自内向外）

装配位置：`success_event_stream.rs:1584-1671`。

```
reqwest::Response::bytes_stream()                       success_event_stream.rs:1058
  └─ [可选] GunzipStream                                 success_event_stream.rs:27-36 (decode_event_stream)
       └─ FirstChunkStream(预读的 buffered_prefix, rest)  success_event_stream.rs:38-44 / relay.rs:32-62
            └─ [可选] GunzipStream（legacy 路径二次解码）   success_event_stream.rs:1584-1588
                 └─ GeminiOAuthSseStream                 success_event_stream.rs:1592 / 1635
                      └─ UpstreamModelObserverStream     success_event_stream.rs:1593-1599 / 1636-1642
                           └─ BridgeStream               success_event_stream.rs:1600-1605 / 1643-1648
                                └─ [仅 fixer 开启] ResponseFixerStream  success_event_stream.rs:1606-1610
                                     └─ MaybePluginChunkStream          success_event_stream.rs:1611-1616 / 1649-1654
                                          ├─ use_sse_relay=true  → spawn_usage_sse_relay_body(...)  usage_tee.rs:754
                                          └─ use_sse_relay=false → Body::from_stream(UsageSseTeeStream::new(...))
```

关键分支开关：

- `use_sse_relay = is_codex_responses_event_stream_path(cli_key, path)`，`success_event_stream.rs:1569-1572`；
  helper 在 `success_event_stream.rs:201-207`：仅 `cli_key == "codex"` 且 path ∈
  `{/v1/responses, /responses, /v1/codex/responses}`（去尾斜杠后）。
- **本任务的目标场景（codex CLI + POST /v1/responses）走的是 relay 分支**。
  claude / gemini / grok 以及 codex 的 `/v1/chat/completions` 走 `UsageSseTeeStream` 直接 `Body::from_stream`。
- `enable_response_fixer_for_this_response`（`success_event_stream.rs:1558-1559`）只决定链中是否插入
  `ResponseFixerStream`，两个分支的尾部（relay vs tee）结构相同。

### relay 分支内部结构（已证实）

`spawn_usage_sse_relay_body`（`usage_tee.rs:754-1062`）：

1. 建 `tokio::sync::mpsc::channel::<DownstreamRelayItem>(32)`（容量常量 `usage_tee.rs:713`）。
2. `UsageSseTeeStream::new(upstream, ctx, idle_timeout, initial_first_byte_ms).with_defer_terminal_error()`
   （`usage_tee.rs:769-770`）。`defer_terminal_error` 让 tee 在看到终态 error 帧时**不**立即 finalize，
   把最终 error_code 判定交给 relay 任务（`usage_tee.rs:345-348`、`386-402`、`449-471`）。
3. `tokio::spawn` 一个转发任务（`usage_tee.rs:772`），主循环 `tokio::select! { biased; tx.closed() => ..., next_item(&mut tee) => ... }`
   （`usage_tee.rs:849-906`）。`tx.closed()` 用来提前感知客户端断开，避免误记 `GW_STREAM_ABORTED`。
4. 客户端断开后对 codex 进入 drain 窗口（`usage_tee.rs:807-847`），drain grace：codex 上限 15s / 无 idle_timeout 时 10s，
   其他 5s / 2s（`usage_tee.rs:784-805`）。
5. 返回体：`Body::from_stream(DownstreamRelayBodyStream::new(rx, completion_delivered))`（`usage_tee.rs:1058-1061`）。
   `DownstreamRelayBodyStream::poll_next`（`usage_tee.rs:740-751`）把 `DownstreamRelayItem.item` 原样吐出，
   并在 `item.completion_seen && item.item.is_ok()` 时置 `completion_delivered`。

### 提交（commit）点（已证实）

`success_event_stream.rs:1673-1695`：构建 `Response::builder().status(status)` + 复制 header + `x-trace-id`，
`abort_guard.disarm()` 在 `1679`，`LoopControl::Return(...)` 在 `1680`。

**这一行之后，HTTP 状态码与响应头已不可改**。PRD 里「已 commit 只能追加事件」的物理约束由此得到代码确认。

提交前还有一段可回退的窗口：`success_event_stream.rs:1263-1491` 的 buffered prefix 循环。
它把前缀累积在内存里（上限 1 MiB，`MAX_STREAM_INTERNAL_ERROR_GUARD_BYTES` = `success_event_stream.rs:22`），
在这段窗口内可以整体丢弃并改走 provider failure / 空体终态
（`record_buffered_provider_failure` `:413`、`finalize_buffered_stream_error_response` `:701`）。

### 流「被切断」的确切机制（代码路径已证实；下游可见现象为推断）

以本案例（上游 HTTP 200，流传输中途物理断裂）为例：

1. reqwest 层产生 `Poll::Ready(Some(Err(reqwest::Error)))`。
2. 该 `Err` 逐层原样上抛：`GunzipStream` / `UpstreamModelObserverStream`（`usage_tee.rs:124-127`，
   顺带 finalize 路由观测）/ `BridgeStream`（`protocol_bridge/stream.rs:304`）/
   `ResponseFixerStream`（`response_fixer/stream.rs:346-361`，先把缓冲区排空成一个 `Ok` chunk，再吐 `Err`）/
   `MaybePluginChunkStream`（`plugin_chunk.rs:167`）。
3. `UsageSseTeeStream::poll_next_inner` 命中 `Poll::Ready(Some(Err(err)))` 分支（**usage_tee.rs:474-519**）：
   - 先算 `is_codex_stream_tail_error_successish`（`usage_tee.rs:212-224`，要求 `usage_seen || completion_seen`）；
   - 中途断裂时 `completion_seen == false` → 不成立 → 走 `usage_tee.rs:506-517`：
     `finalize(Some(GatewayErrorCode::StreamError.as_str()), evidence{origin: UpstreamReadError, ...})`，
     **请求日志在这一刻就已落地为 `GW_STREAM_ERROR`**；
   - 然后返回 `Poll::Ready(Some(Err(err)))`（`usage_tee.rs:518`）。
4. relay 任务收到 `Err`：`usage_tee.rs:894-903` —— 注释写着「尽力把流错误透传给客户端」，
   `tx.send(DownstreamRelayItem { item: Err(err), completion_seen: false })` 后 `break`。
5. `DownstreamRelayBodyStream::poll_next` 吐出 `Poll::Ready(Some(Err(err)))`（`usage_tee.rs:746`）。
6. `axum::body::Body::from_stream` 把该 `Err` 转成 body 错误。

**推断（无仓库测试覆盖）**：axum 0.7（`src-tauri/Cargo.toml:39`）的 `Body::from_stream` 在底层流出错时
让 hyper 以「异常终止」结束响应体。对 HTTP/1.1 chunked 响应意味着**不发送终止的 `0\r\n\r\n` 块**，
连接被中止。codex CLI 侧看到的是传输层/协议层错误（incomplete message / connection reset），
而不是一个合法的 SSE 事件——这正是用户观察到的「流被硬切断」。

已在仓库中确认**没有**任何 e2e 测试断言这条路径下客户端收到的字节
（`grep GW_STREAM_ERROR` 在 `src-tauri/src/` 只命中 `error_code.rs:63/112` 与 `usage_tee.rs:912` 注释）。
实现本任务时需要新增该覆盖。

### 其它终止分支（与本任务相关）

| 终止方式 | 代码位置 | 下游看到什么 | 是否 finalize |
|---|---|---|---|
| 上游正常 EOF | `usage_tee.rs:385-404` | 干净的流结束 | 是（`ctx.error_code`，通常 `None`） |
| 上游读错误 | `usage_tee.rs:474-519` | **`Err` → 硬切断** | 是（`GW_STREAM_ERROR`） |
| idle 超时 | `usage_tee.rs:363-383` | `Poll::Ready(None)` → **干净 EOF 但无任何 error 事件**（静默截断） | 是（`GW_STREAM_IDLE_TIMEOUT`） |
| 终态 error 帧（非 defer） | `usage_tee.rs:436-471` | 上游原始 error 帧被转发（仅 plugin marker 情况）或流直接结束 | 是（`GW_FAKE_200` / `GW_STREAM_ERROR`） |
| 客户端断开 | `usage_tee.rs:854-862`、`971-1055` | 无（下游已走） | 是（`GW_STREAM_ABORTED` 或成功） |
| Drop 兜底 | `usage_tee.rs:661-711` | 无 | 是（`GW_STREAM_ABORTED` 或成功） |

注意 idle-timeout 分支同样是「客户端看不到错误」的场景，且它连 `Err` 都没有——
如果本任务只处理 `Err` 分支，`GW_STREAM_IDLE_TIMEOUT`（→524）配置的规则不会生效。

## Caveats / Not Found

- axum/hyper 对 body 错误的线上表现是**推断**，需在实现阶段用 e2e 测试固定下来。
- `UsageSseTeeStream<S, B, R>` 的 `B` 是泛型 `AsRef<[u8]>`（`usage_tee.rs:286-302`）。
  虽然全部真实调用点都是 `Bytes`，但泛型形态导致 tee 内部无法直接构造注入用的字节块，
  需要额外 trait bound（如 `B: From<Bytes>`）或把注入放到 relay/外层。详见 [[tail-injection-points]]。
- `finalize.rs` 与 `types.rs` 都不涉及字节流，注入能力必须新建，符合 team-lead 的前置判断。
