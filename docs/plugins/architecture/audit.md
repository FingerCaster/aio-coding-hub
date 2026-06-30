# 插件架构审计

本文记录将官方 catalog 收敛到 `official.privacy-filter`，并将社区插件架构切到 Extension Host-only 后的审计结论。

## 决策

只保留 `official.privacy-filter` 作为 bundled official Extension Host plugin。

移除之前官方 catalog 中的 built-in prompt optimizer、safety detector 和 generic redactor examples。它们仍然是有效扩展场景，但应通过 Extension Host gateway hooks 作为社区插件实现。

WASM、process 和第三方 native 属于 unsupported pre-release legacy runtime。它们可以出现在迁移说明或拒绝测试中，但不能作为当前公开社区插件路径推荐。

## 架构依据

成熟插件系统通常保持小而可信的 host core，并暴露稳定 extension points，而不是不断累积 host-owned examples：

- VS Code uses manifest-declared [contribution points](https://code.visualstudio.com/api/references/contribution-points) and [activation events](https://code.visualstudio.com/api/references/activation-events).
- Chrome extensions require manifest-declared [permissions](https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions) and use constrained background [service workers](https://developer.chrome.com/docs/extensions/develop/migrate/to-service-workers).
- IDE plugin platforms expose explicit [extension points](https://plugins.jetbrains.com/docs/intellij/plugin-extension-points.html) and versioned compatibility contracts.

AIO Coding Hub 采用同样形态：

- `plugin.json` 声明 ID、`main`、Extension Host runtime、contributions、capabilities、config schema 和 host compatibility。
- Hooks 是明确的 gateway/log extension points，带有 bounded timeouts 和 host-trimmed contexts。
- 社区代码执行不会进入 Rust main process 和 WebView。
- `official.privacy-filter` 使用普通 Extension Host runtime。第三方包和官方插件都不能声明 legacy native engines；官方身份只体现在 bundled 分发和默认配置。

## 信任边界

当前 host trust boundary：

- Trusted：Rust host、gateway pipeline、database、packaged official privacy redaction service。
- Semi-trusted：signed marketplace metadata 和 package checksums。
- Untrusted by default：local packages、marketplace packages、GitHub release packages、Extension Host bundled JavaScript output、legacy rule files、legacy WASM bytecode、legacy process runtime binaries。

`official.*` namespace 必须继续由宿主拥有。本地、marketplace 和 GitHub 包必须使用类似 `acme.plugin-name` 的 publisher namespace。

## 扩展模型

推荐模型只有一个：

1. Extension Host：使用 `main` 加载打包后的 JavaScript 输出，通过 `contributes.gatewayHooks`、`protocolBridges`、`commands`、provider UI sections 和 capability dependency table 接入宿主。

Gateway hooks 必须使用 `contributes.gatewayHooks` 与 `api.gateway.registerHook`。Protocol bridge 必须使用 `protocolBridges` 与 `protocol.bridge` capability。Provider extension values 和 provider UI sections/fields 必须使用 `provider.extensionValues` capability。

不要开放第三方 native 插件，除非先补齐独立 signed binary policy、ABI stability story、crash isolation model、upgrade story 和 platform-specific security review。

## 性能与稳定性建议

保持 hot path 可预测：

- 按 priority 顺序执行 hooks，并使用固定 timeout budgets。
- 在暴露给插件前，对 request 和 response bodies 做大小边界控制。
- Stream hooks 保持 chunk-based，并提供 sliding-window context，而不是缓冲完整 stream。
- 按 plugin ID、version 和 runtime key 缓存 Extension Host worker state。
- 对非安全增强使用 fail open；只对用户明确启用的 security/privacy gates 使用 fail closed。
- 记录 runtime failures 和 circuit-open skips，避免坏插件持续拖慢 gateway。
- official bundled plugins 要少而聚焦，控制 host startup、binary size 和维护风险。

## v1.1 Performance Budgets

- Empty plugin pipeline request hook：不应有 allocation-heavy runtime dispatch，在维护者笔记本 performance smoke 上低于 25 microseconds。
- One noop Extension Host plugin request hook：在维护者笔记本 performance smoke 上低于 250 microseconds。
- 没有 `gateway.response.chunk` plugins 时：direct stream pass-through path 必须保持 active。
- Extension Host worker/cache 必须在 plugin snapshot refresh 后清理。
- Privacy Filter：compiled detector 必须按 plugin ID、version、installed directory 和 runtime key 缓存。

## 当前形态

Bundled official plugin：

- `official.privacy-filter`：与 `packyme/privacy-filter` 对齐的 Extension Host 插件，通过 `privacy.redact` 调用宿主脱敏服务，用于 irreversible pre-upstream privacy redaction 和 log redaction。

开放给社区的能力：

- Extension Host prompt helpers。
- Extension Host response safety checks。
- Extension Host log redactors。
- Extension Host commands、gatewayHooks 和 protocolBridges。
- Provider adapter facades remain internal；公开 provider 插件 API 尚未开放。

## 后续审计点

在把 plugin API v1 标记为 stable 前：

- 确认 hook names、capability names 和 host-mediated label names 已足够稳定，可以进入 semantic versioning。
- 把 official examples 保留为文档中的 community patterns，而不是 bundled host plugins。
- 增加 plugin hook overhead 和 Privacy Filter 在大型但允许 payload 上 redaction latency 的 benchmarks。
- 增加 telemetry-safe counters，记录 plugin timeouts、skips 和 quarantines，但不记录 sensitive payloads。

## v1.1 加固决策

- Plugin API v1.1 使用 `plugin-api-v1-contract.json` 作为 source of truth。
- Provider-neutral request context 通过 `request.normalizedMessages` 提供。
- Plugin refresh 时会清理 runtime caches。
- Plugin hot-path performance smoke tests 是 release readiness 的一部分。
- `create-aio-plugin replay` 当前不执行 Extension Host gateway hooks；fixture model 由宿主 `plugin_export_replay_fixture` 和运行报告保持一致。

## 0.62 Platform Kernel Decision

0.62 保持 Plugin API v1 externally compatible，重点是收紧内部平台边界而不是扩张公开 API。Contract metadata 成为 drift checks 的来源；hook 行为通过 internal descriptors 路由；runtime dispatch 从 gateway pipeline orchestration 中拆出；provider-specific behavior 开始迁移到 provider adapter facades 后面。

0.62 does not add public provider plugin APIs. Provider adapter facades remain internal so gateway selection, failover, circuit breaking, limits, OAuth handling, and session binding stay owned by the Rust gateway core.
