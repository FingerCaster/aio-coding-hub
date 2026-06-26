# aio-coding-hub Plugin Example Developer Loop Phase 1 Design

日期：2026-06-26

## Summary

这一阶段的目标不是继续扩大插件 API，也不是把插件市场做成完整应用商店，而是补齐插件作者最需要的第一条路：从一个真实示例出发，跑通 `validate --strict -> replay --explain -> pack -> publish-check`，并理解这个示例如何对应 GUI 精选市场里的示例方向。

当前宿主已经具备 Plugin API v1、声明式规则运行时、运行诊断、replay fixture export、安装预检、remote install、市场解析和精选市场入口。短板在于示例插件仍主要停留在文档层：`examples/prompt-helper`、`examples/redactor`、`examples/response-guard` 已被市场卡片和文档提到，但还没有成为可由 `create-aio-plugin` 生成、校验、回放、打包和发布检查的开发资产。

本阶段将示例插件定位为“开发模板和测试资产”，不是“默认可安装市场包”。GUI 精选市场仍可展示这些示例，但必须保持 example-only，避免用户误以为宿主已经内置或发布了这些 artifact。

## Current State

已经具备的基础：

- `packages/create-aio-plugin` 提供 `doctor`、`validate --strict`、`replay --explain`、`pack`、`publish-check`、`sign`、`verify`。
- `packages/create-aio-plugin/src/scaffold.ts` 目前只有通用 `rule` 和 `wasm` scaffold。
- `packages/create-aio-plugin/src/devtools.ts` 已支持声明式规则校验、回放解释、打包和发布 metadata 输出。
- `src-tauri/src/infra/plugins/replay_export.rs` 已能从 trace 导出 host-owned replay fixture，但 request/response body 仍不会从 request logs 中恢复。
- `src/pages/plugins/PluginMarketPanel.tsx` 已把 `examples/prompt-helper`、`examples/redactor`、`examples/response-guard` 作为精选示例卡片展示，且不可安装。
- `docs/plugins/examples/README.md` 已列出这些示例方向，但使用“应包含”措辞，说明它们还不是实际可跑示例资产。

主要缺口：

- 插件作者不能直接生成 `examples/prompt-helper`、`examples/redactor`、`examples/response-guard` 的完整目录。
- 现有 `rule` scaffold 只覆盖一个简单 request body 替换规则，无法展示 Claude 和 Codex/OpenAI Responses 两种常见请求形态。
- 示例没有稳定 fixture，因此 `replay --explain` 不能证明示例和宿主行为一致。
- 示例没有被纳入 `pack` / `publish-check` 测试，市场发布 metadata 路径仍偏抽象。
- 文档、精选市场卡片和 devtools 之间尚未形成同一套“示例是开发模板、不是已发布包”的产品边界。

## Goals

1. 为三类示例提供可生成、可校验、可回放、可打包、可发布检查的模板资产：
   - `examples/prompt-helper`
   - `examples/redactor`
   - `examples/response-guard`
2. 让 `create-aio-plugin` 支持生成这些示例，输出完整目录结构。
3. 每个示例至少包含 `plugin.json`、规则文件、fixtures 和 README。
4. 每个示例都能通过 `validate --strict`。
5. 每个示例至少有一个 fixture 能通过 `replay --explain` 得到预期 mutation / warning / block 结果。
6. 示例能被 `pack` 打包，并能通过 `publish-check` 输出市场发布 metadata。
7. 文档明确这些示例当前是开发模板，不是默认可安装市场 artifact。

## Non-Goals

本阶段不做：

- 不改变 Plugin API v1 manifest shape。
- 不新增 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放 `plugin.storage`、`network.fetch`、`file.read`、`file.write` 或 `secret.read`。
- 不开放 JS、TypeScript、WebView/browser 插件 runtime。
- 不默认启用 marketplace WASM execution。
- 不新增第三方 native runtime。
- 不把示例插件内置进宿主 runtime。
- 不发布真实默认远程 market index。
- 不把 `examples/*` 精选卡片改成可安装状态。
- 不做账号、评分、评论、推荐、支付或市场运营后台。

## Product Direction

### 1. 示例是插件作者的起点

示例插件要回答“我要做类似能力，从哪里开始”。它们应当像真实插件一样组织目录，而不是只有文档片段。

固定 CLI 入口：

```bash
pnpm create-aio-plugin acme.prompt-helper example:prompt-helper
pnpm create-aio-plugin acme.redactor example:redactor
pnpm create-aio-plugin acme.response-guard example:response-guard
```

本阶段不新增 `create-aio-plugin example ...` 子命令，避免同时维护两套入口。现有 `create-aio-plugin <publisher.plugin-name> [template]` 命令继续作为唯一 scaffold 入口。

### 2. 示例必须覆盖真实 request / response shape

示例不能只匹配一个玩具字符串。第一阶段至少覆盖：

- Claude messages 结构。
- Codex/OpenAI Responses input/content 结构。
- 对 response 示例，至少覆盖 non-stream response body；stream chunk 可以保留为文档说明或 fixture note，不强行在第一阶段做完整流式 replay。

### 3. 示例仍是 example-only 市场卡

GUI 精选市场继续展示这些示例，但按钮保持“示例”且 disabled。示例资产只是开发模板，不代表宿主已经提供经过签名、托管和可安装的 `.aio-plugin` 包。

后续阶段如果要把示例发布成可安装 artifact，应另开阶段处理 artifact 托管、checksum/signature、公钥、默认源和撤销策略。

## Example Definitions

### `examples/prompt-helper`

目标：在请求发往 provider 前追加一条约束或提示片段。

配置：

- runtime：`declarativeRules`
- hooks：`gateway.request.afterBodyRead`
- permissions：`request.body.read`, `request.body.write`
- fixtures：
  - `fixtures/claude-request.json`
  - `fixtures/codex-request.json`
- expected behavior:
  - Claude fixture 中追加或改写用户消息中的提示片段。
  - Codex/OpenAI Responses fixture 中追加或改写 `input_text`。

### `examples/redactor`

目标：展示社区声明式规则脱敏，覆盖请求和日志。

配置：

- runtime：`declarativeRules`
- hooks：`gateway.request.beforeSend`, `log.beforePersist`
- permissions：`request.body.read`, `request.body.write`, `log.redact`
- fixtures：
  - `fixtures/request-hit.json`
  - `fixtures/request-miss.json`
  - `fixtures/log-redact.json`
- expected behavior:
  - 命中 secret/token/email-like 字段时替换为 `[REDACTED]`。
  - 未命中 fixture 不产生 mutation。
  - log fixture 只展示日志脱敏语义，不保存敏感原文到文档正文。

### `examples/response-guard`

目标：展示响应侧轻量检查、告警或阻断。

配置：

- runtime：`declarativeRules`
- hooks：`gateway.response.beforeSend`
- permissions：`response.body.read`, `response.body.write`
- fixtures：
  - `fixtures/response-warn.json`
  - `fixtures/response-pass.json`
- expected behavior:
  - 命中风险文本时产生 warning 或替换标记。
  - 未命中 fixture pass。

## Architecture

### 1. Scaffold Model

扩展 `packages/create-aio-plugin/src/scaffold.ts`，把模板从通用 `rule | wasm` 扩展为：

```ts
type ScaffoldTemplate =
  | "rule"
  | "wasm"
  | "example:prompt-helper"
  | "example:redactor"
  | "example:response-guard";
```

每个 example template 返回 `ScaffoldFiles`，包含：

- `plugin.json`
- `rules/main.json`
- `fixtures/*.json`
- `README.md`

模板内容应保持纯数据和字符串生成，不引入文件系统副作用。文件写入仍由 `runCreateAioPluginCli` 负责。

### 2. Devtools Validation Loop

`packages/create-aio-plugin/src/scaffold.test.ts` 应把示例作为真实资产验证，而不是只检查字符串存在：

```text
createPluginScaffold(example)
  -> validatePluginFilesStrict(files)
  -> replayHookExplain(files, fixture, hook)
  -> packPlugin(files)
  -> publishCheckPluginBytes(...)
```

测试应断言：

- strict validation ok。
- replay explain 返回预期 `actionKind` / `mutationSummary` / `matchedRuleIds`。
- pack checksum 符合 `sha256:<64 hex>`。
- publish-check 输出 manifest id、runtime、hooks、permissions 和 compatibility summary。

### 3. CLI Routing

保持 CLI 简单。推荐继续使用现有入口：

```bash
create-aio-plugin <publisher.plugin-name> [template]
```

新增模板值：

- `example:prompt-helper`
- `example:redactor`
- `example:response-guard`

这样不会引入新的 command parser 分支，也能复用现有写文件逻辑。

### 4. Documentation

更新文档，统一三处说法：

- `docs/plugins/developer-guide.md`：示例开发路径从 `create-aio-plugin ... example:*` 开始。
- `docs/plugins/examples/README.md`：把“应包含”改成“包含”，并列出生成命令和 fixture。
- `docs/plugins/examples/privacy-filter.md`：继续说明 `official.privacy-filter` 是唯一内置官方插件，其他示例是开发模板。
- `docs/plugins/reference/publishing.md`：说明示例模板可跑 `publish-check`，但不代表已发布到默认市场。

### 5. GUI Consistency

本阶段不需要改 GUI 行为。只需确保文档说法与现有精选市场一致：

- `examples/prompt-helper`、`examples/redactor`、`examples/response-guard` 是示例方向。
- GUI 卡片仍保持 disabled example-only。
- 真实可安装状态留给未来发布 artifact 阶段。

## Data Flow

```text
create-aio-plugin acme.prompt-helper example:prompt-helper
  -> write plugin.json / rules / fixtures / README
  -> validate --strict
  -> replay --explain fixtures/...
  -> pack
  -> publish-check
  -> author reads metadata and docs
  -> GUI featured market still shows example-only until artifact is published
```

## Testing Strategy

### Unit Tests

Add tests in `packages/create-aio-plugin/src/scaffold.test.ts`:

- generates prompt-helper example with Claude and Codex fixtures.
- prompt-helper validates and replay explains a request mutation.
- generates redactor example with request/log fixtures.
- redactor validates and replay explains hit and miss paths.
- generates response-guard example with response fixtures.
- response-guard validates and replay explains warning/replacement and pass paths.
- all examples pack and publish-check successfully.

### Docs Checks

Update `scripts/check-plugin-system-docs.mjs` to require stable phrases:

- `example:prompt-helper`
- `example:redactor`
- `example:response-guard`
- `示例是开发模板，不是默认可安装市场包`

### Regression Checks

Run:

```bash
pnpm --filter create-aio-plugin test
pnpm check:plugin-system-docs
pnpm check:spec-links
pnpm typecheck
pnpm lint
```

If implementation changes docs only or package-only files, still run the package tests and docs checks. If generated bindings are untouched, do not regenerate them.

## Acceptance Criteria

- `create-aio-plugin <id> example:prompt-helper` creates a runnable prompt helper example.
- `create-aio-plugin <id> example:redactor` creates a runnable redactor example.
- `create-aio-plugin <id> example:response-guard` creates a runnable response guard example.
- Each example has `plugin.json`, rules, fixtures, and README.
- Each example passes strict validation.
- Each example has at least one fixture that produces expected `replay --explain` output.
- Each example can be packed.
- Each example can produce `publish-check` metadata.
- Documentation explains how to choose and run examples.
- Documentation and GUI wording remain consistent that examples are development templates, not default installable market packages.
- No Plugin API v1, runtime capability, backend install boundary, or market trust boundary changes are introduced.

## Risks

- Declarative rule runtime may not express every desired prompt/response transformation elegantly. If a behavior is too complex, prefer a simpler example that demonstrates the hook and fixture path over adding runtime features.
- Tests can become brittle if they assert full JSON output. Prefer checking stable fields: manifest id, matched rule ids, action kind, mutation summary and checksum shape.
- Users may confuse example templates with market packages. Keep GUI disabled state and docs wording explicit.

## Future Work

After this phase:

- Publish selected examples as signed `.aio-plugin` artifacts.
- Add default official market index metadata for installable examples.
- Add richer GUI “open example docs” action for example-only cards.
- Expand replay parity coverage between Rust host and TypeScript devtools.
- Add stream chunk replay fixtures once stream replay behavior is stable.
