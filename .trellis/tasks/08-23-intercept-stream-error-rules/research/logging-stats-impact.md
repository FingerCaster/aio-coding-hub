# Research: 追加 error 事件后，请求日志与统计需要同步做什么

- **Query**: 追加事件后，请求日志/统计（request_end、usage 统计）需要同步做什么，避免把改写后的流误记为成功？
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 好消息：默认就不会被误记为成功（已证实）

在目标场景（上游 200、流中途断裂、无 `response.completed`）下，
finalize 已经在**注入之前**就把请求判为失败：

`usage_tee.rs:474-519` 的 `Err` 分支：

```
completion_seen = tracker.completion_seen()            // false（中途断裂）
codex_successish = is_codex_stream_tail_error_successish(..., completion_seen, completion_seen)
                 // usage_tee.rs:212-224：要求 usage_seen || completion_seen → false
→ finalize(Some(GatewayErrorCode::StreamError.as_str()),
           StreamTerminalEvidence::new(UpstreamReadError, false, false, false, terminal_error_seen))
```

链路：`finalize`（`usage_tee.rs:524-610`）→
`emit_request_event_and_spawn_request_log`（`streams/request_end.rs:237-346`）→

- `status_for_stream_request_log(ctx.status=200, Some("GW_STREAM_ERROR"))`
  （`request_end.rs:175-177` → `status_override::effective_status`）→ **502**（`status_override.rs:11-17`）；
- `mark_last_stream_attempt_terminal_failure`（`request_end.rs:208-235`）改写最后一个 attempt：
  `outcome = "stream_error: code=GW_STREAM_ERROR"`、`decision = "abort"`、
  `error_category = SYSTEM_ERROR`（`stream_error_category` `:179-193` 默认分支）；
- `active_request_finish_reason`（`:195-206`）→ `Failed`；
- `terminal_signal = Some("error")`（`:245-247`）。

**所以「不被记为成功」不需要额外工作 —— 需要的是「不要意外把它变成成功或变成另一个错误码」。**

### 真正的风险：注入位置错误会污染 tracker（已证实的机制，后果为推断）

`UsageSseTeeStream::poll_next_inner` 的 `Ok(chunk)` 分支在
`usage_tee.rs:426` 调用 `self.tracker.ingest_chunk(chunk.as_ref())`。

如果注入的字节从 tee 的**上游**进入（例如放在 `MaybePluginChunkStream` 或
`ResponseFixerStream` 层），tracker 会摄入它，触发 `usage.rs:1229-1300` 的分类：

| 注入内容 | tracker 反应 | 代码位置 | 后果 |
|---|---|---|---|
| `event: error` + `data.error` 存在 | `terminal_error_seen = true`；`fake_200_detected = true` | `usage.rs:1259-1268` | 错误码从 `GW_STREAM_ERROR` 变成 `GW_FAKE_200`（`usage_tee.rs:449-454` 或 `:949-953`） |
| `data.type == "error"` | 同上 | `usage.rs:1271-1281` | 同上 |
| `response.failed` + `status: "failed"` | `terminal_error_seen = true` | `usage.rs:1284-1300` `is_terminal_error_status` | 同上（`fake_200` 视是否有 `error` 对象） |
| 且 `detect_stream_internal_errors` 开启 | `stream_terminal_type` 命中 → `terminal_error_seen` + `fake_200_detected` | `usage.rs:1245-1250`、`:94-99` | 同上，且写入 `stream_internal_error` 证据 |

两者都映射到 502（`status_override.rs:11-17` 同时列出 `StreamError` 与 `Fake200`），
所以客户端可见状态码不变。但：

- **`error_code` 会从 `GW_STREAM_ERROR` 变成 `GW_FAKE_200`** —— 破坏 R7「不破坏既有 `GW_FAKE_200` 行为」，
  也让用户配置的规则语义混乱（本来是传输断裂，日志上变成上游发了假 200）。
- relay 分支还会额外走 `usage_tee.rs:913-969` 那段「terminal_error_seen 容错」逻辑，
  在 `usage_seen || completion_seen` 成立时**把请求判为成功**（`usage_tee.rs:933-947`）。
  中途断裂时 completion_seen=false 所以不会误判，但**若断裂发生在 `response.completed` 之后**
  （tail error 场景），注入会让本来成功的请求走进一条不同的分支——需要显式验证。
- `push_special_setting` 之外还会污染 `stream_internal_error` 证据
  （`usage_tee.rs:608` `with_stream_internal_error(self.tracker.stream_internal_error_evidence().cloned())`）。

**规避方式**：注入必须发生在 `tracker.ingest_chunk` **之后**，即
`UsageSseTeeStream` 内部或其下游（[[tail-injection-points]] 的 P1 / P2 / P3 都满足）。

### 时序约束：审计元数据必须在 finalize 之前 push（已证实）

`streams/request_end.rs:309`：

```rust
response_fixer::special_settings_json(&ctx.special_settings),
```

在 `emit_request_event_and_spawn_request_log` 内部同步读取。
而该函数由 `finalize`（`usage_tee.rs:596`）同步调用。

因此：

- 规则匹配结果的审计对象（`UpstreamErrorResponseRewrite::special_setting()`，
  `upstream_error_response_rules.rs:70-83`）必须在 `finalize()` 调用**前** push；
- 参考既有做法：`usage_tee.rs:1019-1039`（client_abort）就是先 `push_special_setting` 再 `tee.finalize(...)`。

这直接排除了 [[tail-injection-points]] 里 P2（relay `Err` 分支）作为**唯一**注入点的方案 ——
到那里时 finalize 已经跑完了。

### usage 统计的影响（已证实）

`finalize`（`usage_tee.rs:534`）先 `let usage = self.tracker.finalize();`：

- 中途断裂时 `response.completed`（携带 usage）没到 → `usage` 为 `None`；
- `terminal_evidence.usage_seen |= usage.is_some()` → 保持 false（`usage_tee.rs:536`）；
- `usage_metrics` 为 `None`（`:566`）→ 请求日志无 token 计费数据。

`effective_error_code` 的空成功检查（`usage_tee.rs:557-565`）：
`error_code` 已是 `Some(GW_STREAM_ERROR)` → 不会被改写成 `GW_EMPTY_RESPONSE`。

**追加事件不改变以上任何一项**（因为注入的字节不进 tracker）。
如果后续想让注入帧携带部分 usage，需另行设计 —— 但那会与「不记为成功」冲突，不建议。

### 必须保持的其它不变量（已证实）

| 不变量 | 代码位置 | 说明 |
|---|---|---|
| attempt 状态保留上游真实状态 | `request_end.rs:218` `status_for_stream_request_log(upstream_status, ...)` | ⚠️ 注意：这里**已经**把 attempt.status 覆写成了合成的 502（`:221`），并非上游真实的 200。R8「attempt 状态继续保留上游真实状态」与现状**不一致**，需澄清 |
| 熔断/探针收敛 | `streams/finalize.rs:115` `finalize_circuit_and_session`，由 `request_end.rs:242-243` 调用 | 它可能**改写** `completion.error_code`（`:244`）；追加事件不应影响它 |
| `completion_delivered` 不被误置 | `usage_tee.rs:743-745` | 注入 `DownstreamRelayItem` 时 `completion_seen` 必须为 `false` |
| `active_requests` 终态 | `request_end.rs:325-328` | 由 `error_code` 决定，注入不改 |
| `ctx.observe == false` 时不落日志 | `request_end.rs:249-251` | 注入仍应发生（客户端体验），只是不记日志 |
| 不持久化响应体 / 原始 SSE / 关键词 / 凭据 | 契约第 3 节 | 审计对象只用 `special_setting()` 的有界字段；`message` 已被 `truncate_chars` 限长（`upstream_error_response_rules.rs:220`、`:421-423`） |

### 需要澄清的矛盾点（发现）

**R8 说「attempt 状态继续保留上游真实状态」，但现状代码不是这样。**

`streams/request_end.rs:208-235` `mark_last_stream_attempt_terminal_failure`：

```rust
let effective_status = status_for_stream_request_log(upstream_status, Some(error_code));  // :218
attempt.status = Some(effective_status);                                                  // :221
```

传入的 `upstream_status` 是 `ctx.status`（= 200），但 `status_for_stream_request_log`
把它经 `status_override` 映射成 502，再写回 `attempt.status`。
即**流式路径的 attempt.status 本来就是合成值 502**，不是上游真实的 200。

对照非流式路径 `upstream_error_response_rules` 的契约要求
「Request-log status is client-visible; attempt status remains the real upstream status」
（契约第 3 节）——流式路径与该契约本来就不一致。

这不是本任务引入的问题，但 R8 的验收条目会撞上它。实现前需要 team-lead 决策：
（a）承认流式路径的既有差异并在契约里写明；或（b）顺手修正 `attempt.status` 为 200
（属行为变更，会影响现有测试）。

### 建议的日志/审计增量（推断）

追加事件时建议 push 的 special setting 形状（复用 `special_setting()` 并加流式标记）：

```json
{
  "type": "upstream_error_response_rule",
  "scope": "stream_tail",
  "ruleId": "...", "ruleName": "...",
  "providerId": 0, "providerName": "...",
  "upstreamStatus": 200,
  "clientStatus": 200,
  "matchedStatus": 502,
  "statusMode": "stream_tail_append",
  "messageMode": "override",
  "streamErrorCode": "GW_STREAM_ERROR"
}
```

- `scope` 从 `"response"`（`upstream_error_response_rules.rs:73`）改成 `"stream_tail"`，
  与 `client_abort` 用 `"scope": "stream"`（`usage_tee.rs:1023`）的惯例一致；
- `clientStatus` 必须是 200（真实下发值），`matchedStatus` 记录用于匹配的合成码 —— 
  这两个字段的区分是 R5「两条路径语义差异必须写明」在日志层的落地；
- **不要**放入 message 文案本身以外的任何 body/SSE 内容。

## Caveats / Not Found

- **未找到**：任何断言「流中途断裂 → 客户端收到的字节」的 e2e 测试
  （`grep GW_STREAM_ERROR src-tauri/src/` 只命中 `error_code.rs:63/112` 与 `usage_tee.rs:912` 注释）。
  新增覆盖时可复用 `usage_tee.rs:1615-1623` 的 `reqwest::Error` 构造技巧：
  ```rust
  let read_error = reqwest::Client::new().get("://invalid-url").build()
      .expect_err("invalid URL should produce a reqwest error");
  ```
  以及 `RelayBodyStream::new(rx)`（`streams/relay.rs:9-30`，仅 `#[cfg(test)]`）驱动 tee。
- R8 与 `request_end.rs:218-221` 现状的矛盾是**新发现**，PRD 未提及，需决策。
- 建议的 special setting 字段名为推断，需与前端日志展示（`gateway/events.rs`、前端 badge）对齐后确定。
