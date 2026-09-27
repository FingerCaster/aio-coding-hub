# 技术设计：流式故障接入上游错误拦截规则

关联：[[prd]]。研究依据见 `research/` 其余文件，本文不重复其证据，只引用结论。

> **基线声明**：本文行号原基于 `459818cf`（`FingerCaster/beta-release-channel`）。
> 实现已迁移至 worktree `intercept-stream-error-rules`，base 为 `origin/main`
> = `64f2c6a0`（旧分支落后 53 个提交）。**实现时一律以
> [[baseline-migration-64f2c6a0]] 的「新基线」列为准**；该文件已重验全部关键假设，
> 结论是本设计的架构、P1 注入方案、方案 A 修复**均无需变更**，仅行号与 §6 一处表述
> 需按其修订。

## 1. 设计总览

```
                         ┌─ pre-commit ─────────────────────────────┐
上游 200 + SSE 开始       │ success_event_stream.rs 三个产生点        │
        │                │ :1099-1150 / :1206-1261 / :1429-1481     │
        │                │ → 完整改写（状态码 + 整个 body）          │
        ├────────────────┤   复用 build_response()                  │
        │                └──────────────────────────────────────────┘
        │  commit 决策点（:1307-1329 / :1365-1369 / :1483-1486）
        │                ┌─ post-commit ────────────────────────────┐
        └────────────────┤ usage_tee.rs                             │
                         │ :474-519 (Err 分支) → 注入 error 帧      │
                         │ :363-383 (idle EOF) → 注入 error 帧      │
                         │ → 追加事件 + 干净 EOF，状态码不变(200)   │
                         └──────────────────────────────────────────┘
```

两侧共用同一套规则配置与同一个匹配入口，但**动作不同**。这是 PRD R2 强调
「不得一刀切」的落地方式：由代码位置天然区分，而非运行时判断某个布尔量。

## 2. 匹配入口：新增合成故障匹配函数

### 2.1 为何不能直接调 `match_response_rule`

`match_response_rule`（`upstream_error_response_rules.rs:170-241`）有两处阻碍：

1. `:179-181` 拒绝非 4xx/5xx 的上游状态；流式故障的上游状态是 200。
2. `body = None` 时，配了 keyword 的规则返回 `ConditionResult::Unknown` → 整体
   `None`（`:194-198`、`:326-333`），导致 fail-open。

### 2.2 设计

在 `upstream_error_response_rules.rs` 新增：

```rust
pub(in crate::gateway) fn match_synthetic_failure_rule(
    rules: &[UpstreamErrorResponseRule],
    cli_key: &str,
    provider_id: i64,
    provider_name: &str,
    synthetic_status: StatusCode,   // status_override 后的值：502 / 524
    gateway_error_code: &str,       // GW_STREAM_ERROR / GW_STREAM_IDLE_TIMEOUT
    upstream_headers: &HeaderMap,
) -> Option<UpstreamErrorResponseRewrite>
```

实现要点：

- **不复制匹配逻辑**。内部构造伪 body 后委托给现有 `match_response_rule`，把
  `synthetic_status` 作为 `upstream_status` 传入（它是 5xx，天然通过 `:179-181`）。
- **伪 body**（PRD R1a）：`format!("{gateway_error_code} {description}")`，例如
  `GW_STREAM_ERROR stream transport error`。以 `&[u8]` 形式传入 `body` 参数，使
  keyword 规则走 `:335-341` 的正常匹配路径而非 `Unknown`。
- **`message_behavior: Passthrough` 的处理**：伪 body 不是 JSON，
  `extract_upstream_message`（`:362-373`）会走 `:371-372` 的纯文本分支，返回伪 body
  文本本身。这会把网关内部错误码暴露给客户端——**不可接受**。故本函数必须在委托前
  判定：若规则为 `Passthrough`，则改用一段固定的、面向用户的兜底文案，或直接判定该
  规则不适用于流式路径（见 §2.3 决策）。
- 伪 body 仅在栈上存活，不写入任何持久化通道（PRD R8）。

### 2.3 `Passthrough` 消息行为在流式路径的语义（需实现时确认）

流式传输中断**没有上游错误消息可透传**。三种可选语义：

| 方案 | 行为 | 评价 |
| --- | --- | --- |
| S1 | 视 `Passthrough` 规则在流式路径不适用，跳过 | 最保守，但用户配了规则却静默不生效，重复本次踩坑 |
| S2 | `Passthrough` 退化为固定兜底文案（如「上游流式传输中断」） | 推荐：规则仍生效，文案不泄露内部错误码 |
| S3 | 透传伪 body | 不可接受：泄露 `GW_STREAM_ERROR` 等内部标识 |

**采用 S2**，并在 UI 说明「流式故障无上游消息可透传，将使用固定提示」。

## 3. post-commit：流尾注入（主战场，覆盖约 93% 实际故障）

### 3.1 注入点

采用研究推荐的 **P1**：`UsageSseTeeStream::poll_next_inner`。两个分支都要改：

- `usage_tee.rs:474-519`（`Err` 分支）→ 传输断裂，对应 `GW_STREAM_ERROR`
- `usage_tee.rs:363-383`（idle timeout 干净 EOF）→ 对应 `GW_STREAM_IDLE_TIMEOUT`(524)

选 P1 的理由（研究已证实）：一处改动同时覆盖 relay 与非 relay 分支；位置在
`tracker.ingest_chunk`（`:426`）**之后**，注入字节不会被 tracker 摄入；可精确控制
finalize 与审计 push 的先后。

### 3.2 注入机制

复用 M2（`protocol_bridge/stream.rs:167-212`）的「队列 + terminated 标志」模式：

1. 命中规则时，构造合成帧；
2. 本次 poll 交付 `Ok(frame)`（**不返回 `Err`**）；
3. 置 `stop_after_tail = true`，下次 poll 返回 `Poll::Ready(None)` → 干净 EOF。

**必须返回 `Ok` + `None` 而非 `Err`**：返回 `Err` 会让下游看到 chunked 编码异常终止，
客户端仍表现为「流被切断」，达不到目的。

未命中规则时，保持现状（返回 `Err` / 干净 EOF），不改变既有行为。

#### 3.2.1 relay 路径必须绕过 terminal firewall `[2026-08-24 执行期修订]`

本节初版写「不需要 M1 的 marker 模式，本场景不需要二次识别」。**实测证伪**：codex relay
（`spawn_usage_sse_relay_body`）把每个 `Ok(chunk)` 都过 `CodexTerminalFirewall::ingest`，
其 `inspect_frame` 对 `event: response.failed` 走
`classify_codex_stream_internal_error` → `FrameDecision::DropTerminal`，**注入帧会被
「post-commit 丢弃」**——正好在用户报告的 codex 场景下静默失效。

**修订方案**：firewall 的职责是审查**上游**帧；网关自造帧不受其管辖。故：

| 路径 | 交付方式 |
| --- | --- |
| relay（codex `/v1/responses`） | 帧存入 `pending_tail`，由 relay 在终止臂**直连 `tx`** 递送，绕过 firewall。`Err` 臂用帧替代原 `Err`；`None` 臂（idle timeout）在 `firewall.finish()` 后补发 |
| 直连（claude / gemini / grok） | 无 relay，`poll_next_inner` 直接返回 `Ok(frame)` |

用显式字段 `relay_owns_tail: bool`（builder `with_relay_owned_tail()`）区分，不复用
`defer_terminal_error` 的语义。注入帧不计入 `forwarded_chunks/bytes`——该计数衡量的是
转发的**上游**字节，网关自造帧不属于它。

##### 3.2.1.1 `Err` 臂的 fail-closed 早退（自查发现的实现缺陷）`[2026-08-24]`

上表的 relay `Err` 臂初版实现是「先 `firewall.finish()`，若 fail-closed 则 `break`」——
沿用了既有代码的形状。**该形状会吞掉注入帧**：真实 TCP 中断多数切在 SSE **帧中间**，
firewall 的 `pending` 非空，`finish()` 必然返回 `Some("partial_frame_at_eof")`
（`terminal_firewall.rs:223-236`），于是 `break` 在 `take_pending_tail()` 之前发生，
尾帧永不下发——**恰好在主目标场景（codex + 真实网络中断）失效**。

初版测试桩在**帧边界**截断，firewall `pending` 为空、不 fail-closed，因此没暴露该缺陷。

修正：把 fail-closed 记为 `firewall_dropped_tail_bytes` 标志而非控制流早退，尾帧无条件
递送；仅在「无规则命中 **且** firewall 已 fail-closed」时保持既有的静默结束
（`None if !firewall_dropped_tail_bytes` 透传原 `Err`，`None => {}` 静默结束）。
理由：firewall 丢弃的是**不完整的上游字节**，而这正是客户端最需要被告知流已失败的时刻；
网关自造帧不在其管辖范围内（同 3.2.1 的原则）。

实测确证：探针输出 `fail_closed_reason == Some("partial_frame_at_eof")`，且
`mid_frame_truncated_codex_stream_still_gets_the_rule_error_event` 通过——即该修正
是**承重**的，不是防御性冗余。

##### 3.2.1.2 附带发现：`Err` 路径的 firewall 审计条目不会被持久化（既有行为，不改）

`stream_terminal_firewall / dropped_after_commit` 这条 special setting 在 `Err` 臂里
**写入过晚**：`tee` 在 `poll_next` 内部产生 `Err` 时已调用 `finalize()`，日志快照此时
已定格，之后 relay 循环的 `push_special_setting` 落不进日志（实测：库中该请求的
`special_settings_json` 只有 `codex_session_id_completion` 与 `response_fixer` 两条）。
`None`（干净 EOF）臂无此问题，因为它的 `finish()` 早于 `finalize()`。

这是**既有时序**，与本任务无关（本任务未改动 `finalize` 时序），修它需要改动 `finalize`
的调用位置——blast radius 远超本任务，故**不修**，仅在测试注释与此处如实记录，避免
后来者误以为该审计字段可靠。

### 3.3 SSE 帧格式

按协议分派。**分派键是请求路径的线协议，不是 `cli_key`**：codex 与 grok 都说 Responses
协议（`configured_model_route.rs` 的 `is_supported_inference_request`），故
`is_responses_protocol_stream(cli_key, path)` 覆盖二者；grok 的 Responses SSE 确带 `event:`
行（既有固件 `mock_runtime_router_grok_responses_sse_is_transparent_and_logged`）。若按
`cli_key == "codex"` 分派，grok + `/v1/responses` 会得到无 `event:`、无 `type` 的裸 `data:`
帧，客户端无从派发，截断依旧静默——违反 PRD R4。Responses 首选 `response.failed`（仓库固件
形状见 `routes.rs:14289-14291`）：

```
event: response.failed
data: {"type":"response.failed","response":{"status":"failed","error":{"type":"upstream_error","code":"upstream_error","message":"<规则文案>"}}}

```

（末尾 `\n\n` 为帧边界，`proxy/sse.rs:7-25`。）

claude / gemini / grok 按 `research/multi-protocol-stream-error-formats.md` 的对应
格式构造。**禁止使用 `data: [DONE]` 承载错误**——`proxy/sse.rs:44-47` 会直接返回
`None`，错误信息丢失。

帧序列化复用 M3 的 `sse_frame` helper（`anthropic.rs:355-358`），已上提至
`gateway/proxy/sse.rs`（该文件已有 `find_sse_event_end` / `parse_sse_frame`）作为共享
helper，避免第四份重复实现。

**执行期改进（2026-08-24）**：错误对象本体不在帧构造处手写，改为取
`UpstreamErrorResponseRewrite::client_error_payload(cli_key)`——该方法从 `build_response`
抽出，使**流式帧与非流式 HTTP 信封的 error 对象同源**。否则同一条规则在两条路径上的
error 形状会各自演化并逐渐漂移。帧构造只负责各协议的**外层包装**（`event:` 行、
`response.failed` 的 `response.error` 嵌套、还是纯 `data:`）。


### 3.4 审计元数据时序（硬约束）

`special_settings` 在 finalize 内被同步序列化进请求日志
（`streams/request_end.rs:309`）。故顺序必须是：

```
1. match_synthetic_failure_rule(...)          → 得到 rewrite
2. push_special_setting(rewrite.special_setting_for_stream_tail())   ← 必须在此
3. tee.finalize(Some(GW_STREAM_ERROR), ...)
4. 返回注入帧
```

违反此序则规则审计不进日志。这条约束直接排除了研究中的 P2/P3 方案。

#### 3.4.1 尾帧路径专用的审计变体 `[2026-08-24 评审后新增]`

尾帧路径**不能**直接用 `special_setting()`：后者输出规则配置的 `clientStatus`，而 post-commit
的客户端状态恒为 200——规则的状态行为在此路径上是 no-op，照原样上报等于谎报「状态已改写」。
故新增 `special_setting_for_stream_tail()`，在原字段基础上覆盖两项：

- `scope: "stream_tail"`（原为 `"response"`）——区分「完整信封改写」与「向活流追加错误事件」；
- `clientStatusApplied: false`——明示规则的状态行为未生效。

**为何不把 `clientStatus` 直接写成 200**：前端
`src/services/gateway/requestLogSpecialSettings.ts:169-171` 对整条 marker fail-closed
（`clientStatus` 不在 400..=599 即 `return null`），写 200 会让审计条目**整条消失**，比报告
一个未生效的状态更糟。故保留原值并用布尔字段表达「未生效」。

**渲染层同步（必须，否则数据诚实但界面仍撒谎）**：`UpstreamErrorResponseRuleMarker` 增补
`clientStatusApplied: boolean`（**缺失即视为 `true`**，使既有日志与全部 pre-commit 改写读数
不变），`formatUpstreamErrorResponseRuleTooltip` 在其为 `false` 时把状态行改为
「状态码：502 → 503（未生效：流已下发，客户端仍为 200）」并追加一行
「改写方式：在流末尾追加错误事件」。本任务之前 `clientStatus` 恒为真实下发值，是本任务
引入 post-commit 路径后才可能失真，故渲染层必须一并修正。

## 4. pre-commit：完整改写（少数场景，但不得降级）

pre-commit 的流式失败点均走 `record_system_failure_and_decide[_no_cooldown]`，此时响应头
未发出。

### 4.1 落地方案 `[2026-08-24 执行期修订：改动面从三点降到一点]`

本节初版设计「在三个点判定 `Abort` 之后各自调用 `match_synthetic_failure_rule` +
`build_response` 构造完整响应返回」。**实测发现有更小且更合契约的落点**：

pre-commit 的 `Abort` **并不自建客户端响应**——`attempt_record.rs` 的
`record_system_failure_and_decide_impl` 对 `Abort` 只返回 `LoopControl::BreakRetry`
（`:240`），循环耗尽后由 `finalize::all_providers_failed` 统一构造响应，而**那里已经会应用
`last_outcome.error_response_rewrite`**（`finalize.rs:266-274`，含 `build_response` 与审计
push）。缺的只是「pre-commit 流式失败的 outcome 从不携带 rewrite」这一环。

**修订方案**：在 `attempt_record.rs` 唯一的 `last_outcome` 汇聚点（`AttemptOutcome::new`
处）挂钩 `match_synthetic_failure_rule_by_code`，命中则经既有
`AttemptOutcome::with_error_response_rewrite` 附上。

优势：
- **改动面**：1 处，而非 3 处；且未来新增 pre-commit 流式失败点自动被覆盖。
- **不新建响应构造代码**：完全复用 `all_providers_failed` → `build_response` 链路，
  与非流式最终错误改写走同一条路，形状不会漂移。
- **PRD R2a 天然满足**：只往 outcome 上挂数据，**完全不改 `decision`**。
  `RetrySameProvider` / `SwitchProvider` 照原样重试与切换；只有整个循环真的失败到
  `all_providers_failed` 才会用到该 rewrite。初版设计在三点直接返回响应，反而有短路
  failover 的风险。
- **与契约第 200 行一致**：rewrite 挂在 outcome 上，随「最后一次证据」自然生效或被覆盖，
  无需任何跨 attempt 保留状态（正是步骤 3 试图引入而被回退的东西）。
- **非流式失败不受影响**：`match_synthetic_failure_rule_by_code` 的 allow-list 只认
  `GW_STREAM_ERROR` / `GW_STREAM_IDLE_TIMEOUT`，其余错误码返回 `None`。

不传上游 headers：被截断的 200 响应本就没有 `Retry-After` 可尊重。

allow-list 的字符串入口 `match_synthetic_failure_rule_by_code` 由 `as_str()` 反查枚举
（`synthetic_failure_code_from_str`），**不写第二份硬编码列表**，避免两处漂移。

### 4.2 不在范围内的 pre-commit 终态

- `finalize_sanitized_stream_terminal` / `FinalizeAsEmptyBody` 产生 `GW_FAKE_200`，
  `finalize_buffered_stream_error_response` 走 `terminal_request_error` 自建响应——两者
  都不在 allow-list 内，按 PRD Out-Of-Scope「不处理 `GW_FAKE_200` / `GW_EMPTY_RESPONSE`
  的专门语义」保持原样。
- `collect_bounded_final_wire` 的读错误 / idle timeout 与 `validate_complete_codex_sse`
  失败虽然产生 `GW_STREAM_ERROR` / `GW_STREAM_IDLE_TIMEOUT`，但决策是
  `SwitchProvider`——它们同样只在成为「最后一次证据」时才生效，语义正确。

## 5. `last_outcome` rewrite 覆盖：**本设计作废，不在本任务范围** `[2026-08-24 更正]`

本节初版依据 `research/last-outcome-rewrite-loss.md` 的方案 A，设计了「`FailoverRunState`
新增 `last_rewrite_outcome`，`finalize::all_providers_failed` 优先取它」的修复。该设计
**基于错误的缺陷判定，已实现后全部回退**（rollback R3）。

**作废理由**：初版只读了契约第 99 行「A later success clears terminal rewrite candidates」，
漏读同段第 200 行：

> Stream retries retain bounded early evidence in the Provider chain;
> **final failure projects the last evidence.**

即「最终失败投射最后一次证据」——覆盖行为是**契约明文要求**的，不是缺陷。该语义另有固化测试
`routes.rs:10828 mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
把守：规则配 `500 → 503`、改写文案字面写着 `"must not survive a later different failure"`，
A 返回 500（命中）+ B 返回 502（不命中）时断言客户端得 **502、日志 502、无 rewrite marker**。

**Gate 3 完整回归实测**：2993 passed / **1 failed**，失败项正是该测试。主 agent 未改动既有
测试去迎合实现，如实上报契约冲突，运营者选择回退。

**当前状态**：`context.rs` / `mod.rs` / `finalize.rs` / `routes.rs` 四处改动已全部 revert，
`finalize.rs` 恢复为直接读 `last_outcome`。误判复盘见
`research/last-outcome-rewrite-loss.md` 第 7 节。

**对本任务其余部分的影响：无。** 步骤 4/5 的流尾注入与 pre-commit 改写发生在**单次
attempt 内部**（流终止时刻立即匹配并注入/改写），不经过 `all_providers_failed` 的跨 attempt
取值逻辑，因此与契约第 200 行无交集。用户报告的原始故障（单供应商流式 502）也不在该场景内。

**若将来确需「早期改写优先」**：那是契约变更，须先改
`upstream-error-handling-contract.md` 第 200 行与上述固化测试，并在 PRD 层面立项，
不得作为本任务的顺带修复。

## 6. 可见性与类型改造

| 项 | 现状 | 改为 | 位置 |
| --- | --- | --- | --- |
| `mod upstream_error_response_rules` | 私有 | `pub(in crate::gateway)` | `proxy/mod.rs:30` |
| `match_response_rule` / `UpstreamErrorResponseRewrite` | `pub(super)` | `pub(in crate::gateway)` | `upstream_error_response_rules.rs:16,170` |
| `StreamFinalizeCtx` | 无规则字段 | 新增 `upstream_error_response_rules: Vec<...>` | `streams/types.rs:156` |
| 规则传入 | — | 在 `build_stream_finalize_ctx` 调用处从 `ctx: CommonCtx` 取（`CommonCtxOwned` 不携带） | SSE：`success_event_stream.rs:2305` |

> **§6 表述修正（基线迁移）**：原文称「`build_stream_finalize_ctx` 的 SSE 调用点全仓
> 唯一」。新基线上该函数有 **3 个生产调用点**：`success_event_stream.rs:2305`（SSE）、
> `success_non_stream.rs:745`、`success_non_stream.rs:846`（后两者为**非流式**路径，新增）。
> 故「持有 `StreamFinalizeCtx` ⇒ 已过 commit」的构造保证须收窄为：**「SSE 路径的
> `StreamFinalizeCtx` 构造点唯一（`:2305`），且位于 commit 决策 break 之后」**。
> 规则传入方案不受影响，但补齐构造点时必须覆盖新增的两处非流式调用点。

`UsageSseTeeStream<S, B, R>` 的 `B: AsRef<[u8]>` 是泛型，tee 内部无法从字节构造 `B`。
两个选项：给 `B` 加 `From<Bytes>` bound（所有真实调用点 `B = Bytes`），或加一层
`Bytes` 特化包装。**优先尝试 bound 方案**；研究标注该方案未经编译验证，若泛型推导受阻
则退回特化层。

## 7. 必须保持的不变量

| 不变量 | 位置 | 违反后果 |
| --- | --- | --- |
| 注入字节不被 `SseUsageTracker` 摄入 | `usage_tee.rs:426` | `event: error` 会置 `terminal_error_seen` + `fake_200_detected`（`usage.rs:1259-1269`），错误码由 `GW_STREAM_ERROR` 变 `GW_FAKE_200`，违反 R7 |
| 不置 `completion_delivered` | `usage_tee.rs:743-745` | 污染探针成功判定与 `is_codex_client_abort_successish`（`:158-176`） |
| 已发出前缀逐字节不变 | commit 后不可回退 | 违反 R3 |
| 不复用 plugin marker | `plugin_chunk.rs:15` | 与插件错误路径语义冲突；新 marker 另取常量 |
| `stop_after_terminal_error` 既有语义 | `usage_tee.rs:301,356-358,466` | 复用需确认不影响 plugin 路径；建议新增独立字段 |
| 未命中规则时行为不变 | — | 避免影响未配置规则的用户 |

## 8. 契约与 UI 更新

### 8.1 契约（必改，否则自相矛盾）

`.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md`
现有两处**明确禁止**本任务的行为，必须更新：

- 第 3 节：「HTTP 200 stream errors and transport errors never enter rewrite matching」
- 第 4 节矩阵行：`Transport failure or HTTP 200 SSE error | Never evaluate final HTTP rewrite rules`

同时需新增记录：流式路径按合成状态码匹配、非流式按上游真实状态码匹配的语义差异
（PRD R5）；已 commit 只能追加事件（R3）；attempt status 记合成码的既有事实（R8）。

### 8.2 UI

`GeneralTab.tsx:809` 的标签「最终 HTTP 错误改写」已不准确（现在也管流式故障），需
更名并补充说明：流式故障按合成状态码（502/524）匹配、关键词匹配的是网关故障描述而非
上游内容、`Passthrough` 在流式路径使用固定文案（§2.3 S2）。

## 9. 测试策略

| 层 | 用例 |
| --- | --- |
| 单元 | `match_synthetic_failure_rule`：合成码命中、上游真实码不误命中、keyword 经伪 body 命中、`Passthrough` 走固定文案 |
| 单元 | 注入 Stream：先 `Ok(frame)` 后 `None`（照 `protocol_bridge/stream.rs:339-391` 模式） |
| 单元 | tracker 不摄入注入帧：错误码仍为 `GW_STREAM_ERROR`，未变 `GW_FAKE_200` |
| 回归 | 契约第 200 行不被破坏：`mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider` 保持通过（早期 rewrite **不得**存活到后续不同失败） |
| 单元 | 成功路径不受影响：任一 attempt 成功 → 走 `Return`，rewrite 不被使用 |
| 端到端 | 客户端 body 含追加的 error 帧（照 `routes.rs:7233-7241` 模式） |
| 端到端 | **帧中间**截断（真实 TCP 中断的主要形状，firewall `partial_frame_at_eof` fail-closed）下尾帧仍下发 —— §3.2.1.1 缺陷的复现器 |
| 端到端 | 未配置规则时行为与现状完全一致（帧边界截断与帧中间截断两种形状各一条） |
| 真机 | **codex CLI 实际解析追加事件**——研究明确指出仓库无 codex CLI 源码或协议文档，此项无法靠代码断言，必须真机验证 |

## 10. 兼容性与回滚

- **配置兼容**：不新增 settings 字段，复用现有 `upstream_error_response_rules`
  结构，无迁移、无 schema 变更。
- **行为兼容**：未配置规则或规则未命中时，所有路径行为与现状逐字节一致。
- **回滚**：功能入口是「规则是否命中」，回滚只需停用规则即可恢复现状；代码层面
  各改动点相互独立，可按 §11 顺序逆序回退。

## 11. 实施顺序（详见 `implement.md`）

1. 可见性与类型改造（§6）——无行为变更，先让编译通过
2. `match_synthetic_failure_rule` + 单元测试（§2）
3. ~~`last_outcome` 修复（§5）~~ —— **已实现后回退，判定错误，见 §5**。步骤编号保留以
   免打断外部引用
4. post-commit 注入（§3）——主战场
5. pre-commit 完整改写（§4）
6. R10 补 `terminal_origin`（PRD 附带项）
7. 契约与 UI（§8）
8. 端到端测试 + 真机验证（§9）

第 3 步与第 4 步无依赖关系，可并行；但建议先完成第 3 步，因为它能独立验证「规则确实
被应用到最终响应」这条链路，为第 4 步排除干扰。
