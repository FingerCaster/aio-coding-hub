# 修复余额为空时刷新失效

## Goal

当账户余额先显示为 0 或尚未获取、上游余额随后恢复时，用户点击账户用量刷新按钮应立即发起一次权威远端查询并展示恢复后的余额，不得要求先点击 Provider 可用性测试，也不得等待自动刷新或定时刷新。

## Background

- 当前链路由 React TanStack Query、Tauri `provider_account_usage_refresh` IPC 和 Rust `ProviderAccountUsageRuntimeState` 共同持有查询生命周期。
- 归档任务 `08-08-fix-zero-balance-manual-refresh-regression` 已修复过同类问题，但用户在当前 beta 分支仍观察到“余额没有后刷新无效，测试成功后才恢复”。本任务必须以当前代码和实际时序重新定位，不能假定前端旧竞态仍是唯一根因。
- Provider 可用性测试、熔断器状态、Provider 顺序/启停和账户余额刷新是相互独立的产品行为；测试成功不得成为余额刷新的前置条件。

## Requirements

1. 覆盖余额状态从 `zero_balance`/未获取到正余额的单击刷新路径，并确保调用者等待本次强刷或合并后的唯一尾随强刷结果。
2. fresh 缓存、已有自动/定时请求在途、关闭定时刷新、多个展示消费者和连续手动点击都必须绕过旧快照，且旧完成不得覆盖较新的手动结果。
3. 保持 Provider 级 generation/config token、前端 exact query-key cancellation、共享 runtime 缓存所有权和 route projection 的现有隔离边界；修复应落在所有适配器共享层，除非证据明确限定到某协议。
4. 手动刷新不得调用或依赖 Provider 可用性测试、circuit reset、Provider mutation/reorder、Session 或模型路由副作用。
5. 失败时展示准确的查询失败/配置状态，不伪造余额、不泄漏凭据、上游响应、PII 或敏感错误文本。
6. 覆盖 Sub2API、NewAPI billing、NewAPI account 和 custom adapter 共享刷新路径；不改变各适配器协议解析契约。

## Acceptance Criteria

- [x] 自动化回归证明 `zero_balance -> 上游恢复 -> 单击手动刷新` 发起新的远端查询并展示正余额，无需 Provider 测试或自动刷新；内置请求同时显式绕过 HTTP 共享缓存。
- [x] deferred/in-flight 回归证明旧自动请求晚完成不能覆盖手动结果；在途场景最多产生一次合并尾随强刷，调用者收到尾随结果。
- [x] fresh cache、timed refresh disabled、连续点击、多消费者和 target generation/config token 变化均有可验证行为。
- [x] 测试证明可用性测试、circuit、Provider 列表/顺序/启停、Session 和路由状态未被手动刷新修改。
- [x] 至少一个内置适配器和 custom 适配器通过共享 runtime 测试，前端 query/UI 回归保持通过；sub2api、NewAPI 和 custom 请求均覆盖缓存控制。
- [x] 相关 Rust/TypeScript focused tests、生成绑定检查、格式化、typecheck、lint、Clippy 和 diff 检查通过。

## Out Of Scope

- 不改变账户用量远端协议、余额计算、路由门禁判定规则或 Provider 可用性测试语义。
- 不清理或回滚工作区中与本任务无关的既有用户改动。
