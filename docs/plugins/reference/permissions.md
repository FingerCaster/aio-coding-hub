# Hook context 与 mutation labels

Extension Host public manifest 不支持 top-level `permissions`。社区插件只通过 `capabilities` 声明 Host API/contribution surface，例如 `gateway.hooks`、`commands.execute`、`provider.extensionValues` 和 `protocol.bridge`。

本页列出的名称是 gateway hook visible context、mutation envelope、audit 记录和 legacy official runtime history 使用的内部 labels。宿主会按 hook、capability、context budget 和运行时策略裁剪 context；插件返回超出当前 hook envelope 或预算的 mutation 时，宿主会拒绝该输出。

Internal active labels：

- `request.meta.read`：低风险，读取方法、路径、CLI key、trace ID 等元信息。
- `request.header.read`：中风险，读取非敏感请求头。
- `request.header.readSensitive`：高风险，读取 `Authorization`、`Cookie` 等敏感请求头。
- `request.header.write`：高风险，修改请求头。
- `request.body.read`：高风险，读取请求体。
- `request.body.write`：高风险，修改请求体。
- `response.header.read`：低风险，读取响应头。
- `response.header.write`：中风险，修改返回给 CLI 的安全响应头。
- `response.body.read`：高风险，读取有大小预算保护的完整非流式响应体。
- `response.body.write`：高风险，修改非流式响应体。
- `stream.inspect`：高风险，读取流式响应 chunk 和 sliding window。
- `stream.modify`：高风险，替换或阻断流式响应 chunk。
- `log.redact`：中风险，在日志持久化前脱敏。

Reserved permissions for future host-mediated APIs 只作为内部命名保留，不是 public Extension Host manifest 字段：

- `plugin.storage`：中风险，使用隔离插件存储。
- `network.fetch`：高风险，通过宿主代理发起网络请求。
- `file.read`：高风险，通过宿主代理读取文件。
- `file.write`：高风险，通过宿主代理写入文件。
- `secret.read`：critical 风险，读取宿主管理的密钥。

如果 legacy official runtime history 中出现保留 label，宿主会按内部 runtime policy 拒绝或隔离。社区 Extension Host manifest 中出现 `permissions` 字段会被 `PLUGIN_INVALID_MANIFEST` 拒绝。

面向用户的安装和更新确认以 capabilities、contributions、runtime、package trust 和风险标签为准；新增 capability 需要用户重新确认，Extension Host 不提供 manifest `permissions` 字段。
