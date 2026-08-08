# 零余额手动刷新回归实施计划

## 1. 定位与失败测试

- [x] 对比归档任务 `07-17-fix-account-balance-manual-refresh` 与当前共享 runtime 链路。
- [x] 确认 Provider 可用性测试不拥有账户刷新或缓存失效职责。
- [x] 增加 runtime 异步测试，复现 fresh zero 恢复及旧请求在途时的尾随强刷。
- [x] 记录现实现的失败输出，确认根因后再修改生产逻辑。

## 2. Runtime 修复

- [x] 用 force epoch 将显式刷新等待绑定到实际强刷提交，拒绝普通完成事件提前满足。
- [x] 对在途请求合并至多一个 pending 尾随强刷，并在完成后立即唤醒调度器。
- [x] 保证 pending/in-flight force 不依赖定时刷新、桌面 heartbeat 或 Gateway lease 才能完成。
- [x] 保留 generation/config token、并发上限和 route projection 提交保护。

## 3. 跨层回归

- [x] 覆盖 Sub2API、NewAPI billing、NewAPI account 和 custom 共享 runtime 所有权。
- [x] 扩展前端 Query 测试，锁定零余额恢复、连续刷新、旧自动结果和副作用隔离。
- [x] 运行 Provider 编辑器相关集成测试与账户 gate/failover focused tests。

## 4. 质量与收尾

- [x] 执行 Rust fmt/check/clippy/full tests、前端 test/typecheck/lint、`git diff --check`。
- [x] 执行 `trellis-check` 与 `trellis-break-loop`，同步账户用量契约和通用检查清单。
- [ ] 提交产品修复、记录完成信息并归档任务。
