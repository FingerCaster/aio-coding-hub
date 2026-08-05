# Implementation Plan

## Ordered Work

- [x] 完成当前仓库、参考提交和 `origin` 保护规则研究，形成持久化证据。
- [x] 规划并实现 fail-closed 分类器、机器策略、自测及 package 命令。
- [x] 基于当前 `ci.yml` 接入三档路由与固定 `ci-gate`，保留 fork 全量检查。
- [x] 增加 cross-layer CI 范围合同并更新 spec index。
- [ ] 执行分类器、合同、workflow、前端、Rust 验证与全量自审。
- [ ] 提交工作变更，归档任务并记录 session；更新 Orca 状态但不 push/merge。

## Review Gates

- 写代码前完成 `trellis-before-dev` 所要求的 task artifacts、spec index、shared guides 与研究资料读取。
- 实现后由 `trellis-check` 全范围复核 PRD/design/implement 与所有受影响 package spec。
- 提交前确认差异只属于本任务、基线未漂移、worktree 无不识别修改。
