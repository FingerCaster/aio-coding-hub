# 插件 SDK

`@aio-coding-hub/plugin-sdk` 提供 Extension Host 插件 manifest、hooks、capabilities、host-mediated context/mutation labels 和 validation helpers 的共享 TypeScript 契约。

SDK 面向这些场景：

- 插件作者编写 `plugin.json`。
- scaffold 和 packaging tools。
- marketplace/index validation。
- examples 和 compatibility tests。

## 包位置

本仓库中的 SDK 位于：

```text
packages/plugin-sdk
```

运行 SDK 检查：

```bash
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
```

## 主要类型

TypeScript SDK 导出：

- `PluginManifest`
- `PluginRuntime`
- `ExtensionRuntime`
- `PluginHook`
- `GatewayHookName`
- `PluginPermission`
- `PluginPermissionRisk`
- `PluginCapability`
- `PluginHookContext`
- `PluginHookResult`
- `PluginApi`
- `PrivacyApi`

同时导出辅助函数：

- `permissionRisk(permission)`
- `validateManifest(manifest)`

`PluginPermission` 和 `permissionRisk` 保留给 hook context/mutation label 风险展示、audit 和 legacy official runtime history。Extension Host manifest 不能声明 top-level `permissions`；`validateManifest` 会拒绝该字段。

`create-aio-plugin` 使用 SDK 做 manifest validation，并针对真实插件目录提供本地开发命令：

```bash
pnpm --filter create-aio-plugin cli doctor ./acme.redactor
pnpm --filter create-aio-plugin cli validate --strict ./acme.redactor
pnpm --filter create-aio-plugin cli pack ./acme.redactor
pnpm --filter create-aio-plugin cli publish-check ./acme.redactor
```

`create-aio-plugin replay` 当前不在本地执行 Extension Host gateway hooks；hook 行为应通过宿主 `plugin_hook_execution_reports`、`plugin_export_replay_fixture` 和桌面应用内复测确认。

开发工具的诊断对象使用稳定 shape，便于 CLI、GUI 和测试共享：

```json
{
  "severity": "error",
  "code": "PLUGIN_MISSING_CAPABILITY",
  "message": "gatewayHooks contribution requires gateway.hooks",
  "path": "plugin.json#/capabilities",
  "hint": "Add gateway.hooks to manifest.capabilities or remove contributes.gatewayHooks."
}
```

## TypeScript 中的最小 Manifest

```ts
import type { PluginManifest } from "@aio-coding-hub/plugin-sdk";
import { validateManifest } from "@aio-coding-hub/plugin-sdk";

const manifest: PluginManifest = {
  id: "acme.redactor",
  name: "Acme Redactor",
  version: "0.1.0",
  apiVersion: "1.0.0",
  main: "dist/extension.js",
  runtime: { kind: "extensionHost", language: "typescript" },
  activationEvents: ["onGatewayHook:gateway.request.afterBodyRead"],
  contributes: {
    gatewayHooks: [{ name: "gateway.request.afterBodyRead", priority: 50 }]
  },
  capabilities: ["gateway.hooks"],
  hostCompatibility: {
    app: ">=0.60.0 <1.0.0",
    pluginApi: "^1.0.0",
    platforms: ["macos", "windows", "linux"]
  },
  configSchema: {
    type: "object",
    required: ["enabled"],
    properties: {
      enabled: {
        type: "boolean",
        title: "启用处理",
        description: "关闭后插件不会修改请求内容。",
        default: true,
        "x-aio-ui": { widget: "switch", order: 10 }
      }
    }
  }
};

const result = validateManifest(manifest);
if (!result.ok) {
  throw new Error(`${result.error.code}: ${result.error.message}`);
}
```

Extension Host 入口示例：

```js
module.exports.activate = function(api) {
  api.gateway.registerHook("gateway.request.afterBodyRead", function(context) {
    const body = String(context?.request?.body ?? "");
    if (!body.includes("SECRET_TOKEN")) return { action: "continue" };
    return {
      action: "replace",
      requestBody: body.replaceAll("SECRET_TOKEN", "[REDACTED]")
    };
  });
};
```

## Capability 依赖

`validateManifest` 会检查贡献点需要的 capability：

| Contribution | Capability |
| --- | --- |
| `commands` | `commands.execute` |
| `providers` | `provider.extensionValues` |
| provider UI sections / fields | `provider.extensionValues` |
| UI button fields in host-rendered sections/panels | `commands.execute` |
| `gatewayHooks` | `gateway.hooks` |
| `protocolBridges` | `protocol.bridge` |

`providers.card.badges` 和 `providers.card.actions` 当前不是 capability dependency trigger。

`privacy.redact` 是宿主提供的脱敏 API capability。声明后，Extension Host 入口可以通过 `api.privacy.redactText` 和 `api.privacy.redactRequestBody` 调用宿主脱敏服务；它不自动声明 gateway hook，仍需要和 `gateway.hooks`、`contributes.gatewayHooks` 配合使用。

## SDK 边界

SDK 是契约包。它不执行插件代码，也不会授予宿主能力。

`PluginHookResult` 使用与 gateway host 相同的 active mutation envelope：

```ts
const result = {
  action: "replace",
  requestBody: "{\"messages\":[]}",
  headers: { "x-plugin-redacted": "1" }
} satisfies PluginHookResult;
```

替换内容时使用 `requestBody`、`responseBody`、`streamChunk`、`logMessage` 和 `headers`。`contextPatch` 不是 active gateway mutation field。

真正的宿主强制检查仍发生在 Rust application 中：

- manifest compatibility checks。
- capability contract checks。
- hook context trimming。
- mutation envelope enforcement。
- runtime timeout 和 failure policy handling。
- package checksum/signature verification。

## 版本建议

- `apiVersion` 应与插件 API 主版本保持一致。
- 插件包版本使用 SemVer。
- 如果 SDK 只添加向后兼容的类型，插件 API 主版本可以不变。
- Breaking hook、host-mediated label、runtime 或 manifest changes 需要新的插件 API 主版本。
