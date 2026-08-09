# Orca 清理与 `0.60.40` 发布最终质量检查

检查日期：2026-08-10（Asia/Shanghai）

## 审阅结论

- 稳定版 `0.60.40` 已从 `origin` 仓库的标准 release-please / release workflow
  路径发布成功。
- 发布后的本地 `HEAD`、`origin/main`、Git tag 和 GitHub Release target 均为不可变提交
  `99272e4b2beffc52f483efef2e3985d9867d8051`。
- 8 个目标子 worktree 已从 Orca 和 Git worktree 注册表移除，原分支全部保留且仍精确指向
  清理前 SHA。
- 当前发布会话终端仍在唯一可见 tab 中；Beta 旧终端已不存在。旧 hook 终端仍存在于
  runtime terminal list，但不在视觉布局中，且此前 tab 级关闭返回 `tab_not_found`。本次复核
  按 fail-closed 约束只读检查，没有绕过 Orca 或修改该句柄。
- 未发现本任务引入的业务源码或发布内容问题。唯一未通过的本地合同检查是 Windows
  `pnpm check:generated-bindings` 的已知 linker 环境限制 `LNK1140`；PR #30 的 Linux
  `Generated IPC bindings contract` 已成功，不能将该本地失败归类为源码失败。

## 发布证据

### Release PR

- PR：`#30`，标题 `chore(main): release aio-coding-hub 0.60.40`，状态 `MERGED`。
- base/head：`main` / `release-please--branches--main--components--aio-coding-hub`。
- merge commit：`99272e4b2beffc52f483efef2e3985d9867d8051`。
- 变更范围严格为 6 个标准发布文件：`.release-please-manifest.json`、`CHANGELOG.md`、
  `package.json`、`src-tauri/Cargo.lock`、`src-tauri/Cargo.toml`、
  `src-tauri/tauri.conf.json`。
- `change-scope`、`support-contract`、三平台 desktop support、`frontend`、`rust`、
  `ci-gate` 和 Windows dev build 均成功；未选择的 `docs-contract` 正常跳过。
- Linux `frontend` job 中的 `Generated IPC bindings contract` 步骤成功。

### 版本与来源

- `.release-please-manifest.json`：`0.60.40`。
- 根 `package.json`：`0.60.40`。
- `src-tauri/Cargo.toml`：`0.60.40`。
- `src-tauri/tauri.conf.json`：`0.60.40`。
- `CHANGELOG.md` 顶部版本：`0.60.40`，compare range 为
  `aio-coding-hub-v0.60.39...aio-coding-hub-v0.60.40`。
- `refs/heads/main`、`refs/tags/aio-coding-hub-v0.60.40`、GitHub Release
  `target_commitish` 和本地 `HEAD` 均为 `99272e4b2beffc52f483efef2e3985d9867d8051`。

### Release workflow 与制品

- Actions run：`31326329193`，事件 `workflow_dispatch`，head SHA 为上述 merge SHA，
  总体结论 `success`。
- `release-please`、4 个平台 build、`assemble-release-candidate`、`promote-release`、
  `publish`、`publish-homebrew-cask` 共 9 个 job 全部成功。
- Homebrew job 按既有无 token 路径成功完成，实际 tap sync 步骤明确跳过。
- GitHub Release `aio-coding-hub-v0.60.40` 已发布，`draft=false`、
  `prerelease=false`。
- Release 恰有 `scripts/release-promotion.mjs` 定义的 14 个公开资产；名称集合严格相等，
  所有资产状态均为 `uploaded` 且均有 SHA-256 digest。
- `latest.json` 的 `version` 为 `0.60.40`，平台集合严格为 `windows-x86_64`、
  `darwin-x86_64`、`darwin-aarch64`、`linux-x86_64`；4 个条目的 URL 与对应公开资产一致，
  signature 均非空。

## Orca 与 Git 清理证据

- Orca runtime `1.4.176` 状态为 `ready`，runtime ID 与执行记录一致：
  `e262a591-bdd0-475f-b11e-ee6636f0a71b`。
- `orca worktree ps --json` 对当前仓库只列出主 worktree，且 `childWorktreeIds` 为空；
  `git worktree list --porcelain` 同样只列出主 worktree。
- 8 条恢复分支逐条复核通过：
  - `FingerCaster/cross-restart-reset` -> `171d5f7657ced58b19bf237c7a1c546bc0c59bb1`
  - `FingerCaster/plugin-runtime` -> `efd72e3a30d11f02982365c49f11e104404b3ed9`
  - `FingerCaster/port-hardening-integration` -> `bc86de0fd66aa10615a7e6aee55da122697dcc79`
  - `FingerCaster/provider-routing` -> `a4c3122f82e979610b75508ac899123fb22fd88d`
  - `FingerCaster/release-hardening` -> `a6394ff1abedb6e33bd0b06f3c8c424107dcd117`
  - `FingerCaster/rust-audit` -> `4f75a9ea1a6d88d4d8b00d73e161672b25c5c4cc`
  - `FingerCaster/sessions-ui` -> `2029e88a012d587ad90e1e751787a2df0b792f9b`
  - `FingerCaster/settings-pricing` -> `0e95ee4e662bf13caedcf0c60cce4eb23a066d32`
- 当前终端 `term_c6068d07-36f4-479c-8e10-734645fb9bd7` 保留；Beta 终端
  `term_085aa34f-5923-4875-aed5-a9f98e4489de` 已不在列表或布局中。
- 旧 hook 终端 `term_73b5ef5b-3967-4ff3-9160-0ee1ca9d483c` 仍在 runtime list，
  但唯一视觉布局只包含当前终端。这是 Orca runtime 与布局之间的剩余状态不一致。
- 主 worktree comment 已记录 `v0.60.40`、release SHA、run ID 和 8 个 worktree 清理结果。

## 用户工作区保护

- staged diff 为空，Git object hash 为 `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`。
- 受保护的既有 unstaged tracked diff 仍仅为用户的 `AGENTS.md`，Git object hash 为
  `c5683faee9fedeafbd0a57d33b8d167c7e4fcf80`；按执行前快照采用 CRLF 与结尾换行计算的
  SHA-256 仍为 `afa073012b546e49bd03cb82e0f65ea671dade52db7bee8055d1640970467a86`。
- 本任务收尾另新增 `release-operations-contract.md` 并修改 cross-layer spec index；两者与
  用户原有 tracked diff 分开审阅，没有进入已发布提交。
- 排除本任务目录后，既有 untracked 普通文件仍为 62 个，与执行前保护快照计数一致。
  `cleanup-execution.md` 已记录清理前后逐文件内容 hash 相等；本次发布提交、PR 合并和只读
  复核均未暂存这些文件。
- 本任务额外未跟踪内容只包含当前任务记录和新的 release operations spec；未改动其他任务、
  `.orca/`、`AGENTS.md` 或用户 HTML 文件。

## 本次质量检查

通过：

- `python ./.trellis/scripts/get_context.py --mode packages`
- `python ./.trellis/scripts/task.py validate .trellis/tasks/08-09-orca-cleanup-release`
- `pnpm lint`
- `pnpm typecheck`
- `pnpm tauri:fmt`
- `node scripts/release-source.selftest.mjs`
- `node scripts/release-promotion.selftest.mjs`
- `node scripts/check-release-signing-secret-scope.selftest.mjs`
- `node scripts/check-release-signing-secret-scope.mjs`
- `pnpm check:ci-change-scope`
- `node scripts/support-matrix.mjs check`
- `node scripts/support-matrix.homebrew-cask.selftest.mjs`
- `node scripts/check-spec-links.mjs`
- `git diff --check`
- `git diff --cached --check`
- `git diff --check HEAD --`

环境限制：

- `pnpm check:generated-bindings` 在本机 Windows 链接阶段退出 1，底层 Rust 返回 101，
  `link.exe` 返回 `LNK1140`（PDB 限制）。该命令未进入 bindings 内容比较；PR #30 的 Linux
  同名合同步骤成功，且 PR 的 `frontend`、`rust` 与最终 `ci-gate` 全部成功。

## 剩余风险

1. 旧 hook 终端是 runtime-only 的隐藏句柄。此前允许的 tab 级关闭已返回
   `tab_not_found`，因此没有通过 pane close、主 worktree stop 或重启 Orca 强制清除；后续可由
   Orca runtime 自身恢复或在明确授权的应用重启窗口中复核。
2. Windows 本机无法完成生成绑定内容比较，原因是本机 linker/PDB 环境限制；发布门由 PR
   Linux 合同成功覆盖，但该本机环境问题仍独立存在。
