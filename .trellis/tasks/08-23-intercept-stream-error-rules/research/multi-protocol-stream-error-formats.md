# Research: claude / gemini / grok 的流式 error 事件格式，与非流信封的差异

- **Query**: claude / gemini / grok 各自流式 error 事件格式，仓库里的先例；`upstream_error_response_rules.rs:30-54` 的 `build_response` 是非流式 JSON 信封，流式场景需要不同形态，请对比说明差异
- **Scope**: mixed
- **Date**: 2026-08-23

## Findings

### 前置：这三个协议是否会走到本任务的故障路径（已证实）

`use_sse_relay` 只对 codex `/v1/responses` 系列为 true
（`success_event_stream.rs:1569-1572` + `:201-207`）。
claude / gemini / grok 的流式响应走 `Body::from_stream(UsageSseTeeStream::new(...))`
（`success_event_stream.rs:1625-1631` / `1663-1669`）。

两条路径**都以 `UsageSseTeeStream` 为核心**，`Err` 分支同为 `usage_tee.rs:474-519`。
差异在于：

- codex relay 路径：`Err` 经 mpsc 转发（`usage_tee.rs:894-903`）后才到 body。
- 其他协议：`Err` 由 `UsageSseTeeStream::poll_next`（`usage_tee.rs:622-625`）直接给 axum body。

结论：**三者同样会遭遇「流被硬切断」，同样需要处理**。
但注意 `is_codex_stream_tail_error_successish`（`usage_tee.rs:212-224`）
里的 `is_codex_responses_path` 前置条件（`usage_tee.rs:147-155`）意味着
非 codex 协议**永远不会**被容错为成功，一律 `GW_STREAM_ERROR`。

### Claude（`/v1/messages`）

#### 仓库先例（已证实）

网关合成 Anthropic 风格 SSE error 帧的实现在 `protocol_bridge/stream.rs`：

`:17-21` 静态常量（cx2cc 桥把 Codex 上游翻译成 Claude 下游时使用）：

```
event: error
data: {"type":"error","error":{"type":"invalid_request_error","message":"bridge_sse_frame_too_large"}}
```

`:178-194` / `:196-212` 动态版本，payload 由 `serde_json::json!` 构造：

```rust
serde_json::json!({
    "type": "error",
    "error": {
        "type": "invalid_request_error",
        "code": GatewayErrorCode::BridgeUnsupportedFeature.as_str(),
        "message": format!("bridge is not registered: {bridge_type}")
    }
})
```

测试断言客户端收到该帧并随后干净 EOF：`protocol_bridge/stream.rs:339-391`。

另有真实 Claude 上游 error 事件固件：
`domain/claude_model_validation/response/tests.rs:38-56`
（外层是 aio 网关自己包的壳，内层 `details` 字符串里嵌套了标准 Anthropic 信封
`{"type":"error","error":{"type":"invalid_request_error","message":"..."},"request_id":"req_123"}`）。

#### 推荐注入样例（字节级）

```
event: error\n
data: {"type":"error","error":{"type":"overloaded_error","message":"<MESSAGE>"}}\n
\n
```

`error.type` 建议取 Anthropic 合法枚举值。若无法确定，沿用仓库既有的
`invalid_request_error`（`protocol_bridge/stream.rs:19`）或
`upstream_error` —— 后者是 `build_response` 里 claude 分支已用的值
（`upstream_error_response_rules.rs:32-38`），**语义一致性最好**：

```
event: error\n
data: {"type":"error","error":{"type":"upstream_error","message":"<MESSAGE>"}}\n
\n
```

注意 Anthropic 流协议的正常终态是 `message_stop`
（`usage.rs:864-876` 把 `message_stop` 列为 completion 事件名）。
追加 `event: error` 后**不要**再补 `message_stop`。

### Gemini

#### 仓库先例（已证实）

`GeminiOAuthSseStream`（`gemini_oauth.rs:138-196`）只做 payload 改写
（`transform_sse_event` `:491-494`、`transform_sse_event_inner` `:517+`），
**没有**任何合成 error 事件的能力。它与 `ResponseFixerStream` 共用
`queued` / `pending_error` 字段模式（`gemini_oauth.rs:145-146`）。

Gemini 流式 payload 形状（已证实，来自测试固件 `gemini_oauth.rs:811`、`:829-830`）：

```
data: {"candidates":[{"content":{"parts":[{"text":"ok"}]}}]}
```

即 **无 `event:` 行、纯 `data:`**。

Gemini 错误信封形状（已证实，非流路径）：`upstream_error_response_rules.rs:46-52`

```json
{"error":{"code":<int>,"status":"UNKNOWN","message":"..."}}
```

#### 推荐注入样例（字节级）

```
data: {"error":{"code":502,"status":"UNAVAILABLE","message":"<MESSAGE>"}}\n
\n
```

**关键差异**：Gemini 不用 `event:` 行。
若强行加 `event: error`，`parse_sse_frame`（`proxy/sse.rs:41-42`）能解析，
但 Gemini 客户端（`google-genai` SDK / gemini-cli）预期的是纯 `data:` 流 —— **推断**加 `event:` 行有风险，
建议**不加**。

**`error.code` 的取值需要决策**：非流路径填的是 `client_status`（真实下发的 HTTP 状态码，
`upstream_error_response_rules.rs:48`）。流式场景 HTTP 状态码仍是 200（R3），
所以填 200 会自相矛盾。建议填**合成状态码**（`GW_STREAM_ERROR` → 502），
与 R1 的匹配语义保持一致，并在契约文档里写明这条差异。

### Grok

#### 仓库先例

**未找到**任何 grok 专属的流式 error 构造代码或固件。

已证实的事实：

- `build_response`（`upstream_error_response_rules.rs:39-45`）把 `grok` 与 `codex` 归为**同一个**
  OpenAI 风格信封 `{"error":{"type","code","message"}}`。
- `is_supported_cli_key` 校验见 `shared/cli_key.rs`（`upstream_error_response_rules.rs:307-310` 引用）。
- grok 流式走 `UsageSseTeeStream` 直连分支（同 claude/gemini）。

#### 推荐注入样例（字节级，推断）

Grok API 兼容 OpenAI `chat/completions` 流式协议 → 无 `event:` 行、纯 `data:`：

```
data: {"error":{"type":"upstream_error","code":"GW_STREAM_ERROR","message":"<MESSAGE>"}}\n
\n
```

同样**不要**追加 `data: [DONE]`（`proxy/sse.rs:44-47` 会把它解析为 `None`；
且 `aggregate_responses_event_stream` 把 `[DONE]` 早于终态判为错误，`proxy/sse.rs:127-132`）。

### 流式 vs 非流式信封的完整对比

`upstream_error_response_rules.rs:30-68` `build_response` 的能力矩阵与流式场景的差异：

| 维度 | 非流 `build_response`（已证实） | 流式追加事件（本任务） |
|---|---|---|
| HTTP 状态码 | 由规则决定：`client_status`，来自 `status_behavior`（`:200-205`）；强制 4xx/5xx（`:206-208`） | **不可改**，保持 200。头已发出（`success_event_stream.rs:1673-1695`） |
| `Content-Type` | 设为 `application/json`（`:59-62`） | 保持上游的 `text/event-stream`，不可改 |
| `Retry-After` 头 | 透传经校验的值（`:64-66`、`safe_retry_after` `:150-168`） | **不可设置**，头已发出 |
| `x-trace-id` 头 | 设置（`:63`） | 已在 commit 时设置（`success_event_stream.rs:1677`） |
| body 形态 | 完整替换为单个 JSON 对象（`:55`、`:67`） | **只能追加**一个 SSE 帧，前缀逐字节不变 |
| 帧封装 | 无（裸 JSON） | codex/claude 需 `event: <name>\n` 前缀；gemini/grok 纯 `data:` |
| 消息来源 | `message_behavior`：passthrough（从 body 抽取 `:210-224`、`extract_upstream_message` `:362-373`）或 override | passthrough **不可用** —— 传输层断裂无 body。只有 override 有意义 |
| 匹配输入 | 上游真实 status + 上游 body（`:170-178`） | 合成状态码（R1）+ **无 body** |
| 规则可命中范围 | status 与 keyword 都可用 | **只有纯 status_codes 规则**能命中；含 keyword 的规则因 `body = None` 返回 `Unknown` → 整体 `None`（`:194-198`、`:326-333`） |
| 前置门槛 | `!(status.is_client_error() \|\| status.is_server_error())` 直接返回 `None`（`:179-181`） | 上游 status 是 200 → **当前必然返回 None**，这正是 PRD 根因 2 |
| 可见性 | `pub(super)`，模块 `mod upstream_error_response_rules;` 私有（`proxy/mod.rs:30`） | `crate::gateway::streams` 访问不到，需放宽为 `pub(in crate::gateway)` |

### 建议的抽象形态（推断）

给 `UpstreamErrorResponseRewrite` 增一个兄弟方法，与 `build_response` 并列：

```rust
// 伪代码，按 build_response 的 cli_key 分派结构复制
pub(in crate::gateway) fn build_stream_tail_event(&self, cli_key: &str) -> Option<Bytes>
```

分派表（cli_key → 帧）：

| cli_key | 事件名 | data payload |
|---|---|---|
| `codex` | `response.failed` | `{"type":"response.failed","response":{"status":"failed","error":{"type":"server_error","code":"...","message":"..."}}}` |
| `claude` | `error` | `{"type":"error","error":{"type":"upstream_error","message":"..."}}` |
| `grok` | *(无 event 行)* | `{"error":{"type":"upstream_error","code":"...","message":"..."}}` |
| `gemini` | *(无 event 行)* | `{"error":{"code":<合成状态码>,"status":"UNAVAILABLE","message":"..."}}` |
| 其它 | — | `None`（与 `build_response` 的 `_ => return None` 一致，`:53`） |

`build_response` 的既有测试 `builds_protocol_specific_error_envelopes`
（`upstream_error_response_rules.rs:667-708`）可直接照抄成流式版本的测试骨架。

## Caveats / Not Found

- **未找到**：grok 流式协议的仓库内证据（无固件、无专属代码）。其格式为按 OpenAI 兼容性推断。
- **未找到**：gemini 流式 error 事件的仓库内先例。`gemini_oauth.rs` 只改写不合成。
- Gemini `error.code` 应填合成状态码还是 200，属产品决策，PRD 未定。
- claude `error.type` 的合法枚举值未在仓库内穷举；建议沿用 `upstream_error`（与非流信封一致）以保证前后端语义一致。
- R4 允许「明确记录为不适用」——若决定只做 codex，需在契约文档写明其余三个协议的 fallback 行为
  （当前是硬切断，不是无害的）。
