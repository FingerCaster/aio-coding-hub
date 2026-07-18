# 多任务串行修复与集成验收

## Goal

在已完成的前置任务 `07-15-external-codex-retry-gateway` 之后，严格按
`1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11 -> 12 -> 13 -> Sol max 最终审核`
修复、同步并关闭八轮终审发现。
每个子任务必须独立实现、检查、提交并归档，前一项未通过时不得
启动后一项。

## Background

- 当前分支为 `FingerCaster/sequential-task-acceptance`，基线为本地
  `main@2e43ee23572e69e34ce2c4cfb60481b58acf9298`。
- 子任务 1-10 已依次实现、提交并归档；第五轮已归档于
  `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5`。子任务 11 已在
  `b430874d` 关闭历史 F1-F8 与 F9-F23 follow-up，并在 `a2abe128` 归档至
  `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-6`。F24 Trellis
  template-hash 观察项按用户决定不属于本任务。
- 子任务 11 的完整门禁、归档和独立 journal 记录已完成；冻结提交
  `2a3788fc62db982737b9873757c354f89e198ce6` 的历史 Sol/Claude 只读报告均已收齐。父任务保持
  `in_progress`，直到报告去重、证据复现、必要的事实纠正和最终冻结审核全部通过；最终 Sol 审核
  通过前不得归档。
- 子任务 12（Round 7）已在 `8bbc619a` 完成实现，并在 `29133ac0` 归档；其后新的 Sol 终审
  发现一项历史用户决定断言缺乏记录证据的 P2。子任务 13（Round 8）已完成该事实修正并归档于
  `356fe32`。该 SHA 仅是 child archive；后续父任务事实投影提交 `2a89a4f` 及其后的投影也属于
  最终审核状态，`356fe32` 本身不是最终审核锚点。
- 协调会话已收齐新的独立 Codex `gpt-5.6-sol / effort=max` 只读终审：冻结父任务 tip
  `6de6ab8b20d2fa9dde00585e8a04b71405b85b9e`，审核范围 `29133ac0..6de6ab8`，结论为
  “无 P0-P2 findings”。终审静态确认 Round 8 archive/manifests、parent fact-projection、最终 tip
  规则与任务范围；未重跑构建/测试是唯一残余风险。本次收口仍不合并 `main`，父任务在 archive
  前继续保持 `in_progress`。
- 用户已确认前置父任务完成，并授权规划校验通过后直接进入实现，无需再次请求规划确认。
  已发生的 Round 6 实现记录保留其真实 `gpt-5.6-luna / effort=max` 模型，已完成的 child 11
  执行记录保留其真实 `gpt-5.6-terra / effort=max` 模型；从 Round 7 起，剩余实现和检查改由
  单个 Orca 管理的 Codex `gpt-5.6-sol / effort=max` 终端串行承担，禁止并发执行终端。
  该执行提示必须只执行已批准的 PRD/design/implement checklist：不得进行无关探索、额外
  finding hunting、架构发散、重构或范围扩张；遇到必须扩大范围的证据时停下交给协调会话裁决。
  该执行终端与只读终审会话隔离。历史冻结审核曾使用 Codex Sol 与 Claude Opus；按用户
   2026-07-18 的最新决定，后续任何冻结审核只使用新开的 Codex
   `gpt-5.6-sol / effort=max` 会话。Claude、Pi 及其他 agent 不再作为审核员，也不再启动。
- 产生当前 F9-F15 findings 的已发生 Round 6 独立只读终审是新开独立 Codex
  `gpt-5.6-sol / effort=max` 会话；该历史事实不与当前实现会话混用。
- 用户现场报告与已验证事实、仍需实现阶段动态验证的边界，统一记录在
  `research/integration-evidence-summary.md` 及各子任务 `research/` 中。
- 除明确的只读 Sol 审核 gate 外，整个任务禁止并发子代理。主会话只协调一个 Orca 执行终端；实现、
  检查、提交、归档和下一任务启动严格串行。后续审核的唯一有效 reviewer 为 Codex
  `gpt-5.6-sol / effort=max`；其只读会话不得改动 tracked 文件、任务状态、分支或 remote。
  不启动 Claude、Pi 或其他 agent 审核，也不以历史 Claude 报告替代新的 Sol 审核。

## Requirements

### R1. 固定任务顺序与启动门槛

顺序固定为：

1. `07-17-fix-multi-provider-failover-503`
2. `07-17-fix-account-balance-manual-refresh`
3. `07-17-fix-newapi-account-usage-response`
4. `07-17-fix-config-export-large-skill-asset`
5. `07-17-sync-upstream-main-after-fixes`
6. `07-17-final-review-security-boundaries`
7. `07-17-final-review-findings-round-2`
8. `07-17-final-review-findings-round-3`
9. `07-17-final-review-findings-round-4`
10. `07-17-final-review-findings-round-5`
11. `07-17-final-review-findings-round-6`
12. `07-18-final-review-findings-round-7`
13. `07-18-final-review-findings-round-8`
14. 父任务最终单一只读 Sol 审核（Codex `gpt-5.6-sol / effort=max`）

每个子任务只有在前一任务的验收标准、质量检查、提交与归档全部完成后才可执行
`task.py start`。父子关系不替代此依赖门槛。

### Current task map (2026-07-18)

| Order | Child | Status |
| --- | --- | --- |
| 1 | `07-17-fix-multi-provider-failover-503` | archived |
| 2 | `07-17-fix-account-balance-manual-refresh` | archived |
| 3 | `07-17-fix-newapi-account-usage-response` | archived; post-fix live evidence linked |
| 4 | `07-17-fix-config-export-large-skill-asset` | archived |
| 5 | `07-17-sync-upstream-main-after-fixes` | archived; conflict audit linked |
| 6 | `07-17-final-review-security-boundaries` | archived |
| 7 | `07-17-final-review-findings-round-2` | archived; F1-F8 and evidence closure complete |
| 8 | `07-17-final-review-findings-round-3` | archived; user selected common-gate option A |
| 9 | `07-17-final-review-findings-round-4` | archived; nine findings and full gates complete |
| 10 | `07-17-final-review-findings-round-5` | archived; six findings and full gates complete |
| 11 | `07-17-final-review-findings-round-6` | archived at `a2abe128`; F1-F23 and full gates closed, F24 excluded by user decision |
| 12 | `07-18-final-review-findings-round-7` | archived at `29133ac0`; its fresh Sol review found the unsupported historical-decision P2 |
| 13 | `07-18-final-review-findings-round-8` | archived at `356fe32`; child archive is not the final-review anchor, new independent Sol review pending |

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
- upstream 同步任务只拥有固定输入、真实 merge、最小冲突修复与合并验证。若缺陷在固定的
  upstream revision 上脱离本次 merge 也成立，即使由终审或回归发现，也只记录为范围外问题，
  通过独立后续任务处理，不得混入同步任务或 merge commit。
- 本父任务中已经完成的 Image Gen 修复按用户决定保留，属于一次性例外，不构成后续
  upstream 合并任务扩大范围的先例。

### R7. 网关与跨任务边界

- 不恢复已由前置任务删除的本地 reasoning guard、continuation-repair 产品面或外部网关
  管理能力。
- 多供应商修复只调整候选可观察性、统一 gate 与准确展示；不得改变 generic retry、
  OAuth/`previous_response_id` 内部重试预算、模型发现严格预算、usage、response-id、
  TTFB、流式/非流式透传、取消语义或 20 MiB 非 SSE 聚合上限。
- NewAPI 账户用量始终是展示功能，不能成为任何网关决策输入。

## Acceptance Criteria

- [x] 子任务严格按 1 至 9 顺序启动，且每一项启动前都有上一项已归档证据。
- [x] 子任务 1 的三供应商回归能区分实际请求、gate skip、同供应商 retry 和供应商切换，
      并保留现有网关契约。
- [x] 子任务 2 的乱序并发回归证明旧自动响应不能覆盖更新的手动刷新结果，且可用性测试
      与账户查询无依赖。
- [x] 子任务 3 使用脱敏真实形状验证 `muyuan`，显示正确 USD 余额/用量或安全、准确地
      fail closed。
- [x] 子任务 4 对 `>1 MiB && <=8 MiB` 资源完成导出/导入 round trip，并保持全部安全
      负例有效。
- [x] 子任务 5 仅在前四项归档后执行，记录不可变 upstream SHA，带入全部不冲突变更，
      且没有覆盖 fork 特有行为。
- [x] 子任务 11 的受影响测试、完整 Rust/前端质量门槛、Docker/Linux watchdog、bindings 与
      `check:precommit:full` / `check:prepush` 已通过，证据记录在其归档工件中。
- [x] 父任务沿用各已归档 child 工件记录的受影响测试、完整 Rust/前端质量门槛和集成行为检查
      历史证据；本次最终只读终审未重跑构建/测试，且将其如实保留为唯一残余风险，不声明新的
      产品门禁执行结果。
- [x] 子任务 6、7 的安全回归、脱敏 live 证据、冲突决策表与稳定分页均完成，随后由新开独立
      Codex `gpt-5.6-sol / effort=max` 会话执行只读终审并给出最终结论。
- [x] 子任务 8 关闭第三轮 findings，并按用户决策 A 保留 provider selection common-gate skipped/
      continue/完整 503 语义；全部门禁通过后才重新进入新开独立 Codex
      `gpt-5.6-sol / effort=max` 只读终审。
- [x] 子任务 9 关闭第四轮九项 findings，完成 handle-bound filesystem authority、settings owner/CAS、
      pre-IPC budgets、安全日志与 archive 自动校验，并保持 Skill 根内内容逐字节导出。
- [x] 子任务 10 关闭第五轮六项 findings，完成 Skill 顶层可信根、settings 副作用/CAS、方案 A
      gate 顺序、OAuth capability 脱敏与 Grok continuation 生产回归，并归档于
      `.trellis/tasks/archive/2026-07/07-17-final-review-findings-round-5`。
- [x] 子任务 11 已关闭第六轮历史 F1-F8 与属于本任务的 follow-up F9-F23（settings/autostart ownership、
  config import rollback lifecycle、有界读取/encoded budget、Image Gen handle stats、journal/task 证据
  纠正），不处理 F24 Trellis template-hash；`b430874d` 后在 `a2abe128` 归档。
- [x] 冻结提交 `2a3788fc` 上已运行互不交换结果的 Codex Sol 与 Claude Opus 只读审核；两份报告
  已收齐，属于历史证据；其后不再启动 Claude 或其他 agent 审核。
- [x] 子任务 12 已关闭冻结提交 `35db0f32` 的有效 findings，在 `8bbc619a` 完成实现并于
      `29133ac0` 归档；其后新的 Sol 终审发现本轮待纠正的历史决定断言 P2。
- [x] 子任务 13 仅纠正该 P2 和 Round 7/8 任务事实投影，并在 `356fe32` 完成归档。
- [x] 协调会话按最终 parent-tip 规则冻结并审查
      `6de6ab8b20d2fa9dde00585e8a04b71405b85b9e`，范围为 `29133ac0..6de6ab8`；新的独立 Codex
      `gpt-5.6-sol / effort=max` 只读终审结论为“无 P0-P2 findings”。该审核未只使用 child archive
      `356fe32`，且未向 remote 推送或合并 `main`。

## Out of Scope

- 修改 Trellis CLI 或实现通用 DAG 调度器。
- 在本 planning 终端修改业务代码、提交、启动 dev server 或运行 `task.py start`。
- 恢复本地 Codex reasoning guard、continuation repair 或受管外部 retry gateway。
- 让账户余额参与路由、熔断、健康、排序或供应商可用性判定。
- 静默丢弃 Skill 资源以换取导出成功。
- 在子任务 5 获准启动前进行任何 upstream 操作。
- 在 upstream 同步任务中修复并非由实际合并冲突或冲突解决引入的上游自身缺陷。
- 修复、重算或验证 F24 Trellis template-hash / safe-commit 机制，或覆盖相关现有 dirty 文件。
