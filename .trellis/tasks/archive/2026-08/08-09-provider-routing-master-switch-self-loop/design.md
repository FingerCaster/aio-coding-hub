# 技术设计

## 边界

1. `domain/providers/queries.rs` 负责数据库候选集合的全局 enabled 过滤。
2. `gateway/proxy/handler/failover_loop` 负责 Provider gate 后、transport commit 前的动态 enabled 与 target 校验。
3. HTTP target validator 只负责地址安全判断，不拥有 Provider、circuit 或 Session 状态。

## 数据流

`route_read` → enabled candidate projection → common provider gates → target URL/DNS validator → transport send。动态 enabled 检查必须位于每次 retry 发送边界，不能只在初次选择时检查。自环拒绝走现有 attempt error 分类，但不得调用普通 upstream failure 记账。

## 兼容性与回滚

- 保留 Provider 专用路由和强制 Provider Header 注入；只在其内部经过同一安全检查。
- 若 DNS 解析或跨平台地址判断无法在当前 target 抽象中安全落地，阻断该子任务，不降级成无界或 fail-open 检查。
- 回滚点为本子任务提交；不回滚其他 worktree 的提交。

## 验证

覆盖 query、provider gate、retry send、target validator、Claude Terminal route 和跨请求 circuit/session 状态。
