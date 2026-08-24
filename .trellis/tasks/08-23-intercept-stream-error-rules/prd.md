# 用上游错误拦截规则接管流式传输中断

## Goal

让「上游错误拦截规则」能够作用于网关合成的流式故障状态码（当前主要是
`GW_STREAM_ERROR` → 502），使流式传输中断不再以裸 502 或被硬切断的 SSE 直接
暴露给 codex 等客户端；同时修复已匹配的改写候选被后续 attempt 覆盖丢弃的缺陷。

## Background

### 触发案例

用户在 Codex CLI（`POST /v1/responses`）遇到网关请求失败：

- `trace 17874758`、`cli:codex`、错误码 `GW_STREAM_ERROR`、错误类别 `SYSTEM_ERROR`
- 日志显示状态码 502，耗时 8931ms，TTFB 2305ms，故障切换路径仅 1 次尝试
- 用户已在设置中配置了针对 502 的上游错误拦截规则，但规则未生效

**关键澄清：该 502 从未发送给客户端。** 它只存在于两处日志投影——请求日志
status（`src-tauri/src/gateway/streams/request_end.rs:175-177`，调用点 `:310`）与
attempt status（`request_end.rs:208-234`，第 `:218` 行）。`ctx.status` 恒为上游真实的
200。codex 客户端实际收到的是 **HTTP 200 + 一条被静默截断的 SSE 流**，没有任何错误
信号。这比收到 502 更糟：客户端无法区分「正常结束」与「传输中断」。`SYSTEM_ERROR`
类别由 `streams/finalize.rs:49-66` 的 `configured_category.or(SystemError)` 得出
（`ctx.error_category` 在 `context.rs:314` 恒为 None）。该类故障会触发 cooldown
（`finalize.rs:149-162`）但**不**记熔断失败（`:256-263` 要求 PROVIDER_ERROR）。

### 根因（已由代码证实）

1. 上游真实返回的是 **HTTP 200**，SSE 流已正常开始传输（TTFB 2305ms 证实首字节
   已达）。流在 8931ms 处断裂。日志状态码 502 是
   `src-tauri/src/gateway/proxy/status_override.rs:11-17` 把 `GW_STREAM_ERROR`
   映射出来的合成值，并非上游返回值（
   `src-tauri/src/gateway/streams/request_end.rs:563` 明确以 `200` 为上游状态）。
2. `match_response_rule` 首行即拒绝非 4xx/5xx 的上游状态（
   `src-tauri/src/gateway/proxy/upstream_error_response_rules.rs:179-181`），上游
   200 直接返回 `None`，规则不进入遍历。
3. 更根本地，`match_response_rule` 全仓仅两个调用点，都在错误路径（
   `failover_loop/response/upstream_error.rs:750`、
   `failover_loop/response/thinking_signature_rectifier_400.rs:574`）。产生
   `GW_STREAM_ERROR` 的流式成功路径 `success_event_stream.rs` 与流终止模块
   `streams/finalize.rs` 均未接入该匹配。

### 与现有机制的边界

仓库已有一套 Codex 流终态错误防护（`stream_internal_errors` +
`stream_internal_error_guard_ms`，UI 归属见已归档任务
`08-14-split-codex-stream-error-settings`），但它接不住本案例：

- 该机制匹配的是上游**主动发出的 SSE error 帧**（
  `success_event_stream.rs:234-256` 的 `is_terminal_error_sse_frame`，覆盖
  `error` / `response.error` / `response.failed` / `response.incomplete`）。
- 本案例属于 `StreamTerminalOrigin::UpstreamReadError`（`usage_tee.rs:506-517`）类的
  **传输物理中断**，上游未发任何 error 帧，没有关键词可供匹配。（`Unclassified`
  origin 生产不可达，三个生产调用点均被 `.with_terminal_evidence` 覆盖；
  `BufferedBodyEof` 在 SSE 路径永不产生。确证本案例 origin 的方法是读该 trace 请求
  日志 `activity_details_json` 的 `terminal_origin` 字段，写入见 `types.rs:132-150`、
  消费见 `request_end.rs:287-299`。）
- 该机制仅在 pre-commit 守护窗口内有效（默认 500ms，上限 5000ms），而本案例
  TTFB 2305ms 已远超窗口且流已 commit。

因此本任务面对的是既有能力的真实空白，不是配置错误。

### 不可逾越的物理约束

流一旦 commit（响应头与前缀 SSE 事件已发往客户端），则：

- 无法改写 HTTP 状态码——响应头已发出；
- 无法替换响应体——前缀事件已被客户端接收并渲染；
- **唯一可行动作是在 SSE 流末尾追加协议合法的 error 事件**。

故已 commit 场景的目标是「善后为可读错误」，而非字面意义的「拦截」。完整改写
（改状态码 + 换整个响应体）只在 pre-commit 阶段可行。

### `GW_STREAM_ERROR` 横跨 commit 两侧（关键实现约束）

同一个 `GW_STREAM_ERROR` 错误码会在 commit 前后两侧产生，两侧能力截然不同：

- **pre-commit 侧**（三个产生点）：`success_event_stream.rs:1099-1150`（首块读错误）、
  `:1206-1261`（空 event-stream）、`:1429-1481`（前缀期读错误）。这些走
  `record_system_failure_and_decide`，可重试、可切换供应商、**可完整改写**。
- **post-commit 侧**：`usage_tee.rs:506-517`、`:452`、`:949-952`、`timing.rs:146`。
  只能追加事件。

因此**不得仅按错误码或合成状态码一刀切匹配**，否则 pre-commit 场景会被错误降级为
「只能追加事件」，白白丢失本可完整改写的能力。

### 生产库实测分布（决定实现优先级）

对本机生产库 `~/.aio-coding-hub/aio-coding-hub.db` 的 `request_logs` 统计
（`error_code = 'GW_STREAM_ERROR'`，共 141 条）：

| 条数 | `terminal_origin` | cli_key |
| --- | --- | --- |
| 97 | 空 | codex |
| 25 | 空 | claude |
| 9 | 空 | grok |
| 5 | `terminal_frame` | codex |
| 4 | `upstream_read_error` | codex |
| 1 | `normal_eof` | codex |

其中 131 条 `terminal_origin` 为空，原因是 `usage_tee.rs:549` 调用的
`StreamActivityTracker::details_json`（`types.rs:121-130`）**不写** `terminal_origin`
字段，而只有 `terminal_details_json`（`types.rs:132-150`）才写。该路径本身位于
post-commit 侧。两组的 `chunk_count` 分别为均值 77 / 最大 1846 与均值 484 /
最大 2772，均证明流已大量转发。

结论：**post-commit「追加事件」覆盖绝大多数真实故障，是主战场**；pre-commit
「完整改写」属少数场景但仍不得降级（见 R2）。用户报告案例（TTFB 2305ms、总耗时
8931ms，远超 guard 窗口 500ms）属 post-commit。

附带发现的可观测性缺口 **`[2026-08-24 更正：该"缺口"不存在]`**：初版称
`usage_tee.rs:549` 不记录 `terminal_origin`、导致约 93% 的流式故障无法从日志定位终止来源。
**实测为假**——持久化日志本就记录（`streams/request_end.rs:293` 既有的
`terminal_details_json`）。详见 R10 的前提更正。

判定边界是干净的，可依赖：不存在 commit 布尔量，但 `build_stream_finalize_ctx` 的
SSE 调用点全仓唯一（`success_event_stream.rs:1544`，位于 commit 决策 break 之后），
故「持有 `StreamFinalizeCtx` ⇒ 已过 commit」是构造保证。commit 的三条判定 break 为
`success_event_stream.rs:1307-1329`（StartStreaming）、`:1365-1369`（guard 到期）、
`:1483-1486`（窗口内 EOF），随后 `:1673-1695` 构造响应（状态码取自 `:1673`，值为上游
200；响应头 `:1674-1677`）并 `abort_guard.disarm()` + `LoopControl::Return`；上抛链
`retry_engine.rs:82-85` → handler **沿途无二次改写机会**。另有持久化标记可用：
evidence 的 `disposition` 为 `"buffered_before_commit"`（`success_event_stream.rs:345`）
或 `"forwarded_after_commit"`（`domain/usage.rs:1241`）。注意
`session_manager.rs:134` 的 `committed: bool` 是 session 绑定语义，与此无关。

### 可复用的追加事件先例

commit 后向流尾追加内容已有实现先例：`usage_tee.rs:465-468` 在插件错误时先下发错误块
再停止（`stop_after_terminal_error = true` 配合
`return Poll::Ready(Some(Ok(chunk)))`）。R3 应复用该模式，无需从零构建注入能力。

### ~~附带缺陷：改写候选被覆盖丢弃~~ `[2026-08-24 更正：不是缺陷]`

> 本节初版把「改写候选被后续 attempt 覆盖」列为附带缺陷。**该判定错误，已更正。**
> 覆盖是契约明文要求的行为，见下。

`failover_loop/response/finalize.rs:266-268` 仅从 `last_outcome` 取
`error_response_rewrite`，而 `last_outcome` 全仓 9 个赋值点中只有 2 个携带
rewrite（`upstream_error.rs:861-863`、`thinking_signature_rectifier_400.rs:583`）；
其余 7 处经 `AttemptOutcome::new` 构造，rewrite 恒为 `None`（
`failover_loop/context.rs:393`）。因此供应商 A 命中规则、failover 到供应商 B 后 B 传输层
失败时（`attempt/attempt_record.rs:203` 以 `None` 覆盖），finalize 取不到 rewrite，
客户端收到 `finalize.rs:275-287` 的兜底 502。

**这是既定语义，不是缺陷**：契约
`.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:199-201`
规定 **「final failure projects the last evidence」**——最终失败投射的是最后一次证据。
最终失败是 B 的传输层失败，客户端理应看到反映 B 的响应；运营者为 502 配的规则针对的是
「上游确实返回 502」，而 B 从未返回 502。该语义另有固化测试
`routes.rs:10828 mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
把守，其改写文案字面写着 `"must not survive a later different failure"`。

误判复盘见 `research/last-outcome-rewrite-loss.md` 第 7 节。


## Requirements

- R1：网关合成的流式故障状态码必须能被上游错误拦截规则匹配。匹配键采用
  **网关合成的有效状态码**（`status_override` 后的值），而非上游真实状态码（200）。
  本期覆盖范围为两类静默截断故障：`GW_STREAM_ERROR`（→502）与
  `GW_STREAM_IDLE_TIMEOUT`（→524）。用户配置 502 即可命中前者，配置 524 命中后者。
  注意 524 走 `usage_tee.rs:363-383` 的**干净 EOF 路径，无 `Err` 分支**，需单独处理，
  仅挂在 `Err` 分支上的实现不会对它生效。
- R1a：为使关键词规则在流式路径可用，网关须为该类故障**合成一个伪 body** 供关键词
  匹配，内容为网关错误码与简短描述（如 `GW_STREAM_ERROR stream transport error`）。
  否则配了关键词的规则会因无响应体返回 `ConditionResult::Unknown` 而导致整条规则
  fail-open（`upstream_error_response_rules.rs:194-198`、`:326-333`）。伪 body 的语义
  是「网关自身故障描述」而非上游响应内容，必须在 UI 中明确说明，避免用户误以为可以
  匹配上游返回的文本。伪 body 不得持久化。
- R2：pre-commit 阶段（响应头尚未发出）命中规则时，**必须执行完整改写**：按规则的
  状态行为与消息行为构造协议兼容错误信封，复用既有 `build_response` 能力。三个
  pre-commit 产生点（`success_event_stream.rs:1099-1150`、`:1206-1261`、`:1429-1481`）
  一律不得降级为「仅追加事件」。实现必须显式区分 commit 两侧，禁止按错误码或合成
  状态码一刀切。
- R2a：pre-commit 命中规则时，既有的重试与供应商切换能力不得被改写逻辑短路——
  改写是重试预算耗尽后的终态行为，不是替代重试的前置拦截。
- R3：已 commit 阶段命中规则时，在 SSE 流末尾追加一个协议合法的 error 事件，
  事件文案由规则的消息行为决定。HTTP 状态码保持原值（200）不变，已发送的前缀
  事件不得改动或撤回。
- R4：追加的 error 事件必须符合目标 CLI 的流式协议格式。首要覆盖 codex
  （`/v1/responses` SSE）；claude / gemini / grok 若在流式路径产生同类合成故障，
  须按各自协议格式处理或明确记录为不适用。
- R5：R1 的匹配语义变更不得影响非流式路径。非流式路径继续以上游真实状态码
  匹配。两条路径的语义差异必须在 UI 与契约文档中写明，避免用户误配。
- ~~R6：修复改写候选覆盖缺陷~~ **`[2026-08-24 撤销]`**：该需求基于错误的缺陷判定，
  **已撤销，不再是本任务的验收项**。契约第 200 行「final failure projects the last
  evidence」明文要求覆盖行为，固化测试
  `routes.rs:10828 mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider`
  把守。曾按该需求实现的改动已在 Gate 3 撞上该测试后全部回退。若将来确需「早期改写优先」，
  须先变更契约与该测试并单独立项。详见 `design.md` §5 与
  `research/last-outcome-rewrite-loss.md` 第 3、7 节。
- R7：不破坏既有行为：现有 `stream_internal_errors` 关键词机制、pre-commit 守护
  窗口语义、内建 capacity alias、共享重试预算、退避、熔断计数、failover 决策、
  Provider override、分享/导入、`GW_FAKE_200` 行为一律保持不变。
- R8：日志与统计需与改写结果一致：追加 error 事件后不得把该请求记为成功；改写审计
  元数据仍只含有界的规则身份、供应商身份、前后状态与行为模式，不得持久化响应体、
  原始 SSE、规则关键词、伪 body 或凭据。
  **既有行为确认保留**：`request_end.rs:218-221` 当前把 attempt status 覆写为合成
  状态码（而非上游真实的 200），本任务不改动该行为——日志中显示合成码更便于识别
  失败。由此产生的「流式路径 attempt 记合成码、非流式路径记上游真实码」差异属
  既有事实，本任务只需在契约文档中如实记录，不做行为变更。
- R9：测试覆盖 pre-commit 改写、已 commit 追加事件、匹配语义（合成码命中/上游码
  不误命中）、rewrite 覆盖修复、成功后清空候选；受影响的 Rust 测试、前端测试、
  typecheck、lint、format、build 与 spec-link 检查必须通过。
- R10（附带）**`[2026-08-24 前提更正]`**：本条初版称「`usage_tee.rs:549` 不记录
  `terminal_origin`，导致约 93% 的流式故障无法从日志定位终止来源」。**该前提经实测为假。**
  持久化的请求日志**本就**记录 `terminal_origin`——最终写入走
  `streams/request_end.rs:293` 的 `terminal_details_json`（**既有代码，非本任务新增**）。
  实测方法：在端到端测试中断言 `activity_details_json` 含 `terminal_origin`，
  **临时回退 `usage_tee.rs` 的改动后该断言依然通过**。
  `usage_tee.rs` 那一行写的是 `touch_activity` 的**待定行**（`status IS NULL AND
  error_code IS NULL`，见 `infra/request_logs.rs:566-596`），随后被最终写入取代。
  **实际保留的改动**：该行改用 `terminal_details_json`，使**进行中（pending）**的活动行
  与最终行字段一致——对「从未完成最终写入」的行（进程异常、reconcile 兜底）有实际价值，
  但**不是**原文所称的日志缺口。仅新增字段，未改 `terminal_signal` 语义与字段名。

## Acceptance Criteria

- [x] 配置一条匹配 502 的拦截规则后，复现流式传输中断（上游 200 + 流中途断裂）
      时规则命中，客户端不再收到裸 502 或无声截断的 SSE。
      → 端到端测试 `truncated_codex_stream_appends_rule_error_event_instead_of_cutting_off`
      （真实截断桩：发头 + 一帧 → 等 700ms 越过 pre-commit 窗口 → 不发 `0\r\n\r\n` 断连）。
- [x] pre-commit 阶段命中规则时，客户端收到按规则状态行为与消息行为构造的协议
      兼容错误信封；状态码与文案均符合规则配置。
      → `precommit_stream_truncation_gets_the_full_rule_rewrite_not_a_tail_frame`
      （得 503 + 规则文案，body 内无 `response.failed`）。
- [x] 已 commit 阶段命中规则时，客户端在流末尾收到一个协议合法的 error 事件，
      文案取自规则；HTTP 状态码仍为 200；此前已下发的事件逐字节不变。
      → 同第 1 条测试断言 `status == 200`、前缀 delta 帧仍在、尾部有 `event: response.failed`。
      **两种截断落点各一条**：帧边界（同上）与**帧中间**
      `mid_frame_truncated_codex_stream_still_gets_the_rule_error_event`——后者是真实 TCP
      中断的主要形状，会让 codex terminal firewall fail-closed（`partial_frame_at_eof`），
      曾使尾帧被吞，已修（design §3.2.1.1）。
      claude 直连形状另有 `direct_stream_path_delivers_the_tail_frame_as_a_stream_item`
      （`event: error`）。
- [ ] **codex CLI 能正常解析并展示追加的 error 事件，不出现协议解析报错或界面卡在未完成
      状态。→ 只能真机验证，尚未完成**（仓库内无 codex CLI 源码或协议文档，无法用代码断言）。
- [x] 非流式路径的匹配行为无回归：仍以上游真实状态码匹配，既有测试全绿。
      → 后端完整回归 2998 passed / 0 failed；`upstream_error_response` 相关 23 条全通过。
- [x] ~~供应商 A 匹配 rewrite、供应商 B 传输层失败的多供应商场景下，最终响应采用
      A 的 rewrite~~ / ~~任一 attempt 成功时先前累积的 rewrite 候选被清空~~
      **`[2026-08-24 随 R6 撤销]`**。改为反向验收：跨供应商场景下 A 的 rewrite
      **必须不**存活到 B 的不同失败——契约第 200 行「final failure projects the last
      evidence」。已验证固化测试
      `mock_runtime_router_ready_provider_cap_stops_before_third_ready_provider` 通过。
- [x] 请求日志中该类请求不被记为成功；审计元数据不含响应体、原始 SSE、规则关键词或凭据。
      → 日志 `error_code = GW_STREAM_ERROR` / `status = 502`；已断言改写文案**未**出现在
      `special_settings_json` 中。审计另以 `scope = "stream_tail"` +
      `clientStatusApplied = false` 声明「规则的状态行为未生效」，UI tooltip 同步不再把
      规则状态当作已下发（详见 design §3.4.1）。
      **`[2026-08-24 表述纠正]`** 本条原写「attempt 状态保留上游真实状态」，
      **与实现相反**：attempt 记的是网关**合成**的 502（实测 `attempts[0].status == 502`，
      非上游真实的 200）。这是 R8 已接受的**既有**行为（`status_override` /
      `effective_status`），本任务未改动它；运营者视角下被截断的流本就应显示 502。
      已在端到端测试中固化该断言。
- [x] UI 与 `.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md`
      明确说明流式路径按合成状态码匹配、非流式路径按上游真实状态码匹配的语义差异，
      以及已 commit 场景只能追加事件、不能改状态码的限制。
      → 契约第 3 节与第 4 节矩阵已改；UI 三处文案已补；前端测试锁住文案。
- [x] 全部相关检查通过：Rust 测试、前端测试、typecheck、lint、build、spec-link。
      **`format` 除外且原因已查明**：`pnpm format:check` 唯一告警文件
      `src-tauri/tauri.conf.json` **不在本任务改动清单内**，且 `origin/main` 上同一文件
      同样不合 prettier 规范（已实测），属**基线既有违规**，未擅自修改。本任务触碰的所有
      前端文件均通过 `npx prettier --check src/components/cli-manager/`。

## Out Of Scope

- 不调整 pre-commit 守护窗口的默认值或上限（用户已明确选择「流尾追加事件」方案，
  不采用「扩大守护窗口」方案）。
- 不改动现有 `stream_internal_errors` 重试/不重试关键词机制的匹配逻辑与配置结构。
- 不为流式错误新增独立的规则集；复用现有上游错误拦截规则配置。
- 不改变 SSE 终态分类算法、缓冲上限、客户端脱敏、failover 或熔断算法。
- 不实现对已 commit 流的状态码或前缀事件修改——物理不可行。
- 不处理 `GW_FAKE_200`、`GW_EMPTY_RESPONSE` 等其他合成 502 来源的专门语义，除非
  它们天然被 R1 的合成状态码匹配覆盖；如需差异化处理另开任务。
