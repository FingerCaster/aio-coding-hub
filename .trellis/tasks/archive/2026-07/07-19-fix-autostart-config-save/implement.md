# 实施计划

1. [x] 将 generated `SettingsUpdate.auto_start` 改为 `Option<bool>`，后端 patch 计算时在 `None` 情况保留最新 canonical 值。
2. [x] 拆分普通 settings commit：显式自启动写入复用 coordinator 并返回 `Some(token)`；无关写入直接持久化并返回 `None`。
3. [x] 让 settings runtime-failure rollback 在 `None` token 时走 settings-only CAS，忽略并保留并发 `auto_start`，不执行 OS sync；`Some` 路径保持现有契约。
4. [x] 修改 CLI Manager patch 与设置页 changed-key 构造，使无关保存不携带 `autoStart`；更新生成绑定与前端类型契约。
5. [x] 保留 Windows adapter 的 `NotFound` 幂等修复，清理单独 manifest 测试产生的非仓库 Cargo.lock。
6. [x] 补后端、前端与 adapter 回归，运行 formatter、generated-binding check、聚焦测试、Rust check、typecheck/lint。
7. [x] 由独立 check 子代理审查副作用边界、并发/rollback 契约和测试缺口；修复发现后提交任务。
8. [x] 快进合并到本地 `main`，确认主工作区用户改动未受影响，打包并校验 Windows x64 MSI。

## Verification

- Frontend focused: 3 files / 41 tests passed.
- Rust: settings service 15, autostart 12, config-import ownership 3, Windows adapter 3 passed.
- `pnpm typecheck`, `pnpm lint`, `pnpm check:generated-bindings`, `pnpm tauri:fmt`, and `pnpm tauri:check` passed.
- `pnpm tauri:clippy` still reports only the unchanged baseline `upstream_error.rs` `too_many_arguments`; the file blob equals HEAD and this task adds no suppression.
- `main@54ba206e` built `AIO Coding Hub_0.60.28_x64_en-US.msi` successfully: 16,146,432 bytes, SHA-256 `4B790DC873FDE1805E3EFB7499F4ECA1115299B764D0E5930A08A56ADE068288`, unsigned local package.
