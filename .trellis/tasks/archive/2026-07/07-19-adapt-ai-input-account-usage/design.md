# 技术设计：兼容账户额度多种查询方式

## 设计目标

在不改变供应商路由和现有查询缓存所有权的前提下，增加两类协议兼容：

1. NewAPI 由用户显式选择模型令牌 billing 或用户账户余额。
2. sub2api 识别已确认的 `rate_limits[].window == "1d"` 日额度。

这两个交付共用账户用量配置、IPC、结果 DTO、展示和安全审计，继续作为一个独立任务实现，不拆父子任务。

## 配置与持久化边界

### 非敏感配置

`core.provider-account-usage/accountUsage` 扩展值继续拥有：

- `adapterKind`: `disabled | sub2api | newapi`
- `newApiQueryMode`: `billing | account`
- `timedRefreshEnabled`
- `refreshIntervalSeconds`

旧 NewAPI 扩展缺少 `newApiQueryMode` 时规范化为 `billing`。历史 `newApiUserId` 不再作为模式选择信号。

内置账户用量扩展通过一个后端规范化函数重建允许的非敏感字段。普通保存、数据库迁移、完整配置导入导出及单供应商分享导入导出都复用该函数，历史 `newApiUserId`、账户令牌及其兼容别名不得再次进入 `values_json`。

### 账户凭据

新增 `provider_account_usage_credentials` 表，以 `provider_id` 为主键并通过外键级联删除，字段只保存：

- 规范化的正整数十进制 `newapi_user_id`
- `newapi_access_token_plaintext`
- `updated_at`

User ID 使用规范化文本而不是 JavaScript number，避免大整数在生成绑定和表单中损失精度。系统访问令牌设置显式字节上限，空值表示缺失；当 User ID 与令牌均被清除时删除整行。

`ProviderSummary` 只增加规范化 User ID 和 `newapi_account_access_token_configured` 布尔值，不返回令牌。网关供应商结构不读取该表。包含系统访问令牌的补丁、数据库记录和配置备份快照均不得派生 `Debug`。

### 保存补丁语义

`ProviderUpsertInput` 增加可选的账户凭据补丁：

- 外层缺失：保留现有账户凭据，兼容其他调用方。
- User ID 字段：设置或清除 User ID。
- 令牌未指定：保留原令牌。
- 令牌指定为非空值：替换原令牌。
- 令牌显式清空：删除令牌。

编辑器切换 `billing/account`、切换为其他适配器或关闭账户用量只更新扩展配置，不清除凭据。“清除账户凭据”同时清空 User ID 和令牌，并在保存事务提交后生效。账户模式允许凭据不完整地保存。

### 数据库迁移

新增版本化迁移并提升 schema version：

- 创建凭据表及外键。
- 从内置账户用量扩展中读取有效的历史 `newApiUserId`，迁移到凭据表。
- 从扩展 JSON 移除历史 User ID；无效旧值不迁移，也不得出现在日志。
- 新安装 baseline 同步包含新表；迁移可重复执行。

迁移不设置查询模式，因此所有旧 NewAPI 供应商仍按 `billing` 工作。

## 查询数据流

```text
Provider + accountUsage config
  |-- sub2api ---------------------- model API Key -> GET /v1/usage
  |-- NewAPI / billing ------------- model API Key -> status + subscription + usage
  `-- NewAPI / account -- private credentials ------> status + user/self
                                                    -> ProviderAccountUsageResult
                                                    -> existing TanStack Query owner
                                                    -> inline display
```

`provider_account_usage_fetch` 先解析显式模式，再只加载对应凭据。账户模式不得读取或发送模型 API Key；billing 与 sub2api 不得读取或发送系统访问令牌。

## NewAPI 模型令牌模式

保留现有同源 URL 规范化、禁止重定向、独立响应上限、应用错误优先和 USD 公式。

解析 subscription 时按 NewAPI 官方源码定义的精确无限额度哨兵判断，禁止使用“大于某阈值”之类的启发式规则。命中哨兵时返回 `available`，以明确消息显示“模型令牌无限额度”，不返回 total、used、balance 或 unit；有限额度和负余额行为保持不变。

## NewAPI 用户账户模式

### HTTP 契约

从同一规范化部署根派生：

- `GET /api/status`：无认证，读取展示单位和换算基数。
- `GET /api/user/self`：`Authorization: Bearer <system-access-token>` 与 `New-Api-User: <user-id>`。

两次请求使用同一个禁止重定向、固定超时的客户端，并分别限制响应体。凭据不完整时在创建客户端或发送请求前返回本地 `configuration_required`。

### 解析与换算

先检查 HTTP 和根级应用错误，再读取业务字段：

- status 必须给出精确支持的 USD 展示类型和有限正数换算基数。
- user/self 必须成功，并包含与配置一致的精确用户 ID。
- `quota` 允许为有限负数，作为账户余额原始额度。
- `used_quota` 必须为有限非负数，作为历史消耗原始额度。
- `balance = quota / quota_per_unit`
- `used = used_quota / quota_per_unit`
- `total = None`，不得由余额与历史消耗反推总额度。

任何字段、单位、身份或算术失败都返回无部分金额的失败结果。上游 message、响应体、主机和个人字段不得进入错误或日志。

## sub2api `rate_limits`

现有根级余额、套餐剩余、订阅周期和有效状态解析保持优先兼容。额外检查根级 `rate_limits` 数组：

- 只识别精确 `window == "1d"`；未知窗口忽略，不猜测为周/月。
- 一个响应最多允许一个 `1d` 项，重复项按歧义失败。
- `limit`、`used`、`remaining` 必须有限且非负，`used <= limit`，并在小的浮点容差内满足 `limit - used == remaining`。
- `window_start` 与 `reset_at` 必须是有效时间，且重置时间晚于窗口起点。
- 合法项映射为 `daily_used = used`、`daily_total = limit`；remaining 和窗口时间只用于一致性验证，不冒充余额或账户过期时间。
- 已知窗口畸形时整个响应失败关闭；只有未知窗口且根级 `isValid` 可识别时维持“可用”。

## 前端行为

NewAPI 配置区增加分段选择：“模型令牌额度”和“用户账户余额”。账户模式显示：

- User ID 输入框。
- 默认遮蔽的系统访问令牌输入框及可见性图标。
- “已配置，留空不改；输入新值替换”的秘密字段语义。
- 显式清除操作；清除先进入表单草稿，保存后才提交。

在 billing 模式下如仍保存账户凭据，显示简短“已保存”状态和清除入口，但查询不使用它们。凭据不完整允许保存，编辑器和账户用量摘要直接显示“需配置账户凭据”。

账户模式的 `used` 展示为“历史已用”，不与 billing 的周期已用混淆。sub2api 日窗口沿用现有 `日 used/total` 指标。

前端 `providerUpsert` 诊断参数必须同时脱敏模型 API Key 和嵌套账户令牌。保存成功日志只使用 `ProviderSummary` 中的非秘密字段。

## 导出、分享与复制矩阵

| 操作 | 查询模式 | User ID | 系统访问令牌 | 行为 |
| --- | --- | --- | --- | --- |
| 完整配置导出/导入 | 包含 | 包含 | 包含 | 完整往返恢复 |
| 单供应商分享/导入 | 包含 | 不包含 | 不包含 | 导入后明确需配置 |
| 本机复制供应商 | 包含 | 包含 | 包含 | 副本立即保持查询能力 |

完整配置 bundle 升级为 schema v3，并为 Provider 增加可选的账户用量凭据快照。快照保存私有 User ID 和令牌，Provider 原有扩展字段只保存经过清理的内置账户用量配置。升级 schema version 可防止旧应用静默接受并丢弃新增凭据。

导入能力必须显式按版本分支：

- v1：维持没有完整 Skill 载荷、没有账户凭据快照的旧行为。
- v2：继续导入完整 installed/local Skill 载荷，但没有账户凭据快照。
- v3：导入完整 Skill 载荷，并在同一数据库事务中恢复账户凭据快照。

schema 校验接受 v1、v2、v3；Skill 载荷能力以 v2 为最低版本，账户凭据快照能力以 v3 为最低版本，不得用提升后的“当前版本”常量替代这两个独立门槛。v1/v2 导入替换供应商时按既有行为创建不带账户凭据的供应商；v3 对每个供应商按快照恢复或明确保持无凭据。

单供应商 share v1 不增加秘密字段，也不需要升级 schema。导出和导入规范化都必须从内置账户用量扩展中移除历史 `newApiUserId` 或任何账户令牌字段，仅保留模式与刷新设置。导入仍保持禁用、无路由写入和原有预览能力边界。

本机复制在 Rust 后端事务内读取并写入私有凭据；前端不接触明文。

## 兼容与安全

- 查询结果继续只用于展示，不进入路由、熔断、可用性、排序、启停或 OAuth 配额。
- TanStack Query 的自动、定时、手动刷新继续使用同一 provider-scoped key 和 query owner。
- 真实令牌、User ID、主机、响应体、个人字段和账户金额不得进入仓库、任务材料、测试、生成绑定内容、日志或 IPC 错误。
- 测试仅使用合成凭据、保留测试域名和合成金额。
- `src/generated/bindings.ts` 必须由 Rust 生成；生成时保留工作区内来源对应的既有用户改动，不手工覆盖。

## 风险与回滚

- **迁移风险**：旧扩展 User ID 搬迁失败不得阻断不相关供应商；用合成迁移测试覆盖有效、无效和重复执行。
- **配置兼容风险**：schema v3 不得让 v2 Skill 载荷退化为 v1 保留本机状态的分支；用 v1/v2/v3 三版本矩阵覆盖准备、导入和回滚。
- **凭据泄露**：完整备份是唯一允许离开数据库的批量导出路径；分享、摘要、日志和错误均有负向测试。
- **部署差异**：未知 NewAPI 单位、字段或非官方无限值失败关闭，不猜测金额。
- **解析回归**：sub2api 新路径只认 `1d`，现有余额/套餐夹具保持通过。
- 回滚可停止读取新表并保留其数据；新增表和字段是加性的。新版本配置 bundle 由旧应用拒绝，避免静默降级。
