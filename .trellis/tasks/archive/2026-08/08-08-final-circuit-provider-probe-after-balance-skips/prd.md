# 修复余额跳过后终局熔断 Provider 未触发自然试探

## Goal

当同一请求的前序 Provider 均因账户余额不足跳过、最后一个候选因熔断跳过并导致无可用 Provider 时，终局候选应按自然回切契约获得试探机会；不得直接结束为无可用供应商。当前可配置模型路由及联合验收完成后再开始实现。

## Requirements

- 复现至少三个 Provider 的候选序列：前两个在共同账户用量 gate 因余额不足跳过，最后一个处于熔断状态。
- 当按现有自然回切策略已没有其他可发送候选时，最后一个熔断 Provider 必须获得一次受现有 probe lease、并发和超时约束保护的试探机会。
- 余额不足的 Provider 仍保持跳过，不得为了制造候选而绕过账户用量 gate，也不得污染其 blocked/recovery 状态。
- 试探成功、失败、已有 in-flight probe、强制路由和不满足
  `request_eligible` 等分支必须继续服从现有 Provider failback、熔断和
  Session 所有权契约。
- 修复必须落在共同候选规划/回切所有权中，不能只针对“三个 Provider”或特定 CLI 写死特殊分支。
- 在当前可配置模型路由任务及父任务联合验收完成后再启动本任务，开发前重新读取届时最新的 account-usage 与 failback 实现。

## Acceptance Criteria

- [x] 自动化测试证明“余额不足、余额不足、终局熔断”的序列会触发一次合规试探，而不是直接返回无可用供应商。
- [x] 试探成功时当前客户端请求继续完成，不产生额外客户端请求；试探失败时保留正确终局错误与 attempt/route-hop 证据。
- [x] 不绕过余额 gate，不重复消费 probe reservation，不引入额外 transport retry，不改变 Provider 顺序上限或熔断计数语义。
- [x] 覆盖已有 in-flight probe、冷却、强制/绑定路由、请求不适用及多个熔断候选，证明只选择契约允许的终局试探对象。
- [x] Provider/failover focused tests、联合网关 E2E、Rust 全量质量门槛和相关前端契约测试通过。

## Notes

- 用户报告的原始场景：一次请求触发三个供应商，两个因余额不足跳过，最后一个因熔断跳过，最终没有可用供应商；最后一个本应触发试探。
