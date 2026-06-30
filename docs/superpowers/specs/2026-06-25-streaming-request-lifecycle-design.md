# Streaming Request Lifecycle Design

日期：2026-06-25

## Summary

本设计修复长时间流式请求在首页和请求日志中状态不可靠的问题。当前补丁为了避免 5 分钟以上流式请求从首页消失，放宽了前端展示层对 pending log 的过滤，但终态语义仍不够清晰：一些请求会在几百分钟后仍显示“进行中”。根因不是展示窗口长短，而是系统没有把“请求是否结束”严格绑定到流生命周期。

新的设计原则是：

1. 请求终态只能由真实生命周期事件产生：流自然结束、上游错误、客户端断开、网关停止/启动恢复。
2. 空闲或长耗时只作为可观测状态，不把请求强制标记为失败或完成。
3. 前端不再用固定时间窗口推断请求是否结束。
4. 后端写入足够的活动信息，让 UI 可以显示“进行中 · 已静默 N 分钟”这类解释性状态。
5. SSE/流式协议的终止标记作为完成判定的辅助事实，避免把截断流误判为成功。

本设计不改变 provider 路由、失败切换、计费和插件 API 的外部行为，只修正流式请求生命周期和 request log 投影。

## Current State

相关背景：

- Issue #304 描述的是：流式响应超过 5 分钟后，首页最近代理记录里的转圈记录暂时消失，直到流结束才重新出现。
- commit `6c4e3aa7a56b4c26a7c9bb78cbe590cc3439ce34` 通过保留 pending logs、启动/停止时 reconcile 未完成记录、调整前端活动投影修复了“消失”问题。
- 新问题是：有些请求几百分钟后仍处于“进行中”。这说明当前系统只是避免隐藏 pending，但没有足够可靠地区分“仍活跃”“已断开但未 finalize”“历史脏 pending”。

当前关键代码：

- `src-tauri/src/gateway/streams/timing.rs`
- `src-tauri/src/gateway/streams/usage_tee.rs`
- `src-tauri/src/gateway/streams/request_end.rs`
- `src-tauri/src/infra/request_logs.rs`
- `src/services/gateway/requestActivityProjection.ts`
- `src/services/gateway/requestLogState.ts`
- `src/services/gateway/traceStore.ts`
- `src/components/home/HomeRequestLogsPanel.tsx`

现有优点：

- 流式响应已经有 `StreamFinalizeCtx` 和 tee stream finalize 入口。
- request log 已支持 placeholder 和最终 log upsert。
- 启动/网关停止恢复可以 reconcile 未完成 pending。

主要缺口：

- 部分 stream wrapper 仍存在总时长 timeout 语义，可能把“时间到了”当作终止条件。
- request log 没有显式记录流最后活动时间，UI 只能用 created/duration/trace last seen 做间接推断。
- 前端的“进行中”状态主要由 `status == null && error_code == null` 推出，缺少“静默很久”的可解释状态。
- 如果最终事件丢失，pending log 只能等网关停止/启动恢复，用户在运行期无法判断它是活跃还是疑似卡住。
- 不同协议的流终止标记没有形成统一辅助判定，客户端断开、上游截断和自然完成的归因容易混在一起。

## External Reference

参考项目 `ding113/claude-code-hub` 的处理方式：

- 它把流式请求拆成首字节超时、流式静默期超时、非流式总超时三类配置。
- 活跃请求来自持久状态：dashboard 查询 `durationMs IS NULL`。
- 流式成功需要自然结束或终止标记，例如 `response.completed`、`message_stop`、`[DONE]`、`finish_reason`。
- 客户端断开和上游异常断开分开归因。
- 流式静默期超时会主动关闭客户端流并 abort 上游连接，但该超时是 provider 可配置项，`0` 表示禁用。

本项目不照搬“静默期超时主动终止”，因为产品决策是：连接没有断开时不因空闲强行结束。可吸收的是它的状态建模：终态由真实结束事件写入，活跃状态来自持久记录，终止标记用于辅助判断完成和截断。

## Goals

本阶段要达成：

1. 流式请求的完成状态只由流自然结束、上游错误、客户端 abort、网关停止/启动恢复决定。
2. 移除或降级固定总时长对流式响应的强制结束作用。
3. request log 持久化最近活动时间或等价观测信息。
4. 首页和请求日志能展示 pending 请求的空闲状态，例如“进行中 · 已静默 18 分钟”。
5. 长时间仍在输出的流不会因为固定窗口被隐藏或标为失败。
6. 长时间没有任何新数据的流不会被伪装成正常活跃；UI 要给出“疑似卡住”的提示。
7. 网关停止、应用启动恢复、通道关闭等场景仍能把未完成 pending reconcile 成稳定终态。
8. Claude、OpenAI Chat、OpenAI Responses/Codex、Gemini 流式响应的终止标记解析与 request finalize 对齐。

## Non-Goals

本阶段不做：

- 不新增 provider 级“静默超时后强制中断”的默认行为。
- 不新增 idle timeout 开关；空闲检测只服务于可观测展示，不作为生命周期控制。
- 不引入新的请求状态机表。
- 不改 provider failover 策略。
- 不改计费规则。
- 不改插件 API。
- 不改变插件 `gateway.response.chunk` hook 的输入、输出、调用顺序、错误 marker 和 audit/replay 结构。
- 不重做首页请求日志 UI。
- 不把历史所有 pending 记录做复杂迁移；只确保新逻辑和恢复逻辑稳定。

## Architecture

### 1. 请求生命周期边界

流式请求只有四类终态来源：

- `completed`：上游流自然结束，tee stream 读到 `None`，且没有错误码。
- `upstream_error`：读流过程中出现上游错误、stream error、fake 200、协议终止错误等。
- `client_aborted`：客户端断开导致流结束或 wrapper drop，并被识别为客户端中断。
- `reconciled`：应用启动或网关停止时发现仍未终态的 pending request log，由恢复流程写入 `GatewayStop` 或 `StartupRecovery`。

空闲、长耗时和 UI 展示窗口都不是终态来源。

对客户端断开要单独处理：如果客户端断开时后台已经读到了格式匹配的终止标记，可以把内部结果视为上游完成；如果没有终止标记，则按客户端 abort 或截断流处理。不能仅凭 usage token 大于 0 判断完成，因为部分协议会在首个或中间事件中提前带 usage。

### 2. 活动观测模型

request log 增加“最近活动”语义，字段为：

```text
last_activity_ms INTEGER NULL
activity_details_json TEXT NULL
```

`last_activity_ms` 的写入规则：

- placeholder 插入时设置为 `created_at_ms`。
- 流式 tee 每次读到 chunk 时更新内存中的最近活动时间。
- 后端最多每 30 秒把最近活动 flush 到 request log 一次；最终 finalize 必须无条件写入最后已知活动时间。
- 非流式请求可以保持为 `created_at_ms` 或最终完成时间；前端只对 pending 流式请求使用该字段。
- reconcile 未完成 pending 时保留最后活动时间，并把 reconcile 原因写入 `error_details_json`。

最近活动必须落在持久 request log 上，而不是只放在前端 trace store。这样前端刷新、应用重启后仍可解释 pending 状态。

持久化接口：

```text
request_logs::touch_activity(trace_id, cli_key, last_activity_ms, details)
```

约束：

- 只更新 `status IS NULL AND error_code IS NULL` 的 pending row。
- 不创建新 request log；placeholder 缺失时记录 warn 并等待最终 log upsert。
- `last_activity_ms` 只增不减。
- `activity_details_json` 只保存有界摘要，例如最近 chunk 时间、累计 chunk 数、协议终止标记是否已观察到，不保存响应正文。

`StreamFinalizeCtx` 中必须持有轻量 activity tracker，让 usage、活动时间和终止标记在同一个 finalize 上下文里收敛：

```text
StreamActivityTracker
  trace_id
  cli_key
  created_at_ms
  last_activity_ms
  last_flushed_activity_ms
  chunk_count
  terminal_signal
```

tee stream 收到 chunk 时只更新 tracker；当距离上次 flush 超过 30 秒时异步触发 `touch_activity`。finalize 时把 tracker 快照合并进最终 request log。

### 3. 流式终止标记

本阶段要补齐流式终止标记解析，并与 finalize 对齐。终止标记是辅助事实，不替代 `Poll::Ready(None)` / 上游关闭 / 错误事件这些真实生命周期事件。

实现时归一化为同一个内部信号，避免每个 wrapper 各自解释正文：

```text
StreamTerminalSignal
  kind: "completed" | "failed" | "incomplete" | "error"
  protocol: "openai_responses" | "openai_chat" | "anthropic" | "gemini" | "generic_sse"
  raw_marker: string
```

需要识别的标记：

```text
OpenAI Responses / Codex:
  completed: response.completed
  failed: response.failed
  incomplete: response.incomplete

OpenAI Chat Completions:
  completed: data: [DONE] 或 choices[].finish_reason 非 null
  error: data 中的 error 对象

Claude / Anthropic:
  completed: message_stop
  error: error

Gemini:
  completed: finishReason / finish_reason 非空
  error: error

Generic SSE:
  error: event/data 中的 error 对象或已存在 fake 200 检测命中
```

规则：

- 已知协议流自然结束时，必须根据归一化信号 finalize：`completed` 计为成功；`failed`、`incomplete`、`error` 按对应错误终态处理。
- 已知协议流上游关闭但没有 completion 信号：按上游截断或 stream error，而不是成功。
- Generic/未知协议流没有 completion marker 约定时，自然结束且没有错误信号才计为成功。
- 客户端 abort 且已观察到完成标记：可按上游已完成处理，并保留 `client_abort_after_completion` 观测信息。
- 客户端 abort 但没有完成标记：按客户端中断，不把部分 usage 当作完成证据。

解析位置应靠近 stream tee/finalize，避免前端根据正文或 trace 自行推断终态。

### 4. 终止标记与日志状态码

`HomeRequestLogsPanel` 当前通过 `RequestLogSummary.status`、`error_code` 和 `isPersistedRequestLogInProgress` 渲染状态徽标。终止标记不能只写入 `activity_details_json` 或 trace details；它必须先被后端归一化为最终 request log 的 `status/error_code/excluded_from_stats`，再由前端共享 helper 展示。

最终日志写入契约：

```text
terminal_signal=completed
  status: 上游成功状态码，通常为 2xx
  error_code: null
  excluded_from_stats: 0

terminal_signal=failed | incomplete | error
  status: 非成功状态码；若上游 HTTP 是 200，也要映射为 5xx/502 语义
  error_code: GW_STREAM_ERROR 或更具体的既有错误码，例如 GW_FAKE_200
  excluded_from_stats: 按现有错误统计规则处理

known protocol stream closed without completion signal
  status: 502
  error_code: GW_STREAM_ERROR

client abort before completion
  status: 499
  error_code: GW_REQUEST_ABORTED 或 GW_STREAM_ABORTED
  excluded_from_stats: 1

client abort after completion signal
  status: 上游成功状态码
  error_code: null
  details: 记录 client_abort_after_completion

gateway stop / startup recovery
  status: 499
  error_code: GW_REQUEST_INTERRUPTED_BY_GATEWAY_STOP 或 GW_REQUEST_INTERRUPTED_BY_RESTART
  excluded_from_stats: 1
```

关键约束：

- 不能出现 `status = 200` 且 `error_code = GW_STREAM_ERROR` 但首页仍显示“200 成功”的结果。后端必须把本阶段产生的协议错误写成非成功 status；前端 `computeStatusBadge` / `requestLogState` 也必须让终态错误码优先于成功 status，用来兜住历史或异常日志。
- `GW_STREAM_IDLE_TIMEOUT` 不得由本阶段的空闲观测产生；空闲只影响“进行中 · 已静默 N 分钟”文案。
- `HomeRequestLogsPanel` 不解析 SSE marker，不关心协议细节；它只消费 `requestLogState` 和 `computeStatusBadge` 给出的统一状态。
- `requestActivityProjection` 的 pending 判定仍以 `status IS NULL AND error_code IS NULL` 为准；一旦终止标记归一化为最终日志，就不能继续显示“进行中”。

### 5. 流式 wrapper 职责

`TimingOnlyTeeStream`、`UsageSseTeeStream`、`UsageBodyBufferTeeStream` 负责五件事：

- 透传 chunk。
- 收集 usage/ttfb/错误信息。
- 在真实终态时调用 request end finalize。
- 更新 activity tracker。
- 识别并记录协议终止标记。

它们不应使用固定总时长把一个仍连接的流变成 `Poll::Ready(None)`。如果需要观测长耗时，应通过活动字段、日志或 trace event 记录，而不是终止流。

本阶段不引入 idle timeout 配置或开关。任何“连续无数据后终止流”的新行为都不属于本设计。若现有代码存在 total timeout 或 idle timeout 会把仍连接的流主动 `Ready(None)`，实现时必须移除该终止行为或改为仅记录观测信息。首字节/connect 这类连接建立前的超时不在本设计范围内。

### 6. 插件兼容边界

流式生命周期监控不能影响现有插件系统。实现约束：

- `MaybePluginChunkStream` 和 `gateway.response.chunk` hook 的位置保持不变；插件仍然看到 response fixer / protocol bridge 之后、usage tee 之前的 chunk。
- lifecycle tracker 只观察 tee 收到的 chunk，不修改 chunk，也不插入、删除、重排 chunk。
- `PLUGIN_STREAM_ERROR_MARKER` 保持 `: aio-plugin-error\n`，插件 blocked/failed 产生的 SSE error chunk 仍按现有方式透传给客户端。
- 插件错误 marker 可以被 lifecycle tracker 识别为 `GW_STREAM_ERROR`，但不能改变插件 audit、execution report、replay fixture 的结构。
- 不在插件 hook input/output 中新增必填字段；插件 SDK 和 manifest 兼容范围不因本阶段变化而改变。
- `activity_details_json` 不保存响应正文或插件修改后的 chunk 正文，只保存计数、时间和终止信号摘要。

### 7. 前端投影

`requestActivityProjection` 继续合并 request logs 和 live traces，但状态显示要分层：

- `completed`：有 status 或 error_code。
- `in_progress_active`：pending，且最近活动距离当前时间低于提示阈值。
- `in_progress_idle`：pending，且最近活动距离当前时间超过提示阈值。
- `reconciled`：status/error_code 来自 gateway stop/startup recovery。

提示阈值只影响文案和样式，不改变请求终态。阈值使用 10 分钟，常量命名为 `PENDING_IDLE_NOTICE_MS`，避免被理解为生命周期 timeout。

UI 表达：

- 活跃：`进行中`
- 静默：`进行中 · 已静默 18 分钟`
- 长耗时但仍有活动：`进行中 · 已运行 43 分钟`
- 恢复终止：展示已有错误码和恢复原因。

### 8. 恢复与清理

`reconcile_unresolved_pending` 保留，但只在应用启动、网关停止等明确生命周期边界触发。它不应按 pending 年龄周期性扫掉仍可能活跃的请求。

reconcile 写入：

- `status = 499`
- `error_code = GatewayStop` 或 `StartupRecovery`
- `excluded_from_stats = 1`
- `error_details_json` 包含 `reason`、`reconciled_at_ms`、`pending_age_ms`、可选 `last_activity_ms`

## Data Flow

1. 请求进入 gateway，创建 placeholder request log，状态为空，记录 `created_at_ms` 和 `last_activity_ms`。
2. 上游返回流，tee stream 透传 chunk。
3. 每个 chunk 更新内存中的 last activity 和终止标记 tracker。
4. 每 30 秒最多 flush 一次 activity 到 pending request log。
5. 流自然结束、错误或客户端 abort 时，tee stream finalize，写入最终 request log，status/error_code/duration/token/last_activity/terminal marker 等字段完整。
6. 前端拉取 request logs 和 live traces，合并为活动投影。
7. 如果 log 仍 pending，前端根据 `last_activity_ms` 显示活跃或静默提示。
8. 应用启动或网关停止时，reconcile 未完成 pending。

## Error Handling

- 上游读流错误：最终 log 写入 stream error，不再保持 pending。
- 客户端 abort：最终 log 写入 client abort 语义，并排除统计或按现有规则处理。
- 日志队列拥塞：placeholder 和最终日志继续使用 write-through fallback，避免丢失终态。
- 活动更新失败：不影响流透传；最终 log 仍是更高优先级事实。
- 前端没有 `last_activity_ms`：回退到 created time，但文案不要暗示确定活跃。
- 终止标记解析失败：按真实流生命周期继续 finalize，同时在 details 中记录解析失败摘要，不阻塞透传。
- 前端展示状态不一致：把 `status/error_code` 到徽标文案的规则集中在 `HomeLogShared.computeStatusBadge` 和 `requestLogState`，不要在 `HomeRequestLogsPanel` 内部分散判断。

## Testing

后端测试：

- 长时间流式请求只要流未结束，不因固定总时长 finalize。
- 流自然结束后 pending log 更新为终态。
- 上游流错误后 pending log 更新为错误终态。
- 客户端 abort 后 pending log 更新为 abort 终态。
- 客户端 abort 但已观察到完成标记时，不把完成请求误记为失败。
- 上游关闭但没有完成标记时，不把截断流误记为成功。
- placeholder 写入后最终 log upsert 不会丢失 `last_activity_ms`。
- activity flush 每 30 秒最多一次，finalize 一定写入最终 activity。
- activity flush 只更新 pending row，不会修改 completed row。
- reconcile 只处理 status/error_code 为空的 pending rows，并保留 last activity。
- `failed`、`incomplete`、`error` 终止信号即使来自 HTTP 200 SSE，也会写成首页可识别的失败日志。
- 插件 blocked/failed 生成的 `PLUGIN_STREAM_ERROR_MARKER` 仍透传，且 request log 归一化为 stream error。
- 启用 `gateway.response.chunk` 插件后，插件收到的 chunk 内容、sequence 和 audit/report 行为不因 lifecycle tracker 改变。

前端测试：

- pending 且最近活动新鲜时显示进行中。
- pending 且最近活动很旧时仍显示进行中，但带静默提示。
- completed log 不显示静默提示。
- `status=200` 但带终态错误码的历史/异常日志不会在状态徽标中显示为成功。
- `client_abort_after_completion` 不显示为“已中断”，只在详情中保留观测信息。
- live trace 和 pending log 合并时不重复显示。
- 老 pending log 不再从列表消失。

集成验证：

- 运行 `cd src-tauri && cargo test request_logs --lib` 或更窄的相关测试。
- 运行 `pnpm test:unit -- src/services/gateway/__tests__/requestActivityProjection.test.ts src/components/home/__tests__/HomeRequestLogsPanel.test.tsx`。
- 运行 `pnpm tauri:check`。

## Success Criteria

- 一个仍在持续输出的 10 分钟以上流式请求始终在首页可见。
- 一个已断开的流式请求不会无限期保持“进行中”。
- 一个连接未断但长时间没有新数据的请求显示为“进行中 · 已静默 N 分钟”，而不是被隐藏或强制失败。
- 一个缺失终止标记的截断流不会被计为成功。
- 首页请求日志状态与终止标记归一化结果一致，不会把 fake 200、incomplete 或 stream error 显示为“成功”。
- 网关停止或应用重启后，遗留 pending log 会被 reconcile 成稳定终态。
- 没有新增 idle timeout 开关，也没有新增默认强制中断长流的固定时间。
