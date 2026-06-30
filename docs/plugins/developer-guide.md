# 插件开发总指南

这份指南把 AIO Coding Hub 插件从开发到发布的主线串起来。需要查字段定义时看 [Manifest v1 完整规范](../plugin-manifest-v1.md)，需要查某个运行时细节时再跳到 [API 参考](./reference/README.md) 或 [运行时说明](./runtime/README.md)。

Extension Host 是唯一 community runtime。社区插件使用 `runtime.kind = "extensionHost"`，`main` 指向打包后的 JavaScript 输出，并通过 `contributes` 和 `capabilities` 声明自己要接入的 host surface。

## 适合做成插件的能力

插件适合扩展本地网关链路中可被明确 hook 的行为，例如：

- 在请求发往上游模型前改写、追加或脱敏 prompt。
- 在响应返回给 CLI 前做有边界的检查、替换、阻断或告警。
- 在请求日志持久化前做不可逆脱敏。
- 注册命令、protocol bridge 或 provider extension values。
- 通过 `configSchema` 给用户提供可视化配置项，比如开关、选择器、文本输入和复选组。

插件不适合接管 CLI 内部编辑器、读取宿主数据库连接、直接操作 WebView，或把第三方 native 动态库加载进 Rust 主进程。

## 推荐开发路径

简写成一条主线就是：

```text
doctor -> validate --strict -> pack -> publish-check -> install/update -> export replay fixture -> fix -> reinstall
```

1. 明确插件目标和 contribution point：请求前处理通常用 `contributes.gatewayHooks` 注册 `gateway.request.afterBodyRead` 或 `gateway.request.beforeSend`，日志脱敏用 `log.beforePersist`。
2. 编写最小 `plugin.json`：声明 `main`、`runtime.kind = "extensionHost"`、必要的 `activationEvents`、`contributes`、`capabilities`、`hostCompatibility` 和 `configSchema`。
3. 在 `dist/extension.js` 中导出 `activate(api)`，并使用 `api.gateway.registerHook`、commands 或其他 Extension Host API 注册行为。
4. 准备 fixture：至少覆盖 Claude `messages[].content[].text` 和 Codex/OpenAI Responses `input[].content[].text` / `input_text` 形态，作为宿主 trace 复现和发布检查材料。
5. 使用 `pnpm --filter create-aio-plugin cli doctor` 和 `validate --strict` 做 package health、manifest 与入口文件校验。
6. 使用 `pnpm --filter create-aio-plugin cli pack` 打包为 `.aio-plugin`。
7. 发布前运行 `pnpm --filter create-aio-plugin cli publish-check ./acme.redactor` 生成 release metadata。
8. 在 Plugins 页面本地导入或从市场安装，先检查安装预检，再确认 capabilities、数据访问和贡献点影响，启用插件，检查审计日志。
9. 对真实请求问题，从宿主导出 `plugin_export_replay_fixture`，用导出的 trace、attempts、runtime reports 和本地 body fixture 复现。
10. 修复 Extension Host 入口后重新打包、安装并复测。
11. 发布前计算 `sha256`，可信索引分发时补 Ed25519 签名。

## 10 分钟快速开始

直接从完整示例模板 scaffold 一个 Extension Host 插件：

```bash
pnpm --filter create-aio-plugin test
pnpm --filter create-aio-plugin cli acme.redactor example:redactor
```

也可以选择其他示例模板：

```bash
pnpm --filter create-aio-plugin cli acme.prompt-helper example:prompt-helper
pnpm --filter create-aio-plugin cli acme.redactor example:redactor
pnpm --filter create-aio-plugin cli acme.response-guard example:response-guard
```

示例是开发模板，不是默认可安装市场包。它们用于学习 manifest、`dist/extension.js`、fixtures、`validate --strict`、`pack` 和 `publish-check` 的完整路径；Plugins 页面里的同名精选卡片仍保持示例状态，不会绕过宿主安装校验。

生成目录后，先检查 package health，再校验 `plugin.json` 和 Extension Host 入口：

```bash
pnpm --filter create-aio-plugin cli doctor ./acme.redactor
pnpm --filter create-aio-plugin cli validate --strict ./acme.redactor
```

添加 Claude 和 Codex request shapes 作为宿主 trace 复现材料。Claude fixture 示例：

```json
{
  "request": {
    "body": "{\"messages\":[{\"role\":\"user\",\"content\":\"SECRET_TOKEN\"}]}"
  }
}
```

Codex/OpenAI Responses fixture 示例：

```json
{
  "request": {
    "body": "{\"input\":[{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"SECRET_TOKEN\"}]}]}"
  }
}
```

当前 `create-aio-plugin replay` 不在本地执行 Extension Host gateway hooks；该命令会返回 `PLUGIN_REPLAY_UNSUPPORTED`。本地工具只负责 package health、manifest、入口文件、打包和发布 metadata 校验，hook 行为以宿主运行报告、导出的 fixture 和桌面应用内复测为准。

如果问题来自真实网关请求，先在 Plugins 页面或 request log 操作里导出 replay fixture。宿主命令名是 `plugin_export_replay_fixture`；它会把 trace id、hook name、plugin id、attempts 和 `plugin_hook_execution_reports` 放进 fixture。当前 request logs 不持久化完整 body，所以导出结果会在 `notes` 里说明缺口，插件作者需要用本地 fixture 补齐要复现的 request/response body。

打包插件并从 Plugins 页面本地安装：

```bash
pnpm --filter create-aio-plugin cli pack ./acme.redactor
pnpm --filter create-aio-plugin cli publish-check ./acme.redactor
```

在 Plugins 页面选择本地包 `acme.redactor.aio-plugin` 后，宿主会先展示安装预检。确认插件 id、版本、runtime、contributions、capabilities、host compatibility、checksum/signature 和风险提示无误后，再执行真实安装。安装后确认 `gateway.hooks` capability 并启用插件。命中请求后，插件详情面板应展示 hook completion 或 block/failure events，且不应存储 sensitive payload text。

`publish-check` 只输出市场发布 metadata，不写入插件包，也不替代 `pack`、`sign`、`verify` 或宿主安装时的 checksum/signature/compatibility/revoked 检查。

## 插件市场入口

Plugins 页面默认展示“精选插件”，面向普通用户提供简洁安装入口。用户不需要理解 market index JSON、signature 或 trusted public key，就可以看到官方 Privacy Filter 和推荐社区示例方向。

“高级来源”用于插件开发者或自定义源用户。它保留 market index URL、index JSON 和索引签名输入，但默认折叠。高级来源加载出的条目仍然走同一套安装卡片和宿主安装校验。

SDK 检查命令：

```bash
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
pnpm --filter create-aio-plugin test
```

## 插件目录结构

最小 Extension Host 插件结构：

```text
acme.redactor/
  plugin.json
  dist/
    extension.js
  fixtures/
    claude-request.json
    codex-request.json
```

`plugin.json` 必须位于包根目录，或 `.aio-plugin` 解压后的唯一顶层目录内。`main` 必须指向包内的 `.js` 或 `.cjs` 文件；TypeScript 源码可以保留在仓库里，但发布包应包含打包后的 JavaScript 输出。

## 最小 Extension Host 插件

`plugin.json`：

```json
{
  "id": "acme.redactor",
  "name": "Acme Redactor",
  "version": "0.1.0",
  "apiVersion": "1.0.0",
  "main": "dist/extension.js",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  },
  "activationEvents": ["onGatewayHook:gateway.request.afterBodyRead"],
  "contributes": {
    "gatewayHooks": [
      {
        "name": "gateway.request.afterBodyRead",
        "priority": 50,
        "failurePolicy": "fail-open"
      }
    ]
  },
  "capabilities": ["gateway.hooks"],
  "hostCompatibility": {
    "app": ">=0.60.0 <1.0.0",
    "pluginApi": "^1.0.0",
    "platforms": ["macos", "windows", "linux"]
  }
}
```

`dist/extension.js`：

```js
const secretPattern = /(api_key|token|password)=[A-Za-z0-9_-]+/gi;

module.exports.activate = function(api) {
  api.gateway.registerHook("gateway.request.afterBodyRead", function(context) {
    const body = String(context?.request?.body ?? "");
    const redacted = body.replace(secretPattern, "$1=[REDACTED]");
    if (redacted === body) return { action: "continue" };
    return { action: "replace", requestBody: redacted };
  });
};
```

关键规则：

- `id` 使用 `publisher.plugin-name`，不要使用 `official.*`，该命名空间只属于宿主内置插件。
- `version` 使用 SemVer。
- `apiVersion` 表示插件 API 契约版本，不等于应用版本。
- `runtime.kind = "extensionHost"` 是社区插件唯一运行时。
- `main` 指向打包后的 JavaScript 输出。
- `contributes.gatewayHooks` 决定插件在哪些阶段被调用。
- `capabilities` 必须包含对应贡献点需要的能力。
- `hostCompatibility` 决定应用版本、插件 API 版本和平台兼容性。

## Capability 依赖

| Contribution | Capability |
| --- | --- |
| `commands` | `commands.execute` |
| `providers` | `provider.extensionValues` |
| provider UI sections / fields | `provider.extensionValues` |
| UI button fields in host-rendered sections/panels | `commands.execute` |
| `gatewayHooks` | `gateway.hooks` |
| `protocolBridges` | `protocol.bridge` |

缺少依赖 capability 的 manifest 会被拒绝。不要把 capability 当成用户权限文案；它是宿主用来控制 Extension Host API surface 的契约。
`providers.card.badges` 和 `providers.card.actions` 当前不是 capability dependency trigger。

## Hooks 与请求形态

常用 hook：

- `gateway.request.afterBodyRead`：读取请求体后、发往上游前，适合隐私过滤、prompt 改写和请求体检查。
- `gateway.request.beforeSend`：完成 provider 解析和最终协议整理后、真正发送上游前，适合必须保证最终 upstream body/header 被修改的插件。
- `gateway.response.chunk`：处理流式响应 chunk，只能看到当前 chunk 和有界滑动窗口。
- `gateway.response.after`：处理非流式完整响应体。
- `log.beforePersist`：日志入库前脱敏。

Claude 和 Codex/OpenAI Responses 的请求结构不同。插件应避免只适配一种结构：

```json
{
  "messages": [
    { "role": "user", "content": [{ "type": "text", "text": "hello claude" }] }
  ]
}
```

```json
{
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [{ "type": "input_text", "text": "hello codex" }]
    }
  ]
}
```

宿主会在预算允许且当前 hook contract 提供对应 context 时暴露 `request.normalizedMessages`，用于给插件一个 provider-neutral 的消息视图。Extension Host manifest 不能声明 top-level `permissions`；需要修改原始请求体时，插件仍应返回 `requestBody`，但宿主会按 hook、capability 和 output budget 决定是否接受该 mutation。

## 配置表单

插件通过 `configSchema` 暴露用户配置。宿主负责渲染低代码表单并在保存前后校验。

示例：

```json
{
  "type": "object",
  "required": ["enabled", "mode"],
  "properties": {
    "enabled": {
      "type": "boolean",
      "title": "启用处理",
      "description": "关闭后插件不会修改请求内容。",
      "default": true,
      "x-aio-ui": { "widget": "switch", "order": 10 }
    },
    "mode": {
      "type": "string",
      "title": "处理模式",
      "default": "balanced",
      "enum": ["balanced", "strict"],
      "x-aio-ui": {
        "widget": "select",
        "enumLabels": {
          "balanced": "平衡",
          "strict": "严格"
        },
        "order": 20
      }
    }
  }
}
```

优先使用 JSON Schema 标准字段 `title`、`description`、`default`、`enum`、`required`。需要界面提示时再使用 `x-aio-ui`。

## 本地校验与宿主回放材料

常用命令：

```bash
pnpm --filter create-aio-plugin test
pnpm --filter create-aio-plugin cli acme.redactor example:redactor
pnpm --filter create-aio-plugin cli doctor ./acme.redactor
pnpm --filter create-aio-plugin cli validate --strict ./acme.redactor
pnpm --filter create-aio-plugin cli pack ./acme.redactor
pnpm --filter create-aio-plugin cli publish-check ./acme.redactor
```

`doctor` checks package health and reports structured diagnostics with `severity`, `code`, `message`, `path`, and `hint`.
`validate --strict` keeps Plugin API v1 compatibility but adds package-level checks for Extension Host `main`, artifact paths, target compatibility, hook contributions, capabilities, and package layout.
Warnings do not fail the command in 0.62.1; any `error` severity diagnostic returns a non-zero exit code.

验收一个插件时，至少确认：

- `plugin.json` 能通过校验。
- `main` 指向的 Extension Host 输出文件存在、位于包内、大小在限制内。
- Claude fixture 和 Codex/OpenAI Responses fixture 都能通过宿主运行报告、导出的 replay fixture 或集成测试覆盖预期行为。
- 未声明 capability 的贡献点不会生效。
- 打包后的 `.aio-plugin` 能在 Plugins 页面导入、确认并启用。

## 发布与升级

本地分发可以直接使用 `.aio-plugin`。远程分发必须提供可信的下载地址和 `sha256`。通过可信索引发布时，建议提供 Ed25519 签名和对应公钥。

升级时要特别检查：

- 新版本是否仍满足 `hostCompatibility`。
- 是否新增 capabilities 或贡献点。新增能力必须由用户重新确认。
- 是否改变 `configSchema`。需要通过 `configVersion` 和默认值保证旧配置可迁移。
- 回滚是否能恢复旧版本、旧配置快照和启用状态。

## 官方 Privacy Filter 示例

`official.privacy-filter` 是当前唯一内置官方插件。它对齐 [packyme/privacy-filter](https://github.com/packyme/privacy-filter) 的核心脱敏能力，用于请求发往上游前和日志持久化前的不可逆脱敏。完整说明见 [Privacy Filter 示例](./examples/privacy-filter.md)。

这个示例说明了三件事：

- 插件页面可以完全根据 `configSchema` 和 `x-aio-ui` 渲染配置，而不是写专用页面。
- 官方 bundled 插件应少而聚焦，避免扩大宿主维护面。
- 社区版同类插件应使用 Extension Host gateway hooks，而不是请求第三方 native 能力。

## Legacy runtime 迁移说明

WASM、process 和第三方 native 运行时属于 unsupported pre-release legacy runtime。当前公开 manifest validation 会拒绝这些运行时。旧插件应迁移为 Extension Host 插件，把逻辑放进 `dist/extension.js`，通过 `contributes.gatewayHooks` 声明 hook，再在 `activate(api)` 中调用 `api.gateway.registerHook`。

## 常见排障

- 插件无法启用：先看 `hostCompatibility`、runtime policy、manifest 校验错误、capability 依赖和配置是否有效。
- 报 `PLUGIN_MISSING_MAIN`：确认 `main` 指向包内 `.js` 或 `.cjs` 文件。
- 报 `PLUGIN_UNSUPPORTED_RUNTIME`：说明插件仍使用 unsupported pre-release legacy runtime，需要迁移到 Extension Host。
- 请求没有被改写：检查 hook 是否选对，是否声明 `contributes.gatewayHooks`、`capabilities: ["gateway.hooks"]`，fixture 是否覆盖实际 provider 请求结构。
- 只能看到原始本地请求：隐私过滤保护的是 gateway-to-upstream body 和持久化日志；client-to-gateway 的本地入站请求在 hook 前仍是原文。
- 日志仍有敏感值：确认插件注册 `log.beforePersist`，且 Extension Host hook 返回 `logMessage` mutation。
