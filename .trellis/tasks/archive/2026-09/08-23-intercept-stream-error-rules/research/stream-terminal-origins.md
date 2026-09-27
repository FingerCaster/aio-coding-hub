# Research: GW_STREAM_ERROR 的终止来源与 commit 相对位置

- **Query**: `streams/finalize.rs:41-45` 列出的 5 种终止来源（Unclassified / NormalEof / UpstreamReadError / TerminalFrame / BufferedBodyEof）各自在什么情况下产生；哪些发生在 commit 前、哪些必然在 commit 后
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 先纠正一个容易误读的点

`streams/finalize.rs:29-47` 的 `incomplete_probe_error_code` **不是 GW_STREAM_ERROR 的通用产生处**。
它只在熔断 probe 派发且 `error_code` 尚未确定时被调用：

```
finalize.rs:120-129
  let probe_ownership = ctx.dispatch_ownership.as_ref().filter(|o| o.is_probe());
  ...
  } else if probe_ownership.is_some() && effective_error_code.is_none() {
      effective_error_code = incomplete_probe_error_code(terminal_evidence);
  }
```

即：`:41-45` 那个五路 `match` 表达的是「probe 场景下，这 5 种 origin 都不算可信成功，
一律归为 `GW_STREAM_ERROR`」。普通请求的 `GW_STREAM_ERROR` 由各 tee 直接传入。

### 全部 11 种 origin 的定义

`src-tauri/src/gateway/streams/types.rs:14-26` / `as_str` 在 `29-43`。
可信成功判定在 `types.rs:72-79`（只有 `NormalEof`+completion+normal_eof，或
`CompletionDelivered`+completion+usage，且无 terminal error）。

### 五种来源逐一说明

#### 1. `Unclassified` — 防御性默认值，生产不可达

| 位置 | 说明 |
|---|---|
| `streams/request_end.rs:41-47` | `StreamRequestCompletion::success` 的默认 evidence |
| `streams/request_end.rs:68-74` | `StreamRequestCompletion::failure` 的默认 evidence |

**代码已证实**：`emit_request_event_and_spawn_request_log` 的全部生产调用点都紧跟
`.with_terminal_evidence(...)` 覆盖默认值：

- `usage_tee.rs:596-609`（`UsageSseTeeStream::finalize`）
- `usage_tee.rs:1174-1185`（`UsageBodyBufferTeeStream::finalize`）
- `timing.rs:62-73`（`TimingOnlyTeeStream::finalize`）

其余直接用 `success` / `failure` 的地方全在 `#[cfg(test)]`
（`request_end.rs:523, 540, 598, 618, 649, 705, 785`）。
→ **`Unclassified` 在当前代码里没有生产产生路径**；出现即说明有新增调用点漏了
`with_terminal_evidence`。相对 commit：不适用。

#### 2. `NormalEof` — 上游流自然结束

| 位置 | 触发条件 | error_code |
|---|---|---|
| `usage_tee.rs:385-403` | tee `poll_next_inner` 收到 `Poll::Ready(None)`，且不处于 defer-且-已见终态帧的状态 | `ctx.error_code`（SSE 成功路径恒为 `None`，见 `context.rs:315`） |
| `usage_tee.rs:933-947` | relay 任务：见过终态错误帧 + `upstream_ended_normally` + codex 宽容成立 | `None` |
| `usage_tee.rs:954-967` | 同上但宽容不成立 | `GW_FAKE_200`（`fake_200_detected`）或 `GW_STREAM_ERROR` |

`normal_eof = true`。注意：`error_code = None` 时 `finalize` 还会做
`is_empty_success` 检查（`usage_tee.rs:557-565`）→ 可能变成 `GW_EMPTY_RESPONSE`。

**相对 commit：必然在 commit 后。** 理由：`UsageSseTeeStream` 只在
`success_event_stream.rs:1590-1671` 构造，而那已在缓冲前缀循环 `break` 之后
（详见 [[downstream-commit-boundary]]）。

#### 3. `UpstreamReadError` — 传输层读错误

| 位置 | 触发条件 | error_code |
|---|---|---|
| `usage_tee.rs:492-503` | SSE tee 收到 `Err`，但 `is_codex_stream_tail_error_successish` 成立（codex responses 路径 + 2xx + 有输出 + completion_seen）→ 宽容 | `None` |
| `usage_tee.rs:506-517` | SSE tee 收到 `Err`，宽容不成立 | **`GW_STREAM_ERROR`** |
| `usage_tee.rs:1265-1277` | `UsageBodyBufferTeeStream`（非 SSE 缓冲体）收到 `Err` | `GW_STREAM_ERROR` |
| `timing.rs:144-156` | `TimingOnlyTeeStream`（非 SSE 计时体）收到 `Err` | `GW_STREAM_ERROR` |

宽容判定：`usage_tee.rs:212-224` `is_codex_stream_tail_error_successish`。
**相对 commit：必然在 commit 后**（三个 tee 都在响应构造之后才存在）。

**这是用户案例最可能的 origin。** 依据：codex `/v1/responses` 走 relay 路径
（`use_sse_relay = is_codex_responses_event_stream_path`，`success_event_stream.rs:1569-1572`），
relay 内 `next_item(&mut tee)` → `poll_next` → `poll_next_inner(cx, true, true)`
（`usage_tee.rs:622-625`），`finalize_terminal = true`；无 completion 帧
（8931ms 处断裂）→ 宽容不成立 → `GW_STREAM_ERROR` + `SYSTEM_ERROR`
（类别推导见 [[downstream-commit-boundary]]）。

#### 4. `TerminalFrame` — 流中途出现终态错误 SSE 帧

| 位置 | 触发条件 | error_code |
|---|---|---|
| `usage_tee.rs:449-470` | 非 defer 模式：`tracker.terminal_error_seen()` 首次为真 | `GW_FAKE_200`（若 `fake_200_detected`）否则 `GW_STREAM_ERROR` |
| `usage_tee.rs:937-946` / `957-966` | relay 模式且 `!upstream_ended_normally` | 同上分支 |

非 defer 模式会**截断下游流**：`usage_tee.rs:469` `return Poll::Ready(None)`；
例外是插件错误块 —— `465-468` 先把该 chunk 发出去再停
（`stop_after_terminal_error = true`），这是「commit 后仍向流尾追加内容」的既有先例。

codex responses 路径由于 `with_defer_terminal_error()`（`usage_tee.rs:769-770`）
不走 `449-470`，改由 relay 尾部 `913-969` 统一裁决。
**相对 commit：必然在 commit 后。**

#### 5. `BufferedBodyEof` — 非流式缓冲体正常结束

| 位置 | 触发条件 | error_code |
|---|---|---|
| `usage_tee.rs:1237-1248` | `UsageBodyBufferTeeStream` 收到 `Poll::Ready(None)` | `ctx.error_code` |
| `timing.rs:125-137` | `TimingOnlyTeeStream` 收到 `Poll::Ready(None)` | `ctx.error_code` |

`normal_eof = true`，`completion_seen = false`。**SSE 中继路径永不产生该 origin**
（两个产生点都不是 `UsageSseTeeStream`）。
在 `finalize.rs:68-75` 有专门的 failback 绑定放宽：
`trusted_failback_binding_success` 对 `BufferedBodyEof` 只要求
`normal_eof && !terminal_error_seen`。
**相对 commit：必然在 commit 后**（非流式响应的 commit 点在各自的 `success_non_stream.rs`
路径）。

### 其余 6 种 origin（对照用）

| origin | 产生位置 | 说明 |
|---|---|---|
| `CompletionDelivered` | `usage_tee.rs:1007-1017` | relay 检测到客户端断开，但 completion 已下发且宽容成立（`is_codex_client_abort_successish`） |
| `IdleTimeout` | `usage_tee.rs:366-381` | SSE tee 空闲计时器到期 → `GW_STREAM_IDLE_TIMEOUT`（→524） |
| `TotalTimeout` | `usage_tee.rs:1200-1234`、`timing.rs:88-122` | 非流式总超时 → `GW_UPSTREAM_TIMEOUT`（→524） |
| `ClientAbort` | `usage_tee.rs:1040-1053`（`!relay_drain_timed_out`） | 客户端断开且宽容不成立 → `GW_STREAM_ABORTED`（→499） |
| `RelayDrainTimeout` | `usage_tee.rs:1040-1053`（`relay_drain_timed_out`） | 断开后 drain 窗口到期（窗口计算 `784-805`：codex 上限 15s、无 idle 配置时 10s） |
| `DirectDrop` | `usage_tee.rs:686-708`、`usage_tee.rs:1304-1326`、`timing.rs:168-181` | tee 未 finalize 就被 drop |

### pre-commit 也能产生 GW_STREAM_ERROR（重要，且不经过 streams/finalize.rs）

`GW_STREAM_ERROR` 有三个 **pre-commit** 产生点，全在
`failover_loop/response/success_event_stream.rs`，走
`record_system_failure_and_decide`，类别为 `SYSTEM_ERROR`，可重试/可切换供应商：

| 位置 | 场景 | outcome 前缀 |
|---|---|---|
| `success_event_stream.rs:1099-1150` | first-chunk probe 读错误 | `stream_first_chunk_error` |
| `success_event_stream.rs:1206-1261` | probe 返回空 event-stream | `stream_first_chunk_eof` |
| `success_event_stream.rs:1429-1481` | 缓冲前缀期间读错误 | `stream_prefix_read_error` |

决策来自 `stream_transport_decision(UpstreamTransportRetryKind::Read, ...)`
（`success_event_stream.rs:152-167`）。

→ **同一个 `GW_STREAM_ERROR` 码横跨 pre/post commit 两侧**：pre-commit 侧走 failover
可完整改写；post-commit 侧只能追加流尾事件。规则匹配若按错误码/合成状态码触发，
必须区分这两侧，否则 pre-commit 场景会被错误地降级成「只能追加事件」。

### 汇总表

| origin | 产生模块 | 相对 commit | 典型 error_code |
|---|---|---|---|
| `Unclassified` | 仅默认值 / 测试 | 不适用（生产不可达） | — |
| `NormalEof` | `usage_tee`（SSE tee + relay） | 必然 post-commit | `None` / `GW_FAKE_200` / `GW_STREAM_ERROR` |
| `UpstreamReadError` | `usage_tee`（SSE tee、buffer tee）、`timing` | 必然 post-commit | `None`（宽容）/ `GW_STREAM_ERROR` |
| `TerminalFrame` | `usage_tee`（SSE tee + relay） | 必然 post-commit | `GW_FAKE_200` / `GW_STREAM_ERROR` |
| `BufferedBodyEof` | `usage_tee`（buffer tee）、`timing` | 必然 post-commit | `ctx.error_code`（多为 `None`） |
| （pre-commit 三点） | `success_event_stream` | pre-commit | `GW_STREAM_ERROR` |

## Caveats / Not Found

- **推断**：用户案例的 origin 判定为 `UpstreamReadError`（`usage_tee.rs:506-517`）是基于
  「TTFB 2305ms 有首字节 + 8931ms 断裂 + 无 completion + 最终码 `GW_STREAM_ERROR` +
  类别 `SYSTEM_ERROR`」的推理。若要确证，需读该 trace 请求日志里
  `activity_details_json` 的 `terminal_origin` 字段（写入点
  `streams/types.rs:132-150` `terminal_details_json`，消费点 `request_end.rs:287-299`）。
  `NormalEof` 在无 completion 时通常会因 `error_code = None` 被记为成功或
  `GW_EMPTY_RESPONSE`，与报告不符，故排除。
- 未查：`streams/gunzip.rs` / `plugin_chunk.rs` 是否会把上游读错误包装成别的
  `reqwest::Error`，从而影响 tee 侧看到的是 `Err` 还是 `None`。若实现依赖精确区分
  `NormalEof` 与 `UpstreamReadError`，建议补查。
- 相关文件：[[downstream-commit-boundary]]、[[pre-commit-guard-window]]、[[retry-keyword-mechanism]]

## 附记：R10 的前提与本文件矛盾，已实测判定本文件正确（2026-08-24）

`prd.md` 的 R10 曾称「`usage_tee.rs:549` 不记录 `terminal_origin`，导致约 93% 的流式故障
无法从日志定位终止来源」。该表述**与本文件第 163-164 行矛盾**——本文件明确写出写入点为
`streams/types.rs:132-150` 的 `terminal_details_json`、消费点为 `request_end.rs:287-299`，
即持久化日志本就带 origin。

**实测裁决：本文件正确，R10 的前提为假。** 方法：在端到端测试
`truncated_codex_stream_appends_rule_error_event_instead_of_cutting_off` 中断言
`activity_details_json` 含 `terminal_origin` 与 `upstream_read_error`，
再**临时回退 `usage_tee.rs` 的改动**——断言依然通过。

`usage_tee.rs` 那一行写的是 `touch_activity` 的**待定行**（`status IS NULL AND
error_code IS NULL`，`infra/request_logs.rs:566-596`），随后被最终写入取代，因此它从来
不是持久化 origin 的来源。步骤 6 的改动被相应缩小为「让 pending 行与最终行字段一致」。

**教训**：PRD 转述研究结论时**放大**了缺陷（研究说「若要确证需读该字段」，PRD 变成
「该字段没被写」）。与 `last-outcome-rewrite-loss.md` 第 7 节是同类错误的第二例——
两次都是在「判定某处是缺陷」时未做证伪。
