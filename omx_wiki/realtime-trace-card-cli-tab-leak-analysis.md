---
title: "Request Logs processing realtime card tab leak analysis"
tags: ["request-log", "trace-store", "logs-page", "realtime-card", "cli-filter"]
created: 2026-07-04T22:34:41.2488663+08:00
updated: 2026-07-04T22:34:41.2488663+08:00
sources: []
links: []
category: debugging
confidence: high
schemaVersion: 1
---

# Request Logs processing realtime card tab leak analysis

## 现象

Request Logs 页面在「处理中」状态会出现卡片串栏：Codex/Claude 的处理中卡片可能出现在另一个 CLI 栏目里，也可能在「全部」或自身栏目里暂时看不到，Claude 栏目里也可能同时混入 Codex 和 Claude 卡片。最终记录展示正常，问题集中在处理中实时卡片，不是普通 row 的最终归属错位。

## 结论

高置信结论：问题根因在前端筛选链路。`LogsPage` 对已落库的 `requestLogs` 和 `activeRequests` 做了 CLI tab 过滤，但传给 `HomeRequestLogsPanel` 的 `traces` 仍然是全局未过滤集合。`HomeRequestLogsPanel` 后续会用这些全局 trace 构造 `RealtimeTraceCards`，所以处理中卡片能跨 CLI tab 串栏。

这也解释了为什么最终记录正常：最终记录主要来自已经按 `cli_key` 过滤过的 `filteredLogs`，而串栏的是 trace store 驱动的处理中实时卡片。

## 直接证据

- `src/pages/LogsPage.tsx:109` 从 `useTraceStore()` 读取全局 `traces`。
- `src/pages/LogsPage.tsx:142` 开始的 `filteredLogs` 会按当前 `cliKey`、状态、错误码、路径过滤已落库记录。
- `src/pages/LogsPage.tsx:160` 开始的 `filteredActiveRequests` 会按当前 `cliKey` 等条件过滤 active request。
- `src/pages/LogsPage.tsx:300-302` 传入面板时，`activeRequests={filteredActiveRequests}` 和 `requestLogs={filteredLogs}` 是过滤后的，但 `traces={traces}` 仍是未过滤的全局 trace 集合。
- `src/components/home/HomeRequestLogsPanel.tsx:652-657` 把 `displayedRequestLogs`、`displayedActiveRequests` 和 `displayedTraces` 一起交给 `buildRequestActivityProjection`。
- `src/components/home/HomeRequestLogsPanel.tsx:872-874` 使用投影结果渲染 `RealtimeTraceCards`。
- `src/services/gateway/requestActivityProjection.ts:174-191` 从传入的所有 `traces` 构造 `mergedTraceMap` 和 `realtimeCards`，这里没有额外 CLI 过滤。
- `src/services/gateway/requestActivityProjection.ts:198-222` 会用 `visibleRealtimeTraceIds` 抑制相同 trace 的普通记录/active request 行，因此 trace 是否进入实时卡片会影响页面上最终看到的是卡片还是 row。
- `src/components/home/RealtimeTraceCards.tsx:104` 用 `!trace.summary` 判定进行中状态，`src/components/home/RealtimeTraceCards.tsx:356` 使用 `trace.cli_key` 展示 CLI 标识。
- `src/services/gateway/traceRequestLogMerge.ts:101` 合并 summary 时保留的是 `trace.cli_key`，不是重新从当前 tab 推导。
- `src/pages/__tests__/LogsPage.test.tsx:122-153` 现有测试只覆盖「live traces 传给 panel」，没有覆盖按 CLI tab 过滤 traces 的回归场景。

## 为什么表现会变化

- 选中 Claude tab 时，Codex 的落库记录和 active request 会被过滤掉，但全局 Codex trace 仍会传进面板，所以 Codex 的处理中卡片可能出现在 Claude tab。
- 「全部」或 Codex tab 有时看不到同一张处理中卡片，是因为这些 tab 里可能已经包含对应的落库记录或 summary，投影层会把 trace 合并为终态/普通记录，并通过 `visibleRealtimeTraceIds` 抑制重复展示。
- Claude tab 缺少对应 Codex 落库记录时，同一个全局 trace 更容易保留为 in-progress realtime card，于是看起来像「只在 Claude 里有」。
- 当 trace 还没有 summary、或者落库记录和 trace 的到达时序不同，就会出现「有时都存在、有时只在某个 tab、有时混杂」的非稳定表现。

## 证据与推断边界

已确认事实是：CLI tab 下 logs/activeRequests 被过滤，traces 未过滤；实时卡片由 traces 构建；最终记录由过滤后的 logs 主导。基于这些事实，可以高置信解释用户描述的「处理中卡片串栏、最终记录正常」。

仍然不能完全排除后端 `cli_key` 写入或 snapshot 错配，但只有在普通 row 或最终记录也出现在错误 CLI tab 时，这个方向才应重新提升优先级。就当前症状和代码路径看，后端归属错配是低置信备选假设。

## 修复方向

后续实现时，优先在 `LogsPage` 中派生 `filteredTraces`，至少应用 CLI tab 条件：

```ts
cliKey === "all" || trace.cli_key === cliKey
```

然后把 `filteredTraces` 传给 `HomeRequestLogsPanel`，而不是全局 `traces`。

需要另外决定 traces 是否跟随页面上的路径、状态、错误码过滤：

- 路径过滤可以按 `trace.method` 和 `trace.path` 组合文本过滤。
- 状态和错误码过滤只对已有 `trace.summary` 的 trace 有明确语义。
- 当前 `activeRequests` 在状态/错误码过滤开启时会被隐藏，trace 过滤策略最好与这个行为保持一致，避免处理中卡片绕过筛选条件。

## 回归测试建议

在 `src/pages/__tests__/LogsPage.test.tsx` 增加 CLI tab 回归测试：

- trace store 同时放入 Claude 和 Codex trace。
- 进入 Claude tab 时，传给 `HomeRequestLogsPanel` 的 traces 只包含 Claude。
- 进入 Codex tab 时，传给 `HomeRequestLogsPanel` 的 traces 只包含 Codex。
- 进入 all tab 时，两个 traces 都可见。

这个测试能锁住本次根因，防止以后只过滤 logs/activeRequests 而漏掉 traces。
