# origin required checks 兼容性

## 查询范围与结果

- 远端语义：仅 `origin`，其 fetch/push URL 均为 `https://github.com/FingerCaster/aio-coding-hub.git`。
- 仓库：`FingerCaster/aio-coding-hub`
- 分支：`main`
- classic branch protection 只读查询：GitHub API 返回 `404 Branch not protected`。
- repository rulesets 只读查询：GitHub API 返回 `[]`。
- 未修改 branch protection、ruleset、required status checks 或任何其他 GitHub 设置。
- `upstream` 未用于 GitHub 查询或操作；本地配置仍是 fetch URL `https://github.com/dyndynjyxa/aio-coding-hub.git`、push URL `DISABLED`。

## required checks 结论

在上述查询可见范围内，`origin/main` 当前没有 classic branch protection，也没有 repository ruleset，因此没有现存 required check context 需要改名、保留或迁移。增加新的 `ci-gate` 不会与当前 required checks 配置冲突，也不会因旧 required context 在 docs-only 运行中 skipped 而让合并永久等待。

这不构成修改 GitHub 设置的授权。实现只应在 workflow 内提供固定且稳定的 job id/name `ci-gate`；若以后启用分支保护，可由仓库管理员另行把 `ci-gate` 设为 required check。固定 gate 将所有条件 job 的 success/skipped 结果汇总为单一、始终出现的检查，比直接要求可能被条件跳过的 `frontend`、`rust` 或 desktop matrix context 更适合未来的分支保护。

## workflow 兼容性要求

- `ci-gate` 必须使用 `if: always()`，否则某个依赖失败或 skipped 时 gate 自身可能被隐式跳过，失去稳定 check context。
- gate 必须显式要求 `change-scope` 成功，并验证 `scope`、`full_ci`、`docs_checks` 输出属于允许域且彼此一致。
- PR 上要求 `pr-title == success`，push 上要求它确实 `skipped`。
- `checked-docs` 要求文档合同成功，full jobs skipped；`process-docs` 要求文档合同和所有 full jobs都 skipped。
- `full` 要求 `support-contract`、`desktop-support-contract` 聚合矩阵、`frontend`、`rust` 全部成功；若 mixed 变更选择了文档合同，也要要求其成功。任何未被选择的 job 必须明确为 `skipped`。
- 三平台 desktop matrix 不应分别成为未来 required contexts；gate 应消费 `needs.desktop-support-contract.result` 的聚合结果并把它纳入 full 合同。

当前没有远端 required checks 作为回归 oracle，因此兼容性验证重点应放在静态 job/needs/if 合同、自测场景与 Actions 实际运行结果。远端配置仍保持只读。
