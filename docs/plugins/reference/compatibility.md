# 插件兼容性

插件兼容性使用 SemVer 描述。宿主安装、启用和升级插件时，会同时检查插件版本、插件 API 版本、应用版本、平台和运行时 ABI。

Manifest 中的关键字段：

- `version`：插件自身发布版本。
- `apiVersion`：该 manifest 使用的插件 API 版本。
- `hostCompatibility.app`：兼容的 AIO Coding Hub 应用版本范围。
- `hostCompatibility.pluginApi`：兼容的 pluginApi 版本范围。
- `hostCompatibility.platforms`：可选的平台 allowlist。

WASM 插件还需要声明 WASM ABI 版本：

```json
{ "kind": "wasm", "abiVersion": "1.0.0" }
```

宿主会拒绝不支持的主版本。0.62.x 只支持 Plugin API major `1`，因此 manifest 的 `apiVersion` 必须是 `1.x.y`，即使 `hostCompatibility.pluginApi` 声明支持 `^1.0.0` 也不能使用 `2.0.0`。未来插件 API 变更必须保持向后兼容；无法兼容时，需要提升主版本并让旧插件继续按旧契约运行或被明确标记为不兼容。

## 0.62 Internal Platform Kernel

Plugin API v1 remains externally compatible in 0.62. 这个版本重组的是宿主内部平台边界：contract metadata、hook descriptors、runtime policy、runtime cache lifecycle 和 provider adapter facades。

0.62 does not add public provider plugin APIs. Provider adapter 仍是 host-internal 设计，用于先降低 gateway/provider 分支扩散和维护成本；未来是否公开 provider 插件 API，需要另行设计版本化契约。

0.62 keeps third-party JavaScript and WebView plugin execution unsupported. 社区插件继续使用 `declarativeRules`；WASM 仍受宿主策略控制，未启用时安装或执行会被拒绝。

## 0.62.1 Developer Loop Boundary

0.62.1 does not change Plugin API v1. `doctor`, `validate --strict`, and `replay --explain` are developer tooling around the same manifest and hook contract.

Provider behavior remains host-owned. Provider ordering, failover, OAuth limits, token counting, cx2cc translation, and session binding are covered by internal acceptance tests, but no Provider Plugin API is exposed.

WASM remains policy-gated. The scaffold and pack flow can carry WASM artifacts, but marketplace WASM execution is not enabled by default.

## 0.62.2 Lifecycle Boundary

0.62.2 still keeps Plugin API v1 externally stable. 安装预检、更新 diff、rollback availability、quarantine reason 和 trust summary 都是宿主解释现有安装/更新规则的 lifecycle layer，不是新的 manifest schema，也不是插件可调用的新 API。

安装或更新前，宿主会把 `hostCompatibility`、runtime support、permissions、hooks、checksum/signature 和 package source 组合成 preview/diff 给用户确认。确认后，真实 install/update 仍会重新执行完整校验；preview/diff 不能作为跳过兼容性、签名或权限策略的依据。

兼容性判断仍以这些字段为准：

- `hostCompatibility.app` 必须匹配当前 AIO Coding Hub 版本。
- `hostCompatibility.pluginApi` 必须匹配当前 Plugin API v1。
- `hostCompatibility.platforms` 如果存在，必须包含当前桌面平台。
- runtime 必须由宿主策略支持。

Quarantined 和 incompatible 插件不能启用。更新新增的 permissions 会进入 pending，不会静默继承授权。

0.62.2 也不开放 browser-like plugin container。Linux、macOS、Windows 上的插件仍运行在宿主支持的本地 runtime 中；第三方插件不能把 AIO Coding Hub 内部变成浏览器或 WebView 插件容器。
