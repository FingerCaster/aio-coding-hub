# 修复 Sub2API 跨周期余额刷新

## Goal

当 Sub2API Plus/订阅供应商的日、周或月用量窗口已经跨期，但 `/v1/usage`
仍返回上一周期的零额度时，用户单击账户用量刷新即可得到窗口维护后的权威余额，
不再要求先执行 Provider 可用性测试或真实模型请求。

## Background

- 当前前端手动刷新已经绕过 TanStack Query 的 fresh cache，并由 Rust runtime
  的 force epoch 等待本次强刷或唯一尾随强刷；这解决了旧请求逆序覆盖和在途请求
  合并问题，但不改变供应商服务端状态。
- 当前所有内置账户用量 GET 都携带 `Cache-Control: no-cache, no-store` 和
  `Pragma: no-cache`。现场使用禁缓存头和唯一查询参数仍连续得到零额度，排除了
  HTTP/CDN 缓存作为本次根因。
- Sub2API 的 `/v1/usage` 会读取订阅记录，但鉴权中间件对该端点设置
  `skipBilling`，因此不执行 `ValidateAndCheckLimits -> EnsureWindowMaintenance`；
  普通模型请求会执行该维护。现场时序证明一次模型请求之后 `/v1/usage` 立即从零
  恢复为正额度。
- 缺少 `model` 的 `POST /v1/chat/completions` 会经过鉴权和订阅窗口维护，随后在
  参数校验阶段返回 HTTP 400；验证期间供应商请求数、账面成本和实际成本均未增加。
- 2026-07 的 query completion ordering、2026-08-08 的 runtime force epoch 和
  2026-08-13 的 HTTP cache-busting 都是有效的独立修复，但没有覆盖这个服务端窗口
  维护前置条件。

## Requirements

1. 手动刷新 Sub2API 时先执行现有权威 `/v1/usage` GET；不得无条件发送维护预检。
2. 只有原始 JSON 同时严格证明以下事实时，才允许一次维护预检：
   `mode == "unrestricted"`、`isValid == true`、`subscription` 是对象、
   `remaining <= 0`，且日/周/月至少一个正额度周期满足 `used >= limit`。
3. 维护预检必须是带同一 Bearer 凭据、JSON body 严格为 `{}` 且不含 `model` 的
   `POST /v1/chat/completions`。每次手动强刷最多增加一次预检。
4. 仅当预检返回预期 HTTP 400 时，才再次 GET `/v1/usage` 并返回第二次权威结果；
   网络失败或其他状态必须失败关闭，不伪造余额恢复，也不继续第二次 GET。
5. 自动初始查询、定时刷新和 Gateway 后台刷新不得发送预检。钱包零余额、
   `quota_limited` API Key 总额度耗尽、认证失败、异常或不完整响应也不得发送预检。
6. NewAPI billing/account 和 Custom adapter 的网络协议与结果解析保持不变。
7. 手动刷新不得调用 Provider 可用性测试，不得重置 circuit，不得改变 Provider
   启停/顺序、Session、路由候选或其他查询缓存。现有 force epoch、generation、
   尾随强刷和 Provider 隔离语义必须保持。
8. 不记录或暴露 API Key、真实主机、上游响应正文、账户标识或真实账户金额；
   新增失败路径沿用现有脱敏和有界超时策略。

## Acceptance Criteria

- [x] 自动化 HTTP 回归证明手动刷新严格执行
  `GET zero subscription -> POST {} 400 -> GET positive`，最终返回第二次正额度结果。
- [x] 回归断言预检 JSON 是空对象且不存在 `model`，Authorization 仅发往由同一
  已验证 Base URL 派生的 `/v1/chat/completions`。
- [x] 自动/定时刷新只执行一次 GET；钱包零余额和 `quota_limited` 零额度只执行
  GET，不产生 POST。
- [x] 预检返回非 400 或失败时保留首次零额度权威结果，且不执行第二次 GET。
- [x] Runtime 测试证明普通调度传递 `Background`、force epoch 调度传递 `Manual`，
  在途自动请求之后的手动尾随强刷仍只合并为一次。
- [x] 既有 force epoch、逆序完成、Provider generation/config token、route projection、
  adapter 隔离、cache-busting 和前端 query/UI 回归保持通过。
- [x] Rust focused/full tests、Clippy、Rust 格式化、TypeScript typecheck/lint、生成绑定
  检查和 `git diff --check` 通过。

## Out Of Scope

- 修改 Sub2API 服务端 `/v1/usage` 或其订阅维护实现。
- 在自动刷新中主动维护远端窗口，或把维护预检扩展到 NewAPI/Custom。
- 改变余额计算、route gate 判定、Provider 可用性测试或真实模型探测语义。
- 通过真实计费请求制造或耗尽额度来复现跨期状态。
