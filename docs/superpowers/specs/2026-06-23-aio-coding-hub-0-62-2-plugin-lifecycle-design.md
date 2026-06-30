# aio-coding-hub 0.62.2 Plugin Distribution and Lifecycle Design

日期：2026-06-23

## Summary

0.62.2 是插件分发与生命周期稳定版。0.62.0 稳定 Gateway-first 插件内核，0.62.1 补齐插件开发者闭环和运行观测，0.62.2 继续保持 Plugin API v1 外部兼容，把已经存在但分散的安装、更新、远程包、回滚、隔离和风险解释能力收口成一个可理解、可测试、可维护的生命周期层。

本版本不追求新增插件执行能力。核心目标是让用户在安装或更新插件前知道“这个插件是什么、会影响哪里、有什么风险、是否兼容、是否可信、能不能回滚”；让宿主在插件出问题或被撤销时能给出稳定状态和恢复路径。

## Current State

当前分支已经具备 0.62.2 的基础：

- `PluginManifest` 已包含 `description`、`author`、`homepage`、`repository`、`license`、`checksum`、`signature`、`category` 等发布相关元数据。
- `PluginStatus` 已包含 `available`、`installed`、`enabled`、`disabled`、`update_available`、`incompatible`、`quarantined`、`uninstalled`。
- `PluginInstallSource` 已区分 `local`、`market`、`github_release`、`offline`、`official`。
- 本地 `.aio-plugin` 包安装已经支持安全解压、manifest 校验、checksum、signature policy、高风险 unsigned package 拦截、安装目录 materialize。
- 远程包安装已经支持受限 URL、checksum required、market/GitHub release source、签名校验。
- market index parsing 已支持 compatible、update_available、revoked、risk_labels、install_block_reason。
- 更新流程已经保存 permission delta：旧授权继续保留，新权限进入 pending。
- repository 已保存 `plugin_versions`，服务层已有 rollback 到指定版本的能力。
- quarantine 已存在，用于 revoked plugin 或其他宿主判定的强制隔离。
- GUI 插件页已展示安装来源、状态、风险、运行观测、更新入口、回滚入口和官方插件安装入口。

主要缺口不是能力完全没有，而是边界和产品语义还不够集中：

- 安装前没有统一 preview model，GUI 和命令层难以一致展示风险、兼容性和信任状态。
- 更新前 diff 还没有成为一等概念，permission delta、hook/runtime/config 变化没有统一解释。
- lifecycle status 的来源、转换和用户可执行动作需要更明确。
- local、remote、market、GitHub release、official 的安装结果缺少统一 lifecycle summary。
- rollback 和 quarantine 已有基础，但 GUI、测试和文档还没有形成完整验收闭环。

## Goals

0.62.2 必须交付：

1. 统一的插件安装前预检模型。
2. 统一的插件更新 diff 模型。
3. 更清晰的插件生命周期状态和可执行动作。
4. GUI 中可理解的生命周期面板。
5. local、remote、market、GitHub release、official 安装路径的一致验收。
6. rollback、quarantine、permission delta、compatibility、checksum/signature 的测试覆盖。
7. 面向用户和插件作者的生命周期文档。

## Non-Goals

0.62.2 不做：

- 不改变 Plugin API v1 manifest shape。
- 不引入 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放 JS、TypeScript 或 WebView 插件 runtime。
- 不把 Tauri2 GUI 变成 browser-like plugin container。
- 不默认开放任意 marketplace WASM 执行。
- 不做完整插件市场产品化。
- 不做自动后台更新。
- 不做复杂账号体系、评分、评论、推荐、支付或远程运营后台。
- 不让插件控制 provider selection、failover、OAuth、token counting、session binding。

## Product Direction

目标体验分成三个场景。

### 1. 安装前

用户导入本地 `.aio-plugin`、安装官方插件、从远程包安装或从 market listing 安装前，宿主先给出 preview：

- 插件名称、id、版本、描述、作者、license、homepage/repository。
- runtime 类型和当前宿主是否支持。
- hooks 列表、优先级和 failure policy。
- permissions 列表、风险等级和简短说明。
- hostCompatibility 是否满足当前 app/pluginApi/platform。
- package checksum 是否匹配。
- signature 是否已验证、未签名是否允许。
- 安装来源：local、official、market、github_release、offline。
- 是否会覆盖已有插件。
- 是否存在阻塞原因。

用户看到的是“能不能安装”和“为什么”，而不是裸错误字符串。

### 2. 更新前

用户更新插件前，宿主给出 diff：

- fromVersion -> toVersion。
- runtime 是否变化。
- hooks 是否新增、删除或 priority/failure policy 变化。
- permissions 是否新增、删除、仍已授权或变成 pending。
- configVersion 是否变化。
- compatibility 范围是否变化。
- package trust 是否变化：checksum、signature、source。
- 是否可回滚到当前版本。

新增权限必须进入 pending，不能因为更新自动获得新权限。更新后如果插件处于 enabled，但新增必需权限未授权，宿主应拒绝继续启用或要求用户处理 pending permissions。

### 3. 出问题后

插件出问题时，用户能看到稳定状态和恢复动作：

- runtime failure 仍在 0.62.1 的运行观测中展示。
- revoked 或宿主判定危险的插件进入 quarantined。
- quarantined 插件不能 enable。
- rollback 只允许回到 repository 已记录的历史版本。
- uninstall 保留 audit，不物理删除所有历史证据。
- 关键状态变化都写入 audit log。

## Architecture

0.62.2 增加的是 lifecycle layer，不是新的 runtime layer。

### 1. Package Inspection

新增或整理一个 host-owned inspection 入口，用于读取 `.aio-plugin` 或 market listing 并返回 preview。它应该复用现有 package extraction、manifest validation、checksum/signature verification、market compatibility evaluation，不重复定义规则。

建议模型：

```text
PluginInstallPreview
  plugin identity
  package source
  manifest summary
  compatibility result
  runtime support result
  permission risk summary
  hook summary
  trust summary
  existing install summary
  blocking reasons
  warnings
```

preview 可以由 local package、remote package bytes 或 market listing 生成。remote package 仍然必须在下载后用 checksum 验证，不因为 preview 放宽安装策略。

### 2. Update Diff

新增或整理一个 update diff builder，用当前已安装 `PluginDetail` 和待安装 manifest/package 生成差异。这个 builder 不负责写数据库，只负责给安装服务和 GUI 提供一致解释。

建议模型：

```text
PluginUpdateDiff
  pluginId
  fromVersion
  toVersion
  versionDirection
  runtimeChange
  hookChanges
  permissionChanges
  configVersionChange
  compatibilityChange
  trustChange
  rollbackAvailable
  blockingReasons
  warnings
```

`permissionChanges` 至少区分：

- unchangedGranted
- unchangedPending
- addedPending
- removed

### 3. Lifecycle Service

`plugin_service` 继续负责真实安装、更新、回滚、隔离、启用、禁用和卸载。0.62.2 的调整重点是让这些操作共享同一套 preview/diff/lifecycle summary，而不是在每条路径里各自拼接审计细节。

建议内部边界：

- package layer：安全解压、读取 manifest、计算 checksum。
- trust layer：checksum/signature/public key policy。
- compatibility layer：host/pluginApi/platform/runtime support。
- diff layer：当前插件 vs 待安装插件。
- lifecycle service：执行状态转换并写 audit。
- GUI/query layer：展示 preview、diff 和 detail，不推断规则。

### 4. GUI Lifecycle Panel

GUI 插件详情页需要增加生命周期视角，而不是只展示 manifest 字段。

建议区域：

- 当前状态：enabled/disabled/update available/quarantined/incompatible。
- 来源与信任：install source、checksum、signature verified/unsigned、developer mode。
- 版本与回滚：current version、previous versions、rollback action。
- 更新影响：如果 update_available，展示待更新来源和 diff 摘要。
- 权限变化：granted、pending、added on update。
- 隔离原因：quarantine reason 和最后 audit。
- 可执行动作：enable、disable、update、rollback、uninstall，按状态禁用不合法动作。

GUI 不需要做完整 marketplace 页面。0.62.2 只要求插件详情和导入/更新流程能解释 lifecycle。

## Functional Scope

### Required

- 添加插件安装前 preview 能力。
- 添加插件更新 diff 能力。
- 统一安装、更新、回滚、quarantine 的 lifecycle summary。
- GUI 展示安装前风险和更新前差异。
- GUI 展示 quarantine reason、rollback availability、source/trust summary。
- 保证 enabled 插件更新后新增权限不会自动授权。
- 保证 quarantined 和 incompatible 插件不能启用。
- 补齐 service、command、GUI、docs 测试。

### Optional

- 展示插件历史版本列表。
- 从 audit 中提取更友好的 last lifecycle event。
- 本地 package preview 支持命令或开发工具输出 JSON。
- 对 rollback 增加确认摘要。

### Deferred

- 完整 marketplace 页面。
- 自动后台检查更新。
- 插件源 CRUD GUI。
- 签名 key 管理 GUI。
- WASM lifecycle 特化 UI。
- Provider plugin lifecycle。

## Data Flow

### Local Package Install

```text
.aio-plugin file
  -> package inspection
  -> manifest validation
  -> compatibility/runtime/trust evaluation
  -> install preview
  -> user confirmation
  -> install_plugin_from_local_package
  -> lifecycle summary + audit
  -> plugin detail refresh
```

### Local Package Update

```text
.aio-plugin file
  -> package inspection
  -> current plugin detail
  -> update diff
  -> user confirmation
  -> update_plugin_from_local_package
  -> permission delta persisted
  -> lifecycle summary + audit
  -> plugin detail refresh
```

### Remote Package Install

```text
market listing or explicit remote input
  -> download package bytes
  -> checksum verification required
  -> signature verification when policy provides signature/public key
  -> install preview or update diff
  -> user confirmation
  -> install_plugin_from_remote_package_bytes
  -> lifecycle summary + audit
```

### Quarantine

```text
market revoked or host policy decision
  -> quarantine_revoked_plugin
  -> status = quarantined
  -> enable blocked
  -> audit event retained
  -> GUI shows reason and recovery options
```

### Rollback

```text
selected previous version
  -> repository.get_plugin_version
  -> update_plugin_manifest
  -> status and permissions remain policy-safe
  -> audit event retained
  -> gateway plugin refresh
```

## Error Handling

Lifecycle operations should return stable codes where possible. GUI can translate codes into short Chinese messages while preserving technical detail in logs.

Important error classes:

- package unreadable or invalid archive;
- missing or invalid `plugin.json`;
- incompatible host/app/pluginApi/platform;
- unsupported runtime or disabled runtime policy;
- checksum mismatch;
- signature policy incomplete;
- unsigned high-risk package rejected;
- plugin id mismatch;
- update targets a different plugin id;
- update is downgrade unless explicitly allowed by rollback path;
- plugin is quarantined;
- plugin has pending required permissions;
- rollback version not found.

Preview should report blockers and warnings without mutating state. Install/update should enforce the same blockers again; preview is user experience, not security boundary.

## Compatibility

Plugin API v1 remains externally compatible. 0.62.2 may add host-side preview/diff command models, GUI models, docs and tests, but must not require plugin authors to change valid v1 manifests.

Existing local/offline packages that install today should continue to install unless they already violate current host policy. `validate --strict` from 0.62.1 remains the developer-side package health check; 0.62.2 lifecycle preview is host-side install/update explanation.

Provider Plugin API remains private. WASM remains policy-gated.

## Testing Strategy

### Rust Service Tests

Cover:

- local package preview returns manifest, runtime, hooks, permissions, risk, compatibility and trust summary;
- preview reports missing manifest without installing;
- preview reports incompatible hostCompatibility;
- preview reports unsupported runtime policy;
- install enforces checksum/signature policy after preview;
- update diff reports version, hook, runtime, permission and configVersion changes;
- update preserves existing granted permissions and moves new permissions to pending;
- quarantined plugin cannot be enabled;
- rollback loads a recorded historical version;
- remote install requires checksum;
- revoked market listing maps to blocked/quarantine flow.

### Command Tests

Cover:

- generated Tauri command bindings expose preview/diff inputs and outputs;
- local preview command does not mutate DB;
- update preview command fails clearly when package id does not match selected plugin;
- remote install path keeps existing URL/checksum restrictions.

### Frontend Tests

Cover:

- install preview renders plugin identity, source, compatibility, runtime, permission risk and blockers;
- update diff renders permission delta and version change;
- quarantine reason renders and enable action is unavailable;
- rollback action is only shown when a previous version is available;
- pending permissions after update are visible before enable;
- unsigned/high-risk warning is visible when present.

### Existing Gates

Continue running:

- `pnpm --filter create-aio-plugin test`
- `pnpm --filter create-aio-plugin typecheck`
- `pnpm check:plugin-api-contract`
- `pnpm check:plugin-system-docs`
- `pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx`
- `pnpm typecheck`
- `cd src-tauri && cargo test plugin --lib`
- `cd src-tauri && cargo test provider --lib`
- `pnpm check:prepush`

## Acceptance Criteria

0.62.2 可以验收时应满足：

1. 用户安装插件前能看到统一 preview，不需要从错误 toast 猜风险。
2. 用户更新插件前能看到清晰 diff，尤其是新增权限和 runtime/hook/config 变化。
3. 新增权限不会在更新时被自动授权。
4. quarantined/incompatible 插件不能被启用。
5. rollback 只能回到已记录版本，并写入 audit。
6. local、remote、market、GitHub release、official 路径共享相同生命周期语义。
7. GUI 插件详情能解释当前状态、来源、信任、版本、回滚和隔离原因。
8. 所有生命周期状态转换都有测试和 audit。
9. 文档明确 0.62.2 不是完整 marketplace，也不是新插件 runtime/API 版本。

## Release Boundary

0.62.2 的完成定义是“插件生命周期可解释、可测试、可恢复”。它不追求更多 hook，不增加 Provider 插件能力，也不改变 API v1。下一步如果要继续做插件生态，建议在 0.63.0 再规划 Gateway 插件能力增强；如果要做插件市场，则应在生命周期稳定之后单独规划轻量 market/source 管理。
