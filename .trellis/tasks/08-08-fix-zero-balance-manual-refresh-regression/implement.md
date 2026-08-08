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
- [x] 提交产品修复、记录完成信息并归档任务。

## 完成记录

- 产品提交：`eedf1069 fix(providers): make manual balance refresh authoritative`。
- 根因：共享账户用量 runtime 只记录通用完成次数；旧自动请求在途时，手动强刷只能登记尾随标记，并依赖下一次固定调度 tick 才真正发出，因此调用方可能先读回旧的零余额快照。
- 修复：使用 force epoch 将每次显式刷新绑定到对应强刷结果；通过事件唤醒在旧请求完成后立即派发至多一个合并的尾随强刷，并在同一代配置校验和 route projection 提交完成后才唤醒等待者。
- 行为保证：零余额恢复不依赖 Provider 可用性测试、自动刷新、桌面 heartbeat 或 Gateway lease；Sub2API、NewAPI billing、NewAPI account 与 custom 适配器共享同一语义。
- 回归验证：runtime focused 15 项、账户用量 focused 83 项、前端 304 个文件共 2749 项、Rust lib 2624 项通过（4 项忽略）且全部集成测试通过。
- 质量门：`cargo fmt/check/clippy`、TypeScript typecheck、ESLint、生成绑定检查、`git diff --check`、Trellis task validate 均通过。
