# Provider 路由总开关与自环保护

## Goal

在当前 fork 的 Provider 选择和 failover 执行链中补齐全局 enabled 硬门与 Provider 目标自环拒绝，同时保持 fork 已有账户用量门控、熔断/探测回切、Session 绑定、模型路由和 Provider 专用路由。

## Evidence

- 自定义排序查询只检查 `sort_mode_providers.enabled`，未检查 `providers.enabled`：`src-tauri/src/domain/providers/queries.rs:1108-1114`。
- 现有测试固定了禁用 Provider 仍留在自定义候选池的行为：`src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs:315-355`；这是需要更新的产品契约，而不是盲目复制候选测试。
- 当前 attempt executor 有代理递归保护，但没有每次 Provider send 前的 gateway target DNS/URL 自环验证。
- 候选参考：`73a15d6d`、`ecd82606`；不采用候选删除 `/:cli_key/_aio/provider/:provider_id/*path` 的改动。

## Requirements

- `R1`：Provider master switch 对默认路由、自定义路由、Session 复用、source provider 和每次 retry send 一致生效。
- `R2`：Provider 在真正发送前重新读取/验证 enabled 状态；请求中途禁用后不得继续向该 Provider 发起后续 upstream 请求。
- `R3`：自环验证覆盖配置 URL、解析后的地址和 DNS 别名，使用有界解析、缓存和超时；保留现有系统代理递归保护。
- `R4`：自环或禁用均以结构化、可观测的本地失败结束，不污染 circuit、account-usage 或 Session 绑定状态。
- `R5`：Provider 专用路由和 `x-aio-provider-id` 当前产品流程继续可用；鉴权不在本任务内。

## Acceptance Criteria

- [ ] 全局禁用 Provider 不再进入可发送的 custom route 候选；默认/源 Provider 查询语义保持一致。
- [ ] 禁用发生在 retry 之间时，后续 upstream send 数为零，并有回归测试。
- [ ] IPv4/IPv6 loopback、解析到本机监听地址、别名和正常远端目标均有正反测试。
- [ ] self-loop 检查失败不增加 Provider circuit failure、不提交 Session binding、不改变 account-usage gate 结果。
- [ ] Claude Terminal Provider 专用路由测试和现有 failover/account-usage/model-routing 测试通过。
- [ ] 鉴权、整仓候选合并和 Responses cache 未进入差异。
