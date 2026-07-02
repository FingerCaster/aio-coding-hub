# 插件 Manifest

插件 manifest 文件名是 `plugin.json`，遵循 [Manifest v1 完整规范](../../plugin-manifest-v1.md)。

必填字段：

- `id`：带发布者命名空间的 ID，例如 `publisher.plugin-name`。
- `name`：展示给用户的插件名称。
- `version`：使用 SemVer 的插件版本。
- `apiVersion`：使用 SemVer 的插件 API 版本。
- `main`：Extension Host 入口文件，例如 `dist/extension.js`。
- `runtime`：社区插件必须使用 `runtime.kind = "extensionHost"`，`language` 必须是 `typescript`。
- `hostCompatibility`：应用版本和插件 API 兼容性约束。

`official.*` 命名空间只保留给内置官方插件。本地包、marketplace 包和 GitHub 包必须使用自己的发布者命名空间。

最小 Extension Host runtime：

```json
{
  "main": "dist/extension.js",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  }
}
```

Gateway hook 贡献点示例；公开文档中的规范名是 `contributes.gatewayHooks`：

```json
{
  "activationEvents": ["onGatewayHook:gateway.request.afterBodyRead"],
  "contributes": {
    "gatewayHooks": [
      {
        "name": "gateway.request.afterBodyRead",
        "priority": 100,
        "failurePolicy": "fail-open",
        "timeoutMs": 5000
      }
    ]
  },
  "capabilities": ["gateway.hooks"]
}
```

`timeoutMs` 可选；不填时使用宿主默认 hook timeout。需要扫描大 payload 或调用宿主侧重处理逻辑的插件可以自行声明更长的正整数毫秒值。

Capability dependency table：

| Contribution | Required capability |
| --- | --- |
| `commands` | `commands.execute` |
| `providers` | `provider.extensionValues` |
| `ui.providers.editor.sections` | `provider.extensionValues` |
| `ui.providers.editor.fields` | `provider.extensionValues` |
| UI button fields in host-rendered sections/panels | `commands.execute` |
| `gatewayHooks` | `gateway.hooks` |
| `protocolBridges` | `protocol.bridge` |

Protocol bridge MVP skeleton 只稳定 `protocolBridges` 的 `manifest` 声明、`protocol.bridge` 能力依赖、贡献注册表元数据和安装预检展示。当前执行入口会返回 `PLUGIN_EXTENSION_PROTOCOL_BRIDGE_NOT_IMPLEMENTED`；完整协议互换执行链仍属于未来宿主集成。插件不能只靠声明 bridge 就接管 OpenAI、Gemini 或 Claude 协议转换。

契约、SDK 和 Rust 校验认识的 UI 插槽名称多于当前前端已挂载位置。当前前端实际挂载 `providers.editor.sections`、`settings.sections` 和 `logs.detail.tabs`。`providers.editor.fields` 会被 SDK/Rust 校验为 provider UI contribution 并要求 `provider.extensionValues`，但当前前端没有对应类型化插槽挂载；`providers.card.badges`、`providers.card.actions` 和其他契约插槽目前只是 `manifest` 已知或仅用于元数据。

`hostCompatibility` 必须包含 `app` 和 `pluginApi`。`platforms` 当前是解析和展示元数据，不参与本地安装阻断或市场兼容性筛选；不要把它写成当前已强制执行的平台白名单。

`configSchema` 可以包含标准 JSON Schema 展示字段和 AIO `x-aio-ui` 元数据。详见 [Config Schema](./config-schema.md)。

plugin API v1 的 active hooks 见 [Hooks](./hooks.md)。Reserved hooks 会在宿主真正实现前被 manifest 校验拒绝。Reserved permissions 只作为内部/legacy runtime history 的 host-mediated label 保留，不是 public Extension Host manifest 字段。

旧的 WASM、process 和第三方 native manifest 属于 unsupported pre-release legacy runtime。当前公开插件应迁移到 Extension Host，迁移说明见 [运行时说明](../runtime/README.md)。
