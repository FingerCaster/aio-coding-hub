# 插件 API 参考

这里放插件作者需要查的稳定契约。日常开发先读 [插件开发总指南](../developer-guide.md)，遇到字段、hook、capability、host-mediated context labels 或打包规则不确定时，再回到本目录查询。

## 必读契约

- [Manifest](./manifest.md)：`plugin.json` 必填字段、Extension Host runtime、`main`、贡献点、capabilities 和兼容性入口。
- [Hooks](./hooks.md)：网关与日志 hook 的触发时机、上下文字段、超时和可修改字段。
- [Permissions](./permissions.md)：内部 context/mutation label 名称、风险等级和 Extension Host 非 manifest-permission 边界。
- [Config Schema](./config-schema.md)：配置表单 schema、`x-aio-ui` 和低代码渲染规则。

## 工具与发布

- [SDK](./sdk.md)：`@aio-coding-hub/plugin-sdk` 的 Extension Host manifest、hook result 和 validation helper 边界。
- [Publishing](./publishing.md)：`.aio-plugin`、`sha256`、Ed25519 签名、`publish-check`、市场索引、远程安装和 rollback。
- [Compatibility](./compatibility.md)：SemVer、`pluginApi`、platforms 和 legacy runtime 兼容规则。

## 调试与观测

- [Hooks](./hooks.md#observability-and-replay)：`plugin_hook_execution_reports`、`plugin_export_replay_fixture` 和各 hook 的 replay 边界。
- [运行时说明](../runtime/README.md)：host-owned lifecycle、Extension Host activation/dispose 和 release guard。

## 规范来源

- [Manifest v1 完整规范](../../plugin-manifest-v1.md)：规范性 manifest 文档。
- [plugin-api-v1-contract.json](../plugin-api-v1-contract.json)：hook、host-mediated labels、capability 和 runtime 的机器可读契约。
