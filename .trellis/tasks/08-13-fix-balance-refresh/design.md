# 余额刷新修复设计

## 边界与数据流

手动按钮调用 `refreshProviderAccountUsage(queryClient, providerId)`，先取消同一 Provider 的 exact TanStack Query key，再通过 `provider_account_usage_refresh` IPC 进入 Rust runtime。IPC 重新加载当前 target；runtime 负责 generation、in-flight 合并、force epoch、远端 fetch、显示 snapshot 和 route projection 的提交。React 只消费 query cache，不直接写缓存或测试可用性。

## 诊断假设

重点验证以下边界，而不是先修改 UI：

- force 请求是否在 fresh `zero_balance` snapshot 上仍会启动远端 fetch；
- 旧请求在途时，手动请求是否等待尾随 force，而非任意 completion notification 或旧 snapshot；
- scheduler 是否在 force pending/in-flight 且 desktop/gateway lease 不存在时保持活跃，并在旧请求完成后及时派发尾随请求；
- target config token/generation 替换时，旧完成是否会错误提交或唤醒为旧结果；
- IPC target 加载失败/无 target 分支是否绕过 runtime，造成与正常 target 路径不同的缓存语义。

本次复核确认上述本地生命周期契约已由当前分支的 force epoch、尾随强刷和
query 回归覆盖。剩余缺口在远端 HTTP 边界：sub2api、NewAPI billing/account
以及 custom adapter 的实际账户用量 GET 没有声明缓存绕过，代理或 CDN 可以在
每次新 IPC 请求时仍返回旧的零余额响应。Provider 可用性测试是独立探针，不能
作为刷新前置条件；它偶尔改变上游请求时序只会掩盖该缺口。

## 设计原则

1. 以共享 runtime 为唯一远端查询和提交所有者；不增加 availability-test 或 circuit side effect。
2. 每次合并后的显式刷新使用 checked monotonic force identity；普通后台完成只能发通知，不能满足手动 waiter。
3. 结果提交顺序固定为 target identity 校验、display snapshot、route projection、force completion notification；waiter 在同一锁下校验 generation/token 并克隆结果。
4. 所有账户用量 HTTP 请求在发送前统一设置 `Cache-Control: no-cache, no-store`
   与 `Pragma: no-cache`。RequestBuilder、已构造 Request、内置 sub2api/NewAPI
   和 custom adapter 必须全部经过共享 helper；不得依赖 availability-test 触发缓存失效。
5. 若复现显示 runtime 已满足上述契约，则沿 IPC 与前端 query 继续定位，不保留未经测试证明的状态机改动。

## 兼容与回滚

不修改数据库 schema、IPC 参数或适配器协议。修复可通过回滚账户用量请求边界提交恢复；现有缓存无需迁移。
