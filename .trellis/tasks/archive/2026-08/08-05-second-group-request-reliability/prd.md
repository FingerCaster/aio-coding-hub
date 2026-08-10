# 第二组请求可靠性与 Codex 压缩体适配

## Goal

在独立 worktree 中完成两项已确认的请求可靠性修复：补齐现有传输错误重试退避，并参考 `KNaiFen/aio-coding-hub` 为 Codex 压缩 JSON 请求接入有界多编码规范化；最终通过质量门禁后合并到 `main`。

## Requirements

- R1：传输错误子任务只修复已有 `UpstreamRetryPolicy.backoff_ms` 在 transport retry 路径未生效的问题，不新增 UI 或改动策略配置模型。
- R2：Codex 压缩请求子任务选择性移植 `KNaiFen/aio-coding-hub@5b13683b` 的 `13a3c6f` 与 `909b7a0` 最终合同，支持 gzip/deflate/br/zstd 及有界堆叠解码，接通 BodyReader、插件、指纹、reasoning/session 和 identity 上游转发；不整仓合并。
- R3：两项修复均有针对性自动化测试，覆盖正常、变更、失败/超限和旧行为回归；必须保持请求体有界和敏感数据边界。
- R4：保留当前 fork-specific 路由、Provider、Codex 和日志语义；不借本组修复第一组之外的 UI 展示或产品行为。

## Acceptance Criteria

- [ ] 传输错误配置重试在所有 RetrySameProvider transport 路径应用 backoff_ms，现有预算/切换/熔断行为不变。
- [ ] 压缩 Codex 请求可解析并记录实际 reasoning/session 字段，目标端点以明文上游转发，失败路径有界且在零上游尝试时 fail-closed。
- [ ] 参考 fork 对比记录固定来源提交和最新合同；集成只包含本组必要的选择性移植。
- [ ] 受影响 Rust 测试、fmt/check/lint、前端类型/绑定检查和最终质量审查通过。
- [ ] 两个子任务先在独立 worktree 完成并验证，再以非破坏方式合并到本地 `main`，保留用户已有工作树改动。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
