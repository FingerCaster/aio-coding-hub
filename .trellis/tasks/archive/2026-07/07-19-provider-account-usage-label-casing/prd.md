# 供应商账户用量标签大小写统一

## Goal

将编辑弹窗账户用量中的 sub2api/NewAPI 显示文案统一为 Sub2Api/NewApi 风格。

## Requirements

- **R1**：只统一供应商编辑弹窗账户用量折叠面板内的可见品牌文案，不修改供应商卡片、后端、IPC、查询、绑定或持久化。
- **R2**：将 `sub2api` 的可见展示统一为 `Sub2Api`，将 `NewAPI` 的可见展示统一为 `NewApi`。
- **R3**：统一范围覆盖适配器按钮、折叠摘要、“NewApi 查询方式”字段标题及对应可访问名称，避免同一面板内出现旧大小写风格。
- **R4**：内部枚举值仍为 `sub2api` / `newapi`，现有 TypeScript 标识符、协议名称、配置格式和保存语义保持不变。
- **R5**：不改变上一轮完成的等宽按钮、条件凭据和刷新设置布局。

## Acceptance Criteria

- [x] **AC1 / R1-R3**：sub2api 模式的按钮与折叠摘要均显示 `Sub2Api`。
- [x] **AC2 / R1-R3**：NewAPI 模式的按钮、折叠摘要、查询方式标题和可访问名称均显示 `NewApi`。
- [x] **AC3 / R4-R5**：内部适配器值、模式切换回调参数、布局类及所有交互行为不变。
- [x] **AC4**：相关 Vitest、TypeScript 类型检查、ESLint、Prettier 与 `git diff --check` 通过；Kimi WebBridge 实际页面不再出现旧显示形式。

## Verification

- 相关 Vitest：2 个文件，81/81 通过。
- `pnpm lint`、`pnpm typecheck`、3 个目标文件 Prettier 与 `git diff --check` 通过。
- 独立 `trellis-check` 无发现；目标组件旧 `NewAPI` 可见字面量为 0，`sub2api/newapi` 仅保留为内部值。
- Kimi WebBridge 实际页面确认折叠摘要与适配器按钮显示 `Sub2Api / NewApi`。
- Spec 结论：本次仅统一组件内可见文案，不改变协议、配置或项目级命名契约，无需更新 `.trellis/spec/`。

## Notes

- 这是已完成账户用量折叠功能的轻量文案统一，沿用当前独立 worktree 与分支。
