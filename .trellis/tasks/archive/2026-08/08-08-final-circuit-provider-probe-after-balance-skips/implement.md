# 余额跳过后终局熔断 Provider 试探实施计划

## 1. 前置确认

- [x] 读取 failover route 与 Provider account-usage 契约，确认账户 gate 仍早于 circuit/probe gate。
- [x] 记录当前 HEAD、工作树和模型路由父任务归档状态。
- [x] 用 planner 输入复现 P1/P2 blocked closed + P3 open 的错误决策。

## 2. Planner 修复

- [x] 在 unbound 分支中仅用未被账户状态阻断的候选判断 `all_open` 和首个实际恢复对象。
- [x] 保留直到选中恢复对象为止的原始 route 前缀，使余额 gate attempt 顺序和多 open fallback 保持可观测。
- [x] 全部候选均余额阻断时返回 `Stay`，不创建空 dispatch 或 probe reservation。
- [x] 不修改 stable-session、forced/managed、gate、lease、circuit 或 transport 所有权。

## 3. 回归测试

- [x] 增加 planner 单测覆盖用户三 Provider 场景和 all-blocked 场景。
- [x] 增加 gateway E2E，断言两个余额阻断 attempt、最后 Provider probe 成功、一次上游调用及 circuit 恢复。
- [x] 运行 probe planner、账户 gate、all-open route focused tests。
- [x] 运行 Rust fmt/check/clippy/full tests 与 `git diff --check`。

## 4. 收尾

- [x] 执行 `trellis-check`，将通用根因写入 failover/account-usage 规格。
- [x] 提交产品修复，归档任务；随后启动零余额手动刷新回归任务。

## 验收门槛

- [x] 当前客户端请求内完成试探，不创建下一次客户端请求。
- [x] P1/P2 的 account-usage 状态、circuit、Session 和 transport 均无副作用。
- [x] P3 的 probe lease 只获取一次，成功/失败/cooldown/in-flight 仍由既有机制收敛。
- [x] Provider 顺序、Ready-provider 上限、attempt/route-hop 语义无回归。

## 完成记录

- 根因是 effective-unbound planner 用 account-blocked `CLOSED` 候选污染
  `all_open` 判定；修复后只用未阻断候选分类恢复窗口，但保留原 route
  前缀给共同 account gate 产生有序审计证据。
- 新增 2 个 planner 回归和 1 个真实 Gateway E2E；用户场景在同一请求内
  产生 P1/P2 余额跳过、P3 `new_unbound_session` probe 成功，调用数为
  0/0/1，P3 circuit 回到 `CLOSED`。
- focused 通过：planner 26/26、all-open 7/7、account gate 1/1、新 E2E
  1/1。
- 全量通过：Rust 库 2619 passed/4 ignored 及全部集成测试、fmt、check、
  Clippy；前端 304 files/2749 tests、typecheck、lint；`git diff --check`
  无空白错误。
- 已执行 break-loop 分析，并同步 failover、account-usage、
  cross-layer thinking guide 与 Markdown 模板。
- 产品修复提交：`8587e5c1`。
