# 集成余额刷新与 Beta updater 修复并发布

## Goal

合并余额查询缓存修复与 Beta updater 平台映射修复，处理冲突，经质量门禁后发布递增 Beta。

## Requirements

- R1：将 `69e9fab2` 的余额查询缓存绕过修复完整合入当前 `origin/main`，保留其适配器协议、凭据隔离和测试覆盖。
- R2：将独立 updater 修复 worktree 的未提交差异纳入集成分支，分离 Tauri OS target、静态 manifest key 和官方资产名；处理所有实际冲突，不覆盖无关主工作区改动。
- R3：余额手动刷新和 Beta 检查均保持原有跨层合同；不得通过放宽签名、URL、manifest 或路由安全校验来规避失败。
- R4：运行 Rust/前端/生成绑定/发布合同门禁；对冲突解决后的最终 diff 做 findings-first 审核。
- R5：将集成分支推送并合入 `origin/main` 后，以合入 commit 的不可变 40 位 SHA 发布严格高于 `0.60.41-beta.3` 的公开 Beta，保持 `draft=false`、`prerelease=true`、`make_latest=false`、Stable 和 Homebrew 隔离。
- R6：发布后独立核对 14 项资产、签名、manifest/pointer/state 字节一致性，并在 Windows Beta 客户端验证 updater 检查不再出现 target mismatch；余额刷新修复至少完成构建级和聚焦测试验证。

## Acceptance Criteria

- [ ] 两组变更均存在于同一集成分支，`git diff --check` 通过且无未解决冲突/无关源码漂移。
- [ ] 余额用量 focused Rust/前端测试、updater 四平台回归、Rust fmt/check/Clippy、typecheck/lint/generated bindings 和发布合同全部通过；环境缺口需明确记录。
- [ ] PR 合入 `origin/main` 后记录最终 40 位源 SHA，并以该 SHA 构建 Beta。
- [ ] 新 Beta 版本严格高于 `0.60.41-beta.3`，Release/pointer/state/14 assets/签名与 Stable 隔离合同全部通过。
- [ ] 实际 Windows Beta 检查成功；余额为零时直接点击刷新可以触发无缓存远端请求，不依赖 Provider 测试。
- [ ] 集成 worktree、临时任务产物和发布辅助文件按 Trellis/Orca 规则清理，主工作区既有改动保持不变。

## Notes

- 来源：余额修复提交 `69e9fab2fe6fd19cf4abae16178193697156307f`；updater 修复 worktree `beta-updater-target-fix` 的 `desktop.rs` 未提交差异。
- 基线：`origin/main`，当前公开 Beta 为 `aio-coding-hub-v0.60.41-beta.3`。
- 本任务只负责集成、冲突处理、验证和发布，不修改主工作区现有未提交文件。
