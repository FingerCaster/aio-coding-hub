# Bug Analysis: Sub2API 手动刷新跨周期零额度

## 1. Root Cause Category

- **Category**: B / D / E - 跨层契约、测试覆盖缺口、隐含上游假设。
- **Specific Cause**: AIO 的手动刷新确实绕过了 TanStack Query、进程快照和 HTTP
  缓存，但 Sub2API Plus 的 `/v1/usage` 鉴权路径设置了 `skipBilling`，不会执行
  `EnsureWindowMaintenance`。跨日、周或月后，读端点因此继续读取上一周期的零额度；
  普通模型端点才会先维护窗口。此前没有把“上游读端点是否负责维护生成读模型所需的
  服务端状态”写成账户用量契约。

## 2. Why Fixes Failed

1. **Query completion ordering 修复**：解决了旧的自动请求晚完成覆盖手动结果，
   但两次请求都从上游读到了同一个未维护窗口。
2. **Runtime force epoch / tail refresh 修复**：证明每次点击确实产生新的远端 GET，
   但新的 GET 仍走 `skipBilling`，没有改变 Sub2API 的窗口状态。
3. **HTTP cache-busting 修复**：增加了 `no-cache` 头和唯一查询尝试；现场连续 GET
   仍为零，说明问题不是 CDN 或本地响应缓存。
4. **Provider 测试路径**：它偶然经过了会维护窗口的模型鉴权链路，因此看起来能恢复，
   但把测试作为刷新前置条件会引入错误的产品副作用，也没有明确表达真正的维护操作。

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | 跨层契约 | 手动强刷必须分别枚举 UI/query、runtime、HTTP 缓存和上游服务状态边界；记录读端点是否执行窗口维护 | DONE |
| P0 | 请求回归 | 对 Sub2API 订阅耗尽原始 JSON 固定验证 `GET -> POST {} 400 -> GET`，以及后台/钱包/API Key 不触发 | DONE |
| P0 | 失败关闭 | 预检仅接受 HTTP 400；网络错误或其他状态保留首次结果，不伪造恢复、不二次 GET | DONE |
| P1 | 类型边界 | Runtime 用 `Manual` / `Background` intent 明确限制预检只能来自用户强刷 | DONE |
| P1 | 原始数据判定 | 触发谓词只读取严格 raw JSON，不使用显示 DTO 状态反向猜测模式 | DONE |
| P1 | 代码审查清单 | 账户用量变更必须审计 route、circuit、availability、provider order 和凭据/响应泄漏 | DONE |

## 4. Systematic Expansion

- **Similar Issues**: 任何“刷新后读模型仍旧”的供应商适配器都可能把缓存、
  本地快照和上游维护混为一谈，尤其是按周期聚合的 billing/usage 端点。
- **Design Improvement**: 将维护性预检放在远端适配器边界，保持 UI、IPC 和路由
  投影无感；未来新增供应商时必须说明读端点与写/维护端点的状态关系。
- **Process Improvement**: 复现时先抓取独立 HTTP 时序，再决定是否为缓存问题；
  不把“Provider 测试成功”当成账户用量刷新正确性的证据。
- **Knowledge Gap**: HTTP `no-cache` 只影响中间缓存，不会让上游跳过的业务维护
  自动发生。

## 5. Knowledge Capture

- [x] 更新账户用量跨层契约，加入 Sub2API 手动维护预检的请求、谓词和失败矩阵。
- [x] 更新跨层思考指南，要求强刷审计上游服务状态边界。
- [x] 增加 domain、HTTP sequence 和 runtime intent 回归测试。
- [x] 同步 `src/templates/markdown/spec/` 中的对应模板。
