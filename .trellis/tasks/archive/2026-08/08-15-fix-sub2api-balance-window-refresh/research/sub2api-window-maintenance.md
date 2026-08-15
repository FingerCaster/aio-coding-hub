# Sub2API 订阅窗口维护证据

## Observed Sequence

对同一订阅型 API Key 连续执行普通 GET、禁缓存 GET 和带唯一查询参数的 GET，
`/v1/usage` 均返回 `isValid=true` 且剩余额度为零。执行一次正常模型端点鉴权路径后，
下一次 `/v1/usage` 立即返回正额度。因此旧值不是 AIO 本地缓存，也不是 HTTP/CDN
响应复用。

另用订阅型 Key 验证 `POST /v1/chat/completions` body `{}`：服务返回 HTTP 400
`invalid_request_error`，请求数、账面成本和实际成本均未增加。

## Source Evidence

Sub2API `backend/internal/server/middleware/api_key_auth.go`：

- `/v1/usage` 设置 `skipBilling`。
- 订阅记录仍会加载并写入 context。
- `ValidateAndCheckLimits` 和 `EnsureWindowMaintenance` 完全位于 `!skipBilling` 分支。

Sub2API `backend/internal/handler/gateway_handler.go`：

- unrestricted 订阅响应直接用 context 中的订阅 usage 与 group limit 计算 remaining。
- 该 handler 不维护 daily/weekly/monthly window。
- 模型 handler 在鉴权中间件之后校验必填 `model`，缺失时返回 HTTP 400。

## Falsified Hypotheses

- TanStack Query 旧完成覆盖：既有取消/强刷回归覆盖该层，现场独立 HTTP GET 仍为零。
- Rust runtime fresh snapshot：手动 force epoch 已产生新的远端 GET，现场抓取证明如此。
- HTTP/CDN cache：no-cache/no-store、Pragma 和唯一 URL 均不能改变响应。
- Provider 测试本身刷新 AIO 状态：真正改变结果的是它经过了 Sub2API 的非 skipBilling
  鉴权/订阅维护路径。

## Required Prevention

以后声明“手动远端刷新权威”时，必须枚举上游端点是否执行生成该读模型所需的服务端
维护。Mock fetcher 每次返回新值只能证明本地刷新链路，不能证明上游读端点会更新其
状态。
