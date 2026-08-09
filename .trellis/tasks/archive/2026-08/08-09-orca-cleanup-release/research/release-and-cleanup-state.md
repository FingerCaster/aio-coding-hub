# Orca 清理与 `0.60.40` 发布状态研究

## 研究范围

本文件记录 2026-08-09 规划阶段的只读证据。检查使用本机版本匹配的 Orca CLI、Git、
`gh -R FingerCaster/aio-coding-hub`、任务归档和发布工作流；未关闭终端、删除 worktree、
修改 Git 引用、推送、合并 PR 或触发 workflow。

Trellis 研究子代理连续遇到上游 `502`，未能写入结果；下列内容由主会话依据已执行的只读
命令整理，保留了异常与非等价证据。

## Git 与发布基线

- `origin/main`：`a8c525cdaadce77dd4b00363962e501bc5fae491`，同时为
  `aio-coding-hub-v0.60.39`；该提交是 release PR #29 的 merge commit。
- 本地 `main`：`bc86de0fd66aa10615a7e6aee55da122697dcc79`，比 `origin/main`
  领先 25 个已提交变更。主 worktree 另有用户已有的 tracked/untracked 变更，没有纳入这
  25 个提交。
- `.release-please-manifest.json`、`package.json`、`Cargo.toml` 和 `tauri.conf.json`
  的当前发布基线均为 `0.60.39`。
- `.github/workflows/release.yml` 仅由 `workflow_dispatch` 触发。无 `release_tag` 时运行
  release-please；release PR 合并后的第二次无参数 dispatch 才创建 tag/草稿 Release，
  解析 immutable SHA，并进入 build/assemble/promote/publish/Homebrew job 图。
- `release.yml`、`release-promotion.mjs` 与自测要求稳定三段式 tag、非 prerelease draft、
  exact asset matrix、候选 SHA/run identity/digest 一致、上传不覆盖、发布前再次核对 tag SHA。

## 错误 release PR #30

- PR #30 基于 `a8c525c`，错误标题为 `chore(main): release aio-coding-hub 0.60.6`。
- 其版本提交为 `ef8e55f813d033ba0d13a56fc7a5bd478dcd3673`，Cargo lock 同步提交为
  `8941cec5f91cf72bf6201a3e1fa8a12d2d40dc43`。
- PR 中 `.release-please-manifest.json` 与 `package.json` 都被降到 `0.60.6`；变更范围是
  标准六个 release 文件，但 changelog compare 为 `v0.60.39...v0.60.6`，因此检查成功也
  不能视为可发布。
- Git 历史中的 `b4378f135ae72afc87bd56aa72723df0c9e1c327` 含有旧的
  `Release-As: 0.60.6`，解释了 release-please 的显式降级选择。
- release-please 官方文档支持在 `main` 上创建空提交并用提交正文
  `Release-As: x.x.x` 指定下一版本：
  <https://github.com/googleapis/release-please#how-do-i-change-the-version-number>。

结论：在不暂存用户文件的前提下创建新的空提交
`chore(release): prepare aio-coding-hub 0.60.40` / `Release-As: 0.60.40`，推送后无参数
dispatch。PR #30 必须刷新为 `0.60.40` 并通过内容/CI 验证才能合并；若仍错误，先保留证据，
关闭错误 PR/生成分支后只允许再生成一次，第二次仍错误则停止，不手工篡改生成分支。

## Orca 主 worktree 终端

主 worktree ID：
`4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/UGit/aio-coding-hub-fork`。

| Terminal | 证据 | 处理 |
| --- | --- | --- |
| `term_c6068d07-36f4-479c-8e10-734645fb9bd7` | 当前发布会话，持续输出本任务命令 | 保留 |
| `term_085aa34f-5923-4875-aed5-a9f98e4489de` | Agent 已结束；内容为 Beta 方案调查并停在建任务询问；用户明确 Beta 后续单独讨论 | tab 级关闭 |
| `term_73b5ef5b-3967-4ff3-9160-0ee1ca9d483c` | 旧 Trellis hook 会话，已给出最终结果，之后仅停在提示符 | tab 级关闭 |

执行前必须用当前 Orca 的 `terminal close --help` 确认 tab 级参数；历史记录证明普通 pane close
可能被 runtime 恢复，不能凭旧参数猜测。

## 子 worktree 审计

下列 worktree 均满足：Git status 为空、Orca Agent 状态为 `done`、对应 Trellis 任务已归档
为 `completed`。本任务只删除 checkout/Orca 注册，保留所有 Git 分支。

| 名称 | HEAD | 整合证据 |
| --- | --- | --- |
| `port-hardening-integration` | `bc86de0f` | 与本地 `main` 完全相同 |
| `plugin-runtime` | `efd72e3a` | `git cherry main` 的两项均为 `-`（patch 等价） |
| `provider-routing` | `a4c3122f` | 两项均为 `-` |
| `release-hardening` | `a6394ff1` | 两项均为 `-` |
| `rust-audit` | `4f75a9ea` | 唯一项为 `-` |
| `sessions-ui` | `2029e88a` | 两项均为 `-` |
| `settings-pricing` | `0e95ee4e` | 两项均为 `-` |
| `cross-restart-reset` | `171d5f76` | 初始提交 patch 等价；review 提交不是同 patch-id，但 `git range-diff 171d5f76^! a2295582^!` 将其映射到主线整合提交，差异集中在与同时集成测试的适配；原分支继续保留 |

八个完整 Orca ID 都是：

```text
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/cross-restart-reset
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/plugin-runtime
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/port-hardening-integration
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/provider-routing
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/release-hardening
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/rust-audit
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/sessions-ui
4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/OrcaProjects/aio-coding-hub-fork/settings-pricing
```

结论：逐项 `orca terminal stop --worktree <完整 id>` 后再
`orca worktree rm --worktree <完整 id> --force` 是可恢复的清理路径。任何 stale PTY 或
rm 失败都应保留对象、重新列出状态；不使用原始 `git worktree remove` 或目录删除。

## 推荐发布顺序与停止条件

1. 保存 Git/Orca/终端/用户脏状态快照并 fetch `origin/main`；远端不再为预期祖先时停止。
2. 关闭两个旧 tab，逐项 stop/rm 八个子 worktree；每项用 Orca 与 Git 双清单复核。
3. 运行 prepush 与 release source/promotion/signing/CI scope 合同测试；失败即停止。
4. 创建唯一的空 `Release-As: 0.60.40` 提交并推送 `main`；不得暂存当前用户文件。
5. 第一次无参数 dispatch，严格验 PR 版本/文件/changelog/CI；错误或未通过时不合并。
6. merge release PR，验证 `origin/main` 与四个版本文件后，第二次无参数 dispatch。
7. 等待完整发布 job 图终态，核验 tag SHA、Release 身份、14 项当前资产矩阵、digest、
   `latest.json` 版本/签名/四平台映射和 Homebrew job。
8. 只有全部通过才更新 Orca comment 并归档任务；发布后的 Trellis 记录不改变发布源 SHA。

若发布 run 已创建 draft 但未晋升，先检查失败 job 和候选身份再决定幂等重试；禁止盲目重跑、
覆盖已有资产或手工将不一致 draft 发布。
