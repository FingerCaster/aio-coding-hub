# Orca 清理与本地发布前检查执行证据

执行日期：2026-08-10（Asia/Shanghai）
主 worktree：`4f80921d-2ad0-47a0-8fb8-f71113fbfaf4::D:/UGit/aio-coding-hub-fork`
Orca CLI：`C:/Users/Administrator/AppData/Local/Programs/orca/resources/bin/orca.exe`
Orca runtime：1.4.176，runtime ID `e262a591-bdd0-475f-b11e-ee6636f0a71b`

## 执行边界

- 只使用版本匹配 Orca CLI 的 `terminal close --tab`、`terminal stop`、`worktree rm`、
  `terminal list/show/read` 和 `worktree ps` 操作 Orca 状态。
- 未使用原始 `git worktree remove`、目录删除、分支删除或 stash 操作。
- 未 commit、fetch、push、调用 `gh`、修改 PR/Release、dispatch workflow 或操作 `upstream`。
- Orca 1.4.176 的 `worktree rm --force` 实际会删除多数被移除 worktree 的本地分支，且
  `worktree rm --help` 没有保留分支参数。经主会话明确授权，每次 rm 前保存完整 ref 与 HEAD，
  rm 后若 ref 消失，仅用 `git branch <原分支> <原 HEAD>` 原样恢复，并立即核对 SHA。

## 主 worktree 完整性

清理前与所有 preflight 结束后的快照完全一致（写入本证据文件之前）：

- `HEAD`：`bc86de0fd66aa10615a7e6aee55da122697dcc79`，分支 `main`。
- staged diff hash：`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`（无 staged 内容）。
- unstaged tracked diff hash：`c5683faee9fedeafbd0a57d33b8d167c7e4fcf80`。
- `git status --porcelain=v1 --untracked-files=all` 的条目集合完全一致。
- 每个既有 untracked 普通文件的 `git hash-object --no-filters` 值完全一致。
- 全部本地 branch refs hash：`e6396ad94712df135706e048759f1f7727278ce2`，前后相同。
- stash list hash：`6f9d4e1a997a77d7179daeccba1351c5baa968f9`，前后相同。
- 本文件是上述比较完成后唯一由本执行新增的工作树文件。

## 主 worktree 终端

`orca terminal close --help` 明确显示：`--tab` 会关闭整个 tab 并等待持久删除。

| 终端 | 操作与结果 | 最终状态 |
| --- | --- | --- |
| `term_c6068d07-36f4-479c-8e10-734645fb9bd7` | 未执行 close/stop；清理期间持续为当前工作会话 | 保留，且是最终视觉布局中唯一 tab |
| `term_085aa34f-5923-4875-aed5-a9f98e4489de` | 清理前 Agent 为 `done`；`terminal close --tab --json` 返回 `ok:true`、`closeMode:"tab"`，tab `27e2cbd5-4f82-4c30-b0ce-ad3a577ff158` | 已从 terminal list 与视觉布局移除 |
| `term_73b5ef5b-3967-4ff3-9160-0ee1ca9d483c` | 清理前内容停在已完成结果；两次 `terminal close --tab --json` 均返回 `runtime_error: tab_not_found` | 持久视觉布局中已无该 tab，但 runtime terminal list 仍保留 stale handle；按 fail-closed 边界未改用 pane close 或主 worktree stop |

最终 `terminal list --include-visual-layouts` 的视觉布局仅含当前 tab
`bae7fba7-83b0-417c-aad4-216354f75723`（`term_c6068d07-36f4-479c-8e10-734645fb9bd7`）。
`term_73...` 的 runtime stale handle 是剩余 Orca 状态风险。

## 子 worktree 清理

每项在 rm 前均即时确认：Git status 为空、HEAD/分支与审计记录一致、对应归档任务
`status=completed`，且 Orca Agent 集合为空或全部为 `done`。随后执行
`orca terminal stop --worktree id:<完整 ID> --json`，确认 `hasAttachedPty=false`、
`liveTerminalCount=0`、terminal list 为空，再执行 `orca worktree rm --force --json`。

| worktree | 清理前 HEAD | stop | rm | 分支最终状态 |
| --- | --- | ---: | --- | --- |
| `cross-restart-reset` | `171d5f7657ced58b19bf237c7a1c546bc0c59bb1` | 1 | `removed:true`，CLI 返回 `preservedBranch` | `FingerCaster/cross-restart-reset` 保留在原 SHA |
| `plugin-runtime` | `efd72e3a30d11f02982365c49f11e104404b3ed9` | 2 | `removed:true`；CLI 同步删除 ref | 主会话精确恢复 `FingerCaster/plugin-runtime` 到原 SHA |
| `port-hardening-integration` | `bc86de0fd66aa10615a7e6aee55da122697dcc79` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/port-hardening-integration` 到原 SHA |
| `provider-routing` | `a4c3122f82e979610b75508ac899123fb22fd88d` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/provider-routing` 到原 SHA |
| `release-hardening` | `a6394ff1abedb6e33bd0b06f3c8c424107dcd117` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/release-hardening` 到原 SHA |
| `rust-audit` | `4f75a9ea1a6d88d4d8b00d73e161672b25c5c4cc` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/rust-audit` 到原 SHA |
| `sessions-ui` | `2029e88a012d587ad90e1e751787a2df0b792f9b` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/sessions-ui` 到原 SHA |
| `settings-pricing` | `0e95ee4e662bf13caedcf0c60cce4eb23a066d32` | 1 | `removed:true`；CLI 同步删除 ref | 精确恢复 `FingerCaster/settings-pricing` 到原 SHA |

最终复核：上述八个完整 ID 在 `orca worktree ps --json` 中均为 0 项；对应路径在
`git worktree list --porcelain` 中均为 0 项且目录不存在；八个本地分支均精确指向表中原 SHA。

## 本地 preflight

未安装依赖，也未修复检查发现的环境/无关失败。

| 命令 | 退出码 | 结果 |
| --- | ---: | --- |
| `pnpm typecheck` | 1 | 环境失败：`node_modules/.bin/tsc` 不存在，`tsc is not recognized` |
| `pnpm lint` | 1 | 环境失败：`node_modules/.bin/eslint` 不存在，`eslint is not recognized` |
| `pnpm check:prepush` | 1 | 在第 1/15 项 `lint` fail-fast；同一缺失 `eslint`，后 14 项未由聚合命令执行 |
| `node scripts/release-source.selftest.mjs` | 0 | annotated、lightweight、missing draft tag 场景通过 |
| `node scripts/release-promotion.selftest.mjs` | 0 | 全部断言通过 |
| `node scripts/check-release-signing-secret-scope.selftest.mjs` | 0 | 全部断言通过 |
| `node scripts/check-release-signing-secret-scope.mjs` | 0 | workflow signing scope contract 通过 |
| `pnpm check:ci-change-scope` | 0 | classifier 与 CI workflow contract self-test 均通过 |
| `node scripts/support-matrix.mjs check` | 0 | 支持矩阵合同通过 |
| `node scripts/support-matrix.homebrew-cask.selftest.mjs` | 0 | Homebrew Cask self-test 通过 |
| `node scripts/check-plugin-system-docs.mjs` | 0 | 通过 |
| `node scripts/check-plugin-api-contract.mjs` | 0 | 通过 |
| `node scripts/check-spec-links.mjs` | 0 | 通过 |
| `node scripts/check-sync-upstream-policy.selftest.mjs` | 0 | 手工审阅策略 self-test 通过 |
| `node scripts/check-sync-upstream-policy.mjs` | 0 | 手工审阅策略检查通过 |
| `node scripts/check-generated-bindings.mjs` | 1 | Windows 链接环境失败：`LINK : fatal error LNK1140`（PDB 限制）；未得到 bindings 内容比较结果 |
| `pnpm tauri:fmt` | 0 | `cargo fmt -- --check` 通过 |
| `git diff --check` | 0 | 通过 |
| `git diff --cached --check` | 0 | 通过 |
| `git diff --check HEAD --` | 0 | 通过 |

## 结论与剩余风险

- 八个已审阅子 worktree 均已从 Orca 和 Git worktree 注册表移除；原分支与 stash 全部保留。
- 当前发布终端完整保留；Beta 旧 tab 已持久关闭。旧 hook 终端的 tab 已不在视觉布局，但
  Orca runtime 仍暴露无法用 `--tab` 关闭的 stale handle，需由 Orca runtime 自身恢复/重启清除。
- 发布合同、CI scope、支持矩阵、Homebrew、文档/API/spec、upstream policy、Rust 格式与 diff
  检查通过。
- 完整 prepush 质量门尚未通过：前端本地依赖不完整；generated bindings 检查另受 Windows
  `LNK1140` 阻塞。发布前需在依赖完整且链接环境可用的执行环境重新运行失败项和完整
  `pnpm check:prepush`。
