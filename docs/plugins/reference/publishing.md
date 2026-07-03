# 插件发布

插件包格式是 `.aio-plugin`。它本质上是一个 zip archive，`plugin.json` 必须位于压缩包根目录，或唯一顶层目录内。

发布检查清单：

- 校验 `plugin.json`。
- 控制 package size 和 entry count。
- 对 package bytes 计算 `sha256`。
- 使用 `publish-check` 输出 release metadata。
- 通过可信 index 发布时，用 Ed25519 签名 release metadata。
- 对新增 capabilities、runtime/hook/config 变化和 breaking update 写清升级与 rollback 说明。

当前实现支持本地/离线包导入、受约束的远程 `.aio-plugin` 下载、checksum/signature verification、更新时的 capability/contribution diff、已撤销插件 quarantine，以及 rollback snapshots。

## 0.62.2 生命周期行为

0.62.2 把安装、更新、回滚和隔离相关信息收口为宿主侧 lifecycle explanation layer。它不改变 `plugin.json` v1 的字段形状，也不新增插件可调用 API。

### 安装预检

用户从 Plugins 页面导入本地 `.aio-plugin` 时，宿主会先读取包并展示安装预检。预检会说明插件身份、来源、runtime、contributions、capabilities、host-mediated risk labels、兼容性、checksum/signature、已有安装覆盖关系、warnings 和 blocking reasons。

预检通过不等于安装已经完成。用户确认后，真实安装仍会重新执行包解压安全检查、manifest 校验、checksum/signature verification、host compatibility、runtime policy 和 capability/contribution policy。发布者应该把预检视为“给用户解释将要发生什么”，而不是绕过安装校验的入口。

### 更新差异

本地更新前，宿主会比较当前已安装版本和待安装包，展示：

- version direction。
- runtime change。
- hook added/removed/changed。
- capability unchanged granted、unchanged pending、added pending、removed。
- `configVersion` change。
- compatibility 和 trust change。
- 当前版本是否可回滚。

新增 capability 必须进入 pending。发布者可以在 release notes 中提前说明新增 capability 的原因，但宿主不会因为插件升级而静默授予新 capability。

### 回滚与隔离

更新会保留可回滚的历史快照。rollback 只允许回到仓库已记录且仍可用的历史版本；如果当前版本的安装目录已经缺失，宿主会在更新预览里标记不可回滚。

撤销或宿主判定危险的插件会进入 `quarantined`。隔离插件不能启用；用户需要卸载、回滚到可用版本，或等待可信来源发布新版本。隔离和回滚会保留 audit 记录，便于追踪生命周期状态变化。

远程包安装刻意保持窄能力：

- 下载 URL 必须是无凭据的 `https://` 或 `file://`。
- artifact path 必须以 `.aio-plugin` 结尾。
- 包在解压前会受到大小限制。
- remote 和 GitHub release install 必须提供 checksum。
- 如果同时提供 signature 和 trusted public key，宿主会校验 Ed25519 signature。

开发者工具输出 base64 编码的 Ed25519 signature。Public key 是原始 32-byte Ed25519 public key 的 base64 编码，和宿主 verifier 输入保持一致。

## publish-check

`pnpm --filter create-aio-plugin cli publish-check <plugin-dir>` 会读取插件目录，复用打包/校验路径计算 package metadata，并输出适合放进市场索引的字段，例如 plugin id、version、checksum、signature 状态、runtime、contributions、capabilities、risk labels 和 compatibility summary。

示例模板可以运行 publish-check，例如 `example:prompt-helper`、`example:redactor` 和 `example:response-guard` 生成的目录都应能输出发布 metadata。这个 metadata 只说明包具备发布前检查信息；它不代表示例已经被上传、签名、加入默认 market index，或变成默认可安装市场包。

`publish-check` 不写 `.aio-plugin` artifact，不替代 `pack`、`sign` 或 `verify`。它的职责是让发布者在提交市场索引前看到宿主安装时会关心的 metadata。真实安装仍由宿主重新下载包、校验 checksum、校验 signature、判断 compatibility、应用 capability/contribution policy，并处理 revoked / incompatible install blocks。

## Market Index

市场索引是一个发布清单，不是插件运行时 API。Plugins 页面可以加载 market index URL，也可以解析用户粘贴的 index JSON。索引条目应至少包含：

- plugin id、name、latest version。
- `.aio-plugin` download URL。
- `sha256` checksum。
- 可选 signature。
- 可选 trusted public key。
- compatibility summary。
- risk labels。
- revoked 状态和 install block reason。

默认市场视图会把精选插件展示成用户可读卡片，只显示用途、版本、风险、信任和安装状态。完整 checksum、signature 和 raw index fields 不应占据默认视图。

自定义 market index 属于高级来源。高级来源可以加载临时 URL 或粘贴 JSON，但它只是发布者和高级用户入口；真实安装仍由宿主重新执行 checksum、signature、compatibility、runtime policy 和 revoked checks。

market index URL 只用于定位索引来源。trusted public key 用于校验 release signature；它不能扩大插件 capabilities，也不能绕过 host compatibility、runtime policy、checksum、capability contract 或 quarantine 规则。

## Trust And Install Blocks

远程或市场安装必须提供 checksum。宿主会下载 `.aio-plugin` 后重新计算 `sha256`，和索引中的 checksum 对比。提供 signature 和 trusted public key 时，宿主会校验 Ed25519 signature；没有 trusted public key 时，插件仍可能被当作 unsigned package 展示给用户。

revoked / incompatible install blocks 必须在市场 UI 和宿主安装路径同时生效。UI 可以提前禁用安装按钮并解释原因；宿主命令仍要在真实安装时重新检查 revoked、宿主应用版本、`pluginApi` 兼容性、运行时策略和包安全限制。`hostCompatibility.platforms` 当前只作为元数据展示，不参与这些安装阻断。

## Replay Fixtures In Publishing

`plugin_export_replay_fixture` 导出的 replay fixture 是开发 workflow artifact，不是 release artifact。它适合放进 issue、PR 或本地 fixtures，用于复现某个 trace 的 hook 行为。由于 request logs 当前不持久化完整 request/response body，fixture 会携带 notes，发布者不应把它当成用户数据快照或市场证明材料。

推荐发布前至少保留：

- 一个正常命中 fixture。
- 一个未命中 fixture。
- 一个边界或失败策略 fixture。
- 对应的宿主运行报告、导出的 replay fixture 或 CI 检查。
