# 插件示例

这里放官方示例和推荐社区插件形态。示例的目标是展示插件系统应该怎样被使用，而不是扩展宿主内置插件数量。

- [Privacy Filter](./privacy-filter.md)：当前唯一内置官方 Extension Host 插件 `official.privacy-filter`，对齐 `packyme/privacy-filter` 的核心脱敏能力。

## 示例清单

| 示例 ID | 生成模板 | 目标 | Hooks | Capabilities | Fixtures / 覆盖路径 |
| --- | --- | --- | --- | --- | --- |
| `official.privacy-filter` | 内置官方插件 | 请求和日志脱敏 | `gateway.request.afterBodyRead`, `gateway.request.beforeSend`, `log.beforePersist` | `gateway.hooks`, `privacy.redact` | 官方 fixture 存在于宿主资源目录；覆盖配置 UI、request replay export 和日志脱敏边界 |
| `examples/prompt-helper` | `example:prompt-helper` | 在请求进入 provider 前补充提示词约束 | `gateway.request.afterBodyRead` | `gateway.hooks` | 包含 `fixtures/claude-request.json` 和 `fixtures/codex-request.json`；覆盖 Claude messages 和 Codex/OpenAI Responses request mutation |
| `examples/redactor` | `example:redactor` | 展示 Extension Host gateway hook 脱敏形态 | `gateway.request.beforeSend`, `log.beforePersist` | `gateway.hooks` | 包含 request hit/miss 和 log redact fixtures；覆盖 pack、publish-check 和市场安装元数据 |
| `examples/response-guard` | `example:response-guard` | 在 non-stream 响应返回后做轻量检查或标记 | `gateway.response.after` | `gateway.hooks` | 包含 `fixtures/response-warn.json` 和 `fixtures/response-pass.json`；覆盖响应 mutation 和 pass 路径 |

`examples/*` 是开发模板，不是默认可安装市场包。生成出的目录可以运行 `validate --strict`、`pack` 和 `publish-check`；Extension Host hook 行为要通过宿主运行报告、导出的 replay fixture 和桌面应用内复测确认。发布为真实 `.aio-plugin` artifact 仍需要单独的 checksum、signature、托管和市场索引流程。

这些示例都保持在 Plugin API v1 范围内。宿主负责运行诊断、fixture 导出、安装校验和市场索引解析；插件只声明 `main`、Extension Host runtime、contributions、capabilities 和自己的打包输出。
