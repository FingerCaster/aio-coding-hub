# aio-coding-hub Plugin Runtime Lifecycle Registry Design

> Status: Superseded.
>
> MUST NOT be executed. This historical design still treated
> declarativeRules as the public community plugin path. Use the Extension
> Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

日期：2026-06-27

## Status

Superseded by `2026-06-27-aio-coding-hub-plugin-extension-host-platform-design.md`.

This document captured an earlier runtime-lifecycle design that still treated
`declarativeRules` as the public community plugin path. The branch has not been
released, and the product direction has changed: the plugin platform should be
Extension Host first, with UI, provider, protocol bridge, gateway, command, and
diagnostic contribution points. The lifecycle ideas in this document remain
useful only as a subsystem inside the new Extension Host platform.

## Summary

本阶段聚焦插件宿主稳定性，新增 host-owned Runtime Lifecycle Registry。它不是新的插件 API，也不是新的插件语言入口，而是把插件运行资源、缓存、健康状态、失败熔断和销毁路径收口到一个内部生命周期边界。

当前 Plugin API v1 对外保持稳定。社区插件开发主线继续是 `plugin.json + declarativeRules JSON`。WASM 保留为未来 policy-gated code-plugin 能力；process runtime 继续是 PoC；第三方 `native` 不开放。Registry 的目标是先把宿主内部的资源创建、使用、刷新、更新、回滚、禁用、隔离、卸载和应用退出做成可测试、可维护、不会无限增长的模型。

一句话目标：插件越多、运行时越复杂，宿主也应该知道每个插件资源是谁创建的、什么时候该用、什么时候必须释放、连续失败时怎么跳过，以及退出时如何全部清理。

## Current State

当前插件系统已经有较完整的基础：

- Plugin API v1 contract 已由 `docs/plugins/plugin-api-v1-contract.json`、Rust domain 和 `@aio-coding-hub/plugin-sdk` 共同约束。
- 社区主力 runtime 是 `declarativeRules`，并有 host runtime、devtools replay、doctor、validate、pack 和 publish-check。
- `official.privacy-filter` 使用宿主内置 `native:privacyFilter`，第三方插件不能声明 host-native engine。
- WASM runtime 已有 foundation、ABI 文档和 Rust/WASM SDK，但执行仍受 host policy gate 控制。
- process runtime 已有 JSON-RPC over stdio PoC，默认关闭，也不是 Plugin API v1 execution surface。
- `RuntimeGatewayPluginExecutor` 已能执行 declarative rules 和 official privacy filter，并具备部分 cache dispose 测试。
- `PluginRuntimeManager` 已承担 runtime policy 和 runtime dispatch 的一部分职责。
- `runtime_reports` 已有 retention，避免 hook execution reports 无限增长。
- 插件安装、更新、回滚、隔离、预检、更新 diff、market trust 和官方命名空间保护已经形成基础生命周期层。

主要短板是生命周期边界仍然分散：

- declarative rule cache、official privacy filter cache、未来 WASM module cache 和 process child handle 没有统一资源登记模型。
- 插件 refresh、update、rollback、disable、quarantine、uninstall 和 app shutdown 缺少同一套 dispose 协议。
- runtime identity 现在靠局部 cache key 约束，未来 runtime 增加后容易出现旧版本资源继续存活或无法精准释放。
- 连续 timeout、runtime error、protocol error 之后，健康状态和临时跳过策略容易分散在 pipeline、executor、runtime reports 和 UI 中。
- process runtime 一旦接入真实路径，需要统一 start timeout、hook timeout、idle recycle、hard shutdown、stdout/stderr drain limit、request/response byte limit 和 per-plugin concurrency limit。

## Plugin Development Runtime Decision

本阶段明确插件开发方向，避免把插件生态过早变成多语言运行时平台。

### Public Best Practice

公开社区插件开发最佳实践只保留一条主线：

```text
plugin.json + declarativeRules JSON + fixtures + create-aio-plugin replay/pack/publish-check
```

这意味着当前版本不宣传“多语言插件开发”，也不把 TypeScript、JavaScript、Python、Go 或进程插件作为普通用户可选 runtime。

### Runtime Boundaries

- `declarativeRules`：当前唯一稳定社区 runtime。适合 prompt helper、redaction、response guard、warn/block/replace/appendMessage。
- WASM：未来 code-plugin runtime，继续 policy-gated。只有当 Registry 能统一管理 module cache、memory/time budget、failure state 和 dispose 时，才考虑开放更广泛执行。
- process runtime：仍是 PoC，不属于 Plugin API v1，不进入 marketplace 默认能力。
- `native`：只给宿主内置官方引擎，例如 `official.privacy-filter`。
- TypeScript SDK 和 `create-aio-plugin`：是开发工具链，不是插件运行语言。

如果未来开放 WASM，也建议只有一条官方最佳实践：Rust -> WASM。不要同时把多种解释型语言运行时带进宿主生命周期内核。

## Goals

本阶段必须交付：

1. 新增内部 Runtime Lifecycle Registry，统一登记插件 runtime resources。
2. 定义稳定 runtime identity：`plugin_id + version + installed_dir/updated_at + runtime_kind + runtime_key`。
3. 统一插件资源释放入口，覆盖 refresh、update、rollback、disable、quarantine、uninstall 和 app shutdown。
4. 将 declarative rules cache 和 official privacy filter cache 纳入 Registry 管理或注册式 dispose 边界。
5. 为未来 WASM/process runtime 预留相同生命周期接口，但不开放新的社区执行面。
6. 增加 bounded failure accounting 和 circuit-open 基础，避免坏插件持续拖慢 gateway。
7. 保持 Plugin API v1、manifest shape、SDK public contract 和现有功能行为一致。
8. 用单元测试证明 cache retain/dispose、旧版本驱逐、失败计数、circuit reset 和 shutdown dispose 可验证。

## Non-Goals

本阶段不做：

- 不改变 `plugin.json` v1 字段。
- 不新增 Plugin API v2。
- 不开放 Provider Plugin API。
- 不开放 JS、TypeScript、Python、Go 或 WebView/browser 插件 runtime。
- 不让 Tauri2 GUI 变成内嵌浏览器式插件容器。
- 不默认开放 marketplace WASM execution。
- 不把 process runtime 接成普通社区插件能力。
- 不开放第三方 `native` runtime。
- 不新增 `plugin.storage`、`network.fetch`、`file.read`、`file.write`、`secret.read` 等 reserved permissions。
- 不重做插件市场、评分、评论、账号或远程运营后台。

## Architecture

### 1. Registry Ownership

新增 `RuntimeLifecycleRegistry`，归宿主 app/plugin runtime layer 所有。它不直接理解 GUI，也不直接改 manifest。它只管理运行资源和健康状态。

建议职责：

```text
RuntimeLifecycleRegistry
  register(runtime identity, resource handle)
  mark_used(runtime identity)
  record_success(runtime identity)
  record_failure(runtime identity, failure kind)
  should_execute(runtime identity)
  retain_active(active runtime identities)
  dispose(runtime identity)
  dispose_plugin(plugin id)
  dispose_all()
  snapshot_health(plugin id)
```

Registry 应该是轻量、内存内、host-owned 的结构。持久化事实仍由现有 audit logs、plugin details 和 runtime reports 负责。Registry 只保存运行期健康和资源句柄，不保存敏感 payload。

### 2. Runtime Identity

统一 runtime identity，避免旧资源和新资源混用：

```text
RuntimeIdentity
  pluginId
  version
  installedDir
  updatedAt
  runtimeKind
  runtimeKey
```

其中：

- `runtimeKind` 例：`declarativeRules`、`native:privacyFilter`、`wasm`、`process`。
- `runtimeKey` 例：规则文件列表、native engine name、WASM entry artifact、process command fingerprint。
- `installedDir` 和 `updatedAt` 用来区分同版本重新安装、回滚和本地覆盖。

现有 `runtime_cache_key` 可以继续作为 key helper，但 Registry 应该成为 cache retain/dispose 的上层边界。

### 3. Resource Handles

Registry 只要求资源实现最小释放协议。

建议内部模型：

```text
RuntimeResource
  kind
  dispose()
  diagnostics()
```

第一阶段可以从同步 dispose 开始，覆盖现有 cache。未来 process runtime 接入时，再支持 async shutdown 或内部 block-on 的 bounded shutdown helper。

资源类型：

- declarative rules parsed runtime cache。
- official privacy filter compiled detector/native engine cache。
- future WASM module/engine/cache。
- future process child/session handle。
- bounded diagnostics buffer。

### 4. Lifecycle States

Registry 内部状态建议保持简单：

```text
registered
active
unhealthy
circuit_open
disposing
disposed
```

状态语义：

- `registered`：资源已登记，但未必刚执行过。
- `active`：最近成功或尚未触发失败阈值。
- `unhealthy`：出现失败，但还没达到临时跳过阈值。
- `circuit_open`：连续失败达到阈值，短时间内跳过执行。
- `disposing`：正在释放资源。
- `disposed`：资源已释放，不允许继续执行。

这不是用户可见状态，也不替代 `PluginStatus`。用户看到的 enabled/disabled/quarantined 等仍来自插件生命周期服务。

### 5. Host Lifecycle Events

Registry 应成为以下事件的统一入口：

```text
plugin snapshot refresh -> retain_active(active identities)
plugin update -> dispose old identity after successful swap
plugin rollback -> dispose previous current identity and activate rollback identity
plugin disable -> dispose_plugin(plugin id) or mark inactive and dispose runtime resources
plugin quarantine -> dispose_plugin(plugin id), block future execution through existing status checks
plugin uninstall -> dispose_plugin(plugin id)
app shutdown -> dispose_all()
```

真实数据库状态转换仍由现有 plugin service/repository 负责。Registry 只处理内存资源和运行健康。

### 6. Gateway Execution Flow

执行前后建议流向：

```text
GatewayPluginPipeline
  -> load enabled plugin detail
  -> build RuntimeIdentity
  -> registry.should_execute(identity)
  -> RuntimeGatewayPluginExecutor.execute_plugin_sync(...)
  -> registry.record_success(identity) or registry.record_failure(identity, failure)
  -> runtime report/audit as today
```

如果 `should_execute` 返回 false：

- pipeline 记录 `circuitOpen` 或等价 runtime report。
- fail-open hook 默认跳过该插件。
- fail-closed hook 仍按现有 failure policy 决定是否阻断。
- 不在这里自动 quarantine。Quarantine 仍应是更明确的宿主生命周期动作。

### 7. Circuit Behavior

第一阶段 circuit-open 保持保守：

- 只按 runtime identity 计数，不按全局插件永久拉黑。
- 连续失败达到小阈值后进入临时 `circuit_open`。
- circuit-open 有短 TTL 或直到 plugin refresh/update/disable/enable 后重置。
- 成功执行会清理连续失败计数。
- timeout、WASM trap、process protocol error、oversized output、runtime policy error 可以计入 failure kind。
- permission missing、plugin disabled、quarantined、incompatible 这类状态不应进入 runtime circuit，它们已经是生命周期/权限层决策。

具体阈值在 implementation plan 中按现有测试和性能预算选择，避免在 spec 中提前绑定过细数值。

### 8. WASM and Process Future Boundary

Registry 必须为 WASM/process 做好接口位置，但本阶段不扩大执行能力。

WASM 未来接入 Registry 前必须满足：

- bounded input/output JSON。
- bounded guest memory。
- fuel 或等价 execution budget。
- module/cache eviction on plugin refresh/update/uninstall。
- dispose on disable/quarantine/app shutdown。

Process runtime 未来接入 Registry 前必须满足：

- start timeout。
- hook timeout。
- idle recycle。
- hard shutdown and reap。
- stdout/stderr drain limits。
- request/response byte limits。
- per-plugin process concurrency limits。
- no marketplace enablement by default。

## Data Flow

### Plugin Refresh

```text
plugin repository snapshot
  -> build active RuntimeIdentity list
  -> registry.retain_active(active identities)
  -> stale resources dispose
  -> executor uses refreshed identities
```

### Plugin Update

```text
install/update package
  -> validate manifest/trust/permissions as today
  -> persist new plugin version/current state
  -> build old identity and new identity
  -> registry.dispose(old identity)
  -> plugin detail refresh
```

### Plugin Rollback

```text
rollback selected historical version
  -> persist rollback state and permissions/config reconciliation as today
  -> registry.dispose(previous current identity)
  -> next hook execution registers rollback identity if needed
```

### Disable, Quarantine, Uninstall

```text
state transition in plugin service
  -> registry.dispose_plugin(plugin id)
  -> future gateway snapshot excludes or blocks plugin as today
```

### App Shutdown

```text
tauri/app shutdown path
  -> registry.dispose_all()
  -> best-effort bounded cleanup
```

## Functional Impact

Expected behavior should remain the same for users:

- Existing declarative rules plugins continue to run.
- Official Privacy Filter continues to run.
- Plugin import, install preview, update diff, rollback, quarantine, permissions and config keep current behavior.
- PluginsPage does not need a new default UX for this phase.

Internal behavior changes:

- Runtime caches become owned or retained through Registry.
- Stale plugin resources are released through one path.
- Consecutive runtime failures can temporarily skip a runtime identity.
- Tests can assert resource counts and health transitions without relying on incidental cache internals.

## Testing Plan

Backend tests should cover:

- Registry registers a resource and disposes it by exact identity.
- `retain_active` keeps active identities and disposes stale identities.
- `dispose_plugin` releases all identities for one plugin.
- `dispose_all` releases all registered resources and is idempotent.
- Updating same plugin id/version but different installedDir or updatedAt disposes the old resource.
- Declarative rules cache is released through Registry retain/dispose.
- Official Privacy Filter cache is released through Registry retain/dispose.
- Consecutive runtime failures enter circuit-open for that runtime identity.
- Successful execution resets unhealthy/failure state.
- Circuit-open skip records a runtime report or equivalent structured diagnostic.
- Disabled/quarantined/uninstalled plugin resource disposal does not require a gateway request to happen later.
- Process runtime PoC tests continue to prove timeout kill/reap behavior, even though process execution remains disabled by default.

Frontend tests are optional for this phase unless a small diagnostics label is exposed. If exposed, keep it user-facing and avoid developer-only noise on the default Plugins page.

Release verification should include:

```bash
pnpm test:unit src/pages/__tests__/PluginsPage.test.tsx src/pages/plugins/__tests__/pluginMarketModel.test.ts
pnpm typecheck
pnpm lint
```

Rust verification should target plugin runtime and lifecycle tests. Exact commands should be finalized in the implementation plan based on available test names and current workspace speed.

## Acceptance Criteria

- Plugin API v1 contract file remains unchanged unless the change is purely documentation metadata already agreed in a separate spec.
- `declarativeRules` remains the only stable community runtime in docs and tooling copy.
- No JS/TS/process/native third-party runtime is presented as supported.
- Registry has explicit resource registration, retain and dispose behavior.
- Plugin update/rollback/disable/quarantine/uninstall paths call Registry cleanup directly or through a shared service boundary.
- Old runtime resources are not reused after plugin snapshot changes.
- Declarative rules and official privacy filter caches can be observed in tests as released after stale retain/dispose.
- Circuit-open prevents repeated execution of a failing runtime identity within the configured window.
- Runtime reports or diagnostics make circuit-open skips distinguishable from normal disabled/quarantined states.
- Existing plugin install/update/rollback/Privacy Filter behavior remains functionally consistent.
- Relevant backend tests, frontend tests, `pnpm typecheck` and `pnpm lint` pass before implementation is considered complete.

## Risks and Mitigations

- Risk: Registry becomes a second plugin service.
  Mitigation: keep it in-memory and runtime-only. Database lifecycle state remains in existing service/repository.

- Risk: circuit-open hides important security plugins.
  Mitigation: continue honoring failure policy. Fail-closed hooks must still block or report according to existing pipeline rules.

- Risk: async process disposal complicates the first implementation.
  Mitigation: first phase supports sync resource dispose for existing caches; process runtime remains PoC and only uses the interface boundary in documentation/tests.

- Risk: implementation accidentally changes public plugin API.
  Mitigation: add contract drift checks and keep manifest/SDK fields unchanged.

## Documentation Updates

Update documentation after implementation:

- `docs/plugins/architecture/audit.md`：记录 Runtime Lifecycle Registry 决策。
- `docs/plugins/runtime/wasm.md`：继续说明 WASM must be lifecycle-registry managed before wider enablement。
- `docs/plugins/runtime/process-poc.md`：继续说明 process runtime disabled by default，并列出接入 Registry 前置条件。
- `docs/plugins/developer-guide.md`：强调当前最佳实践是 `declarativeRules`，WASM 是未来高级路径，不是默认插件语言。

## Implementation Boundary

Implementation plan should be split into small steps:

1. Add Registry model and tests with fake disposable resources.
2. Connect existing runtime cache retain/dispose behavior to Registry.
3. Connect plugin lifecycle events to cleanup calls.
4. Add failure accounting and circuit-open tests.
5. Update docs and release verification.

The implementation must avoid speculative UI changes and avoid opening new plugin runtime capabilities.
