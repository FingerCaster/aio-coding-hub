# 发布自启动修复版本

## Goal

将已经完成本地 MSI 验证的 `main` 推送到 `origin`，通过仓库现有的
release-please 双阶段流程发布下一补丁版本 `0.60.29`，并验证发布构建绑定不可变
提交 SHA、Release 已公开且支持矩阵资产完整。

## Background

- 当前本地 `main` 为 `90a0cd773458409ab428d768e33963e7cb4f969b`，相对
  `origin/main` 领先 18 个提交；最新已发布版本为 `0.60.28`。
- 自启动设置修复已合并到本地 `main`，本地 MSI 已由用户验证通过。
- 发布前复现到 `pnpm tauri:clippy` 在
  `src-tauri/src/gateway/proxy/handler/failover_loop/response/upstream_error.rs:45`
  因 8 个函数参数触发 `clippy::too_many_arguments`。该回归由尚未推送的
  `45d01dd7` 引入，必须在推送前修复。
- `.github/workflows/release.yml` 仅由 `workflow_dispatch` 触发：第一次创建或更新
  release-please PR，合并该 PR 后第二次创建 Release 并构建资产。
- 首次推送 `4f2621ab` 后，GitHub Actions run `29697960436` 证明 Windows 本地
  Clippy 未覆盖 Linux `cfg` 分支。Linux `cargo clippy --all-targets --locked --
  -D warnings` 另报告 8 个发布阻塞：3 个 native dialog `unused_mut`、3 个
  `while_let_on_iterator`、一个 Unix 未使用的 task-handle wrapper，以及一个仅被
  Windows 测试使用的 setter。

## Requirements

1. 仅操作 `origin` / `FingerCaster/aio-coding-hub`；不得读取、合并或推送
   `upstream`。
2. 在独立 worktree 中以最小私有参数对象消除 `upstream_error_decision` Clippy
   失败；不得添加
   `#[allow(clippy::too_many_arguments)]`，不得改变瞬时错误规则匹配、重试预算、
   failover 决策或熔断计数语义。
3. 消除 run `29697960436` 的 8 个 Linux 专属 lint，只允许不可变 shadowing、
   等价 `for` 迭代、删除平台内确实未调用的 wrapper，以及收窄测试 helper 的
   `cfg`；不得改变 native dialog、Image Gen 安全删除/统计或 Skill 导出行为。
4. 修复必须通过 Rust 格式、聚焦测试、Windows Clippy、Cargo check、Linux
   Clippy 以及与改动风险相称的完整发布前门禁，之后才可继续发布。
5. 推送后必须等待 `main` CI 成功；不得在失败或未完成状态下启动发布流程。
6. 第一次发布 workflow 必须生成 `0.60.29` release PR。等待 Cargo.lock 同步和 PR
   CI 成功后，审查版本文件、Changelog 与 diff，再合并 PR。
7. 合并 release PR 后仅以 fast-forward 更新本地 `main`，第二次运行发布 workflow。
   下游构建必须使用 release job 解析出的不可变提交 SHA，而非只按尚不可获取的
   draft tag checkout。
8. 持续监控所有平台构建、`latest.json`、公开发布与 Homebrew job，直到 workflow
   完成；不得在 Actions 仍运行时提前报告完成。
9. 保留主工作区现有未提交文件，不暂存、不提交、不还原、不删除这些用户改动。

## Out Of Scope

- 任何 `upstream` 同步、漂移修复或上游缺陷修复。
- 除已确认 Windows/Linux Clippy 发布阻塞外的重构、行为调整或新功能。
- 修改瞬时错误重试规则已经确认的产品语义。

## Acceptance Criteria

- [x] `pnpm tauri:clippy`、聚焦 Rust 测试、`pnpm tauri:fmt`、
      `pnpm tauri:check` 和最终发布前门禁全部通过。
- [x] Clippy 修复只重组 `upstream_error_decision` 输入，现有决策测试保持通过且
      不使用 lint 例外。
- [x] run `29697960436` 的 8 个 Linux lint 全部消失；Linux Clippy 通过，且相关
      Image Gen、Provider Share 与 config-migration 测试保持通过。
- [x] 修复提交通过独立 worktree 合并到本地 `main`，随后仅推送到 `origin/main`；
      主工作区用户改动保持原样。
- [x] `main` CI 与 release PR CI 均成功，release PR 将版本从 `0.60.28` 更新为
      `0.60.29`，Cargo.lock 已同步且 Changelog 内容与提交范围一致。
- [x] tag `aio-coding-hub-v0.60.29` 指向 release PR 合并提交的确切不可变 SHA。
- [x] GitHub Release `aio-coding-hub-v0.60.29` 非 draft，`latest.json` 及支持矩阵
      要求的全部资产存在，发布 workflow 所有必需 job 成功。
- [x] 发布任务归档和开发日志记录完成，其 bookkeeping 提交最终推送到
      `origin/main`。

## Completion Evidence

- 修复提交：`4f2621ab`、`495e9d1b`。
- release PR：`#15`，squash commit
  `76fbdea5ec31788136332a08170bf5feedbe2523`。
- `main` CI：run `29700170426` 与 `29701745592` 均成功；release PR CI：run
  `29701005098` 成功。
- 正式发布：run `29701767005` 成功，tag `aio-coding-hub-v0.60.29` 直接指向
  `76fbdea5ec31788136332a08170bf5feedbe2523`。
- Release 已公开且非 prerelease，共 24 个非空、带 SHA-256 的 uploaded 资产；
  14 个矩阵必需资产齐全，`latest.json` 覆盖 4 个官方 updater 平台。
- Homebrew Cask 生成 job 成功；因仓库未配置 `HOMEBREW_TAP_TOKEN`，tap 同步步骤按
  workflow 设计跳过。
