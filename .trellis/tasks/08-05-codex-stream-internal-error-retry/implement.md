# 执行计划

1. [x] 读取当前 settings/gateway/stream/logging/share 代码和参考最终迁移，确认当前 schema/DB 版本与 fork 共享 helper。
2. [x] 扩展 retry policy、global guard setting、generated bindings、frontend adapters/validation 和 Provider override/share/backup。
3. [x] 实现 Codex SSE evidence 提取、脱敏、classification 和 meaningful-output 判定单元测试。
4. [x] 在当前 success_event_stream.rs 保留 probe/continuation/fake200 逻辑，逐块接入 1 MiB/guard 状态机与 decoded stream。
5. [x] 将 stream-internal match 接入现有 retry engine/attempt budget/backoff/circuit；补齐 paused-time route tests。
6. [x] 更新 attempt/error-details/request-log projections、ProviderChain、最终错误详情和复制 UI。
7. [x] 处理 400 + capacity 默认规则的全局/Provider 迁移并添加分享、导入、备份回归。
8. [ ] 运行 focused Rust/TS、generated bindings、fmt/typecheck/lint/build；执行完整 cross-layer check。
9. [ ] 修复审查发现并形成一个 feat(gateway) 原子提交；记录未移植参考逻辑和残余风险。

## 高风险回滚点

- success_event_stream.rs：任何 probe/circuit/continuation 回归立即回滚 guard 接入块。
- retry_engine.rs/provider_iterator.rs：预算或 backoff 重复计数时撤出新 match，保留 parser/evidence。
- routes.rs/settings migration：仅回滚迁移/测试，不覆盖当前 failback route。
- usage.rs/日志 projection：只允许丢弃 evidence，绝不扩大原始 body/secret 存储范围。
