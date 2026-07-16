# upstream/main 最终同步策略研究

## 证据状态

- 当前基线为本地 `main@2e43ee23572e69e34ce2c4cfb60481b58acf9298`，工作分支为
  `FingerCaster/sequential-task-acceptance`。
- 项目 `AGENTS.md` 明确要求常规仓库操作默认使用 `origin`，`gh` 显式指向
  `FingerCaster/aio-coding-hub`，并保持 `upstream` fetch-only。
- 用户明确规定子任务 5 固定最后执行；只有前四项完成后才可读取/fetch/merge
  `upstream/main`。
- 用户明确要求携带不冲突变更；若冲突涉及 fork 产品行为，必须暂停并提供具体证据与
  选项。
- 本 planning 阶段没有访问、fetch、检查或 merge `upstream`。因此当前没有、也不应
  声称存在某个 upstream SHA、drift 数量或冲突文件。

## 可证伪的进入条件

允许第一次访问 `upstream` 前，必须同时成立：

1. 子任务 1–4 的 Trellis 状态均为 archived/completed，而非仅有任务目录。
2. 每个子任务的验收和检查证据完整。
3. 四个子任务按 `1 -> 2 -> 3 -> 4` 分别提交，当前 HEAD 包含这些提交。
4. 工作树干净，没有跨任务或未归属改动。
5. 子任务 5 已按主会话串行流程启动。

任一条件不成立即可证伪“现在可以访问 upstream”，流程必须停留在门外。

## Remote 与权限结论

| 对象 | 固定策略 |
| --- | --- |
| `origin` | 常规 git/GitHub 目标 |
| `FingerCaster/aio-coding-hub` | `gh -R`/`--repo` 显式仓库 |
| `upstream` fetch | 仅子任务 5 进入门槛后允许 |
| `upstream` push | 始终禁止，不恢复或启用 push URL |
| 本任务向 origin push | 未自动授权；需要其他明确授权 |

这一区分防止双 remote clone 中的隐式仓库解析把检查或变更发送到错误仓库。

## 同步输入固定方法

进入门槛通过后：

1. 记录 fork 的 `pre_merge_head`。
2. fetch `upstream/main`，将远端跟踪引用 peel 为 commit 并记录 `upstream_sha`。
3. 计算并记录 `merge_base`。
4. 以这三个 SHA 审阅双方变更和执行 merge。
5. 验收结束时检查 `upstream_sha` 是结果 HEAD 的祖先。

固定 SHA 让“分析了什么、测试了什么、最终带入什么”可审计。若远端分支随后移动，不在
同一次验收中静默改变输入。

## 语义冲突判定

文本冲突只是信号之一。以下情况即使 git 自动合并也属于必须暂停的产品语义冲突：

- upstream 恢复 fork 已删除的 reasoning guard、continuation-repair 或受管外部网关；
- upstream 改变子任务 1 的统一 gate、路由可观察性或 generic gateway 契约；
- upstream 改变子任务 2 的 Query 所有权，或让账户展示影响路由/健康；
- upstream 改变子任务 3 的 NewAPI 认证、单位或 fail-closed 语义；
- upstream 改变子任务 4 的 8 MiB/64 MiB 安全策略或开始静默丢弃资源；
- upstream 与 fork 对同一用户流程给出不同、均合理但无法由现有规格唯一裁定的行为。

暂停报告至少包含：

| 证据 | 内容 |
| --- | --- |
| 文件/提交 | 冲突位置和双方来源 SHA |
| fork 行为 | 当前用户可见行为及保留原因 |
| upstream 行为 | 新行为及其目的 |
| 影响 | 用户流程、兼容、安全和测试 |
| 选项 | 保留 fork、适配 upstream 或经论证的组合方案及代价 |

用户决定前不得提交任何一侧的产品选择。

## 合并后回归矩阵

| 范围 | 必须证明 |
| --- | --- |
| 历史 | 四项修复提交保留，upstream SHA 成为祖先 |
| 子任务 1 | 三供应商 gate/route、预算、usage、response-id、TTFB、取消、20 MiB 不回归 |
| 子任务 2 | 自动/手动乱序最终值正确，账户展示无路由副作用 |
| 子任务 3 | NewAPI fixtures、USD 单位、准确错误和隐私边界不回归 |
| 子任务 4 | 1–8 MiB round trip 与全部安全负例不回归 |
| upstream drift | 每个受影响包的聚焦测试通过 |
| 仓库总门槛 | build、完整 precommit/prepush、Rust suite 通过 |
| remote | origin 默认，upstream fetch-only，无 upstream push |

## 风险与回滚点

- **输入漂移：** 以固定 SHA 隔离；不在验收中自动追逐新 upstream 提交。
- **干净合并的语义回归：** 通过双边 diff、fork 产品面清单和四项回归捕获。
- **大范围 merge 回滚：** 未提交时 abort；已提交后整体 revert merge commit。
- **历史损坏：** 禁止 rebase、reset 或压平已归档子任务提交。
- **未决冲突：** 暂停并等待用户是正确终态，不以赶进度为由自行选边。

当前没有需要用户预先决定的事项；只有实现阶段出现具体产品语义冲突时才触发决策门。
