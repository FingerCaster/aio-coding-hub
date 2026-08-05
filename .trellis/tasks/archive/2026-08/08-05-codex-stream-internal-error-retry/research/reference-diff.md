# 参考差异

- 参考主功能 957b649 大幅重写 success_event_stream.rs、routes.rs 和 usage.rs，随后 358a999 修正迁移/日志语义；最终合并状态为 ca15f02。
- 参考设计明确使用 RetryPolicyMatch::StreamInternalError，但当前 fork 的 transport backoff 已在 12e565c0 调整了等待边界，不能复制旧等待代码。
- 当前 fork 的 success_event_stream.rs 在功能提交共同祖先之后加入 probe terminal commit、session convergence、Codex continuation 和 Provider route 行为；必须把 guard 作为提交前观察层包在现有终态逻辑外侧。
- 参考默认策略和容量规则迁移是用户可见配置，不应新增隐藏分类器；完整 Provider override 替换全局策略，不合并两者关键词。
- 1 MiB cap、guard timeout 和 downstream committed 都是放行/诊断，不是 Provider failure；最终失败 evidence 只投影最后一次错误，早期 evidence 仍留在 attempts chain。

