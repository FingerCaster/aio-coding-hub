# 插件架构说明

这里放维护者和高级插件作者需要理解的设计约束。普通插件开发优先阅读 [插件开发总指南](../developer-guide.md) 和 [API 参考](../reference/README.md)。

- [安全与隔离](./security.md)：最小 host surface、运行时隔离、fail-closed、quarantine 和默认 hook timeout。
- [架构审计](./audit.md)：官方插件收敛、信任边界、运行时选择、性能与稳定性建议。

0.62 的内部平台内核调整不会改变 Plugin API v1 外部契约。维护者评估兼容性时，先看 [兼容性说明](../reference/compatibility.md)，再对照 [架构审计](./audit.md) 中的 0.62 决策记录。
