# 修复多供应商 503 链路漏记与切换计数

## Goal

让每个启用且属于当前路由候选集的供应商都经过统一 gate，并在请求链路中准确记录实际
请求或 skipped 原因；同时把供应商数量、切换次数和 attempt 次数分开显示，避免把正确的
快速 503 误报为“只尝试了两家”。

## Background

### User observation

- 三个供应商均开启，但 Codex / `gpt-5.6-sol-max` 多次显示 503“全部不可用”、无可用
  供应商、“切换 2 次”、约 13 ms。
- 相邻 `AI INPUT-Air` 记录为 502“上游4XX”、“切换 3 次”、会话复用、约 5.89/6.16 s；
  中间 `AI INPUT-Plus` 200 成功。

### Verified fact

- 本地日志精确复现了该时间窗。503 时 `AI INPUT-Air` 为 cooldown、`muyuan` 为 OPEN；
  会话绑定的 `AI INPUT-Plus` 在进入 failover loop 前被静默移除，所以只有两条 skipped
  attempt。该 503 当时没有仍被允许请求的第三家。
- `failover_max_providers_to_try` 为 5。loop 只在 provider 完成 gate/preparation 后增加
  `providers_tried`，skipped 不消耗实际供应商预算，因此不是“上限 2”或 off-by-one。
- 前端 `buildRequestRouteMeta` 使用 `attempt_count` 生成“切换 N 次”；该值是
  `attempts_json.len()`，会混合 skipped、retry 和不同 provider。
- 详细证据与 file:line 在 `research/failover-503-root-cause.md`。

## Requirements

### R1. One common provider gate

- 会话绑定只负责排序/复用偏好，不得在统一 provider gate 前静默删除仍属于候选集的
  provider。
- circuit open、cooldown、provider limit 等拒绝由 failover preparation 的 common gate
  判定并记录稳定 `outcome=skipped`、error/reason code 和 circuit snapshot。
- gate 拒绝不得发起上游请求，也不得消耗 `max_providers_to_try` 的 Ready-provider 预算。

### R2. Accurate termination

- 当三家都被 gate 拒绝时返回 `GW_ALL_PROVIDERS_UNAVAILABLE`，链路包含三家及各自原因。
- 当第三家仍可请求且未达到明确 Ready-provider 上限时，必须尝试第三家。
- 达到用户配置上限与候选全部被拒绝/失败必须可从 attempts/route/终态区分。

### R3. Accurate presentation

- UI 分别以 route hop 数计算涉及的供应商数，以 `hops - 1` 计算真正的 provider transition，
  以 `attempt_count` 表示包含 retry/skip 的尝试记录数。
- 紧凑标签不得再把 raw attempt 数直接命名为“切换次数”；tooltip 必须与后端 route 一致。

### R4. Preserve gateway contracts

- 不改变每供应商 retry budget、OAuth/`previous_response_id` recovery reservation、provider
  override、模型发现 strict limit、熔断/cooldown 状态机或 health-neutral 请求。
- 不触碰 usage、response-id、TTFB、流式/非流式透传、取消、20 MiB 非 SSE 聚合上限。
- 不恢复已删除的 reasoning guard、continuation repair 或 managed external gateway。

### R5. Dependency

- 本任务是父任务第 1 项；只有本任务验收、提交并归档后才可启动第 2 项。
- 本任务不得访问项目 remote `upstream`。

## Acceptance Criteria

- [ ] 三候选全被 gate 拒绝时，终态为 503，route/attempts 包含三条 skipped 记录且没有
      上游调用。
- [ ] 会话绑定 provider 被拒绝时仍出现在链路中，且不会绕过 circuit/cooldown。
- [ ] 前两家失败/skip、第三家可用时会处理第三家；明确 `max_providers_to_try=2` 时仍按
      Ready-provider 预算停止。
- [ ] UI 对三家、两次 transition、包含 retry/skip 的 N 条 attempt 给出不混淆的标签与
      tooltip。
- [ ] circuit failure counts、cooldown、session binding、provider order 与成功路径不回归。
- [ ] gateway attempt-budget、model discovery、usage、TTFB、response-id、流式和 20 MiB
      相关现有测试保持通过。

## Out of Scope

- 强行请求处于 OPEN/cooldown 的供应商。
- 改变用户配置的 provider/attempt 上限。
- 修改 NewAPI 账户查询、余额刷新或配置导出。
- 恢复任何已删除的 Codex reasoning-guard/continuation 功能。
