# 上游错误处理

## Goal

在现有“错误码匹配重试”区域建立统一的“上游错误处理”产品入口，把每次失败时执行的重试规则与仅在终态执行的最终响应改写明确分离；完成两个子任务的跨层合同、共同 UI、日志呈现与最终集成验收，同时不回退 fork 已有的路由、熔断探测、failback、Codex continuation、压缩请求体和退避行为。

## Background

- 本 worktree 基于 `12e565c0e7fbcb461f0ccb0fccaa5274846f8185`，包含 Codex zstd/br/gzip/deflate 请求体正常化和 transport retry backoff。
- fork 还拥有 circuit probe/failback、Provider route、Codex continuation、health-neutral、count-tokens 和严格辅助请求预算等参考仓库没有或结构不同的行为。
- 只读参考仓库固定为 `D:/UGit/aio-coding-hub-knaifen-reference@5b13683bd2a44699cd8c99e7aeffc317bcc19674`；功能参考提交为 `26c4e02`、`957b649`、`358a999` 和合并状态 `ca15f02`。
- 用户已经完成所有产品决策并批准创建任务、规划和实施；本任务没有待确认的产品问题。

## Task Map

- `08-05-upstream-error-response-rewrite`：最终 HTTP 4xx/5xx 响应改写的设置、运行时、日志、UI 和测试。
- `08-05-codex-stream-internal-error-retry`：原生 Codex Responses HTTP 200 SSE 流内错误的保护窗、分类、重试、审计、设置、UI 和测试。
- 父任务：统一产品入口、共享编辑体验、跨子任务合同、生成绑定、集成回归、浏览器验收、规范更新和最终归档。

## Requirements

1. 在当前错误码匹配重试所在区域提供“上游错误处理”入口，使用分段模式“重试规则”与“最终响应改写”。两种模式复用状态码/关键词编辑体验，但配置 schema、保存字段和执行时机必须独立。
2. 不允许一条规则同时携带 retry 和 rewrite 动作。retry 在匹配到的每次失败中参与现有 failover 决策，rewrite 只消费最终上游 HTTP 失败候选。
3. 页面遵循当前应用的 Lucide、Button、Switch、Tooltip、表单、密度和响应式模式；不复制原型的独立壳层，不嵌套卡片。
4. 首页、实时请求与日志详情必须区分客户端可见状态、upstream attempt 状态、最终改写命中和 Codex 流内错误证据；解析异常 fail open，不影响列表与详情渲染。
5. 设置、Provider override、分享/导入/备份、迁移与生成 TypeScript bindings 必须在两个子任务之间保持一致。
6. 重试、failover、quota、cooldown、circuit、probe/failback 只使用真实上游事实。最终响应改写不得影响这些决策；流内 configured retry 必须复用既有 retry budget/backoff/circuit 语义。
7. 保持 Codex 请求体内容编码正常化、transport backoff、continuation repair、fake-200、非目标 SSE、count-tokens、health-neutral、forced Provider 和严格 helper route 行为。
8. 所有持久化诊断有界且脱敏；不得保存正文、普通 SSE 输出、原始 SSE、规则关键词、自定义消息或凭据。
9. 按两个子任务分别形成原子实现提交；父任务仅追加跨子任务整合、规范和验收所需的最小提交。

## Acceptance Criteria

- [ ] 父子任务 PRD/design/implement 完整且子任务边界可独立验证。
- [ ] 统一入口在桌面和移动视口无溢出、遮挡或卡片嵌套，两个模式、弹窗、保存、启停与日志徽标可交互。
- [ ] retry 与 rewrite 的 backend schema、迁移、执行时机、日志事实源和测试相互独立。
- [ ] 最终 HTTP 改写只影响最终客户端响应；流内恢复只处理未桥接的原生 Codex Responses SSE。
- [ ] HTTP、transport、Codex stream-internal retry 共用配置重试计数，且仅最终 `RetrySameProvider` 等待一次 backoff；跨 Provider 不退避。
- [ ] zstd/br/gzip/deflate 请求正常化、Codex continuation、probe/failback、Provider route、health-neutral、count-tokens、严格辅助请求与非流 fake200 回归测试通过。
- [ ] Rust 单元/路由/迁移/日志测试、`cargo fmt --check`、`cargo check --locked`、`cargo clippy --all-targets --locked -- -D warnings` 通过。
- [ ] generated bindings、前端单测、typecheck、lint、build 和跨层合同测试通过。
- [ ] 使用 paused time 覆盖 guard/backoff；没有依赖墙钟的脆弱测试。
- [ ] 完成 Trellis check、spec 更新、任务归档和提交证据记录；未 merge main、未 push、未发布。

## Out Of Scope

- 不扩展 Claude/Gemini/Grok 或通用 SSE 的流内保护窗。
- 不让最终响应规则匹配 HTTP 200 流内错误或 transport error。
- 不实现下游已提交后的透明回滚、拼接或二次输出。
- 不修改参考仓库中与本功能无关的缺陷或产品差异。
