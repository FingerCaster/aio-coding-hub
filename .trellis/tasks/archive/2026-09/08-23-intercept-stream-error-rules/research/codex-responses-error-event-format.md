# Research: Codex `/v1/responses` SSE error 事件的合法格式

- **Query**: Codex 的 /v1/responses SSE 协议中，error 事件的合法格式是什么？仓库里有没有已构造 Codex 流式 error 事件的先例代码或测试固件？给出精确样例
- **Scope**: mixed（仓库证据 + 协议推断）
- **Date**: 2026-08-23

## Findings

### 帧封装格式（已证实）

仓库全部 SSE 帧构造点使用同一形状：`event: <name>\ndata: <单行 JSON>\n\n`

| 位置 | 代码 |
|---|---|
| `protocol_bridge/inbound/anthropic.rs:355-357` | `format!("event: {event_type}\ndata: {data}\n\n")` |
| `protocol_bridge/stream.rs:192` / `:210` | `format!("event: error\ndata: {data}\n\n")` |
| `streams/plugin_chunk.rs:123` | `"{MARKER}event: error\ndata: {{...}}\n\n"` |

解析侧 `proxy/sse.rs:32-66` `parse_sse_frame` 的行为（已证实）：

- `event:` 行给出事件名；没有 `event:` 行时从 `data.type` 推断（`:58-63`）。
- `data:` 可多行，join 时用 `\n`（`:55`）。
- `data: [DONE]` 直接返回 `None`（`:44-47`）——**不要用 `[DONE]` 承载错误**。
- 以 `:` 开头的行按注释忽略（`:38-40`）。
- 帧边界为 `\n\n` 或 `\r\n\r\n`（`proxy/sse.rs:7-25` `find_sse_event_end`）。

### 仓库认定的 Codex 终态错误事件集合（已证实）

四个事件名/type 被全仓一致地当作 Codex 流终态错误：
`error`、`response.error`、`response.failed`、`response.incomplete`

| 位置 | 用途 |
|---|---|
| `success_event_stream.rs:234-256` `is_terminal_error_sse_frame` | pre-commit 前缀检查 |
| `domain/usage.rs:94-99` `is_codex_stream_terminal_type` | 内部错误分类器 |
| `proxy/sse.rs:112-120` | 非流聚合时把这三者判为失败 |
| `.trellis/spec/.../upstream-error-handling-contract.md:53-55` | 契约明文列出这四个 |

`SseUsageTracker` 的识别面稍窄一些（已证实）：
`is_terminal_error_event_name`（`usage.rs:878-882`）只认 `error` / `response.error`；
`is_terminal_error_event_type`（`usage.rs:899-902`）认 `error` / `response.error` / `*.error`；
`response.failed` 靠 `is_terminal_error_status`（`usage.rs:911-916`，匹配 data 里的
`status`/`response.status` 为 `failed`）或 `stream_terminal_type`（`usage.rs:101-110`，
在 `detect_stream_internal_errors` 开启时，`usage.rs:1245-1250`）识别。

### 仓库中的真实上游固件（已证实，来自实测抓包做成的测试固件）

**A. `response.failed` 完整形状** —— `routes.rs:14289-14291`：

```
event: response.failed
data: {"type":"response.failed","response":{"id":"resp-capacity-first","status":"failed","error":{"type":"server_error","code":"model_at_capacity","message":"Selected model is at capacity"}}}
```

同形状另见 `routes.rs:14382-14384`、`proxy/sse.rs:266-270`、
`success_event_stream.rs:1920-1921`、`domain/usage/tests.rs:816`。

**B. `error` 事件形状** —— `routes.rs:13175-13177`：

```
event: error
data: {"type":"error","error":{"message":"quota exhausted","type":"insufficient_quota"}}
```

同形状另见 `domain/usage/tests.rs:571`：
`b"event: error\ndata: {\"error\":{\"message\":\"upstream failed\"}}\n\n"`。

**C. `response.incomplete` 形状** —— `proxy/sse.rs:283-284`：

```
event: response.incomplete
data: {"type":"response.incomplete","response":{"id":"resp_incomplete","status":"incomplete"}}
```

### 消息提取路径（已证实）

`proxy/sse.rs:177-188` `sse_error_detail` 的查找顺序，说明网关自身认可的 message 字段位置：

1. `data.detail`
2. `data.message`
3. `data.error.message`
4. `data.response.error.message`

`domain/usage.rs:112-125` `stream_error_fields` 同时看
`data.error.<field>`、`data.response.error.<field>`、`data.<field>`。

**结论**：把文案放在 `response.error.message`（配 `response.failed`）
或 `error.message`（配 `error`）都能被本仓库自身解析到。

### 推荐的注入样例（字节级，可直接使用）

> 以下 JSON 必须**单行**。`\n` 为真实换行。文案 `<MESSAGE>` 需做 JSON 字符串转义
> （用 `serde_json::to_string(&msg)`，参考 `plugin_chunk.rs:124`）。

#### 首选：`response.failed`（推断为 codex CLI 最稳的形态）

```
event: response.failed\n
data: {"type":"response.failed","response":{"id":"<RESPONSE_ID>","object":"response","status":"failed","error":{"type":"server_error","code":"GW_STREAM_ERROR","message":"<MESSAGE>"}}}\n
\n
```

单行 Rust 字面量形式：

```rust
format!(
    "event: response.failed\ndata: {}\n\n",
    serde_json::to_string(&serde_json::json!({
        "type": "response.failed",
        "response": {
            "id": response_id,           // 见下方「response id 的取法」
            "object": "response",
            "status": "failed",
            "error": {
                "type": "server_error",
                "code": "GW_STREAM_ERROR",
                "message": message,
            }
        }
    })).unwrap_or_default()
)
```

`response.id` 未知时可退化为不带 `id` 的最小形态（仍合法，`proxy/sse.rs` 的解析不要求 id）：

```
event: response.failed\n
data: {"type":"response.failed","response":{"status":"failed","error":{"type":"server_error","code":"GW_STREAM_ERROR","message":"<MESSAGE>"}}}\n
\n
```

#### 备选：`error`（与仓库固件 B 完全同形）

```
event: error\n
data: {"type":"error","error":{"type":"upstream_error","code":"GW_STREAM_ERROR","message":"<MESSAGE>"}}\n
\n
```

这个形状与 `upstream_error_response_rules.rs:39-45` 的非流 codex/grok 信封
（`error.{type,code,message}`）**完全一致**，只是外面套了 SSE 帧，实现上最省事。

### response id 的取法（已证实可行）

网关已经在流里观测过 `response.created` 的内容：

- `SseUsageTracker` 会记录 model（`usage.rs:1347-1350`）；
- `usage::parse_model_from_json_or_sse_bytes` 被用于从 SSE 字节抽 model
  （`success_event_stream.rs:147`）。

但**没有**现成的「记录 response.id」能力。若要在注入帧里带真实 `resp_xxx`，
需在 tee 或 observer 层新增一个 `Option<String>` 字段捕获
`response.created` → `data.response.id`。
`proxy/sse.rs:92-95` 展示了 `response.created` 的 payload 结构（`data.response` 或 `data` 本身）。

不带 id 也不违反协议（推断）；带上更稳妥。

### 客户端（codex CLI）侧行为 —— 推断，非代码证实

仓库中**没有** codex CLI 源码或协议文档可供交叉验证
（`find -iname "*codex*" -type d` 只命中项目自身目录与 `.codex` 配置目录，无 vendored 源码；
`grep sequence_number` 仅命中历史分析文档 `.omx/artifacts/*.md`，非协议规范）。

基于 OpenAI Responses API 的公开协议与上述固件的一致性，做如下推断：

1. codex CLI 在流未收到 `response.completed` 就结束时会报「stream closed before response.completed」类错误。
   追加一个 `response.failed` / `error` 帧能让它拿到**具体文案**而不是通用截断错误。
2. `response.failed` 的优先级高于 `error`：前者是 Responses API 的一等终态事件，
   带 `response.error.message`；后者在部分实现里被当作未知事件跳过。
   → **建议同时或按序发送不是好主意**（会被解析成两次失败），选一个即可，首选 `response.failed`。
3. 真实上游会带 `sequence_number` 字段（历史分析文档
   `.omx/context/codex-continuation-recent-502-shape-fix-20260707T181234Z.md:14` 提到
   `data_keys=metadata,response_id,sequence_number,type`）。
   仓库自身构造的所有帧都不带 `sequence_number`（`anthropic.rs:355`、`plugin_chunk.rs:123`、
   `protocol_bridge/stream.rs:192`），且这些帧在 e2e 测试中被客户端正常接收
   （`routes.rs:7233-7241`）。因此**推断可省略**；但若客户端严格校验单调性，
   省略比编造一个错误的序号更安全。
4. **绝对不要**在错误后再发 `data: [DONE]`：`parse_sse_frame` 遇到会返回 `None`
   （`proxy/sse.rs:44-47`），且 `aggregate_responses_event_stream` 把
   「[DONE] 先于 response.completed」判为错误（`proxy/sse.rs:127-132`）。

Acceptance Criteria 里「codex CLI 能正常解析并展示追加的 error 事件」这一条
**必须靠真机验证**，无法只凭仓库代码断言。

### Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:52-55`
  —— 四个终态事件名的权威列表。

## Caveats / Not Found

- **未找到**：codex CLI 的 SSE 解析实现或版本化协议文档。第 3 节以外的客户端行为均为推断。
- **未找到**：任何在流尾按配置文案合成 Codex error 事件的既有代码或测试固件。
  仓库现有的 Codex error 帧全部来自**上游**（测试 stub 模拟上游），
  网关自己合成的 error 帧只有 `plugin_chunk.rs` 与 `protocol_bridge/stream.rs` 两处，
  且都是 Anthropic 风格的 `{"type":"error","error":{...}}`（见 [[multi-protocol-stream-error-formats]]）。
- `sequence_number` 是否必需未验证。
