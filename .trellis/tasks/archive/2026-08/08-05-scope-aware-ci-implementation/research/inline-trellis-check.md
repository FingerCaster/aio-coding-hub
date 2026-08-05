# Inline Trellis Check

## Scope

按父任务与 implementation 子任务的 PRD/design/implement、cross-layer index、CI change-scope contract 和 shared guides 对全部实现差异做独立复核。nested delegation 不可用后，用户明确授权在当前会话 inline 完成 Trellis check。

## Findings And Fixes

1. **Mixed tiers were not fail-closed.** 初版沿用参考实现，把 `process-docs + checked-docs` 聚合为 `checked-docs`，违反用户“混合必须 full”的明确要求。现改为任意跨档集合都返回 `full`/`mixed-tiers`；包含 checked 文档时保留 `docs_checks=true`。
2. **Workflow structure coverage was incomplete.** 初版 selftest 覆盖 outputs、needs/if 与 gate wiring，但没有固定触发、docs 禁止重依赖和现有 full 命令清单。现已加入 push/PR dev/main、权限/并发、support/desktop/frontend/Rust 命令库存与 docs 禁止 pnpm/cargo/TUI/candidate 的断言及故障注入。
3. **PR title used an untrusted inline expression.** actionlint 报告 `github.event.pull_request.title` 直接进入 Bash。现通过 `PR_TITLE` 环境变量传递，并把 wiring 纳入 workflow contract selftest。
4. **Gate matrix needed executable ownership.** 将内联 Bash 判定抽为无依赖 `scripts/ci-gate.mjs`；workflow contract selftest 直接调用同一 helper，覆盖 PR/push 下 process/checked/full（含 full mixed docs）以及 selected/unselected、failure/cancelled/missing 结果。

## Targeted Verification

- `pnpm run check:ci-change-scope`：通过。
- `node --check`：分类器、自测、gate helper、workflow contract selftest 通过。
- 三条 checked-docs Node 合同：通过。
- upstream 人工审查同步策略 selftest/实际检查：通过。
- `git diff --check`：通过。
- actionlint：`rhysd/actionlint` 1.7.12，镜像 digest `sha256:b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667`；命令如下，对 `.github/workflows/ci.yml` 通过：

  `docker run --rm -v "<repo>:/repo" -w /repo rhysd/actionlint@sha256:b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667 -color .github/workflows/ci.yml`

全仓首次运行另报告未修改的 `.github/workflows/release.yml` 既有 ShellCheck `SC2129` 样式告警；本任务目标文件单独复跑为 0。

## Result

当前实现未发现剩余的需求或 spec 偏差。完整 frontend/Rust 等价验证由 validation 子任务继续执行。
