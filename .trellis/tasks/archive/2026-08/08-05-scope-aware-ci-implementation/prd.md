# 实现范围分类器与分级 CI

## Goal

基于当前 `ci.yml` 实现三档、失败闭合且可审计的 CI 选路，并以固定 `ci-gate` 防止条件 job 意外跳过后静默通过。

## Requirements

- 新增严格 schema 的 `.github/ci-scope.json`，只允许 exact path 与 prefix+extension allowlist。
- `process-docs` 只允许 `AGENTS.md`、`.trellis/tasks/**/*.{md,json,jsonl}`、`.trellis/workspace/**/*.md`。
- `checked-docs` 只允许 `README.md`、`README_EN.md`、`docs/**/*.md`、`.trellis/spec/**/*.md`。
- `.github/**`、分类器、自测、策略与任何新增 gate 脚本必须由代码硬编码为 `full`；路径/策略歧义也必须 `full`。
- 分类器必须无第三方依赖，严格校验策略和仓库相对路径，使用 NUL `name-status`，R/C 同时分类旧/新路径，delete 分类旧路径。
- PR 使用 merge-base；push 使用 `before`/head；空、错误、全零 SHA、手动/未知事件全部 `full`。
- 任意跨档混合变更必须 `full`；若包含 checked 文档，仍设置 `docs_checks=true` 运行目标合同。
- `change-scope` 始终运行分类器自测并导出 `scope`、`full_ci`、`docs_checks`、`reason`。
- `docs-contract` 只运行现有 `check-plugin-system-docs.mjs`、`check-plugin-api-contract.mjs`、`check-spec-links.mjs`，不引用 TUI/候选发布脚本。
- `support-contract`、`desktop-support-contract`、`frontend`、`rust` 仅在 `full` 运行，且 full 下保留当前全部步骤和 fork 人工审查同步策略。
- `ci-gate` 的 id/name 固定为 `ci-gate`，使用 `if: always()`，显式验证分类域、PR title、docs、support、desktop matrix、frontend、rust 的 selected success/unselected skipped。
- 新增无依赖 workflow contract selftest，结构化校验 outputs、needs/if、完整 gate wiring，并对 Actions 实际 gate helper 执行三档 fail-closed 结果矩阵。
- 增加 package script、cross-layer 合同和 index 条目。

## Acceptance Criteria

- [x] 分类器与自测实现所有要求场景并在 Node 22/当前 Node 上通过。
- [x] CI job 图保持当前触发和全量步骤，只新增范围路由、docs job 和最终 gate。
- [x] workflow 或控制面文件变更必然分类为 `full`，且 full 路径运行分类器自测。
- [x] `ci-gate` 纳入 desktop matrix 聚合结果并拒绝任何意外 skip/failure/输出缺失。
- [x] workflow contract selftest 能通过故障注入拒绝输出、条件、gate 依赖/绑定和结果矩阵漂移。
- [x] cross-layer spec 准确记录策略、事件范围、job 选择和 gate 合同。
