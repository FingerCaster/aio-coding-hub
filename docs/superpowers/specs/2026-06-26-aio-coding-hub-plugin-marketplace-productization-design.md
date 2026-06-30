# aio-coding-hub Plugin Marketplace Productization Design

日期：2026-06-26

## Summary

下一阶段插件市场的目标不是扩大插件 API，也不是建设完整应用商店后台，而是把当前偏开发者调试的 market index 面板，收敛成普通用户能直接理解和操作的桌面 GUI 市场入口。

当前市场已经具备后端基础：market index parsing、remote install、checksum/signature 校验、trusted public key、revoked/incompatible block、update available 和 `publish-check` metadata。问题在于 GUI 把 URL、signature 和 index JSON 直接暴露给用户，导致用户打开插件页后不知道“该装什么、能不能装、是否可信”。下一阶段应采用“官方精选市场优先，高级自定义源折叠”的产品方向。

核心体验目标：

```text
打开插件页 -> 看到精选插件 -> 一眼判断用途/风险/兼容/信任 -> 安装或更新
```

本设计保持 Plugin API v1 外部兼容，不新增 Provider Plugin API，不开放 JS/TS/WebView runtime，不开放文件、网络、密钥或插件存储 API。市场 UI 只解释和触发现有宿主安装路径；真实安全边界仍由 Rust 宿主命令负责。

## Current State

当前实现已经具备：

- `src-tauri/src/infra/plugins/market.rs`：解析 market index，计算 compatible、revoked、updateAvailable、installBlockReason、checksum 和 signature 状态。
- `src-tauri/src/commands/plugins.rs`：提供 `plugin_parse_market_index`、`plugin_install_remote`、`plugin_quarantine_revoked` 等命令。
- `src/services/plugins.ts` 和 `src/query/plugins.ts`：提供 market index parsing 和 remote install 的前端封装。
- `src/pages/plugins/PluginMarketPanel.tsx`：提供市场 URL、签名、JSON 输入，解析后展示 listing 并触发安装。
- `src/pages/PluginsPage.tsx`：把 market panel 放在插件页中，市场安装走现有 `usePluginInstallRemoteMutation`。
- `packages/create-aio-plugin`：已有 `publish-check`，可以输出市场发布 metadata。
- `docs/plugins/reference/publishing.md`：已说明 market index、trusted public key、revoked/incompatible install blocks。

当前主要短板：

- 默认状态不是“市场”，而是“输入 index JSON 的工具”。
- 普通用户需要理解 market index URL、signature 和 JSON，才能看到插件。
- 插件卡片过度展示技术字段，缺少用途、风险、信任和动作的清晰层级。
- revoked、incompatible、missing checksum 等状态没有统一成用户可读决策文案。
- 自定义源能力没有产品边界，和普通安装路径混在一起。
- 没有默认官方精选源，导致市场入口打开时容易显得空。

## Goals

1. 插件页默认展示一个简洁的“精选插件”市场入口，不要求用户输入 JSON。
2. 普通用户能在插件卡片上看到插件用途、版本、安装状态、风险等级、兼容性和信任状态。
3. 未安装、可更新、已安装、不兼容、已撤销、缺少校验信息等状态都有明确按钮和解释。
4. 自定义 market index URL、index JSON、signature 等高级能力保留，但默认折叠到“高级来源”。
5. 安装和更新继续走现有 `plugin_install_remote` 或官方安装命令，不绕过宿主 checksum/signature/compatibility/revoked 检查。
6. 文档和测试覆盖“默认市场可用”和“高级源仍可用”两条路径。

## Non-Goals

本阶段不做：

- 不改变 Plugin API v1 manifest shape。
- 不引入 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放 `plugin.storage`、`network.fetch`、`file.read`、`file.write`、`secret.read`。
- 不开放 JS、TypeScript、WebView/browser 插件 runtime。
- 不默认开放 marketplace WASM execution。
- 不开放第三方 native runtime。
- 不做账号、评分、评论、推荐算法、支付或远程运营后台。
- 不做自动后台静默更新。
- 不做复杂多源优先级、订阅同步或市场源账号体系。
- 不把市场 UI 变成网页容器；它仍是 Tauri2 桌面 GUI 的本地 React 视图。

## Product Direction

### 1. 官方精选市场优先

插件页默认展示“精选插件”。这是一份宿主内置 catalog 或由宿主内置默认源解析出的列表。用户不需要知道它来自 JSON、URL 还是签名索引。

第一批精选插件应覆盖当前已确定的插件方向：

- `official.privacy-filter`：发送前脱敏敏感信息。
- `examples/prompt-helper`：请求发送前补充提示词约束。
- `examples/redactor`：规则化请求和日志脱敏。
- `examples/response-guard`：响应返回前检查或拦截。

其中 `official.privacy-filter` 可以走官方安装入口；社区示例如果还没有真实 artifact，则卡片可以显示“示例”或“待发布”，不能展示可点击安装按钮。市场 UI 不应伪造可安装状态。

### 2. 插件卡片按用户决策排序

每个市场卡片展示顺序：

1. 名称和一句话用途。
2. 状态 badge：已安装、可更新、未安装、不兼容、已撤销、示例。
3. 主动作：安装、更新、已安装、查看详情、不可安装。
4. 风险和信任摘要：低/中/高风险、已签名/未签名、官方/市场来源。
5. 次要技术信息：plugin id、版本、checksum、signature 只在展开详情时显示。

默认卡片不展示 JSON、完整 checksum 或签名字符串。用户需要的是“为什么能装/不能装”，不是原始发布字段。

### 3. 高级来源折叠

当前 `PluginMarketPanel` 的能力保留，但产品上变成“高级来源”：

- 市场索引 URL。
- 索引签名。
- 粘贴 index JSON。
- 加载临时市场。
- 显示解析结果。

该区域默认折叠，并用短文案说明它面向插件开发者或自定义源用户。高级源加载出来的条目也必须使用同一套卡片和状态规则，不应回到技术字段堆叠。

### 4. 安装边界保持宿主负责

GUI 可以提前禁用 revoked/incompatible/missing checksum 的条目，但不能把 UI 判断当成安全边界。点击安装或更新后，宿主仍必须重新执行：

- 下载 URL 校验。
- `.aio-plugin` 包限制。
- checksum 校验。
- signature/trusted public key 校验。
- manifest 校验。
- host/app/pluginApi/platform compatibility。
- runtime policy。
- permission policy。
- revoked/quarantine 状态检查。

失败时，前端显示宿主返回的错误，不在前端猜测或重写安全判断。

## UX Structure

### 插件页一级结构

插件页建议分为三个区：

1. **已安装插件**
   - 保留现有列表、详情、启用/禁用、配置、生命周期、运行观测。
   - 这是管理区。

2. **精选插件**
   - 默认展开。
   - 展示官方和推荐社区插件。
   - 空间上应比当前 JSON 面板更像可浏览的市场。

3. **高级来源**
   - 默认折叠。
   - 提供当前 market index JSON/URL/signature 调试能力。
   - 加载结果复用市场卡片。

### 卡片状态

| 状态 | 主按钮 | 说明 |
| --- | --- | --- |
| 未安装且可安装 | 安装 | checksum/downloadUrl/compatibility 满足基本条件 |
| 已安装最新版 | 已安装 | 禁用按钮，可进入插件详情 |
| 已安装且有更新 | 更新 | 更新仍走宿主预检和安装校验 |
| 不兼容 | 不可安装 | 显示需要的宿主或 Plugin API 范围 |
| 已撤销 | 已撤销 | 禁止安装；已安装插件应提示隔离或回滚 |
| 缺少校验信息 | 不可安装 | 缺少 checksum 或 download URL |
| 示例未发布 | 查看示例 | 不触发安装，可跳到文档或保留为不可安装说明 |

### 文案原则

- 用“可安装 / 不兼容 / 已撤销 / 缺少校验信息”代替 raw `installBlockReason`。
- 用“已签名 / 未签名 / 官方来源 / 自定义来源”代替 signature 原文。
- 风险标签面向用户说明能力，例如“读取请求内容”“修改请求内容”“日志脱敏”，而不是只展示 permission token。
- 技术细节可以在“更多信息”中展示，默认不占主视觉。

## Architecture

### 1. Market View Model

建议在前端引入 GUI view model，把后端 `PluginMarketListing` 和已安装插件状态合并成面向 UI 的结构：

```text
PluginMarketCardView
  pluginId
  name
  summary
  category
  latestVersion
  installedVersion
  state
  primaryAction
  riskLabel
  trustLabel
  sourceLabel
  blockReasonLabel
  listing
```

`state` 可以是：

- `installable`
- `installed`
- `updateAvailable`
- `incompatible`
- `revoked`
- `missingTrustData`
- `exampleOnly`
- `error`

这个 view model 只服务 GUI，不改变 Rust DTO，也不改变 Plugin API v1。

### 2. Featured Catalog Provider

第一阶段使用前端内置 catalog 常量，避免引入远程默认源的网络不稳定问题，也避免增加后端市场源持久化表。catalog 只作为 GUI 展示和动作路由输入，不改变 Rust DTO 或 Plugin API v1。

推荐原则：

- 如果是官方内置插件，用 `plugin_install_official`。
- 如果是有可安装 artifact 的市场插件，用现有 `plugin_install_remote`。
- 如果只是示例方向，没有 artifact，则标记 `exampleOnly`，不显示安装按钮。

后续如果要接真实官方 market index URL，可以在不改变 UI 的情况下替换 catalog provider。

### 3. Advanced Source Parser

高级来源继续调用 `plugin_parse_market_index`。解析后的 listing 进入同一个 `PluginMarketCardView` mapper。

这样可避免两套 UI：

```text
featured catalog -> normalize -> card view
advanced index -> plugin_parse_market_index -> normalize -> card view
```

### 4. Install Actions

主按钮只分发到现有 mutation：

- official plugin -> `usePluginInstallOfficialMutation`
- market plugin -> `usePluginInstallRemoteMutation`
- installed plugin -> select installed detail
- exampleOnly -> no install; disabled primary action with example label

按钮禁用状态来自 view model，但真实安装仍由后端校验。

## Testing Strategy

### Unit Tests

- `PluginMarketCardView` mapper：
  - 未安装 -> installable。
  - 已安装同版本 -> installed。
  - 已安装旧版本 + updateAvailable -> updateAvailable。
  - revoked -> revoked。
  - incompatible -> incompatible。
  - missing checksum/downloadUrl -> missingTrustData。
  - example without artifact -> exampleOnly。

### React Tests

- 插件页默认展示精选插件，不需要输入 JSON。
- 普通用户默认看不到市场 JSON textarea。
- 点击“高级来源”后可以看到 URL、签名、JSON 输入。
- 高级来源加载出的 revoked/incompatible 条目仍禁用安装。
- 可安装市场条目点击后调用 `usePluginInstallRemoteMutation`。
- 官方 Privacy Filter 点击后调用 `usePluginInstallOfficialMutation`。
- 已安装插件卡片显示“已安装”并不重复安装。

### Docs Checks

- 文档说明默认市场是精选入口，不是完整应用商店。
- 文档说明自定义源是高级功能。
- 文档保留 revoked/incompatible/checksum/signature 的宿主边界。

## Acceptance Criteria

- 用户打开插件页，不需要输入 JSON 就能看到精选插件。
- 默认市场视图没有暴露 market index JSON、signature 原文和完整 checksum。
- 每个卡片都有一句话用途、状态、风险/信任摘要和明确主按钮。
- 已安装、可更新、不可安装、撤销、示例未发布都有不同状态。
- 高级来源默认折叠，展开后保留当前自定义 index 能力。
- 高级来源和精选市场复用同一套卡片状态和安装逻辑。
- 安装/更新继续走现有宿主命令，不改变 Plugin API v1。
- 不新增任何高风险插件 API 或 runtime 能力。
- 测试覆盖默认精选市场和高级自定义源两条路径。

## Rollout Plan

1. 先实现前端 view model 和前端内置精选 catalog，不做后端 schema 或 DB 改动。
2. 重构 `PluginMarketPanel`，把默认精选市场和高级来源拆开。
3. 补齐插件页测试，确保普通用户路径不依赖 JSON。
4. 更新发布/开发文档，说明市场产品形态。
5. 未来再考虑默认远程官方 index、多源管理和市场源持久化。

## Risks

- 如果精选 catalog 里放入没有 artifact 的社区示例，用户可能误以为可以安装。必须用 `exampleOnly` 明确区分。
- 如果只做前端常量，市场内容更新需要发版。这个代价在个人项目阶段可以接受。
- 如果过早做多源管理，会增加设置、信任、刷新和冲突处理成本，不适合当前阶段。
- 如果 UI 过度隐藏风险信息，可能降低安全透明度。因此默认展示风险摘要，详细技术字段放进展开区。
