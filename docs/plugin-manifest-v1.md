# 插件 Manifest v1

`plugin.json` 是插件与 AIO Coding Hub 之间稳定的 package contract。Plugin API v1 的社区插件只有一种公开运行时：Extension Host。社区插件必须提供 `main`，声明 `runtime.kind = "extensionHost"`，并把 TypeScript 或 JavaScript 源码打包成宿主可加载的 JavaScript 输出。

Official Privacy Filter 也使用同一套 Extension Host manifest、capability、hook 和 lifecycle 规则。官方身份只体现在 bundled 分发和默认配置上，不体现在独立 runtime、独立权限或宿主特例上。

## 1. 必填字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | string | 全局唯一插件 ID。 |
| `name` | string | 展示给用户的名称。 |
| `version` | string | 插件版本，使用 SemVer。 |
| `apiVersion` | string | 插件 API 版本，例如 `1.0.0`。 |
| `main` | string | Extension Host 入口文件；`main` points at bundled JavaScript output，例如 `dist/extension.js`。 |
| `runtime` | object | 必须是 `runtime.kind = "extensionHost"`，`language` 必须是 `typescript`。 |
| `hostCompatibility` | object | 支持的 AIO Coding Hub 宿主版本范围。 |

Extension Host manifest 不再使用 top-level `hooks` 或 top-level `permissions`。Hook、command、provider UI 和 protocol bridge 通过 `contributes` 声明；宿主用 `capabilities` 控制这些贡献点是否可以生效。

## 2. 可选字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `activationEvents` | array | Extension Host 激活事件，例如 `onGatewayHook:gateway.request.afterBodyRead`。 |
| `contributes` | object | `commands`、`providers`、`protocolBridges`、`gatewayHooks` 和 host-rendered `ui` 贡献点。 |
| `capabilities` | array | 插件声明自己需要的宿主能力，例如 `capabilities: ["gateway.hooks"]`。 |
| `configSchema` | object | 用于用户配置的 JSON Schema subset。 |
| `configVersion` | integer | 配置 schema 版本。 |
| `description` | string | 展示给用户的简短摘要。 |
| `author` | string or object | 作者元数据。 |
| `homepage` | string | 项目主页 URL。 |
| `repository` | string or object | 源码仓库元数据。 |
| `license` | string | 尽量使用 SPDX license expression。 |
| `checksum` | string | Package checksum。 |
| `signature` | string | Package signature。 |
| `category` | string | `security`、`productivity`、`redaction` 或 `utility`。 |

## 3. ID 与版本规则

Plugin IDs 使用 `publisher.plugin-name` 格式。

- publisher 和 name segment 必须是 lowercase ASCII。
- 每个 segment 可以包含字母、数字和 hyphen。
- 使用 dots 分隔 namespace segments。
- Path separators、`..`、spaces、shell metacharacters 和 empty segments 都是非法的。
- `official.privacy-filter` 是唯一 bundled official plugin ID。
- `official.*` namespace 只能通过 built-in official plugin source 安装；local、marketplace 和 GitHub packages 必须使用自己的 publisher namespace。

Versions 必须遵循 SemVer。Pre-release versions 可用于本地开发和 unsigned packages；marketplace stable releases 应使用 release versions。

`apiVersion` 独立于 app version。0.62.x 只支持 Plugin API major `1`，所以 manifest 的 `apiVersion` 必须是 `1.x.y`。宿主可以在同一 major API 内添加 backward-compatible fields。Breaking changes 需要新的 major API。

## 4. Runtime

Extension Host 是唯一 community runtime：

```json
{
  "main": "dist/extension.js",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  }
}
```

`main` 必须是包内相对路径，指向 `.js` 或 `.cjs` 文件。推荐源码使用 TypeScript 或 JavaScript，发布包只携带打包后的 JavaScript 输出。宿主负责加载、激活、超时控制、失败策略和 dispose；插件不能直接创建或持有宿主 runtime 实例。

旧的 WASM、process 和 native 运行时属于 unsupported pre-release legacy runtime。公开插件和官方 bundled 插件都不能声明这些运行时；迁移到 Extension Host 后通过 `contributes.gatewayHooks` 和 `api.gateway.registerHook` 实现网关扩展。

## 5. Host Compatibility

`hostCompatibility` 约束插件安装和启用：

```json
{
  "app": ">=0.60.0 <1.0.0",
  "pluginApi": "^1.0.0",
  "platforms": ["macos", "windows", "linux"]
}
```

不兼容插件会被标记为 `incompatible`，不会进入 hook pipeline。

## 6. Contributions 与 Capabilities

Contribution points 只描述插件希望接入的位置；capability 是宿主开放对应贡献点/API surface 的 manifest contract。缺少依赖 capability 的 manifest 会被拒绝。

| Contribution | Required capability |
| --- | --- |
| `commands` | `commands -> commands.execute` |
| `providers` | `providers / provider UI -> provider.extensionValues` |
| `ui.providers.editor.sections` | `providers / provider UI -> provider.extensionValues` |
| `ui.providers.editor.fields` | `providers / provider UI -> provider.extensionValues` |
| UI button fields in host-rendered sections/panels | `commands -> commands.execute` |
| `gatewayHooks` | `gatewayHooks -> gateway.hooks` |
| `protocolBridges` | `protocolBridges -> protocol.bridge` |

Gateway integration 必须使用 `contributes.gatewayHooks` + `capabilities: ["gateway.hooks"]` + Extension Host 入口中的 `api.gateway.registerHook`。

Protocol bridge MVP skeleton 只稳定 manifest 声明、capability dependency 和安装预检展示。完整协议互换执行链仍属于 future host integration；插件不能只靠声明 `protocolBridges` 就接管 OpenAI、Gemini 或 Claude 协议转换。

`providers.card.badges` 和 `providers.card.actions` 是已命名的 UI slots，但当前 Extension Host manifest validation 不把它们绑定到 `provider.extensionValues`。只有上表列出的依赖会被 SDK 和 Rust validation 强制检查。

## 7. Hook v1

Active hooks in plugin API v1 是当前已经接入 gateway 或 log pipeline 的 hooks。Reserved hooks for future host integration 会被记录下来以稳定命名；但在宿主实现对应调用点前，manifest validation 会用 `PLUGIN_RESERVED_HOOK` 拒绝它们。

| Hook | 触发时机 | 可修改内容 | 默认超时 | 默认失败策略 | Host-mediated context/mutation labels |
| --- | --- | --- | --- | --- | --- |
| `gateway.request.afterBodyRead` | Body reader 完成 allowed body buffering 后 | headers 和 request body | 150 ms | fail-open | `request.meta.read`, `request.header.read`, `request.header.readSensitive`, `request.body.read`, `request.header.write`, `request.body.write` |
| `gateway.request.beforeSend` | provider resolution 后、reqwest 发送 upstream request 前 | headers 和 request body | 150 ms | fail-open | `request.meta.read`, `request.header.read`, `request.header.readSensitive`, `request.body.read`, `request.header.write`, `request.body.write` |
| `gateway.response.chunk` | 每个 bounded streaming response chunk | stream chunk | 150 ms | fail-open | `stream.inspect`, `stream.modify` |
| `gateway.response.after` | 大小预算内的完整 non-stream response | headers 和 response body | 150 ms | fail-open | `response.header.read`, `response.body.read`, `response.header.write`, `response.body.write` |
| `gateway.error` | gateway error response materialization 后、发送前 | headers 和 error response body | 150 ms | fail-open | `response.header.read`, `response.body.read`, `response.header.write`, `response.body.write` |
| `log.beforePersist` | Request 或 audit log 持久化前 | log message | 150 ms | fail-open | `log.redact` |

Streaming hooks 接收 bounded chunks 和固定大小 sliding window，不会接收无限制完整响应。

Reserved hooks：

- `gateway.request.received`
- `gateway.request.beforeProviderResolution`
- `gateway.response.headers`

## 8. Host-mediated context and mutation labels

Extension Host public manifest 不支持 top-level `permissions`。下面这些 labels 是 gateway hook visible context、mutation envelope、audit 和 legacy official runtime history 使用的内部 contract 名称；它们不是社区 Extension Host manifest 字段，也不是插件作者可以通过 `plugin.json` 申请的授权项。

宿主会按 hook、capability、context budget 和运行时策略决定实际提供哪些 context fields，并在应用 mutation 前再次校验输出 envelope。插件必须把缺失或被截断的 body、headers、stream chunk、log message 和 normalized messages 视为正常情况。

Internal active labels：

| Label | Risk | 说明 |
| --- | --- | --- |
| `request.meta.read` | low | 读取 method、path、CLI key、trace ID、provider hints。 |
| `request.header.read` | medium | 读取非敏感 request headers。 |
| `request.header.readSensitive` | high | 读取 `Authorization` 和 `Cookie` 等 sensitive request headers。 |
| `request.header.write` | high | 修改 request headers。 |
| `request.body.read` | high | 读取 request body。 |
| `request.body.write` | high | 修改 request body。 |
| `response.header.read` | low | 读取 response headers。 |
| `response.header.write` | medium | 修改返回给 CLI 的 safe response headers。 |
| `response.body.read` | high | 在预算内读取完整 non-stream response body。 |
| `response.body.write` | high | 修改 non-stream response body。 |
| `stream.inspect` | high | 检查 streamed chunks 和 sliding window。 |
| `stream.modify` | high | 替换或阻断 streamed chunks。 |
| `log.redact` | medium | 持久化前脱敏 log fields。 |

Reserved permissions for future host-mediated APIs 只作为内部命名保留。社区 Extension Host manifest 不能声明它们；如果 legacy official runtime history 中出现保留项，宿主会按内部 runtime policy 拒绝或隔离。

| Label | Risk | Future host-mediated API |
| --- | --- | --- |
| `plugin.storage` | medium | 使用隔离 plugin storage。 |
| `network.fetch` | high | 发起 host-mediated network requests。 |
| `file.read` | high | 读取 host-mediated files。 |
| `file.write` | high | 写入 host-mediated files。 |
| `secret.read` | critical | 读取 host-managed secrets。 |

High-risk 和 critical labels 会用于宿主风险文案、审计和未来 host-mediated API 设计，但不会恢复为 Extension Host public manifest permissions。

Validation 会拒绝：

- Unknown hook names。
- Reserved hook names。
- Extension Host manifest 中的 top-level `permissions`。
- 缺少贡献点所需 capability。
- Extension Host manifest 中的 top-level `hooks`。
- 在 host 提供对应 API 前使用 unsupported legacy runtime 或 legacy gateway rule fields。

## 9. Config Schema 子集

受支持的 `configSchema` subset 包括：

- `string`
- `number`
- `integer`
- `boolean`
- `enum`
- `array`
- `object`
- `password`

插件不能提供 custom GUI code。宿主负责渲染表单、保存前校验，并在启用前再次校验。Sensitive values 不会以 plaintext 返回前端。

## 10. 状态机

状态：

- `available`
- `installed`
- `enabled`
- `disabled`
- `update_available`
- `incompatible`
- `quarantined`
- `uninstalled`

允许的状态转换：

| From | To | Trigger |
| --- | --- | --- |
| `available` | `installed` | 用户安装 package 或 market plugin。 |
| `installed` | `enabled` | 用户确认插件且配置有效。 |
| `installed` | `disabled` | 用户安装但不启用。 |
| `enabled` | `disabled` | 用户禁用插件。 |
| `disabled` | `enabled` | 用户在校验通过后启用插件。 |
| `enabled` | `update_available` | Market 发现新的兼容版本。 |
| `disabled` | `update_available` | Market 发现新的兼容版本。 |
| `update_available` | `enabled` | 更新成功且 capabilities 仍有效。 |
| `update_available` | `disabled` | 更新成功但新增 capability 需要用户确认。 |
| `installed` | `incompatible` | Host/API/platform version 不兼容。 |
| `enabled` | `quarantined` | 重复 crash、timeout、signature failure 或 revoked market status。 |
| `disabled` | `quarantined` | Signature failure 或 revoked market status。 |
| `quarantined` | `disabled` | 用户确认并在校验后恢复。 |
| any active state | `uninstalled` | 用户卸载插件。 |

Upgrade failure 会恢复 previous version、config snapshot、capabilities 和 enabled state。Signature failure 会让插件进入 `quarantined`。Runtime crash 和 repeated timeout 可以让 enabled plugin 进入 `quarantined`。

## 11. Manifest 示例：社区 Prompt Helper

```json
{
  "id": "acme.prompt-helper",
  "name": "Prompt Helper",
  "version": "1.0.0",
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
        "priority": 100,
        "failurePolicy": "fail-open"
      }
    ]
  },
  "capabilities": ["gateway.hooks"],
  "hostCompatibility": {
    "app": ">=0.60.0 <1.0.0",
    "pluginApi": "^1.0.0",
    "platforms": ["macos", "windows", "linux"]
  },
  "configSchema": {
    "type": "object",
    "required": ["mode"],
    "properties": {
      "mode": {
        "type": "string",
        "enum": ["append_instruction", "prepend_context"]
      }
    }
  }
}
```

`dist/extension.js`：

```js
module.exports.activate = function(api) {
  api.gateway.registerHook("gateway.request.afterBodyRead", function(context) {
    const body = String(context?.request?.body ?? "");
    if (!body) return { action: "continue" };
    return {
      action: "replace",
      requestBody: body.replace("DRAFT_PROMPT", "Please answer concisely.")
    };
  });
};
```

这个示例不声明 `permissions`。Extension Host manifest 只能声明 `capabilities` 和 `contributes`；`context.request.body` 是否可见、`requestBody` mutation 是否被接受，都由宿主按当前 hook contract、capability 和 context/output budget 判断。插件必须在字段缺失或为空时继续运行。

## 12. Manifest 示例：Privacy Filter

`official.privacy-filter` 是 bundled official Extension Host 插件。它使用普通插件 manifest、普通 gateway hook 生命周期和普通 Extension Host disposal；官方身份只提供默认安装源和默认配置。

```json
{
  "id": "official.privacy-filter",
  "name": "Privacy Filter",
  "version": "1.0.0",
  "apiVersion": "1.0.0",
  "category": "privacy",
  "description": "Official Extension Host privacy filter aligned with packyme/privacy-filter for pre-upstream prompt and log redaction.",
  "homepage": "https://github.com/packyme/privacy-filter",
  "repository": {
    "type": "git",
    "url": "https://github.com/packyme/privacy-filter.git"
  },
  "license": "MIT",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  },
  "main": "dist/extension.js",
  "activationEvents": [
    "onGatewayHook:gateway.request.afterBodyRead",
    "onGatewayHook:gateway.request.beforeSend",
    "onGatewayHook:log.beforePersist"
  ],
  "capabilities": ["gateway.hooks", "privacy.redact"],
  "contributes": {
    "gatewayHooks": [
      { "name": "gateway.request.afterBodyRead", "priority": 5, "failurePolicy": "fail-closed" },
      { "name": "gateway.request.beforeSend", "priority": 5, "failurePolicy": "fail-closed" },
      { "name": "log.beforePersist", "priority": 1, "failurePolicy": "fail-closed" }
    ]
  },
  "hostCompatibility": {
    "app": ">=0.60.0 <1.0.0",
    "pluginApi": "^1.0.0",
    "platforms": ["macos", "windows", "linux"]
  }
}
```
