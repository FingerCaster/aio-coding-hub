# 研究范围分类与现有 CI 合同

## Goal

形成实现所需的可追溯证据：当前 `ci.yml` 的真实 job/检查合同、可安全降级的文档路径、参考提交的设计与后续修正、以及 `origin` required checks 现状。

## Requirements

- 当前仓库是实现真相；参考仓库提交 `80c4cbd5` 与 `a92ec8f` 仅只读对照，不 cherry-pick。
- 识别 `checked-docs` 可直接运行、无需安装依赖或 Cargo build 的现有 Node 合同。
- 记录分类器对 Git 事件和 name-status 的安全边界，特别是 merge-base、push before/head、rename/copy/delete。
- 通过显式 `FingerCaster/aio-coding-hub` GitHub API 只读查询保护规则，不修改远端。

## Acceptance Criteria

- [x] `research/current-ci-and-reference.md` 记录当前 job、fork 特有检查、参考设计差异和适配建议。
- [x] `research/origin-required-checks.md` 记录 branch protection/ruleset 查询结果与 `ci-gate` 兼容性结论。
- [x] 研究结论足以驱动 implementation 子任务的 PRD/design/implement。
