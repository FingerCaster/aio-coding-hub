# Bug Analysis: 零余额恢复后手动刷新仍依赖测试或等待

## 1. Root Cause Category

- **Category**: B - Cross-Layer Contract
- **Specific Cause**: 账户用量刷新所有权从前端迁入共享 runtime 后，显式手动刷新仍通过
  `completion_generation + N` 推算自己应等待的结果。该计数同时包含普通后台完成、配置同步和
  失效通知，并不标识某次强刷。旧请求在途时，尾随强刷只设置标志并等待 1 秒 scheduler tick，
  现有测试也只直接检查标志，未执行真实调度和返回链路。次要类别是 D（缺少 runtime 异步
  zero-to-positive 回归）和 E（把“收到完成通知”误认为“用户要求的强刷已提交”）。

## 2. Why The Previous Fix Regressed

1. 2026-07-17 的修复正确统一了 TanStack Query key、取消边界和缓存写入者，解决的是前端旧
   Promise 逆序覆盖。
2. 后续 custom account-usage 工作把远端刷新、缓存与 in-flight 合并迁到 Rust runtime，但只保留
   了前端 deferred 测试；新增 Rust 测试没有真正运行 `refresh -> dispatch -> perform_refresh ->
   waiter`，因此同名的“force-tail”契约缺少可执行验证。
3. Provider 可用性测试从不更新账户用量。它耗费的时间让旧后台请求或周期调度先结束，所以在
   用户观察上与余额恢复相关，实际只是掩盖时序缺陷。

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | 每个合并手动强刷使用 checked force epoch；普通完成只唤醒，不能满足 waiter | DONE |
| P0 | Scheduling | fetch 完成事件立即唤醒尾随调度；pending/in-flight force 自身保持 scheduler 活跃 | DONE |
| P0 | Commit Order | generation 校验、snapshot 读取保持原子；route projection 在 force 完成通知前发布 | DONE |
| P0 | Test Coverage | 可控 fetcher 运行 fresh zero、旧请求在途、双 waiter、target replacement 和全 adapter runtime 测试 | DONE |
| P1 | Documentation | 更新账户用量契约与 cross-layer thinking guide，并同步模板 | DONE |

## 4. Systematic Expansion

- **Similar Issues**: 任何用共享完成计数等待特定语义操作的 scheduler、single-flight cache 或后台
  coordinator 都可能被无关事件提前满足；应区分“通知 revision”和“操作 identity”。
- **Design Improvement**: 合并窗口由 pending epoch 明确定义；同一窗口的调用者共享 epoch，当前
  fetch 已开始后最多排一个新 epoch。结果提交、下游 projection 和 waiter completion 形成有序边界。
- **Process Improvement**: 所有把请求所有权迁到新层的重构，都必须把旧层的竞态测试迁成新 owner
  的真实异步测试，不能只为状态辅助函数写单测。

## 5. Knowledge Capture

- [x] 更新 Provider account-usage 可执行契约。
- [x] 更新 cross-layer thinking guide。
- [x] 同步 `src/templates/markdown/spec` thinking guide 模板。
- [x] 增加 Rust runtime 真实异步回归和前端 `zero_balance -> available` 回归。
- [x] 明确 Provider 可用性测试不是账户刷新或恢复的前置条件。
