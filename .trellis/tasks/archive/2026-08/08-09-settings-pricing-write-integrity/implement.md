# 实施清单

1. [ ] 读取 settings ownership contract，枚举普通设置生产 writer。
2. [ ] 为普通 set/patch 建共享 scope 和 changed-key persistence path。
3. [ ] 先补反向完成并发测试，再调整缓存同步。
4. [ ] 将 alias 编辑读取切换为 strict read，并实现 UI 错误阻断。
5. [ ] 对齐 alias schema/version fixture，保留成本统计 fail-open。
6. [ ] 运行聚焦 Vitest/Rust tests、typecheck、lint、bindings 和 diff check。
