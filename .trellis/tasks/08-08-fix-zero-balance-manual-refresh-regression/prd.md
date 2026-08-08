# 修复零余额恢复后手动刷新仍返回旧结果

## Goal

账户用量已显示余额为 0、上游余额随后恢复时，用户点击手动刷新必须直接发起并等待一次权威强制刷新，随后显示恢复后的余额；不得要求先执行 Provider 可用性测试，也不得要求等待自动刷新。

当前可配置模型路由父任务及已排队的“余额跳过后终局熔断 Provider 试探”任务完成后，再启动本任务的定位和实现。

## Regression Context

- 用户于 2026-08-08 再次报告：余额已经耗尽并显示为 0 后，上游余额恢复，点击刷新仍无法恢复；必须先点击 Provider 测试，再刷新才成功。
- 相同现象曾由归档任务 `07-17-fix-account-balance-manual-refresh` 修复。旧根因是自动查询与手动查询通过不同 TanStack Query 生命周期写同一缓存，较旧自动结果可覆盖较新的手动结果。
- 此后归档任务 `08-08-custom-account-usage-script` 将远端刷新、Provider 级缓存、in-flight 合并、尾随强刷和 generation 提交保护迁移到 `ProviderAccountUsageRuntimeState`。本次必须以届时最新代码重新定位，不能假定仍是旧的纯前端竞态。
- Provider 可用性测试与账户用量刷新在产品契约上互不依赖。测试成功可以改变健康/熔断状态，但不得成为余额刷新生效的前置条件。

## Requirements

- 精确复现状态转换：首次权威结果为 `zero_balance`，上游随后返回正可消费额度，用户立即点击一次手动刷新。
- 手动刷新必须绕过仍属新鲜的零余额快照，并等待本次强刷或其合规尾随刷新结果；不能直接返回旧快照，也不能被当前 in-flight 的旧完成吞掉。
- 若同 Provider 已有查询在途，显式强刷继续遵守“当前请求之后至多一次尾随刷新”的有界合并契约，且调用者等待尾随结果，不得把强刷折叠为旧完成。
- 后端 runtime generation/config token、前端 exact-key cancellation 与逆序提交保护必须共同保证旧结果不能覆盖恢复后的新结果。
- 手动刷新不得调用或依赖 Provider 可用性测试、circuit reset、enable/disable、reorder、Session 或模型路由副作用。
- 失败路径应保留准确的失败/陈旧展示语义；不得伪造余额恢复，也不得泄漏 API Key、上游响应或敏感错误文本。
- 需要核查 `sub2api`、NewAPI billing、NewAPI account 和 custom adapter 的共同刷新所有权；修复应落在共享层，除非证据证明故障只属于某个协议适配器。

## Acceptance Criteria

- [ ] 自动化回归证明 `zero_balance -> 上游恢复 -> 单击手动刷新` 会发起新的远端查询并展示正余额，不需要先测试 Provider 或等待自动刷新。
- [ ] deferred/in-flight 测试证明旧查询晚完成不能覆盖手动强刷结果；在途请求场景最多产生一次尾随强刷，调用者收到尾随结果。
- [ ] fresh cache、`timedRefreshEnabled=false`、连续手动点击和多展示消费者场景均遵守同一强刷语义。
- [ ] 可用性测试未被调用，账户刷新不改变 circuit、Provider 启停/顺序、Session 或路由状态。
- [ ] 至少覆盖内置账户用量适配器，并对共享 runtime 路径做协议无关测试；若 custom adapter 走同一路径，也必须保持一致。
- [ ] 相关前端 Query、Rust runtime/IPC、Provider 编辑器集成测试及全量质量门通过。

## Investigation Notes

- 开始定位时先对比归档任务 `07-17-fix-account-balance-manual-refresh` 的旧竞态修复与当前 `ProviderAccountUsageRuntimeState` 的强刷/尾随刷新实现。
- 重点检查：零余额快照的新鲜度短路、强刷标志是否传到 runtime、in-flight 订阅者等待的是当前完成还是尾随完成、完成通知与 generation 提交次序、前端刷新后是否只读取旧 snapshot。
- 不用“测试成功顺带 invalidate 账户缓存”作为修复；这会保留错误依赖并掩盖真实刷新所有权缺陷。
