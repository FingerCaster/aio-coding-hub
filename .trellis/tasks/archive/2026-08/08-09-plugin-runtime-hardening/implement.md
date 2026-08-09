# 实施清单

1. [ ] 记录插件运行时纳入且不恢复 SDK/脚手架的范围决策，读取当前插件 spec/docs/tests。
2. [ ] 实现 preview/install digest binding 与版本 identity 回归。
3. [ ] 收紧 context budgets、wire casing 和旧插件兼容 fixture。
4. [ ] 对齐 header patch fail policy 与 fail-closed log barrier。
5. [ ] 用单一 absolute deadline 覆盖完整 invocation lifecycle。
6. [ ] 增加 idle sweeper 和 config/storage 原子更新。
7. [ ] 运行插件聚焦/full Rust tests、前端测试、bindings、typecheck、lint、Clippy 和 diff check。
8. [ ] 检查差异不包含 SDK/scaffold 恢复或 activation quarantine 扩张。
