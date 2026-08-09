# 实施清单

1. [ ] 读取 gateway failover、attempt budget、upstream error handling 契约，绘制当前发送边界。
2. [ ] 先更新查询与契约测试，明确全局禁用优先级。
3. [ ] 在 common gate/retry boundary 加动态 enabled 检查。
4. [ ] 适配有界 Provider target self-loop validator 与测试缓存。
5. [ ] 补充 failover、account-usage、Session、Provider 专用路由回归。
6. [ ] 运行 Rust format、聚焦 Rust tests、Clippy/audit（可执行时）及 `git diff --check`。
7. [ ] 记录与候选实现的语义差异和未覆盖平台。
