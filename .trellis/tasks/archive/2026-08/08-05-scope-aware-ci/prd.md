# 按变更范围分级 CI

## Goal

在保持当前 fork 全量 CI 检查集合与失败语义不变的前提下，根据可信的变更范围把 CI 分为 `process-docs`、`checked-docs`、`full` 三档，减少纯流程记录和纯文档变更的无效重任务，同时让任何未知或错误状态安全回退到 `full`。

## Requirements

- 从本地 `main` 提交 `12e565c0` 建立并只在 `FingerCaster/scope-aware-ci` worktree/分支实施；不 push、不合并、不修改远端配置。
- 新增机器可读范围策略、无第三方依赖 Node 分类器和充分的分类器自测。
- PR 使用 merge-base 范围；push 使用事件 `before`/`after`（或 head）范围；以 NUL 分隔解析 `git diff --name-status -z`，正确处理 rename/copy/delete。
- 未知、混合、空差异、畸形输入、Git/IO/策略错误、手动与不支持事件全部失败闭合为 `full`。
- `.github/**`、分类器、分类器自测和 CI 策略自身属于硬编码控制面，永远为 `full`，不能被策略降级。
- `process-docs` 仅覆盖 `AGENTS.md`、Trellis task/workspace 等纯流程记录；`checked-docs` 仅覆盖 README、docs 和纯文档 spec。机器合同、源码、配置、脚本、锁文件、构建/发布文件均为 `full`。
- 保留 push/pull_request 对 `dev`/`main` 的触发，以及现有 `pr-title`、`support-contract`、`desktop-support-contract`、`frontend`、`rust` 和 fork 人工审查同步策略检查。
- `full` 必须运行当前所有检查；非全量档只跳过明确的重任务。`checked-docs` 只运行本仓库真实存在且不需要 `pnpm install`/Cargo build 的文档合同。
- 固定名 `ci-gate` 使用 `if: always()`，验证分类输出、被选任务成功、未选任务确实 skipped，并纳入 desktop matrix 结果合同。
- 分类器或 workflow 变更必须触发分类器自测；补充 cross-layer CI 变更范围合同并登记到 spec index。
- 只读检查 `origin` 分支保护/ruleset required checks，并在研究资料与最终报告记录兼容性结论。

## Acceptance Criteria

- [x] 分类器自测覆盖 exact/prefix/extension、unknown/mixed、控制面、rename/copy/delete、PR/push range、empty/malformed/error/manual fail-closed。
- [x] workflow 静态语义验证通过，并以 actionlint 1.7.12 复核目标 workflow。
- [x] 当前仓库相关 Node 合同检查、完整前端 CI 等价检查和 Rust CI 等价检查按风险执行且不降低强度。
- [x] `ci-gate` 对每种分类逐一断言 selected/success 与 unselected/skipped，desktop matrix 不会静默丢失。
- [x] `origin` required checks 兼容性结论已记录，且未修改 GitHub 设置。
- [x] 自审问题已修复并复测；实现、Trellis 任务/归档/日志已提交，worktree 干净。

## Task Map

- `08-05-scope-aware-ci-research`：研究当前 CI、参考提交、仓库合同与远端保护状态。
- `08-05-scope-aware-ci-implementation`：实现分类器、策略、CI 路由和 cross-layer spec。
- `08-05-scope-aware-ci-validation`：执行静态/动态验证、全量自审和兼容性复核。

## Out Of Scope

- 修改 `origin`/`upstream`、GitHub branch protection/ruleset 或 required checks。
- 引入参考仓库不存在于当前仓库的 TUI/候选发布检查。
- 改变现有全量 CI 检查强度、产品行为或发布流程。
