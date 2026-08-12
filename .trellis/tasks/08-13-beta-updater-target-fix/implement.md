# 实施计划

## 1. 规划与基线

- [x] 确认任务、分支和 worktree 均基于最新 `origin/main`。
- [x] 记录 Tauri updater 版本及 OS target / manifest key 的实际合同。
- [x] 记录用户复现的 Windows Beta `UPDATER_MANIFEST_INVALID` 基线结果。

## 2. 实现

- [x] 在 `src-tauri/src/commands/desktop.rs` 提取单一官方平台映射，分离 OS target 与 manifest key。
- [x] 修正候选身份、Beta fresh-check 和安装前复核的调用参数。
- [x] 保持 Stable 行为和严格 URL/签名/raw manifest 校验不变。
- [x] 补齐四平台与负例回归测试。

## 3. 质量门禁

- [x] `cargo fmt --check`、`cargo check --locked`、聚焦 Rust tests、全 targets Clippy。
- [ ] `pnpm check:generated-bindings`、前端 typecheck/lint（该 worktree 缺少 `node_modules`）；`pnpm check:spec-links` 已通过。
- [x] Beta release/source/contract/promotion/channel/signing/support-matrix/Homebrew/CI scope 自测。
- [x] `git diff --check`，审查 diff 只包含本任务文件。

## 4. 集成与发布

- [ ] 提交修复 commit，推送 `FingerCaster/beta-updater-target-fix`，创建 PR 到 `main`。
- [ ] 等待所有 required checks，通过后合并并确认 `origin/main` SHA。
- [ ] 从不可变合入 SHA 运行手动 Beta workflow，严格递增版本并确认 flags/14 assets。
- [ ] 独立验证 release tag、asset digest/signature、latest-beta bytes/state、Stable isolation 和实际 Windows 检查。

## 5. 收尾

- [x] 更新 Beta updater 跨层 spec，记录 OS target 与 manifest key 的分离合同。
- [ ] 按 Trellis 规则提交归档/日志，清理独立 worktree、临时下载物和本任务分支。
