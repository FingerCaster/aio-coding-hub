# AIO Coding Hub 插件开发手册

本目录是 AIO Coding Hub 插件系统的中文入口。这里不再平铺所有专题文档；新开发者先读主线指南，需要细节时再进入参考目录。

插件当前稳定扩展面是本地网关、请求和响应 hook、日志脱敏、命令、provider extension values，以及由宿主渲染的配置和部分 UI 插槽。社区插件统一使用 Extension Host：`main` 指向打包后的 JavaScript 输出，`runtime.kind = "extensionHost"`，贡献点通过 `contributes.*` 和 `capabilities` 声明。

当前文档以现有代码为准：`protocolBridges` 只是 `manifest` 声明、能力依赖、贡献元数据和安装预检展示，执行链尚未接入；契约中列出的 UI 插槽也不都等于前端已挂载。当前前端已挂载的宿主渲染 UI 插槽是 `providers.editor.sections`、`settings.sections` 和 `logs.detail.tabs`。

## 先读什么

- [插件开发总指南](./developer-guide.md)：唯一主线入口，从创建 Extension Host 插件到本地回放、配置表单、打包和发布。
- [Privacy Filter 示例](./examples/privacy-filter.md)：查看官方 Extension Host 插件如何说明隐私过滤边界。
- [插件 API 参考](./reference/README.md)：查 `plugin.json`、hooks、capabilities、host-mediated context labels、config schema、SDK 和发布规则。

## 按目标查找

| 我想做什么 | 阅读 |
| --- | --- |
| 开发第一个插件 | [插件开发总指南](./developer-guide.md) |
| 给插件加配置项 | [Config Schema](./reference/config-schema.md) |
| 处理 Claude/Codex 请求结构 | [插件开发总指南：Hooks 与请求形态](./developer-guide.md#hooks-与请求形态) |
| 查 hook 触发时机 | [Hooks](./reference/hooks.md) |
| 查 capability 与贡献点依赖 | [Manifest](./reference/manifest.md) |
| 查 context/mutation label 风险等级 | [Permissions](./reference/permissions.md) |
| 打包发布 `.aio-plugin` | [Publishing](./reference/publishing.md) |
| 理解旧运行时为什么不开放 | [运行时说明](./runtime/README.md) |
| 理解架构和边界 | [插件架构说明](./architecture/README.md) |

## 目录结构

- `developer-guide.md`：开发者主线手册。
- `examples/`：官方 Extension Host 插件说明和社区示例方向。
- `reference/`：稳定 API 契约和工具链说明。
- `runtime/`：Extension Host lifecycle、流式响应，以及旧 WASM/process notes。
- `architecture/`：维护者视角的安全、隔离、性能和稳定性说明。
- `plugin-api-v1-contract.json`：机器可读的插件 API v1 契约。

## 推荐开发顺序

1. 明确插件目标和需要的 contribution point。
2. 编写 `plugin.json`，声明 `main`、`runtime.kind = "extensionHost"`、最少的 `contributes` 和 `capabilities`。
3. 在 `dist/extension.js` 中用 SDK 形状实现 `activate(api)`，例如 `api.gateway.registerHook`。
4. 准备 Claude 和 Codex/OpenAI Responses fixture。
5. 使用 `create-aio-plugin` 校验真实插件目录。
6. 在桌面应用中启用插件后，用宿主运行报告和导出的 replay fixture 覆盖目标 hook。
7. 行为稳定后再打包 `.aio-plugin`，需要可信分发时再补签名。

## 当前稳定性说明

- Extension Host 是唯一 community runtime。
- 第三方插件代码不在 Rust 主进程或 Tauri WebView 中执行。
- Manifest 校验只接受 Extension Host runtime、已激活 hooks、已知 contributions 和 capability 组合；reserved permissions 只作为内部/legacy host-mediated labels 保留。
- `protocolBridges` 当前是 MVP skeleton，不是可执行协议转换能力。
- `hostCompatibility.platforms` 当前随 `manifest`、预检和市场元数据展示，但不参与本地安装阻断或市场兼容性筛选。
- 当前只有 `official.privacy-filter` 是宿主内置官方隐私过滤插件。社区同类能力应实现为 Extension Host 插件。
- WASM、process 和第三方 native 运行时只作为 unsupported pre-release legacy runtime 说明出现，不是当前推荐路径。
