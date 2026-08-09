# 研究摘要

- `commands/plugins.rs:305-322` 的 local install 输入只有 path，preview checksum 未绑定 confirm。
- `infra/plugins/repository.rs:300/412` 使用 `INSERT OR IGNORE INTO plugin_versions`。
- `gateway/plugins/context.rs:22-29` body budget 使用 gateway 最大请求体；SDK/合同声明 camelCase，而 runtime 可见字段存在 snake_case。
- `gateway/plugins/pipeline.rs:414-443/660-682` 对非法 Header patch 无条件返回错误，即使插件 fail-open。
- Extension Host 已有调用 timeout 和 lazy recycle，但缺少覆盖 gate/queue/cold start/cleanup 的 absolute deadline 与主动 sweeper。
- 候选主线提交：`e94c83bd`、`cab1229a`、`4ee5faa8`、`e6cf04d3`、`d26524f2`、`735cec12`、`94da784b`、`4800bc87`、`871b84dc`。
- 用户已确认保留插件运行时硬化；当前主工作区正在删除 `packages/plugin-sdk`、`packages/create-aio-plugin`，本任务只能修改生产 runtime，不得恢复这两个包。
