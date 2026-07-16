# 修复账户余额手动刷新被旧查询覆盖

## Goal

让一次手动刷新成为同一 provider 账户用量 query 的最新权威结果；首次自动查询、定时刷新
或其他较旧请求不得在其后覆盖缓存，也不需要先运行供应商可用性测试。

## Background

### User observation

- 页面曾显示余额 0；上游余额恢复后连续点击手动刷新仍显示 0。
- 先运行一次供应商可用性测试并成功，再手动刷新会显示新值；等待自动刷新也会更新。

### Verified fact

- 自动查询和定时刷新由 `useQuery` 管理；手动刷新绕过 query fetch 生命周期，直接调用
  IPC 后 `setQueryData` 写入相同 key。
- 自动与手动请求可以并发，较旧自动请求完成时会再次写该 query，覆盖较新的手动结果。
- 可用性测试成功只 invalidates circuit query，不触碰账户 query；它与余额没有产品依赖。
- Rust 命令每次创建新的 `reqwest::Client`，没有本地响应缓存。对授权供应商检查时也未
  观察到常见缓存响应头；HTTP 缓存不是已确认根因。
- 详细证据见 `research/manual-refresh-race.md`。

## Requirements

### R1. One query owner

- 自动、定时和手动账户请求必须复用同一 query key、query function/options 和缓存写入
  机制。
- 手动刷新先取消同键旧 query，再强制执行一次新 fetch；不能因 `staleTime: Infinity`
  直接返回缓存。
- 被取消但底层 IPC 无法物理中止的旧 Promise，其结果不得再提交到 query cache。

### R2. Deterministic concurrency

- 旧自动请求先开始、手动请求后开始、手动先完成、旧自动最后完成时，最终可见值必须是
  手动结果。
- 多个入口同时触发时不得产生无法解释的最后写入；同键请求要么 coalesce，要么遵循明确
  的取消后重取语义。
- loading/error 状态必须与实际 query 生命周期一致，刷新按钮不得在请求结束前错误恢复。

### R3. Display-only isolation

- 手动刷新不得调用供应商可用性测试、circuit reset、provider upsert、enable/disable、
  reorder 或任何 gateway health 操作。
- 成功或失败只更新该 provider 的账户用量 query；不得清理其他 provider 的缓存。

### R4. Dependency

- 只有子任务 1 已验收、提交并归档后才可启动本任务。
- 本任务完成并归档后才可启动 NewAPI 子任务 3。
- 本任务不得访问项目 remote `upstream`。

## Acceptance Criteria

- [ ] deferred-response 测试证明较旧自动响应在手动响应后完成时不能覆盖手动值。
- [ ] 手动点击总会触发一次新的 IPC 请求，不受现有 fresh/Infinity cache 影响。
- [ ] 首次自动、定时与手动入口共享同一 query options 和 key。
- [ ] 连续点击/多组件入口不会产生重复的非受控缓存写入。
- [ ] 无需运行供应商可用性测试即可看到手动请求返回的新值。
- [ ] `gatewayCircuitResetProvider`、provider availability 和其他 provider cache 没有被调用或
      修改。
- [ ] 现有自动刷新 interval、disabled provider、provider edit/delete cache cleanup 测试
      保持通过。

## Out of Scope

- 修改上游服务的余额刷新周期或数据一致性。
- 为未经证据支持的 HTTP cache 添加随机 cache-busting 参数。
- 修改 NewAPI 字段/端点解析；该工作属于子任务 3。
- 让账户余额参与路由、熔断、启停、排序或健康。
