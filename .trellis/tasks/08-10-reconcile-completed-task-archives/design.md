# 技术设计：已完成任务记录收口

## 边界与权威来源

本任务只变更 `.trellis/tasks/**`、task archive contract 和最终 journal。对于已有同名归档的任务，归档目录的 `task.json.status/completedAt` 与 JSONL 自引用路径为权威；active 副本只作为可能包含后续独有资料的恢复来源。

## 分类与处理

| 类别 | 任务 | 处理 |
| --- | --- | --- |
| 已归档、active 基本过期 | `07-19`、`08-08-custom-account-usage-routing`、`08-08-custom-account-usage-script` | 证明无独有语义后移除 active 副本 |
| 已归档、记录分歧 | `08-03-circuit-breaker-probe-failback` | 逐文件审阅，保留归档中的后续设计并补入 active 独有且未被取代的内容 |
| 已归档、active 有后续故障分析 | `08-08-account-usage-route-gate` | 把稳态阻断研究与相关结论合入归档，再移除 active 副本 |
| 已完成、当前分支未归档 | `07-16`、08-05 父任务与两个子任务 | 补充落地证据，子任务先归档、父任务后归档 |
| onboarding 已完成 | `00-join-fingercaster` | 归档并保留原始 onboarding 文档 |

## 数据流

1. 保存 Git、任务列表、用户脏文件、分支和 stash 基线。
2. 对同名 active/archive 运行逐文件 `--no-index` 差异审阅。
3. 通过小范围补丁把独有内容写入既有归档；不整目录覆盖。
4. 解析并验证 5 个 active 副本的绝对路径后逐项删除。
5. 更新 4 个未归档业务任务的证据元数据；用 `task.py archive --no-commit` 依次归档子任务、父任务、07-16 和 onboarding。
6. 校验所有 context manifest、任务唯一性、Git 差异和用户基线。

## 兼容与回滚

- 删除 active 副本之前，所有保留内容必须已存在于归档路径且出现在 `git diff` 中，因此可由工作树 diff 恢复。
- `task.py archive --no-commit` 只移动精确任务路径并改写自引用；若任一步失败，停止后续删除/移动并根据 `git status` 修复。
- 不 cherry-pick 分支上的 archive commit，避免带入与当前 task tree 不一致的父子关系；仅使用其内容作为完成证据。
- 最终提交前不推送；用户文件始终不暂存。

## 取舍

本次不为 Trellis 新增自动重复检测。脚本改造需要独立测试和跨项目兼容审查；当前最小机制是把发现写入 archive contract，并完成一次可审计的数据收口。
