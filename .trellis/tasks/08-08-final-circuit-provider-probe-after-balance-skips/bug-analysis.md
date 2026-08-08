# Bug Analysis: 余额跳过掩盖终局熔断试探

## 1. Root Cause Category

- **Category**: B - Cross-Layer Contract
- **Specific Cause**: 账户用量层正确提供了可信的
  `blocked_provider_ids`，共同 account gate 也正确拒绝发送，但无有效
  Session 绑定的 `NewUnboundSession` planner 仍用所有候选的 circuit
  状态判断 `all_open`。两个实际不可发送的 blocked/closed 候选因此把
  最后一个可恢复的 open 候选排除在 probe intent 之外。次要类别是 D
  （缺少 blocked prefix + final open 的联合 E2E）和 E（隐含假设
  `CLOSED` 等于实际可发送）。

## 2. Why Fixes Failed

1. 先前的余额 gate 修复：为稳定绑定的高优先级回切前缀增加了阻断
   抑制，但为了保留新会话的首次 account skip，刻意没有让 unbound
   分支读取该提示。这个范围划分只考虑了“是否保留候选”，没有区分
   “候选仍在 route 中”与“候选能否参与恢复状态分类”。
2. 既有 all-open 测试：覆盖了多个 open Provider 的失败、冷却和
   in-flight 推进，但没有把 durable account gate 与 unbound probe
   planner 组合起来，因此纯 circuit 测试无法暴露 closed-but-blocked
   对 `all_open` 的污染。

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | unbound planner 用未被可信账户状态阻断的候选判定恢复窗口，同时保留原 route 前缀给共同 gate 审计 | DONE |
| P0 | Test Coverage | 增加 planner 矩阵和三 Provider Gateway E2E，证明同一请求内两个余额跳过后试探最终 open Provider | DONE |
| P1 | Documentation | 更新 failover 与 account-usage 可执行契约，明确“分类可过滤、候选不可删除” | DONE |
| P1 | Code Review | 在 cross-layer thinking guide 及其模板加入 effective-unbound durable-gate 检查项 | DONE |

## 4. Systematic Expansion

- **Similar Issues**: 任何在共同 pre-send gate 之前生成 dispatch intent 的
  planner 都可能把“状态看似可用”误当成“实际可发送”；后续新增持久
  eligibility gate 时必须同时审计 stable-bound 和 effective-unbound
  分支。
- **Design Improvement**: 保持 `blocked_provider_ids` 为 typed planning
  hint，不创建第二份 Provider 列表；恢复分类基于可发送候选，执行和
  审计仍基于原始 route prefix。
- **Process Improvement**: 每个 durable gate 的回归矩阵必须包含
  bound suppression、unbound first skip、unbound final probe、all-blocked
  Stay、cooldown/in-flight 和真实 route E2E。

## 5. Knowledge Capture

- [x] 更新 Gateway failover route 契约。
- [x] 更新 Provider account-usage route 契约。
- [x] 更新 cross-layer thinking guide。
- [x] 同步 `src/templates/markdown/spec` 中的 thinking guide 模板。
- [x] 增加纯 planner 与 Gateway 路由级回归。
- [x] 后续“零余额手动刷新仍需先测试”问题已作为独立 P1 任务记录，避免混入本修复。
