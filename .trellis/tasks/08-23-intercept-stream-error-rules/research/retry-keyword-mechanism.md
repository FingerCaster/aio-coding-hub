# Research: retry-keyword 机制实现，与 upstream_error_response_rules 的关系

- **Query**: 契约 `upstream-error-handling-contract.md:56-69` 描述的 retry-keyword 机制代码实现在哪；它与 `upstream_error_response_rules.rs` 那套规则是否完全独立的两套
- **Scope**: internal
- **Date**: 2026-08-23

## Findings

### 结论

**是完全独立的两套。** 不同 schema、不同持久化字段、不同执行阶段、不同调用点，
零共享代码路径与零共享关键词。契约 `:78-82` 已明文规定
"HTTP 200 stream errors and transport errors never enter rewrite matching"。

| 维度 | retry-keyword（流内部错误） | upstream_error_response_rules（最终 HTTP 改写） |
|---|---|---|
| 持久化字段 | `upstream_retry_policy.stream_internal_errors` | `upstream_error_response_rules` |
| Rust 类型 | `UpstreamStreamInternalErrorPolicy`（`infra/settings/types.rs:140-156`） | `UpstreamErrorResponseRule`（`infra/settings/types.rs`，常量族见 `upstream_error_response_rules.rs:3-10`） |
| 匹配函数 | `classify_codex_stream_internal_error`（`domain/usage.rs:189-278`） | `match_response_rule`（`proxy/upstream_error_response_rules.rs:170-241`） |
| 生效阶段 | pre-commit 前缀检查 / post-commit evidence 记录 | 重试/failover/熔断决策**之后**的终态 HTTP 候选 |
| 触发状态 | 上游 HTTP 2xx + SSE 终态错误帧 | 上游 HTTP 4xx/5xx（`upstream_error_response_rules.rs:179-181` 首行拒绝其他） |
| 调用点数量 | 2 处（见下） | 2 处：`upstream_error.rs:750`、`thinking_signature_rectifier_400.rs:574` |
| 匹配语义 | 大小写不敏感字面子串，正向关键词优先 | priority + Any/All + status codes + 关键词 + CLI/Provider scope |
| 结果 | 改变重试/切换决策（pre-commit）或仅留证据（post-commit） | 构造协议兼容错误信封替换客户端响应 |

### retry-keyword 的核心实现

`src-tauri/src/domain/usage.rs:189-278` `classify_codex_stream_internal_error`：

```rust
pub fn classify_codex_stream_internal_error(
    event_name: &str, data: &Value, enabled: bool,
    retry_keywords: &[String], non_retry_keywords: &[String],
    disposition: &str,
) -> Option<StreamInternalErrorEvidence>
```

- **门禁**：`usage.rs:197` `stream_terminal_type(event_name, data)?` —— 非终态类型直接
  返回 `None`。终态白名单在 `usage.rs:94-99`：`error` / `response.error` /
  `response.failed` / `response.incomplete`（`data.type` 优先，否则看 `event:` 名，
  见 `usage.rs:101-110`）。
- **可搜索文本**：`usage.rs:202-213` —— `event_name` + `data.type` +
  `error.type` + `error.code` + message，`join("\n").to_lowercase()`。
  字段提取器 `stream_error_fields`（`112-125`，依次看 `data.error.X` /
  `data.response.error.X` / `data.X`）与 `stream_error_message`（`134-146`，
  含 `incomplete_details.reason` 回退）。
- **优先级链**：`usage.rs:221-231`

  ```rust
  let (classification, matched_keyword) = if !enabled {
      ("disabled", None)
  } else if let Some(keyword) = retry_match {          // 正向关键词优先
      ("retryable", Some(keyword.as_str()))
  } else if is_codex_capacity_stream_internal_error(event_name, data) {  // 内建 capacity
      ("retryable", None)
  } else if let Some(keyword) = non_retry_match {
      ("non_retryable", Some(keyword.as_str()))
  } else {
      ("unknown", None)
  };
  ```

- **匹配方式**：`usage.rs:215-220` ——
  `searchable.contains(&keyword.to_lowercase())`，空白关键词跳过。字面子串，无正则、
  无分词。
- **内建 capacity alias**：
  - `usage.rs:11` `CODEX_CAPACITY_MESSAGE = "selected model is at capacity"`
  - `usage.rs:12` `CODEX_CAPACITY_ERROR_CODES = ["server_is_overloaded", "slow_down"]`
  - `usage.rs:171-187` `is_codex_capacity_stream_internal_error`（code 精确
    `eq_ignore_ascii_case`；message/type 用 `contains(CODEX_CAPACITY_MESSAGE)`）
  - `usage.rs:154-162` `contains_codex_capacity_signal`（更宽：含 `capacity` /
    `overload` 子串），用于客户端诊断脱敏（`StreamInternalErrorEvidence::contains_codex_capacity_signal`，
    `usage.rs:52-65`）
- **脱敏与截断**：`usage.rs:14-26`（bearer / secret assignment / key-like 正则）、
  `79-92` `bounded_stream_error_text`；上限 `usage.rs:9-10`
  （message 2048 字符、短字段 512 字符）。

### 两个调用点（pre-commit vs post-commit）

**1. pre-commit** — `failover_loop/response/success_event_stream.rs:336-361`：

```rust
if let Some(evidence) = usage::classify_codex_stream_internal_error(
    &event_name, &data,
    config.retry_policy.enabled && config.retry_policy.stream_internal_errors.enabled,
    &config.retry_policy.stream_internal_errors.retry_keywords,
    &config.retry_policy.stream_internal_errors.non_retry_keywords,
    "buffered_before_commit",
) {
    if evidence.is_retryable()
        || usage::is_codex_capacity_stream_internal_error(&event_name, &data)
    {
        return BufferedStreamPrefixDecision::ProviderFailure {
            error_code: GatewayErrorCode::Fake200.as_str(), evidence: Some(evidence),
        };
    }
    return BufferedStreamPrefixDecision::StartStreaming { guard_cap_reached: false };
}
```

- `is_retryable()` 定义 `usage.rs:41-43`（`classification == "retryable"`）。
- 第二个条件保证：即使 `enabled=false`（classification 为 `"disabled"`），capacity
  终态帧**仍然被拦截**，只是决策变成 SwitchProvider。测试证据
  `success_event_stream.rs:2024-2053`。
- `non_retryable` / `unknown` → 直接提交并原样透传上游 SSE。测试证据
  `success_event_stream.rs:1998-2022`。

**2. post-commit** — `domain/usage.rs:1229-1250` `SseUsageTracker::ingest_event`：

```rust
if self.stream_internal_error_evidence.is_none() {
    if let (Some(classifier), Ok(event_name)) = (...) {
        self.stream_internal_error_evidence = classify_codex_stream_internal_error(
            event_name, data, classifier.enabled,
            &classifier.retry_keywords, &classifier.non_retry_keywords,
            "forwarded_after_commit",
        );
    }
}
if self.stream_internal_error_classifier.is_some()
    && stream_terminal_type(...).is_some()
{
    self.terminal_error_seen = true;
    self.fake_200_detected = true;
}
```

- **post-commit 的关键词分类不改变任何路由决策**，只填一次 evidence（`1230` 首个命中即锁定）。
- 真正有行为影响的是 `1245-1250`：只要分类器被安装且看到终态类型，就置
  `terminal_error_seen` **和** `fake_200_detected`。这两个标志随后驱动
  `usage_tee.rs:436-471`（非 defer 模式立即截断）与
  `usage_tee.rs:913-969`（relay 模式的 codex 宽容判定）。
- 分类器安装点：`streams/usage_tee.rs:318-331`
  `with_stream_internal_error_classifier(policy.enabled && stream_internal_errors.enabled,
  retry_keywords, non_retry_keywords)`，前置开关是
  `ctx.detect_stream_internal_errors`。
- `detect_stream_internal_errors` 的值来自
  `success_event_stream.rs:1550-1555` 传入 `build_stream_finalize_ctx` 的
  `is_native_codex_responses_event_stream_path(...)`（字段定义
  `streams/types.rs:190`，赋值 `failover_loop/context.rs:284, 333`）。

### 设置结构与迁移

- `infra/settings/types.rs:140-156`：

  ```rust
  pub struct UpstreamStreamInternalErrorPolicy {
      pub enabled: bool,          // Default: true
      pub retry_keywords: Vec<String>,      // Default: empty
      pub non_retry_keywords: Vec<String>,  // Default: empty
  }
  ```

- 归属于 `UpstreamRetryPolicy`（`types.rs:158-189`），共享
  `max_retries` / `backoff_ms` / `counts_toward_circuit_breaker`（契约 `:38-41`）。
- 默认 HTTP 规则用的是另一个常量：`infra/settings/defaults.rs:115`
  `DEFAULT_CAPACITY_RETRY_KEYWORD = "selected model is at capacity"`，出现在
  `types.rs:174-179` 的 `http_rules[0].body_contains`。**这是第三套机制**
  （HTTP 400 正文匹配），与流关键词同文本但不同字段、不同阶段。
- 关键词清洗：`infra/settings/migration.rs:278-283`（旧 wire alias 读取）、
  `362-363` `sanitize_stream_internal_error_keywords`（trim + 去重，测试
  `migration.rs:2336-2353`、`2445-2458`）。
- DB 迁移种子：`infra/db/migrations/v42_to_v43.rs:9-10`。
- 前端 UI：`src/components/gateway/CodexStreamInternalErrorFields.tsx:116-126`
  （两个多行关键词输入，label「重试关键词（每行一项）」/「不重试关键词（每行一项）」）。

### upstream_error_response_rules 侧的边界

- `match_response_rule`（`upstream_error_response_rules.rs:170-241`）：
  - `179-181` 非 4xx/5xx 立即 `None`（**这是上游 200 流式故障进不来的直接原因**）
  - `183-188` 按 `(priority, index)` 排序，`190-198` 首个匹配即返回，`Unknown` → 整体
    fail open 返回 `None`
  - `200-224` status/message 行为（Passthrough / Override）
  - `226-237` 构造 `UpstreamErrorResponseRewrite`，`safe_retry_after` 见 `150-168`
- 信封构造：`upstream_error_response_rules.rs:30-68` `build_response`
  （claude / codex|grok / gemini 三种 payload；其他 cli_key → `None`）
- 审计元数据：`70-83` `special_setting`（只含规则/供应商身份与前后状态）
- 唯一消费点：`failover_loop/response/finalize.rs:266-287` —— 从
  `last_outcome.error_response_rewrite` 取候选，取不到就落 `error_response(BAD_GATEWAY, ...)`
  硬编码 502

## Caveats / Not Found

- 归档任务 `.trellis/tasks/archive/2026-08/08-10-codex-stream-terminal-firewall/` 的
  `prd.md:33-51`、`design.md:62/137-149` 规划过把 `retry_keywords` 降级为
  `legacy_retry_keywords` 兼容字段。**当前代码里没有 `legacy_retry_keywords`**
  （grep 无命中），即该迁移未落地，两个旧字段仍是唯一的生效配置。
- `src-tauri/src/gateway/routes.rs:14642` 也读了 `.retry_keywords`；未展开核查其用途
  （疑为 provider override 的解析/校验路径）。
- 相关文件：[[downstream-commit-boundary]]、[[pre-commit-guard-window]]、[[stream-terminal-origins]]
