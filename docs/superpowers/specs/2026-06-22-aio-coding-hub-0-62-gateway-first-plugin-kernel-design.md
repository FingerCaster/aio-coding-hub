# aio-coding-hub 0.62 Gateway-first 插件内核迭代设计

> Status: Superseded.
>
> MUST NOT be executed. This historical design treats declarativeRules as a
> stable public community runtime. Use the Extension Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

日期：2026-06-22

## 决策

0.62 的插件迭代采用 Gateway-first 路线：先把 Gateway 插件平台做稳定，Provider 只做内部架构铺垫，不开放 Provider Plugin API。

本版本目标不是新增更多公开插件 API，而是让现有 Plugin API v1 成为可信、可维护、可扩展的平台内核。除非发现 v1 外部契约无法表达当前已支持的 Gateway 插件能力，否则不修改 `plugin.json`、SDK、hook 名称、permission 名称或现有运行语义。

## 当前现状

当前插件系统已经具备 v1 基础：

- `plugin.json` v1、hooks、permissions、config schema、SDK、脚手架和 contract checker 已存在。
- Active hooks 已覆盖请求、响应、流式输出、错误和日志：`gateway.request.afterBodyRead`、`gateway.request.beforeSend`、`gateway.response.chunk`、`gateway.response.after`、`gateway.error`、`log.beforePersist`。
- Reserved hooks 仍不开放：`gateway.request.received`、`gateway.request.beforeProviderResolution`、`gateway.response.headers`。
- `declarativeRules` 是稳定社区 runtime。
- `official.privacy-filter` 是唯一官方 native runtime。
- WASM 有 policy、SDK 和示例，但默认不作为 0.62 的公开稳定执行能力。
- Process runtime 仅保留为 PoC 文档，不进入 0.62 发布目标。

0.62 分支已经开始内部平台化：

- `docs/plugins/plugin-api-v1-contract.json` 成为 contract drift 检查来源。
- Rust 侧已有 hook contract、hook registry、mutation descriptor、runtime manager、runtime policy、runtime cache。
- Provider adapter/capability facade 已开始收敛 provider-specific 逻辑。
- Official privacy filter 正在从 `rule_runtime` 迁出，形成独立 runtime ownership。

## 0.62 目标

### 1. 保持 Plugin API v1 外部兼容

0.62 不引入 Plugin API v2。现有插件作者面对的 `plugin.json`、SDK 类型、hooks、permissions、declarative rules、official privacy filter 配置语义保持兼容。

验收重点不是“新增 API 数量”，而是证明已有 API v1 不漂移、不破坏、能被测试和文档稳定描述。

### 2. 稳定 Gateway hook 平台

Gateway hook 是 0.62 的核心扩展点。每个 active hook 的阶段、读权限、写权限、context fields、mutation fields、timeout、failure policy、audit 行为都必须可从内部 descriptor/contract 中定位。

目标状态：

- Manifest validation 与 hook registry 对 active/reserved hooks 认知一致。
- Context trimming 与 permission enforcement 不再散落成多套语义。
- Mutation enforcement 由 descriptor 驱动。
- Reserved hooks 继续被拒绝，直到真实调用点和测试齐备。

### 3. Runtime ownership 清晰化

Pipeline 负责 hook orchestration；runtime 层负责加载、缓存、执行、policy 和错误归一。

0.62 需要明确三类 runtime：

- `declarativeRules`：社区稳定 runtime。
- `official.privacy-filter`：官方 host-owned native runtime。
- WASM：policy-gated，0.62 不承诺完整 gateway 执行稳定性。

第三方 `native` runtime 继续拒绝。Process runtime 不进入公开能力。

### 4. Provider 只做内部铺垫

Provider 是未来插件系统的重要方向，但 0.62 不开放 Provider Plugin API。

本版本只做内部 adapter/capability facade：

- 收敛 CX2CC、Gemini OAuth、Codex、Claude 等 provider-specific 行为。
- 减少 gateway hot path 中分散的 provider 特例判断。
- 保持 provider selection、failover、circuit、limits、session binding 归 gateway core 所有。
- 不让插件影响 provider 路由决策。

### 5. 测试与验收成为 release gate

0.62 的完成定义必须由测试证明，而不是靠人工判断架构“看起来更好”。

必要验证包括 contract checks、hook fixture tests、runtime tests、gateway integration tests、provider regression tests 和 performance smoke。

## 开发计划

### Phase 0：整理当前分支

目标：把正在进行的 privacy filter runtime 迁移变成独立、可审查的提交。

范围：

- `RuleRuntimeGatewayPluginExecutor` 只负责 `declarativeRules`。
- `OfficialPrivacyFilterRuntime` 负责 official privacy filter 的 load、execute、cache、prune。
- `RuntimeGatewayPluginExecutor` 根据 runtime dispatch 分发到对应 runtime。
- Official privacy filter 行为测试归属到 official runtime 模块。

验证：

- `cargo test official_privacy_filter --lib`
- `cargo test rule_runtime_prunes_cache_entries_not_in_active_plugin_keys --lib`
- `cargo check --locked`
- `RUSTFLAGS=-Dwarnings cargo check --locked`

### Phase 1：Contract Layer 加固

目标：让 `plugin-api-v1-contract.json` 成为 API v1 drift detection 的事实源。

范围：

- Active/reserved hooks 一致性检查。
- Active/reserved permissions 一致性检查。
- Hook context fields、mutation fields、permission dependencies 一致性检查。
- SDK manifest validation 必须按 hook-scoped `permissionDependencies` 执行，不能把某个 hook 的 write/read 依赖误提升为全局规则。
- SDK、docs、scaffold、replay、Rust validation 的 drift 报错可定位。

不做：

- 不全量生成 Rust/TS 代码。
- 不改变公开 contract shape。

验证：

- `pnpm check:plugin-api-contract`
- `node scripts/check-plugin-api-contract.selftest.mjs`
- 破坏性 fixture 能报告具体 drift 类型。

### Phase 2：Hook Registry 收口

目标：将 hook 语义集中到 `HookDescriptor` 和 contract metadata。

范围：

- Pipeline 获取 hook timeout、failure policy、permission/mutation metadata 时优先通过 descriptor。
- Context trimming 和 mutation enforcement 使用同一份 hook descriptor 语义。
- Reserved hooks 继续被 manifest validation 拒绝。

验证：

- 每个 active hook 覆盖 context trimming。
- 每个 active hook 覆盖合法 mutation。
- 每个 active hook 覆盖非法 mutation。
- 每个 active hook 覆盖 failure policy 和 audit。
- 无插件 fast path 不退化。

### Phase 3：Runtime Layer 收口

目标：保持 runtime 分发、policy、cache、error taxonomy 的边界清楚。

范围：

- `RuntimeGatewayPluginExecutor` 作为 gateway plugin executor 入口。
- `PluginRuntimeManager` 负责 dispatch/policy。
- 各 runtime 自己拥有 cache。
- cache key 继续包含 plugin id、version、installed dir、updated_at、runtime key。
- Runtime errors 有稳定 code 和 audit/log 可诊断信息。

验证：

- Declarative rules load/cache/reload/prune。
- Official privacy filter load/cache/prune。
- WASM disabled 返回稳定 `PLUGIN_RUNTIME_DISABLED`。
- WASM enabled 但 gateway execution 未接入时返回稳定 `PLUGIN_WASM_NOT_WIRED`。
- 非 official native privacy filter 被拒绝。

### Phase 4：Provider Adapter 内部铺垫

目标：只收敛已有 provider 特例，不开放 provider 插件 API。

范围：

- Provider adapter/capability facade 继续承接 CX2CC、Gemini OAuth、Codex/Claude 兼容逻辑。
- Gateway orchestration 通过 adapter/capability 查询 provider 特性。
- 迁移时可让 adapter facade 委托 legacy helper，但新增 provider 特例不能继续散落到 hot path。

不做：

- 不允许插件注册 provider adapter。
- 不开放 provider route decision hook。
- 不允许插件控制 failover、circuit、limits、session binding。

验证：

- CX2CC count_tokens、bridge request/response、usage 相关测试保持通过。
- Gemini OAuth body/response translation 保持通过。
- Codex/Claude request/response/provider logging regression 保持通过。
- `cargo test provider --lib`

### Phase 5：文档与发布验收

目标：文档明确 0.62 是内部平台内核版本，不新增公开插件 API。

范围：

- 更新插件架构审计。
- 更新 runtime docs。
- 更新 developer-facing compatibility note。
- 列出 0.62 不包含 Provider Plugin API、JS/TS runtime、WebView plugin runtime。

验证：

- `pnpm check:plugin-system-docs`
- 文档中没有暗示 0.62 已开放 provider 插件 API。

## 实现边界

### 包含

- Gateway Plugin API v1 contract 加固。
- Descriptor-driven hook metadata。
- Descriptor-driven permission/mutation enforcement。
- Runtime dispatch、policy、cache lifecycle 收口。
- Official privacy filter 独立 runtime。
- Declarative rules 行为保持。
- Provider adapter facade 内部化。
- 文档、测试、验收矩阵补齐。

### 不包含

- 不新增 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放第三方 native runtime。
- 不开放 JavaScript/TypeScript runtime。
- 不让插件运行在 Tauri WebView。
- 不开放任意桌面 host API。
- 不把 Skill 市场合并进插件 runtime。
- 不为了未来可能性修改 v1 外部 API。

## 测试用例矩阵

### Contract tests

- Active hooks 与 Rust validation 一致。
- Reserved hooks 与 Rust validation 一致。
- Active permissions 与 docs/SDK 一致。
- Reserved permissions 仍被 manifest validation 拒绝。
- Permission dependencies 与 contract 一致。
- SDK 允许 contract 中无依赖的 write-only hook manifest，同时继续拒绝有依赖的 write-without-read manifest。
- Mutation fields 与 hook descriptor 一致。
- Legacy `contextPatch` 不回到 active contract。

### Hook fixture tests

每个 active hook 至少覆盖：

- granted permissions 下能看到对应 context fields。
- 未授权字段被裁剪。
- 合法 mutation 被应用。
- 未授权 mutation 被拒绝。
- 不属于该 hook 的 mutation field 被拒绝。
- hook error/timeout 进入对应 failure policy。
- audit event 记录 plugin id、hook、outcome、failure policy。

### Runtime tests

- Declarative rules 首次执行 load，后续命中 cache。
- Declarative rules plugin version/updated_at/path 变化后 reload。
- Declarative rules cache prune 只保留 active plugin keys。
- Official privacy filter 首次执行 load，后续命中 cache。
- Official privacy filter cache prune 只保留 active official key。
- Non-official native `privacyFilter` 被拒绝。
- WASM disabled 被拒绝。
- WASM enabled but not wired 返回稳定错误。

### Gateway integration tests

- `gateway.request.afterBodyRead` 能改写 request body。
- `gateway.request.beforeSend` 能改写最终 upstream request body/header。
- `gateway.response.after` 能改写 non-stream response body/header。
- `gateway.response.chunk` 能改写或阻断 stream chunk。
- `gateway.error` 能改写 gateway-generated error response，但失败不能隐藏 host error。
- `log.beforePersist` 能在日志入库前脱敏。

### Provider regression tests

- Provider selection、forced provider、session binding 不变。
- Circuit、cooldown、limits 不变。
- CX2CC bridge 行为不变。
- Gemini OAuth 行为不变。
- Codex/Claude compatibility 行为不变。
- Request log、usage、provider chain JSON 形状不变。

### Performance smoke

- Empty plugin pipeline request hook 低于既有预算。
- One noop declarative plugin request hook 低于既有预算。
- 无 `gateway.response.chunk` 插件时保持 stream direct path。
- Declarative rules 首次解析后缓存生效。
- Official privacy filter compiled detector 缓存生效。

## 验收标准

0.62 插件迭代完成必须满足：

- 外部 Plugin API v1 兼容。
- `pnpm check:plugin-api-contract` 通过。
- `pnpm check:plugin-system-docs` 通过。
- Rust plugin/gateway/provider 相关测试通过。
- `cargo check --locked` 通过。
- `RUSTFLAGS=-Dwarnings cargo check --locked` 通过。
- Hook 语义能从 contract/descriptor 定位。
- Runtime ownership 清楚：declarative rules、official privacy filter、WASM policy 不互相混杂。
- Provider adapter 仍是内部 facade，不暴露给插件作者。
- 文档明确 0.62 不新增公开插件 API。
- Performance smoke 未暴露 gateway hot path 明显退化。

## 版本表述

推荐对外表述：

> aio-coding-hub 0.62 focuses on the Gateway-first plugin platform kernel. It keeps Plugin API v1 compatible while hardening hook contracts, runtime ownership, permission enforcement, and internal provider adapter boundaries.

推荐中文表述：

> aio-coding-hub 0.62 聚焦 Gateway-first 插件平台内核。它保持 Plugin API v1 兼容，重点加固 hook 契约、runtime ownership、权限与 mutation enforcement，以及内部 provider adapter 边界。
