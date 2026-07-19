# 供应商账户用量折叠选项卡

## Goal

缩短供应商编辑弹窗的默认内容长度：将账户用量配置区改为与“限流配置”等现有区域一致的折叠面板，默认只显示摘要，用户主动展开后再查看和编辑完整配置。

## Background

- `src/pages/providers/ProviderEditorDialog.tsx:104` 在认证配置之后直接渲染 `ProviderAccountUsageSection`；`src/pages/providers/ProviderEditorDialog.tsx:131` 在同一弹窗稍后渲染 `LimitsSection`。
- `src/pages/providers/ProviderAccountUsageSection.tsx:18` 只对 API Key 认证方式展示；其内容从 `src/pages/providers/ProviderAccountUsageSection.tsx:27` 开始直接铺开，并随适配器和查询方式增加凭据、定时刷新及刷新间隔等字段。
- `src/pages/providers/LimitsSection.tsx:30` 与 `src/pages/providers/ClaudeModelSection.tsx:20` 已使用无 `open` 属性的原生 `<details>` 面板：默认收起、独立展开，并在摘要中保留标题及说明或状态。
- 供应商卡片的账户用量结果由独立的 `src/components/providers/ProviderAccountUsageInline.tsx` 展示，不属于本次编辑弹窗改造。
- 现有账户用量契约要求 NewAPI 用户账户模式明确呈现凭据缺失状态，并保持适配器切换、凭据草稿与显式清除语义。

## Requirements

- **R1**：只调整供应商编辑弹窗中的 `ProviderAccountUsageSection` 呈现方式；它仍只在 API Key 认证方式下出现。
- **R2**：账户用量使用与现有 `<details>` 区域一致的折叠面板。在关闭、已配置或凭据不完整等所有可见状态下均默认收起；不自动展开，也不持久化展开状态。
- **R3**：展开面板后完整保留现有条件化配置内容，包括适配器、NewAPI 查询方式、User ID、系统访问令牌、清除账户凭据、定时刷新和刷新间隔；不截断或删除字段。
- **R4**：折叠摘要实时显示当前配置状态：关闭、sub2api、NewAPI 模型令牌额度或 NewAPI 用户账户余额。NewAPI 用户账户凭据不完整时，摘要同时显示“需配置账户凭据”的警示状态。
- **R5**：面板继续位于认证配置之后，不移动到“限流配置”等高级设置附近，也不改变弹窗其他区域的相对顺序。
- **R6**：保留现有表单行为与保存语义，包括适配器和查询方式切换、凭据草稿保留、令牌显隐、显式清除凭据、定时刷新开关及刷新间隔约束。
- **R7**：摘要可通过鼠标和键盘使用，展开指示与现有折叠面板一致；账户用量与其他 `<details>` 面板保持相互独立，不引入互斥 Tab 或手风琴状态。

## Acceptance Criteria

- [x] **AC1 / R1-R2**：打开 API Key 供应商编辑弹窗时，账户用量面板在关闭、已配置和凭据不完整三类状态下都默认收起；非 API Key 认证方式仍不显示该面板。
- [x] **AC2 / R2-R3**：激活摘要可以展开和再次收起面板；展开后，每种适配器与 NewAPI 查询方式原有的条件字段均完整可用。
- [x] **AC3 / R4**：无需展开即可区分关闭、sub2api、NewAPI 模型令牌额度和 NewAPI 用户账户余额；用户在展开区切换配置后，摘要同步更新。
- [x] **AC4 / R4**：NewAPI 用户账户凭据不完整时，默认收起的摘要明确显示“需配置账户凭据”，且不会因此自动展开。
- [x] **AC5 / R5-R7**：账户用量面板仍紧跟认证配置，样式、展开指示和键盘交互与现有折叠面板一致，并可与“限流配置”等区域独立展开且无内容重叠。
- [x] **AC6 / R6**：现有账户用量配置组件与编辑弹窗集成测试继续覆盖模式切换、凭据草稿、令牌显隐、显式清除、定时刷新及刷新间隔行为；测试按新交互先展开面板再操作字段。
- [x] **AC7**：新增或更新的自动化测试覆盖默认收起、展开/收起、摘要状态矩阵和凭据缺失警示；相关单元测试、TypeScript 类型检查、ESLint 与格式检查通过。

## Verification

- 聚焦 Vitest：79/79 通过。
- `pnpm typecheck`、目标文件 ESLint、Prettier 与 `git diff --check` 通过。
- 独立检查无发现，未修改代码。
- Kimi WebBridge 桌面验证覆盖默认收起、展开完整字段、NewAPI 模式切换和凭据缺失警示。
- Kimi WebBridge 在 390×844 视口验证展开态与收起态；页面无横向溢出，摘要警示正常换行，无内容重叠。
- 截图：`provider-account-usage-dialog-5be2.jpg`、`provider-account-usage-expanded-desktop.jpg`、`provider-account-usage-newapi-desktop.jpg`、`provider-account-usage-newapi-mobile-expanded.jpg`、`provider-account-usage-newapi-mobile-collapsed.jpg`。
- Spec 结论：本次仅复用既有原生 `<details>` 编辑弹窗模式，未改变查询、IPC、凭据或持久化契约，无需更新 `.trellis/spec/`。

## Out Of Scope

- 不修改供应商卡片上的账户用量余额、额度摘要及其手动刷新交互。
- 不修改账户用量后端、IPC、生成绑定、查询缓存、远端协议、计算规则或持久化结构。
- 不重设计“限流配置”、Claude 模型映射或其他既有折叠面板。
- 不新增跨弹窗持久化的展开状态，也不把多个折叠面板改造成互斥 Tab 或手风琴。
