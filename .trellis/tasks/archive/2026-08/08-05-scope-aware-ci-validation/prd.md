# 验证范围 CI 与保护规则兼容性

## Goal

证明三档分类、workflow job 合同和现有 full CI 均保持正确，并通过全范围自审消除静默跳过或范围误降级风险。

## Requirements

- 运行分类器自测与 package 入口，确认所有要求场景。
- 运行三条 checked-docs Node 合同和 fork upstream 同步策略自测/检查。
- 静态验证 YAML 可解析、job/needs/if/output/gate 语义；必须运行无依赖 workflow contract selftest，能用 actionlint 则追加运行，否则记录不可用原因与替代验证。
- 安装冻结依赖并按当前 workflow 强度运行 support、frontend 全部等价检查。
- 按当前 workflow 强度运行 Rust fmt、lock 漂移、clippy、test、audit 的可行等价验证；平台限制或基线问题必须如实记录，不能降低参数绕过。
- 全量复核受影响 package spec、PRD/design/implement 与 git diff，修复发现后重新运行相关检查。
- 复核 `origin/main` branch protection/ruleset 只读结果，绝不修改 GitHub 设置。

## Acceptance Criteria

- [x] 针对性 Node 与 workflow 静态验证通过。
- [x] frontend 与 Rust CI 等价验证通过，或有明确、可复现且未被掩盖的环境/基线残余风险。
- [x] 自审确认 full 保留全部当前步骤，非 full 只跳过被 gate 断言的重任务。
- [x] required checks 兼容性报告与最终交付一致。
