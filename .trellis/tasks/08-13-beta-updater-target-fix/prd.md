# 修复 Beta updater 平台 target 映射

## Goal

修复 Beta 更新检查在 Windows 上必然失败的问题，使已选择 Beta 频道的用户可以正常发现并安装当前平台的 Beta 更新，同时保持 Stable 频道和其他平台行为不变。

## Background

- 在线 `release-channels/latest-beta.json` 的静态平台键为 `windows-x86_64`、`darwin-x86_64`、`darwin-aarch64`、`linux-x86_64`，当前 Beta 3 清单四个平台 URL 和签名均有效。
- `tauri-plugin-updater` 2.x 将 `Update.target` 填为操作系统值（Windows=`windows`、macOS=`darwin`、Linux=`linux`），而 `tauri_plugin_updater::target()` 返回带架构的 JSON 清单键（例如 `windows-x86_64`）。
- 当前 [src-tauri/src/commands/desktop.rs:507-552](/D:/UGit/aio-coding-hub-fork/src-tauri/src/commands/desktop.rs:507) 将这两个不同语义的值直接比较，并将清单键传给 URL 资产校验，因此 Windows Beta 检查返回 `UPDATER_MANIFEST_INVALID: target must match the current platform and signature must be non-empty`。
- 该错误发生在客户端消费已发布清单之后，不应通过放宽签名、URL 或静态 manifest 校验来规避。

## Requirements

- R1 平台身份分离：明确区分 updater 运行时 OS target、静态 manifest platform key 和官方资产名；为四个支持平台建立单一、可复用的映射。
- R2 Beta 检查修复：Beta fresh-check 和候选身份校验使用正确的静态 manifest key，Windows Beta 3 清单在 Windows target 上不再因 target 映射失败；签名非空、版本、tag、host/path/asset 校验继续严格执行。
- R3 安装一致性：下载安装前的 Beta fresh-check 必须使用与初次检查相同的映射和候选身份规则，不能出现“检查成功、安装复核失败”的跨平台漂移。
- R4 Stable 不回归：Stable endpoint、Stable manifest 行为、版本比较、安装路径和未选择 Beta 的用户行为保持不变；不把 Beta prerelease 误标成 Stable。
- R5 回归测试：至少覆盖 Windows、macOS Intel、macOS ARM、Linux 的 OS target 到静态键映射；覆盖错误 target、空签名、错误资产 URL 拒绝；覆盖 Beta 初次检查与 fresh-check 使用同一映射。
- R6 质量与发布：完成 Rust fmt/check/Clippy 和 updater 聚焦测试，运行生成绑定/规范/发布合同及必要前端门禁；经 PR 合入最新 `origin/main` 后，从不可变 40 位 SHA 发布递增的公开 Beta（`draft=false`、`prerelease=true`、`make_latest=false`），并独立核对 14 个资产、manifest、签名、Beta pointer 与 Stable 隔离。
- R7 工作区隔离：所有代码和发布准备在本独立 worktree 完成，不覆盖主工作区现有未提交改动；完成后清理该 worktree 和本任务临时产物。

## Acceptance Criteria

- [ ] AC1：Windows Beta 检查使用 `windows-x86_64` 查找静态清单，Beta 3 或修复版清单不再返回 target mismatch；候选 metadata 正常生成。
- [ ] AC2：四个平台映射均有自动化测试，错误 target、空签名、非官方 URL/资产和 malformed Beta manifest 继续 fail closed。
- [ ] AC3：Beta fresh-check、候选身份和安装前复核共享同一平台映射；同一候选在检查与安装阶段身份一致，Stable 路径回归测试通过。
- [ ] AC4：受影响 Rust/跨层质量门禁、全量必要测试、generated bindings/spec links、`git diff --check` 和发布合同全部通过。
- [ ] AC5：修复经 PR 合入 `origin/main`，发布源为合入后可达的不可变 40 位 SHA；不覆盖既有 tag 或 Release 资产。
- [ ] AC6：新 Beta 公开发布，版本严格高于 `0.60.41-beta.3`，Release flags、14 项资产、四平台签名/URL、`latest-beta.json` 字节摘要和 channel state 一致；Stable latest、Stable manifest、Homebrew 和未参与 Beta 用户不变。
- [ ] AC7：发布后用实际 Windows Beta 客户端执行检查，确认不再出现 `UPDATER_MANIFEST_INVALID`；独立 worktree 和临时文件清理完成。

## Out Of Scope

- 放宽或删除 Beta manifest 的静态四平台、签名、官方 host/path/asset 校验。
- 改变 updater 依赖版本、Stable 发布语义、GitHub latest 或 Homebrew 发布策略。
- 修改 Beta 频道选择 UI、settings schema 或终态防火墙业务规则。
- 自动替用户安装 Beta 或修改用户本机设置。

## Technical Notes

- `Update.target` 只用于运行时 OS/安装器语义；静态 manifest lookup 和 canonical asset 校验必须使用 `tauri_plugin_updater::target()` 返回的带架构键。
- 任何持久化候选身份字段若保留 target，应明确其语义，避免再次把 OS target 与 manifest key 混用。
