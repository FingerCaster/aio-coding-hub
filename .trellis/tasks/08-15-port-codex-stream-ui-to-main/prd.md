# 将 Codex 流错误 UI 归属移植到最新 main

## Goal

以当前 `origin/main` 为基线，移植旧提交 `926279e6` 的 UI 语义：把原生 Codex
Responses SSE 的流终态防火墙/内部错误设置从通用设置页移动到 Codex tab，并在
Provider 编辑器中只对 Codex Provider 显示对应覆盖项。保留 main 在此期间新增的
无限重试测试模式、CX2CC、模型/账号用量等功能以及现有后端行为。

## Requirements

- 通用设置页仍可编辑 HTTP/传输重试、共享预算和最终 HTTP 错误改写，但不再显示
  Codex 流内部错误开关、关键词或观察窗口。
- Codex tab 提供完整的 Codex 流终态错误区：启用开关、canonical 透传例外（旧版
  兼容重试词只读提示）、首个有效输出观察窗口，以及独立保存入口；设置与页面级
  retry policy/guard 草稿共享。
- Provider override 保留完整策略数据；只有 `cliKey === "codex"` 显示流内部错误
  字段，Claude、Gemini、Grok、CX2CC 等非 Codex Provider 不显示且保存不清空隐藏数据。
- 通用字段和 Codex 专属字段合并更新时不得丢失另一侧字段；两个全局保存入口都提交
  最新完整 `upstream_retry_policy` 与 `stream_internal_error_guard_ms`。
- 只修改前端 UI、前端测试和 `upstream-error-handling-contract.md`；不修改 Rust、
  settings/provider schema、迁移或生成绑定内容。

## Acceptance Criteria

- [x] General tab 没有 Codex 流专属可见文本/控件，通用重试与最终错误改写仍可编辑保存。
- [x] Codex tab 能加载、编辑、校验并保存流终态策略；观察窗口支持 0 和 5000ms，
      越界值不会写入。
- [x] Codex Provider 显示覆盖字段，非 Codex Provider 隐藏字段且原有隐藏策略原样保留。
- [x] 页面共享草稿在 General/Codex 交替编辑保存时不回退、不覆盖无关 AppSettings 字段。
- [x] 相关前端测试、typecheck、lint、格式检查、build、生成绑定检查和 spec-link 检查通过。
- [x] 最终提交只包含本端口相关文件；不 push、不建 PR、不操作 `upstream`，并在 Orca
  active worktree comment 记录提交 SHA 与验证结果。

## 验收证据

- 聚焦 Vitest：6 个受影响测试文件，166 tests passed；reviewer 复核全量前端 2860 tests passed。
- `pnpm typecheck`、`pnpm lint`、`pnpm format:check`、`pnpm build`、
  `pnpm check:generated-bindings`、`pnpm check:spec-links` 均通过。
- `git diff --check` 通过；提交范围仅包含本任务目录及 UI/spec/test 变更，未包含
  `.trellis/.developer`、`.trellis/workspace` 或其他任务目录。
