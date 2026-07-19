# 供应商账户用量控件排版优化

## Goal

优化编辑弹窗账户用量展开区的分段按钮和刷新控件排版，使两列视觉宽度与分组关系一致。

## Requirements

- **R1**：只优化供应商编辑弹窗中账户用量折叠面板的控件排版，不修改供应商卡片、后端、IPC、查询、绑定或持久化。
- **R2**：账户用量适配器使用现有 `RadioButtonGroup` 的等宽铺满模式，使“关闭 / sub2api / NewAPI”三个按钮均分所在列宽，不再只占左侧一小段。
- **R3**：桌面端将适配器选择归为第一逻辑行；NewAPI 模式下，查询方式与适配器并列。启用账户用量时，将“定时刷新”和“刷新间隔”固定在同一个独立逻辑行，并保持在条件凭据字段之后，不再由外层自动网格穿插。
- **R4**：移动端上述逻辑行自然堆叠为单列，按钮文字不得溢出或重叠。
- **R5**：保留所有条件字段、状态切换、凭据草稿、令牌显隐、显式清除、刷新开关、刷新间隔约束及保存语义。
- **R6**：复用现有 `RadioButtonGroup` 与表单样式，不修改共享组件的默认行为，也不新增卡片层级。

## Acceptance Criteria

- [x] **AC1 / R1-R3**：sub2api 展开态中，三个适配器按钮等宽铺满左列；定时刷新与刷新间隔在下一行左右并列。
- [x] **AC2 / R3**：NewAPI 展开态中，适配器和查询方式各自等宽铺满所在列；刷新控件保持独立的成对布局，不受凭据字段数量影响。
- [x] **AC3 / R4**：390px 窄屏下控件按单列排列，按钮文字完整且无横向溢出或重叠。
- [x] **AC4 / R5-R6**：现有账户用量交互测试继续通过，且自动化测试覆盖适配器等宽模式和刷新控件分组。
- [x] **AC5**：相关 Vitest、TypeScript 类型检查、ESLint、Prettier 与 `git diff --check` 通过；Kimi WebBridge 截图确认桌面与窄屏排版。

## Verification

- 聚焦 Vitest：2 个文件，81/81 通过。
- `pnpm lint`、`pnpm typecheck`、目标文件 Prettier 与 `git diff --check` 通过。
- 独立 `trellis-check` 发现并修复凭据区被排到刷新区之后的范围偏移；最终顺序为“选择器 → 条件凭据 → 刷新设置”。
- Kimi WebBridge 桌面 sub2api：三个适配器按钮宽度约 134px，严格等分；刷新开关与间隔各占 404px，同一行排列。
- Kimi WebBridge 桌面 NewAPI：适配器与查询方式各占 399px，内部选项等分；凭据组位于刷新组之前。
- Kimi WebBridge 390×844：两组分段控件各自铺满 290px 单列，页面横向宽度 390/390，无溢出或重叠。
- 截图：`provider-account-usage-controls-sub2api-desktop.jpg`、`provider-account-usage-controls-newapi-mobile.jpg`。
- Spec 结论：本次仅复用既有响应式网格和 `RadioButtonGroup` 全宽能力，没有新增组件模式或跨层契约，无需更新 `.trellis/spec/`。

## Notes

- 这是已完成账户用量折叠功能的轻量视觉排版修正，沿用当前独立 worktree 与分支。
