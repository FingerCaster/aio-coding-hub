# 多任务串行修复与集成验收

## Goal

在已完成的前置任务 `07-15-external-codex-retry-gateway` 之后，严格按
`1 -> 2 -> 3 -> 4 -> 5 -> 父任务集成验收` 修复并验收四项用户问题，最后同步
`upstream/main`。每个子任务必须独立实现、检查、提交并归档，前一项未通过时不得
启动后一项。

## Background

- 当前分支为 `FingerCaster/sequential-task-acceptance`，基线为本地
  `main@2e43ee23572e69e34ce2c4cfb60481b58acf9298`。
- 父任务和五个子任务当前均为 `planning`；本轮没有运行 `task.py start`，也没有修改
  业务代码。
- 用户已确认前置父任务完成，并授权规划校验通过后由主会话直接进入实现，无需再次
  请求规划确认。当前 max 调研终端只完成规划，主会话随后切换 `gpt-5.6-sol`
  medium 实现终端。
- 用户现场报告与已验证事实、仍需实现阶段动态验证的边界，统一记录在
  `research/integration-evidence-summary.md` 及各子任务 `research/` 中。
- 整个任务禁止并发子代理。实现、检查、提交、归档和下一任务启动均由主会话串行协调。

## Requirements

### R1. 固定任务顺序与启动门槛

顺序固定为：

1. `07-17-fix-multi-provider-failover-503`
2. `07-17-fix-account-balance-manual-refresh`
3. `07-17-fix-newapi-account-usage-response`
4. `07-17-fix-config-export-large-skill-asset`
5. `07-17-sync-upstream-main-after-fixes`
6. 父任务全范围集成验收

每个子任务只有在前一任务的验收标准、质量检查、提交与归档全部完成后才可执行
`task.py start`。父子关系不替代此依赖门槛。

### R2. 多供应商失败链路

- 保留用户观察：三个已启用供应商场景中，UI 显示两次切换后快速返回 503；相邻请求
  包含两次约 6 秒的 502、一次 Plus 200 和会话复用。
- 以已验证根因为修复对象：会话绑定供应商在进入统一 gate 前被静默移出候选，导致
  attempt/route 漏记；前端又把 attempt 行数误称为供应商切换次数。
- 不把现场 503 误判为 `failover_max_providers_to_try=2`；本机设置与运行时代码均证明
  上限为 5，现场三家在该时刻分别处于 cooldown/open，而不是第三家仍可请求。
- 若确有第三个可请求候选，必须继续尝试；若候选被 gate 拒绝，必须在链路中以 skipped
  形式可诊断地出现。

### R3. 手动账户刷新

- 手动刷新必须通过同一 TanStack Query 生命周期发起或先取消同键旧查询，确保旧的首次/
  定时响应不能在手动结果之后覆盖缓存。
- 首次自动查询、定时查询和连续手动点击必须按同一 provider query key 串行化或采用明确
  的最后请求获胜语义。
- 账户展示保持 display-only；不得重置或影响路由、启停、排序、熔断、cooldown、健康或
  供应商可用性测试。

### R4. NewAPI `muyuan` 兼容

- 保真记录现象：`账户: 查询失败 · NewAPI 响应缺少 quota 字段`。
- 不再把 `/api/user/self` 的 `success=false` 认证错误误报为 schema 缺字段。
- 使用真实验证已证明可用的模型令牌 billing 契约，正确处理 subscription/usage 字段与
  USD 语义；未知或失败响应必须 fail closed，不能显示伪余额。
- 后续动态验证仅可对用户授权的 `muyuan` 发起最小只读请求；不得输出或落盘 API Key、
  完整响应、PII 或实际余额数值。

### R5. 配置导出大 Skill 资源

- 保留 Skill 文件数量、单 Skill 总字节数、相对路径、符号链接、特殊文件、导入包和
  Base64 解码边界。
- 将单文件预算与现有 8 MiB 单 Skill 总预算对齐，使大于 1 MiB 且不超过总预算的必要
  二进制资源完整参与导出/导入；不得静默跳过 PNG 或生成不完整 bundle。
- 超过 8 MiB、总量超限或其他安全边界仍须明确失败，并在写入目标前完成导入验证。

### R6. 最后同步 upstream

- 子任务 5 开始前禁止访问、fetch、检查或 merge 项目 git remote `upstream`。
- 只有子任务 1-4 均已验收并归档后，才允许读取并获取 `upstream/main`。
- 常规仓库操作继续使用 `origin` / `FingerCaster/aio-coding-hub`；`upstream` 保持
  fetch-only，绝不恢复 push URL 或向其推送。
- 携带所有不冲突变更。若冲突涉及 fork 产品行为或功能，必须暂停并向用户提供具体
  文件、两侧行为证据、影响与可选方案，未经决定不得选择任一侧。

### R7. 网关与跨任务边界

- 不恢复已由前置任务删除的本地 reasoning guard、continuation-repair 产品面或外部网关
  管理能力。
- 多供应商修复只调整候选可观察性、统一 gate 与准确展示；不得改变 generic retry、
  OAuth/`previous_response_id` 内部重试预算、模型发现严格预算、usage、response-id、
  TTFB、流式/非流式透传、取消语义或 20 MiB 非 SSE 聚合上限。
- NewAPI 账户用量始终是展示功能，不能成为任何网关决策输入。

## Acceptance Criteria

- [ ] 子任务严格按 1、2、3、4、5 顺序启动，且每一项启动前都有上一项已归档证据。
- [ ] 子任务 1 的三供应商回归能区分实际请求、gate skip、同供应商 retry 和供应商切换，
      并保留现有网关契约。
- [ ] 子任务 2 的乱序并发回归证明旧自动响应不能覆盖更新的手动刷新结果，且可用性测试
      与账户查询无依赖。
- [ ] 子任务 3 使用脱敏真实形状验证 `muyuan`，显示正确 USD 余额/用量或安全、准确地
      fail closed。
- [ ] 子任务 4 对 `>1 MiB && <=8 MiB` 资源完成导出/导入 round trip，并保持全部安全
      负例有效。
- [ ] 子任务 5 仅在前四项归档后执行，记录不可变 upstream SHA，带入全部不冲突变更，
      且没有覆盖 fork 特有行为。
- [ ] 父任务最终执行受影响测试、完整 Rust/前端质量门槛和集成行为检查，结果全部通过。
- [ ] 全过程没有并发子代理、未泄露密钥/PII、未向任何 remote 推送。

## Out of Scope

- 修改 Trellis CLI 或实现通用 DAG 调度器。
- 在本 planning 终端修改业务代码、提交、启动 dev server 或运行 `task.py start`。
- 恢复本地 Codex reasoning guard、continuation repair 或受管外部 retry gateway。
- 让账户余额参与路由、熔断、健康、排序或供应商可用性判定。
- 静默丢弃 Skill 资源以换取导出成功。
- 在子任务 5 获准启动前进行任何 upstream 操作。
