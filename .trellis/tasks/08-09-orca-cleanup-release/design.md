# 技术设计：Orca 清理与稳定版发布

## 边界与所有权

本任务只协调三类外部状态：本机 Orca worktree/terminal 注册、`origin` 的 `main` 与
release-please PR、以及 `origin` 上的 GitHub Release/Actions。业务源码不在本任务中修改。
发布版本的源代码来自本地 `main` 已有的 25 个提交；本任务新增的版本覆盖提交和 Trellis
记录提交不应被误纳入已确定的发布源。

## 当前状态模型

| 对象 | 当前状态 | 处理策略 |
| --- | --- | --- |
| 主 worktree `D:/UGit/aio-coding-hub-fork` | `main`，有用户改动 | 保留；只读取并保留脏状态 |
| `term_c6068d07-36f4-479c-8e10-734645fb9bd7` | 本次 Codex 发布会话 | 保留到任务结束 |
| `term_085aa34f-5923-4875-aed5-a9f98e4489de` | Beta 讨论已结束、无本任务依赖 | 关闭整个 tab |
| `term_73b5ef5b-3967-4ff3-9160-0ee1ca9d483c` | 早期已完成 Trellis hook 会话 | 关闭整个 tab |
| 8 个子 worktree | 均干净、Agent 完成、任务已归档 | 逐个 Orca stop + rm；Git 分支保留 |

子 worktree 的完整 Orca ID 都使用以下 repo 前缀，并复制完整路径，不缩短为 repo ID：
`4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/<name>`。
候选名称为 `cross-restart-reset`、`plugin-runtime`、`port-hardening-integration`、
`provider-routing`、`release-hardening`、`rust-audit`、`sessions-ui`、`settings-pricing`。

## Orca 清理流程

1. 记录 `orca status --json`、`worktree ps --json`、主 worktree 的 `terminal list`，并为
   每个候选保存 Git `status --porcelain=v1 --untracked-files=all`、HEAD、分支和整合证据。
2. 先关闭两个主 worktree 的旧 tab。执行前用版本匹配的 `terminal close --help` 确认
   tab 级关闭参数；关闭后重新 `terminal list`，不得触碰当前发布会话。
3. 对每个候选执行 `orca terminal stop --worktree <完整 id> --json`，重新列出该 worktree
   的终端并确认没有活动 Agent/PTY。若出现 `terminal_handle_stale` 或 stale PTY，只按
   Orca 指南重新列出、自然退出/再次 stop；不得使用 `git worktree remove` 或文件系统删除。
4. 执行 `orca worktree rm --worktree <完整 id> --force --json`。每次返回成功后同时检查
   `orca worktree ps --json` 与 `git worktree list --porcelain`；失败对象保留并记录原因。
5. 清理完成后再次记录主 worktree 的脏状态，逐项确认与步骤 1 的集合和内容一致。

## 发布流程

### 版本闸门

`origin/main` 已在 `aio-coding-hub-v0.60.39`。仓库历史存在旧的
`Release-As: 0.60.6`，导致 PR #30 生成错误降级版本。推送新代码前创建一个**空提交**，
只包含以下提交信息，不暂存任何用户文件：

```text
chore(release): prepare aio-coding-hub 0.60.40

Release-As: 0.60.40
```

该提交让 release-please 在下一次运行中以 `0.60.40` 为明确目标，同时保留正常的
Conventional Commits 变更日志生成。官方 release-please 文档明确支持这种空提交覆盖方式。

### 两次 workflow dispatch

1. 先确认远端仍为预期 SHA，运行根项目发布前自测和 CI 范围检查；用派生自当前 shell 的
   `node`/`pnpm` PATH 执行提交钩子。
2. 将 `main` 推送到 `origin`。显式触发 `release.yml`（不传 `release_tag`），让
   release-please 更新/重建 release PR。读取 PR 的标题、manifest、`package.json`、
   `Cargo.toml`、`tauri.conf.json` 和 changelog，必须全部显示 `0.60.40`；仍出现
   `0.60.6`、版本回退或变更范围异常时，不合并并进入回退点。
3. 等待该 PR 的必需 CI（尤其 `ci-gate`、前端、Rust、Windows 构建和 Cargo lock 同步）成功，
   用 `gh pr merge -R FingerCaster/aio-coding-hub --merge --delete-branch` 合并。合并后
   验证远端 `main` 的新提交和四个版本文件一致。
4. 再次无参数触发 `release.yml`。工作流会先创建/解析 `aio-coding-hub-v0.60.40` tag，
   将其解析为 immutable SHA，再构建候选、精确校验资产、晋升并发布 Release。
5. 监控该 run 到终态；核验 Release 非 draft、非 prerelease、tag 与 target SHA 一致，
   `latest.json` 版本/签名/平台矩阵正确，Homebrew job 成功或按现有无 token 逻辑明确跳过。

所有 GitHub CLI 调用显式使用 `-R FingerCaster/aio-coding-hub`，不操作 `upstream`。

## 失败与回滚

- Orca stop/rm 失败：保留对应 worktree/分支，不绕过 Orca；继续前先重新列出并记录状态。
- 远端 `main` 在 push 前发生漂移：停止，重新 fetch 并重新审计，不强制覆盖远端。
- 生成的 release PR 版本不是 `0.60.40`：不合并、不手工改生成分支；保留 run/PR 证据，
  可关闭错误生成 PR 后重新触发一次，第二次仍错误则停止并报告 release-please 状态。
- Release run 在候选/晋升前失败：不重试同一未知状态，先读取失败 job；保持 draft/候选，
  只在确认幂等条件后重试。
- Release 已发布但身份、SHA、资产矩阵或 `latest.json` 不一致：停止后续 Homebrew/手工
  上传，不覆盖现有资产，保留 GitHub 审计数据并报告恢复动作。

## 发布后状态

发布验证完成后更新主 worktree Orca comment。Trellis 归档/记录提交只提交本任务和必要的
journal/spec 文件，不把用户已有脏文件或其他活跃任务目录加入提交，也不改变已发布的源 SHA。
