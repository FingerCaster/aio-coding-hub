# 修复 NewAPI 账户用量认证与响应契约

## Goal

让 `muyuan` 的 NewAPI 账户展示使用其模型 API Key 可访问、已真实验证的 billing 接口，
正确归一化总额度、已用量、余额、过期时间和单位；应用层错误必须准确 fail closed，不再
误报“缺少 quota 字段”。

## Background

### User observation

供应商 `muyuan` 显示：`账户: 查询失败 · NewAPI 响应缺少 quota 字段`。用户已授权对该
供应商进行后续最小只读验证，但禁止泄露 API Key、完整响应和 PII。

### Verified fact

- 现有 adapter 使用 provider 模型 API Key 请求 `/api/user/self`。真实响应是 HTTP 200、
  `success=false + message` 的认证错误对象，并没有 `data/quota`。
- NewAPI 官方源码证明 `/api/user/self` 需要 user access token 与 `New-Api-User`，模型
  `sk-` token 不是该凭据。当前 parser 没检查 `success`，直接报 missing quota。
- 同一 `muyuan` 的 `/v1/dashboard/billing/subscription` 与 `/v1/dashboard/billing/usage`
  使用模型 token 均返回 HTTP 200，字段形状和余额公式可验证；公开 status 表明单位为 USD。
- `/api/usage/token/` 在该部署返回 HTTP 500，不能作为主实现路径。
- 详细脱敏证据见 `research/newapi-muyuan-readonly-validation.md`。

## Requirements

### R1. Correct NewAPI request contract

- 对 NewAPI 模型 API Key 使用 billing subscription/usage endpoints，不将其当作 user
  access token。
- 根据 subscription 的 payment-method 语义构造兼容日期窗口，并保留 Base URL 带/不带
  `/v1` 的规范化。
- 读取公开 status 的 `quota_display_type`，以真实展示单位解释 billing 数值；未知单位
  必须 fail closed，不能硬标 USD。

### R2. Correct normalized result

- `total = hard_limit_usd`，`used = total_usage / 100`，`balance = total - used`，字段必须为
  有限数字并保持同一展示单位。
- `access_until` 映射到过期时间；zero/expired/available 状态继续使用统一状态函数。
- 不再以固定 500000 divisor 解释 billing endpoint 已转换的值。旧 quota payload helper
  只有在其认证/来源契约明确时才可保留，不能作为未知响应猜测 fallback。

### R3. Fail-closed errors and privacy

- HTTP 2xx 中的 `success=false`、`error` 对象、缺字段、非有限数字和跨 endpoint 不一致均
  返回准确的 auth/query failure，不显示伪造余额。
- 上游 message 只能映射为有限、脱敏的用户提示；API Key、账户标识、原始 body 和 PII
  不得进入日志、IPC、测试 fixture 或 research。
- 每个响应使用明确的读取上限；不能为账户展示引入无界 body buffering。

### R4. Display-only isolation

- 账户请求不得修改 routing、provider enabled/order、circuit/cooldown、availability、OAuth
  quota 或本地请求用量。
- 继续复用子任务 2 的 query-owned refresh，不新建第二套缓存或调度器。

### R5. Dependency

- 只有子任务 2 验收、提交并归档后才可启动本任务。
- 本任务归档后才可启动配置导出子任务 4。
- 本任务不得访问项目 remote `upstream`。

## Acceptance Criteria

- [ ] 真实形状 fixture 中 `success=false` 映射为准确认证失败，不再显示 missing quota。
- [ ] subscription + usage + USD status fixture 归一化为正确 total/used/balance/expiry。
- [ ] 非 USD、未知单位、非有限数字、部分 endpoint 失败和 error object 均按明确规则处理。
- [ ] URL/date/auth/header 测试证明模型 Key 只发送到预期同源 NewAPI endpoint。
- [ ] API Key、PII、完整真实响应和实际账户数值不出现在日志、错误、fixture 或 git diff。
- [ ] fixture 测试通过后，对 `muyuan` 的最小只读验证返回可计算字段形状，且只记录脱敏
      断言。
- [ ] provider routing/circuit/availability 相关 mock 均未被调用，现有 sub2api 行为不回归。

## Out of Scope

- 保存 NewAPI 用户 access token、登录 cookie 或抓取浏览器会话。
- 猜测不同产品的类似 quota/balance 字段。
- 使用该账户数据做路由、健康或自动禁用决策。
- 把 `/api/usage/token/` 的 live 500 当作零余额。
