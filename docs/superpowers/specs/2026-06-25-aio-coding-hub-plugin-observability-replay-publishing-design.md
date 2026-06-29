# aio-coding-hub Plugin Observability, Replay, and Publishing Design

> Status: Superseded.
>
> MUST NOT be executed. This historical design contains obsolete
> declarativeRules and local `replay --explain` success-path assumptions. Use
> the Extension Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

日期：2026-06-25

## Summary

本阶段是插件生态从“能安装、能运行”走向“能诊断、能复现、能发布”的产品化阶段。当前分支已经完成 Plugin API v1、Gateway-first hook、声明式规则运行时、生命周期预检、远程包安装、rollback、quarantine、runtime hardening 和基础开发工具。下一步不应急着扩大插件权限，也不应引入新的高风险 runtime，而是把现有能力串成完整闭环。

核心目标是让插件作者和宿主用户都能回答四个问题：

1. 插件这次到底有没有运行？
2. 如果运行异常，异常发生在哪个 plugin、hook、runtime、策略或预算边界？
3. 能不能从真实 trace 导出最小 fixture，在开发者工具里复现？
4. 插件如何从示例、校验、打包、签名、市场索引到 GUI 安装形成稳定发布路径？

本设计保持 Plugin API v1 外部兼容，不改变 `plugin.json` v1 shape，不新增 Provider Plugin API，不开放 JS/TS/WebView runtime，也不把 Tauri2 桌面 GUI 变成浏览器式插件容器。

## Current State

当前插件系统已经具备较强基础：

- `docs/plugin-manifest-v1.md`、`docs/plugins/plugin-api-v1-contract.json`、Rust domain 和 `@aio-coding-hub/plugin-sdk` 已经稳定 Plugin API v1 contract。
- Active hooks 已覆盖 gateway request、response、stream、error 和 log-before-persist 的核心链路。
- Reserved hooks 和 reserved permissions 已被拒绝，`plugin.storage`、`network.fetch`、`file.read/write`、`secret.read` 等能力没有开放。
- 主力社区 runtime 是 `declarativeRules`；WASM runtime 仍受 host policy gate 控制；process runtime 仍是 PoC；第三方 native runtime 不开放。
- `official.privacy-filter` 已迁入插件体系，并使用 host-owned native implementation。
- `src-tauri/src/gateway/plugins/pipeline.rs` 已负责 hook 执行、failure policy、audit event、runtime failure、budget 和 circuit behavior。
- `src-tauri/src/commands/request_logs.rs` 已提供 `request_log_get_by_trace_id` 和 `request_attempt_logs_by_trace_id`。
- `src-tauri/src/commands/plugins.rs` 已提供 audit logs、market index parsing、remote install、local install/update/rollback/quarantine 等命令入口。
- `src-tauri/src/infra/plugins/market.rs` 已支持 market listing、兼容性、撤销、更新状态、checksum 和 signed index verification。
- `src/pages/PluginsPage.tsx` 已具备插件列表、详情、安装预检、更新 diff、启用/禁用/卸载、授权、配置、rollback、quarantine/revoked、lifecycle panel、runtime observability 基础展示。
- `packages/create-aio-plugin` 已提供 scaffold、doctor、validate/strict、replay/explain、pack、sign、verify。

主要缺口不是“没有插件 API”，而是以下能力还没有成为一等产品模型：

- 插件运行诊断仍主要分散在 audit logs、runtime failures、trace id 和 UI 文案中，缺少结构化 hook execution report。
- request trace 和 create-aio-plugin replay 之间没有宿主导出的标准 fixture 桥接。
- Rust 宿主声明式规则运行时和 TypeScript replay/explain 之间缺少长期 parity guard。
- 官方示例插件体系不足，当前示例主要围绕 privacy filter，无法覆盖 prompt helper、response guard、日志脱敏、Claude/Codex request shape 等常见路径。
- market index、remote install、checksum/signature 后端能力已有，但市场源配置、listing 展示、安装/更新状态、发布 metadata 还没有形成 GUI 和 CLI 的完整发布闭环。

## Goals

本阶段必须交付：

1. 结构化插件运行诊断模型，让宿主能稳定解释每次 hook 执行结果。
2. Trace replay fixture 导出能力，让真实请求日志可以转化为开发者工具可复现输入。
3. Host/runtime 与 devtools replay 的一致性测试和漂移防线。
4. 一组官方示例插件，覆盖真实开发路径而不是只展示 manifest 字段。
5. 插件市场和发布流程的产品化最小闭环。
6. 对用户文档、开发者文档、发布文档和验收测试进行同步更新。

## Non-Goals

本阶段不做：

- 不改变 Plugin API v1 外部字段形状。
- 不引入 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放 `plugin.storage`、`network.fetch`、`file.read`、`file.write`、`secret.read`。
- 不开放 JS、TypeScript、WebView/browser 插件 runtime。
- 不默认开放 marketplace WASM 执行。
- 不开放第三方 native runtime。
- 不做账号体系、评分、评论、推荐、支付或远程运营后台。
- 不做自动后台静默更新。
- 不让插件控制 provider selection、failover、OAuth、token counting、session binding。
- 不在运行诊断中保存完整敏感 payload；诊断只保存有界摘要、状态、原因和 trace 关联。

## Product Direction

### 1. 插件运行诊断

用户在插件详情页应该能看到“最近运行了什么”，而不是只看到一串 audit event。诊断视图应该按 plugin 和 hook 展示：

- 最近成功、失败、跳过、阻断次数。
- 最近一次运行时间、耗时和 trace id。
- runtime kind。
- failure kind、error code、failure policy。
- 是否因为权限、runtime policy、hook mismatch、context budget、output budget、artifact limit 或 circuit breaker 被拒绝。
- mutation 摘要，例如 body changed、headers changed、chunk changed、log redacted、blocked、warned，不展示完整原文。

这不是新的插件能力，而是宿主对已有执行行为的解释层。

### 2. Trace Replay

用户在请求日志或插件诊断里看到 trace id 后，可以导出 replay fixture。fixture 的目标是“最小可复现”，不是完整请求归档。

导出的 fixture 至少包含：

- trace id。
- hook name。
- provider/model/route 的必要元信息。
- request body 或 response/log 片段的有界内容。
- normalized messages，若宿主当时可生成。
- plugin-relevant headers/meta 的安全子集。
- attempts 摘要，用于解释 provider 路由和上游结果。

导出的 fixture 应能被 `create-aio-plugin replay --explain` 消费。无法导出的情况必须给出稳定错误原因，例如 trace 不存在、日志已被清理、body 不存在、内容超过导出上限、hook 不支持 replay。

### 3. Devtools 与宿主行为一致性

`create-aio-plugin replay --explain` 的价值取决于它和宿主真实 runtime 是否一致。后续需要建立共享 fixture 或 golden case：

- 同一插件、同一 fixture、同一 hook，在 Rust 宿主和 TypeScript replay 中得到一致的 action、mutation summary、block/warn/pass 结果。
- 对权限缺失、hook 不匹配、rule target 不兼容、context truncation、output budget、rule artifact 限制等边界给出一致诊断。
- devtools 可以清楚标注“宿主支持、replay 不支持”的少数情况，避免假装完全等价。

这层是未来继续扩插件生态的维护成本控制点。没有 parity guard，API 不变也会因为实现漂移而变得难维护。

### 4. 示例插件

示例插件应该服务于真实插件作者，而不是只做文档展示。建议先提供三类示例：

- `examples/prompt-helper`：在 `gateway.request.afterBodyRead` 或 `gateway.request.beforeSend` 追加提示词，展示 Claude 和 Codex/OpenAI Responses 两类 request shape。
- `examples/redactor`：使用声明式规则对请求和日志做脱敏，展示 `request.body.read/write` 与 `log.redact`。
- `examples/response-guard`：在 `gateway.response.after` 或 `gateway.response.chunk` 做告警、替换或阻断，展示响应侧 hook。

每个示例都必须能运行 `doctor`、`validate --strict`、`replay --explain` 和 `pack`。示例不依赖高风险权限，不使用 JS/WebView runtime，不要求 WASM 默认启用。

### 5. 插件市场和发布流程产品化

当前后端已经有 market index parsing 和 remote install，下一步要把它变成用户可操作的产品流程：

- GUI 能保存一个默认 market index URL，并允许用户手动输入临时 URL 加载一次。
- GUI 能加载 listing，并显示插件 id、name、latest version、risk labels、compatible、revoked、update available、install block reason。
- 用户能从 listing 安装或更新插件。
- GUI 展示 checksum、signature、source、trusted public key 相关状态。
- revoked 或 incompatible listing 必须明确禁用安装动作。
- `create-aio-plugin publish-check` 输出发布所需 metadata，包括 artifact path、sha256、signature 状态、manifest summary、compatibility 和 permissions。`pack/sign/verify` 保持现有职责，`publish-check` 负责把发布前检查结果整理成 market index 可引用的信息。

这仍是“最小市场闭环”，不做账号、评分、推荐和后台运营。

## Architecture

### 1. 运行诊断模型

新增 host-owned 诊断模型，建议命名为 `PluginHookExecutionReport`。它从真实 pipeline 执行中产生，并可以由 audit/runtime failure 聚合得到。

建议字段：

```text
PluginHookExecutionReport
  id
  traceId
  pluginId
  hookName
  runtimeKind
  status
  startedAtMs
  durationMs
  failureKind
  errorCode
  failurePolicy
  circuitState
  contextBudget
  outputBudget
  mutationSummary
  replayable
  replayExportReason
```

`status` 至少区分：

- `completed`
- `failedOpen`
- `failedClosed`
- `skipped`
- `blocked`
- `budgetRejected`
- `policyRejected`
- `circuitOpen`

`mutationSummary` 只描述变化类型、字段和大小，不保存完整敏感内容。

实现应增加轻量 `plugin_hook_execution_reports` repository/table，并继续保留现有 audit logs 作为 append-only 证据。这样 GUI 可以按 plugin、hook、trace、status 做稳定查询，不需要解析 audit details JSON；audit logs 仍负责生命周期和审计语义。关键原则是：pipeline 是事实来源，GUI 不自行推断 hook 是否成功。

### 2. Trace Fixture Exporter

新增 host-owned exporter，把 request logs、attempt logs 和 plugin execution report 合并为 devtools fixture。

建议模型：

```text
PluginReplayFixture
  schemaVersion
  source
    appVersion
    traceId
    exportedAtMs
  hookName
  request
    method
    path
    provider
    model
    headers
    body
    normalizedMessages
  response
    status
    headers
    body
    chunks
  log
    body
  attempts
  notes
```

不同 hook 只填充必要字段：

- request hook fixture 主要填 request。
- response hook fixture 主要填 request meta 和 response。
- stream chunk fixture 主要填当前 chunk 和有界窗口。
- log hook fixture 主要填 log-before-persist 所需字段。

导出命令放在插件命令边界中，命名为 `plugin_export_replay_fixture`。它以 `traceId`、`hookName` 和可选 `pluginId` 为输入，返回 fixture JSON 或稳定错误码。request log 命令继续负责原始日志查询，插件命令负责面向插件开发者的可复现导出。

### 3. Devtools Parity Layer

`packages/create-aio-plugin` 继续保持轻量，不把整个 Rust runtime 复制一遍。它应该共享 contract、fixtures 和 golden expectations：

- contract metadata 继续约束 hooks、permissions、runtime、failure policies。
- replay fixture schema 固定为 JSON，并在 SDK/devtools 测试中校验。
- Rust runtime tests 和 TypeScript replay tests 读取同类 fixture，断言相同的 explain summary。
- 对 devtools 无法模拟的宿主行为，输出 `unsupportedInReplay` 或等价 warning。

这层的重点是防漂移，不是创造第二套插件运行时。

### 4. GUI Surfaces

GUI 保持 Tauri2 桌面应用体验，不引入内嵌浏览器容器。

建议新增或整理以下区域：

- 插件详情页的“运行诊断”区域：按 hook 显示最近执行报告、失败原因、trace id、导出 replay fixture 动作。
- 请求日志详情中的“插件影响”区域：展示这个 trace 经过了哪些插件，以及每个插件的结果。
- 插件市场区域：加载 market index、展示 listing 状态、安装/更新。
- 发布信息展示：安装或更新前继续复用 preview/diff，同时展示 market source、checksum、signature。

GUI 只调用后端 command 返回的结构化结果，不在前端重新实现兼容性、权限、签名或 runtime policy 判断。

### 5. Documentation and Examples

文档需要同步更新：

- `docs/plugins/developer-guide.md`：加入 trace export -> replay -> fix -> pack 的开发路径。
- `docs/plugins/reference/publishing.md`：加入发布 metadata、market index、checksum/signature、revoked/incompatible 状态。
- `docs/plugins/examples/README.md`：列出官方示例插件和各自覆盖的 hook/permission/fixture。
- `docs/plugins/reference/hooks.md`：补充每个 hook 的 replay 支持状态。

示例插件应当作为测试资产使用，避免文档和真实工具链分离。

## Data Flow

### Runtime Diagnosis

```text
gateway request/response/log
  -> GatewayPluginPipeline
  -> runtime executor
  -> PluginHookExecutionReport
  -> audit/runtime failure persistence
  -> plugin command/query
  -> React Query
  -> PluginsPage diagnosis panel
```

### Trace Replay

```text
request_logs + attempt_logs + plugin reports
  -> replay fixture exporter
  -> fixture JSON
  -> create-aio-plugin replay --explain
  -> explain summary
  -> plugin author fixes rules
```

### Publishing

```text
plugin directory
  -> doctor / validate --strict / replay --explain
  -> pack
  -> sign / verify
  -> publish metadata
  -> market index
  -> GUI market listing
  -> install preview / update diff
  -> install or update
```

## Error Handling

运行诊断必须使用稳定 machine-readable code，不只依赖自由文本。建议错误类别包括：

- `PLUGIN_HOOK_TIMEOUT`
- `PLUGIN_RUNTIME_DISABLED`
- `PLUGIN_RUNTIME_POLICY_REJECTED`
- `PLUGIN_PERMISSION_DENIED`
- `PLUGIN_CONTEXT_BUDGET_EXCEEDED`
- `PLUGIN_OUTPUT_BUDGET_EXCEEDED`
- `PLUGIN_RULE_ARTIFACT_LIMIT_EXCEEDED`
- `PLUGIN_REPLAY_UNAVAILABLE`
- `PLUGIN_MARKET_INDEX_INVALID`
- `PLUGIN_MARKET_SIGNATURE_INVALID`
- `PLUGIN_MARKET_LISTING_REVOKED`
- `PLUGIN_MARKET_LISTING_INCOMPATIBLE`

导出 replay fixture 时，错误必须明确是“无法复现”还是“当前 hook 不支持复现”。这两个情况对用户行动不同：前者可能需要换 trace，后者需要改工具支持范围。

## Compatibility

本阶段保持以下兼容性：

- Plugin API v1 manifest shape 不变。
- `apiVersion` major 仍必须为 `1`。
- 现有插件包继续可安装和运行。
- `declarativeRules` 是默认社区 runtime。
- WASM、process、native 的公开策略不扩大。
- 已有 lifecycle preview/diff/rollback/quarantine 行为继续有效。
- 诊断和 replay fixture 是宿主/工具能力，不是插件可调用 API。

如果内部为了诊断新增数据库表或 command 返回字段，需要提供 migration 和前端空状态兼容。旧数据没有 execution report 时，GUI 应显示“无结构化诊断记录”，而不是报错。

## Testing Strategy

### Rust Backend

- Pipeline tests：成功、失败打开、失败关闭、timeout、circuit open、budget rejection、policy rejection 都产生正确 execution report。
- Repository tests：execution report 可按 plugin、hook、trace 查询，limit 生效。
- Command tests：诊断列表、trace fixture 导出、market listing install/update 状态返回稳定结构。
- Exporter tests：request/response/stream/log hook 的 fixture 字段符合 schema，超限内容被拒绝或截断并记录原因。
- Market tests：revoked、incompatible、checksum mismatch、signed index invalid、update available。

### TypeScript Devtools

- Fixture schema tests：宿主导出的 fixture 能被 devtools 读取。
- Replay parity tests：同一 declarative rule fixture 的 explain summary 与 Rust golden expectation 一致。
- Publishing tests：`publish-check` 输出 metadata，`pack/sign/verify` 继续通过现有职责测试。
- Example tests：官方示例能通过 doctor、validate strict、replay explain。

### Frontend

- Plugins page tests：诊断区域展示 completed/failed/skipped/blocked/budget rejected。
- Trace export action tests：可导出时显示动作，不可导出时展示原因。
- Market UI tests：revoked/incompatible 禁用安装，update available 显示更新入口。
- Empty state tests：没有诊断、没有 market source、没有 listing 时不误导用户。

### End-to-End Acceptance

1. 安装示例 redactor 插件，发送命中请求，插件详情页显示 hook completed、trace id 和 mutation summary。
2. 从该 trace 导出 fixture，用 `create-aio-plugin replay --explain` 得到同类 matched rule 和 mutation summary。
3. 修改规则导致权限或 target 不匹配，宿主诊断和 devtools explain 都给出一致原因。
4. 加载包含 revoked 和 incompatible 插件的 market index，GUI 禁用安装并展示原因。
5. 对一个可更新插件执行 market update，安装前仍显示 update diff、checksum/signature/source。

## Acceptance Criteria

- 用户可以从插件详情页看到结构化 hook 执行结果，不需要阅读原始 audit JSON。
- 用户可以从支持的 trace 导出 replay fixture，并用 devtools replay 复现声明式规则行为。
- Rust 宿主和 TypeScript devtools 对核心 declarative rules fixture 有 parity 测试。
- 官方示例插件至少覆盖 prompt helper、redactor、response guard 三类场景。
- 示例插件覆盖 Claude 与 Codex/OpenAI Responses request shape。
- GUI 能展示 market listing，并正确处理 compatible、revoked、update available 和 install block reason。
- 发布文档说明 pack、checksum、signature、market index、revocation、update metadata。
- 没有新增高风险插件 API 或 runtime。
- Plugin API v1 contract 文档、SDK 和宿主校验仍保持一致。

## Implementation Boundary

建议按以下顺序实现：

1. 运行诊断模型和查询能力。
2. Trace replay fixture exporter。
3. Devtools fixture schema 和 parity tests。
4. 官方示例插件和示例测试。
5. GUI 诊断区和 trace export 动作。
6. GUI market listing 和发布 metadata 工具/文档。

这个顺序让后续每一步都能复用前一步的证据链。市场和示例可以先做最小可用，但不应早于诊断和 replay，否则插件出问题时仍然缺乏排查闭环。

## Risks and Mitigations

- 风险：诊断记录过多导致数据库增长。
  - 缓解：限制每个插件或全局保留数量，按时间和 limit 查询；默认只保存摘要。
- 风险：replay fixture 泄露敏感内容。
  - 缓解：导出前使用有界内容、摘要和用户显式动作；不默认批量导出。
- 风险：devtools 和 host parity 维护成本过高。
  - 缓解：只对 declarativeRules 的核心行为做 golden parity，WASM/process/native 标记为不支持或 policy-gated。
- 风险：市场产品化扩大信任边界。
  - 缓解：market 只是 listing 和受约束安装入口，真实安装仍重新执行 checksum、signature、compatibility、runtime policy、permission policy。
- 风险：示例插件变成另一套维护负担。
  - 缓解：示例纳入 doctor/validate/replay/pack 测试，作为工具链验收资产。

## Fixed Design Decisions

本设计已固定以下决定：

- 不改 Plugin API v1。
- 先做诊断和 replay，再做示例和市场。
- replay fixture 是宿主/开发工具能力，不是插件可调用 API。
- market 产品化保持最小闭环，不做账号和运营后台。

后续 implementation plan 必须按这些决定拆分任务：

- 运行诊断新增轻量 `plugin_hook_execution_reports` repository/table，不让前端解析 audit JSON。
- Trace fixture 导出命令命名为 `plugin_export_replay_fixture`。
- 市场产品化第一版支持一个本地保存的默认 market index URL，并支持临时 URL 加载。
- 发布侧新增 `create-aio-plugin publish-check`，不把发布检查塞进 `pack` 的主职责里。
