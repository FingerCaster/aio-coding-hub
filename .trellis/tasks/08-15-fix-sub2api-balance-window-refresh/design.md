# Sub2API 跨周期余额刷新设计

## Boundary

修复位于 Rust 账户用量 HTTP 适配器边界。前端仍通过同一 TanStack Query key 调用
`provider_account_usage_refresh`，Runtime 仍拥有 single-flight、force epoch、缓存和
route projection。唯一新增的跨层信息是内部 fetch intent：后台调度与用户强刷在
进入 Sub2API 网络函数前被显式区分。

不新增 IPC 字段，不修改生成 TypeScript binding，也不让 React 知道维护预检。

## Root Cause Chain

```text
manual refresh
  -> TanStack forced query
  -> provider_account_usage_refresh
  -> Runtime force epoch
  -> GET /v1/usage
  -> Sub2API skipBilling loads stale subscription window without maintenance
  -> remaining = 0

provider availability/model request
  -> authenticated model endpoint
  -> ValidateAndCheckLimits
  -> EnsureWindowMaintenance
  -> request validation/model handling
  -> later GET /v1/usage observes refreshed window
```

禁缓存只能保证重新执行 GET，不能让该 GET 执行被 Sub2API 明确跳过的服务端维护。

## Internal Contract

在账户用量 domain 定义非序列化内部枚举：

```rust
enum ProviderAccountUsageFetchIntent {
    Background,
    Manual,
}
```

Runtime 根据 `force_epoch.is_some()` 选择 intent。初始/定时/Gateway 调度没有 force
epoch，因此为 `Background`；用户手动刷新创建或等待 force epoch，因此对应真正执行
该 epoch 的立即或尾随请求为 `Manual`。`provider_account_usage_refresh` 在没有 runtime
target 的兼容回退路径也显式传 `Manual`。

`fetch_account_usage_uncached` 把 intent 只交给 Sub2API 分支；NewAPI 和 Custom 不读取它。

## Strict Trigger Predicate

对第一次 `/v1/usage` 的原始 JSON 在解析为显示 DTO 之前计算：

```text
intent == Manual
AND mode is exactly "unrestricted"
AND isValid is exactly true
AND subscription is an object
AND remaining is finite and <= 0
AND any of:
    daily_limit_usd > 0 AND daily_usage_usd >= daily_limit_usd
    weekly_limit_usd > 0 AND weekly_usage_usd >= weekly_limit_usd
    monthly_limit_usd > 0 AND monthly_usage_usd >= monthly_limit_usd
```

数字兼容现有 Sub2API parser 支持的有限 JSON number/numeric string；负 usage、零/负
limit、缺字段、非有限值和未知结构全部返回 false。钱包响应没有 `subscription`，
API Key 限额响应的 mode 是 `quota_limited`，因此不会触发。

## HTTP Flow

1. 使用现有带 no-cache/no-store 的 builder GET `/v1/usage` 并进行有界读取/JSON 解析。
2. 正常解析第一次结果；若 intent 或严格谓词不满足，立即返回。
3. 从已经规范化并验证过的 usage URL 同源派生 `/v1/chat/completions`，保持相同路径前缀。
4. 发送一次 Bearer-authenticated JSON `{}` POST。body 没有 `model`，不能构成有效模型请求。
5. 仅 HTTP 400 表示鉴权/订阅维护已通过并在模型参数校验处终止；其他状态或网络错误
   返回第一次结果，不读取或传播预检响应正文。
6. HTTP 400 后再次使用同一 cache-busting GET。第二次网络、状态、body 或 JSON 失败
   按现有账户用量错误映射返回；成功则解析并返回第二次结果。

每个底层手动 force fetch 最多是 GET + POST + GET。并发手动点击仍由 runtime force
epoch 合并，不在 HTTP helper 内另建缓存或锁。

## Compatibility And Side Effects

- 预检直接访问已配置的同源 Sub2API，不经过 AIO Gateway provider router，因此不写
  circuit、availability、attempt、session 或 provider order 状态。
- 不调用现有 Provider 测试 command；测试按钮与账户余额仍是独立产品行为。
- 首次响应不满足严格谓词时，字节级请求数量与现状一致。
- 预检非 400 时保留第一次结果，避免把未知服务行为解释为额度恢复。
- 不读取预检 body，避免把供应商错误正文引入日志或 UI。

## Test Design

- domain 表驱动测试覆盖严格谓词的每个必要条件、三种周期和 malformed/negative case。
- command HTTP server 测试捕获 method/path/headers/body，验证三步成功序列和最终结果。
- command 测试验证 Background、钱包零余额、quota-limited、非 400 均无多余请求。
- runtime fetcher 记录 intent，验证自动 Background、手动/尾随 Manual 及现有合并语义。
- 运行现有账户用量 domain/runtime/query/UI 套件，确认共享行为无回归。

## Rollback

该改动无存储 schema、配置或 IPC 迁移。若外部兼容性出现问题，可整体回滚 intent、
严格谓词和 Sub2API 三步 helper；现有单 GET 行为及缓存/route runtime 不需要数据修复。
