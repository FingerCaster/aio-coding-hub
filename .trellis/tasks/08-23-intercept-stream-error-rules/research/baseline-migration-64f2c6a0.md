# 基线迁移：459818cf → origin/main 64f2c6a0

**背景**：本任务的规划与研究（`prd.md`、`design.md`、`implement.md`、`research/` 其余
九份）完成于分支 `FingerCaster/beta-release-channel`（HEAD `459818cf`）。用户要求改动
基于最新 main，故迁移至 Orca worktree
`D:/OrcaProjects/aio-coding-hub-fork/intercept-stream-error-rules`
（分支 `FingerCaster/intercept-stream-error-rules`，base `origin/main` = `64f2c6a0`）。

旧分支落后 `origin/main` **53 个提交**，代码已发生实质变化。本文件记录重验结果。

**使用规则**：实现时一律以本文件的「新基线」列为准。其余文档中的行号若与此冲突，
以本文件为准。

## 1. 核心结论：设计与方案全部成立

| 关键假设 | 重验结果 |
| --- | --- |
| 规则首行拒绝非 4xx/5xx 上游状态 | ✅ 不变（`upstream_error_response_rules.rs:179-181`） |
| `status_override` 把 7 种错误码映射为 502 | ✅ 不变（`status_override.rs:11-17`） |
| 上游真实状态为 200（502 是日志投影） | ✅ 不变（`streams/request_end.rs:568`） |
| `match_response_rule` 生产调用点仅 2 个，均在错误路径 | ✅ 数量不变（行号变） |
| `retry_engine` 的 `is_none()` 守卫仍在（不覆盖 rewrite） | ✅ 仍在（`retry_engine.rs:336-338`） |
| 流式路径未接入 rewrite 匹配 | ✅ 不变 |
| **两侧分治前提（对应旧 `:874` 的赋值点在 pre-commit）** | ✅ **成立**，见 §3 |
| plugin 注入先例（M1）可复用 | ✅ 不变（`plugin_chunk.rs:15,121-123`） |

**结论：`design.md` 的架构、注入方案（P1）、`last_outcome` 修复方案（A）均无需变更**，
只需更新行号与 §6 的一处表述（见 §4）。

## 2. 行号对照表

### 2.1 未变动

| 锚点 | 行号 |
| --- | --- |
| `upstream_error_response_rules.rs` 状态拒绝 | 179-181 |
| `status_override.rs` 502 映射 | 11-17 |
| `plugin_chunk.rs` marker 常量 | 15 |
| `plugin_chunk.rs` `plugin_stream_error_chunk` | 121-123 |
| `streams/request_end.rs` `status_for_stream_request_log` | 175 |
| `streams/request_end.rs` attempt status 覆写 | 218 |
| `streams/request_end.rs` `special_settings_json`（审计时序锚点） | 309 |
| `failover_loop/response/finalize.rs` rewrite 取值 | 268 |

### 2.2 小幅偏移

| 锚点 | 旧 | 新 |
| --- | --- | --- |
| `streams/types.rs` `StreamFinalizeCtx` | 153 | **156** |
| `streams/types.rs` `details_json`（不写 origin） | 121 | **124** |
| `streams/types.rs` `terminal_details_json`（写 origin） | 132 | **135** |
| `streams/finalize.rs` `GW_STREAM_ERROR` 归类 | 41-45 | **42-46** |
| `streams/usage_tee.rs` `tracker.ingest_chunk` | 426 | **427** |
| `streams/usage_tee.rs` `stop_after_terminal_error` 字段 | 301 | **302** |
| `streams/usage_tee.rs` plugin chunk 识别 + 停流 | 465-468 | **466-467** |
| `streams/usage_tee.rs` `details_json` 调用（R10 目标） | 549 | **553** |
| `streams/request_end.rs` 上游真实 200 | 563 | **568** |
| `attempt/attempt_record.rs` `last_outcome` | 203 | **206** |
| `attempt/retry_engine.rs` `last_outcome`（带守卫） | 333 | **337** |
| `thinking_signature_rectifier_400.rs` `match_response_rule` | 574 | **576** |
| `thinking_signature_rectifier_400.rs` `last_outcome`+rewrite | 583 | **585-587** |

### 2.3 大幅偏移（实现时务必注意）

| 锚点 | 旧 | 新 |
| --- | --- | --- |
| `upstream_error.rs` `match_response_rule` 调用 | 750 | **842** |
| `upstream_error.rs` `last_outcome`+rewrite | 861-863 | **961-963** |
| `success_event_stream.rs` `build_stream_finalize_ctx`（SSE） | 1544 | **2305** |
| `usage_tee.rs` StreamError 产生点 | 452 / 506-517 / 949-952 | **454 / 509 / 1059 / 1404** |
| `usage_tee.rs` idle timeout（524）分支 | 363-383 | **~373** |

## 3. `last_outcome` 赋值点：从 9 个增至 10 个（已证实）

**带 rewrite（2 个，不变）**：

- `response/upstream_error.rs:961-963`
- `response/thinking_signature_rectifier_400.rs:585-587`

**不带 rewrite（8 个，比旧基线多 4 个）**：

| 位置 | 所属函数 | commit 侧 |
| --- | --- | --- |
| `attempt/attempt_record.rs:206` | 传输层失败 | — |
| `attempt/retry_engine.rs:337` | **有 `is_none()` 守卫，不覆盖** | — |
| `response/success_non_stream.rs:1209` | `handle_success_non_stream`(:603) | 非流式 |
| `response/success_non_stream.rs:1267` | 同上 ← **新增** | 非流式 |
| `response/success_non_stream.rs:1979` | 同上 ← **新增** | 非流式 |
| `response/success_event_stream.rs:825` | `record_buffered_provider_failure`(:549) | pre-commit |
| `response/success_event_stream.rs:970` | `finalize_sanitized_stream_terminal`(:842) | **pre-commit** |
| `response/success_event_stream.rs:1181` | `finalize_buffered_stream_error_response`(:1006) ← **新增** | pre-commit |
| `response/success_event_stream.rs:1593` | `handle_infinite_buffered_event_stream`(:1279) ← **新增** | 待确认 |

### 3.1 两侧分治前提仍成立（关键）

旧基线上 `:874` 属于 `finalize_buffered_stream_error_response`；新基线对应赋值点
`:970` 属于**另一个函数** `finalize_sanitized_stream_terminal`(:842)，函数名不含
"buffered"，一度疑似 post-commit。经读码证实仍是 **pre-commit**：

- `:872`：`neutral_reason = "upstream Codex Responses stream terminated before response commit"`
- `:874`：`evidence.set_disposition("sanitized_before_commit")`
- 其 `error_code` 为 `GatewayErrorCode::Fake200`，非 `StreamError`

**故 `design.md` §3/§4 的两侧划分前提成立，无需修订设计。**

### 3.2 新增赋值点反向印证方案 A 的正确性

赋值点从 9 增至 10（不带 rewrite 的从 7 增至 8），恰好说明：

- **方案 A**（在 `run_state` 维护独立的 `last_error_response_rewrite`）**天然免疫**赋值点
  数量变化，无需逐点改动；
- 方案 B（逐点透传 rewrite）需改 8 处，且此次基线迁移新增 4 处正是「未来新增赋值点会
  再次引入同类缺陷」的实证。

`implement.md` 步骤 3 继续采用方案 A，无需调整。

## 4. `design.md` §6 需修正的一处表述

原文称「`build_stream_finalize_ctx` 的 SSE 调用点全仓唯一」。新基线上该函数有 **3 个
生产调用点**：

- `success_event_stream.rs:2305`（SSE 路径，仍唯一）
- `success_non_stream.rs:745` ← **新增**
- `success_non_stream.rs:846` ← **新增**

新增两处属**非流式**路径，不涉及 SSE commit 语义。故「持有 `StreamFinalizeCtx` ⇒ 已过
commit」这一构造保证须收窄表述为：**「SSE 路径的 `StreamFinalizeCtx` 构造点唯一
（`:2305`），且位于 commit 决策 break 之后」**。§6 的规则传入方案（在该调用处从
`ctx: CommonCtx` 取）不受影响；但步骤 1.4 补齐构造点时需覆盖新增的两处非流式调用点。

## 5. 待确认（不阻塞实现起步）

- `success_event_stream.rs:1593` 所属的 `handle_infinite_buffered_event_stream`(:1279)
  是新函数，其 commit 侧归属未读码确认。由于方案 A 免疫赋值点数量，不阻塞步骤 3；但
  步骤 4/5 划分两侧动作前需确认它属 pre- 还是 post-commit。
- `usage_tee.rs:1404` 是新增的第 4 个 StreamError 产生点，其场景与是否需要注入未确认。
  步骤 4 实现前须逐一核对 4 个产生点（454 / 509 / 1059 / 1404）各自的 commit 侧归属。
