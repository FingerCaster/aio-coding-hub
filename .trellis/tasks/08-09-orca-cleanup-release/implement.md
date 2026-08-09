# 执行计划：Orca 清理与稳定版发布

## 前置审阅门

- [x] 用户确认 PRD、技术设计和本执行清单；确认目标为稳定 `0.60.40`。
- [x] `implement.jsonl` 与 `check.jsonl` 已填入真实 spec/research 条目。
- [x] 通过 `python ./.trellis/scripts/task.py start .trellis/tasks/08-09-orca-cleanup-release` 后才进入执行。

## 有序步骤

### 1. 建立可恢复快照

- [x] 记录 Orca `status`, `worktree ps`、主 worktree terminal list 和当前会话句柄。
- [x] 记录 `git rev-parse main`, `git ls-remote origin refs/heads/main`、`git status --short`
      及用户脏文件清单；确认无 staged 用户文件。
- [x] `git fetch origin main --tags` 后重新验证远端没有漂移。

### 2. 清理 Orca 对象

- [x] 用版本匹配的 Orca help 确认 tab 级 terminal close 参数。
- [x] 处理 `term_085aa34f-5923-4875-aed5-a9f98e4489de` 与
      `term_73b5ef5b-3967-4ff3-9160-0ee1ca9d483c`，保留
      `term_c6068d07-36f4-479c-8e10-734645fb9bd7`；前者已关闭，后者视觉 tab
      已消失但 runtime stale handle 仍按 fail-closed 边界保留并记录。
- [x] 对 8 个已审计子 worktree 执行 Orca stop/rm，逐项复核 Orca 与 Git 注册表；失败项不强删。
- [x] 清理后确认主 worktree 用户脏状态集合未改变。

### 3. 发布前质量门

- [x] 运行 `pnpm check:prepush`（或记录其等价的完整检查集合）及发布合同自测：
      `node scripts/release-source.selftest.mjs`、`node scripts/release-promotion.selftest.mjs`、
      `node scripts/check-release-signing-secret-scope.selftest.mjs`、
      `node scripts/check-release-signing-secret-scope.mjs`、`pnpm check:ci-change-scope`。
- [x] 对 `origin/main..main` 的变更检查 Codex continuation/reasoning guard 特殊发布约束；
      若命中，追加仓库规定的成功续接、可见内容、超时和 usage 验证。
- [x] 运行 `git diff --check`，确认没有待处理失败。

### 4. 生成并合并 `0.60.40` release PR

- [x] 仅创建空的 `Release-As: 0.60.40` 覆盖提交；提交钩子环境从当前 shell 派生 `node`/`pnpm`。
- [x] 将 `main` 推送到 `origin`，记录推送后的 immutable SHA。
- [x] 无参数 dispatch `release.yml`，等待 release-please job 结束。
- [x] 验证唯一 release PR 的版本、文件、changelog 和 head/base；错误版本一律不合并。
- [x] 等待 PR 必需检查通过后以 merge commit 合并，并验证远端 `main`。

### 5. 构建与发布

- [x] 无参数再次 dispatch `release.yml`，记录 run id/url。
- [x] 按 job 图等待 `release-please → build → assemble-release-candidate → promote-release → publish`
      及 `publish-homebrew-cask` 终态。
- [x] 用 `gh release view`、tag SHA、Release asset API 和 `latest.json` 做最终一致性核验。

### 6. Trellis 收尾

- [x] 运行 `trellis-check` 完成全范围质量检查并修复本任务产生的问题。
- [x] 使用 `trellis-update-spec` 评估并记录本次发现的 release-as/不可变来源约束。
- [ ] 仅归档/提交本任务记录与必要 journal/spec；不提交用户或其他活跃任务文件。
- [ ] 复核 Orca comment、Git 状态和发布链接，运行 `task.py archive`。

## 验证与回滚点

| 阶段 | 验证 | 回滚点 |
| --- | --- | --- |
| 快照 | SHA、脏文件、Orca 清单可重现 | 不做任何删除/推送 |
| 清理 | Orca/Git 双清单一致 | 保留失败 worktree，停止该对象操作 |
| 版本 PR | 所有版本文件严格为 `0.60.40` | 不合并错误 PR，可关闭生成分支 |
| 发布 run | tag/source SHA/候选 manifest 一致 | 保持 draft，不覆盖资产 |
| 收尾 | Release 与资产矩阵完整 | 不改变已发布 Release，单独报告 |
