# 自定义 JavaScript 账户用量查询 - 技术设计

## 1. 设计目标与来源

本任务选择性吸收以下两组已验证设计，而不直接 cherry-pick：

- `c75150897145420d630f9927519493f154032227`：自定义 JavaScript 请求/解析、原生确认、权限指纹、网络限制、本地净化与 UI。
- `3cc35e4920b98f7f29cbe575cfb04c542ec3f95d`：Tauri 进程拥有的 Provider 级缓存、消费者租约、in-flight 合并、并发限制和 generation 提交保护。

当前主线的 Provider 类型、配置迁移、分享协议和 IPC 生成已经继续演进，且旧提交中的 Observer/TUI 模块已不存在。因此按当前模块所有权重放改动并保留现有测试，而不是复制提交快照或复活已删除子系统。

本任务终态仍是“账户用量只用于显示”。它提供后端共享快照读取面，但不修改 Provider resolution、gateway gate、circuit、Session 或路由顺序。

## 2. 分层与数据流

```text
Provider Editor 草稿
  -> 后端结构校验
  -> 原生确认（列出完整 Origin + SHA-256 指纹）
  -> 后端注入一次性 proof
  -> Provider 事务内 sanitizer 验证并持久化派生授权

Desktop / 手动刷新
  -> ProviderAccountUsageRuntimeState
  -> 同 Provider in-flight 合并 + 全局并发限制
  -> fetch_account_usage_uncached
       +-> 内置 sub2api/NewAPI
       +-> custom workflow semaphore
            -> 一次性 QuickJS child: request(ctx placeholders)
            -> Rust materialize + validate + HTTPS request
            -> bounded JSON + exact secret redaction
            -> 一次性 QuickJS child: parse(response)
            -> normalized ProviderAccountUsageResult
  -> generation 校验后提交共享快照
  -> Desktop 镜像
```

## 3. 自定义配置与授权

### 3.1 配置字段

`ProviderAccountUsageAdapterKind` 增加 `Custom`。账户用量扩展的本地持久化字段为：

```text
customScript: string, UTF-8 <= 32 KiB
customAllowedOrigins: normalized HTTPS origins, <= 16, each <= 512 bytes
customTimeoutSeconds: integer 2..15
customEnabled: boolean, default false
customPermissionFingerprint: derived SHA-256
customPermissionBaseOrigin: confirmed normalized Base URL origin
```

`customPermissionProof` 与 `customPermissionBaseOriginProof` 仅是一次保存调用内的后端 proof，Renderer 输入先无条件剔除，数据库、IPC、分享和备份均不得保留。

### 3.2 权限指纹

- 授权指纹覆盖精确脚本字节、排序/去重后的额外 Origin、Base URL Origin、Provider UUID、auth mode 和 source-provider 身份；API Key 明文与其 hash 均不进入指纹。
- 上述任一授权边界变化使旧确认失效；Base URL Origin 与完整 Origin 列表在原生确认中明确显示。
- 单独轮换 API Key 不撤销确认；真正执行时始终加载当前密钥。
- 新建/复制产生新的 Provider 身份。复制可保留本机脚本草稿和启用意图；源 custom 已启用时必须为新身份重新确认，取消则零写入，源 custom 未启用时副本保持禁用。
- 自定义 adapter 只对 `api_key` 且 `source_provider_id == null` 的直接 Provider 有效；OAuth/桥接来源不加载任何凭据。

### 3.3 原生确认顺序

启用保存：

1. 丢弃 Renderer proof 并校验草稿。
2. 计算有效 Origin 和指纹。
3. 若当前 Provider 已保存同一指纹/Base Origin，可复用确认；否则打开原生 warning dialog。
4. 用户确认后由后端注入 proof，再进入 Provider 保存事务。
5. 事务 sanitizer 只在 proof 或现有授权与当前配置精确匹配时持久化 enabled/fingerprint/Base Origin。

草稿测试：

1. 先加载不含密钥的 Provider 身份与配置快照。
2. 校验草稿并完成原生确认。
3. 确认后再加载当前凭据快照。
4. 比较 Provider UUID、Base URL Origin、auth/source 身份和草稿权限边界；发生变化则拒绝陈旧执行。
5. 仅 API Key 变化允许使用新密钥继续一次测试。

原生确认使用 non-waiting 单飞槽；并发确认立即返回 `SEC_CONFIRM_BUSY`，不排队积压。

## 4. JavaScript Worker 边界

### 4.1 脚本契约

源码求值结果必须是对象，具有两个同步函数：

```javascript
({
  request: (ctx) => ({
    url: ctx.baseUrl + "/v1/usage",
    method: "GET",
    headers: { Authorization: "Bearer " + ctx.apiKey },
    body: null,
  }),
  parse: (response) => ({
    status: "available",
    balance: response.data.balance,
    unit: "USD",
  }),
})
```

`ctx.apiKey` 和 `ctx.baseUrl` 是固定、不透明的内部占位符。父进程取得并校验有界请求计划后，逐段替换其中每个精确、完整的 token occurrence；脚本对 token 的编码、切分或其他变换不会作用到后续明文替换。该语义固定为自动化契约，不能依赖普通字符串的“替换一次”歧义。

`parse` 只接收 `{ status, data }`，其中 `data` 已有界 JSON 解析并递归替换精确敏感值。它不接收 request URL、headers、ctx 或凭据。

### 4.2 进程与 QuickJS

- `request` 和 `parse` 各启动一次当前可执行文件的 worker 模式，不在 Tauri 主进程求值。
- worker 清空环境后只保留当前平台实际启动动态库所需的最小 allowlist，不继承 HOME/AppData 或通用 LD/DYLD 变量；stdin/stdout 使用单行、最大 512 KiB、带协议握手的 JSON。
- QuickJS 每次运行限制 8 MiB heap、256 KiB stack 和 100 ms engine deadline；父进程另有 100 ms 调用 deadline 与 5 秒启动上限。
- worker 不暴露 fetch、module、filesystem、process、environment、Tauri、timer 或 WebSocket；只启用所需 Eval/JSON/RegExp intrinsic。
- timeout、协议异常、过大输出或 child drop 均触发 kill，并对 wait/reap 设置独立硬上限；超过回收上限必须稳定报错并留下可诊断但不含敏感数据的记录。

这是一条能力受限的执行边界，不宣称可以判断任意脚本意图。用户确认的脚本可以把 API Key 发往确认过的 Origin；精确明文清理只是误回显防护，不是防止编码/变换泄漏的证明。

### 4.3 工作流并发

自定义 workflow 使用 non-waiting semaphore，最多四条完整链路并发。permit 覆盖 request 求值、credentialed HTTP 和 parse 求值；超限返回稳定 busy/query-failed 结果，不创建无界等待队列。

共享账户用量运行时另有最多四个 Provider fetch 的全局 limiter。两层限制职责不同：全局限制所有适配器，自定义限制高风险脚本工作流；获取顺序固定且无反向获取路径。

## 5. Rust 网络边界

- 只允许 GET/POST 和 HTTPS URL；URL 必须无 userinfo，Origin 精确等于 Base URL Origin 或已确认额外 Origin。
- 禁止 redirect；3xx 直接失败，凭据不得跟随目标。
- 禁止 `Cookie`、`Set-Cookie`、`Host`、`Content-Length`、代理认证和逐跳 headers，同时保留明确允许的 `Authorization`/`X-API-Key` 等普通凭据头。
- 最终物化后限制：URL 16 KiB、headers 32 个、名称 128 bytes、值 8 KiB、body 64 KiB。
- 响应 body 最多 64 KiB，必须是成功 HTTP 和合法 JSON；serialized JS 输出最多 64 KiB。
- 自定义请求超时使用已规范化的 2..15 秒配置；构造 client 和错误映射不得包含 URL、响应、密钥或上游消息。

## 6. 输出归一化

允许状态为：

```text
available | zero_balance | expired |
auth_failed | query_failed | configuration_required
```

- 成功类状态可携带现有 DTO 的金额、周期、文本和过期字段。
- 数字必须有限；`expiresAt` 必须是 `i64` JSON integer；文本最多沿用 DTO 的 96 字符上限。
- 失败类状态无条件丢弃脚本附带的所有账户字段并使用后端本地稳定消息。
- 未知字段忽略，未知状态/错误类型/非有限数字/超限文本使整个输出失败。
- `last_fetched_at` 由后端写入，脚本不能指定；adapter 固定为 `Custom`，freshness 由后端决定。

## 7. 共享账户用量运行时

### 7.1 运行时数据

Tauri 管理 `ProviderAccountUsageRuntimeState`：

```rust
struct RuntimeEntry {
    schedule: Option<ProviderAccountUsageRefreshSchedule>,
    result: Option<ProviderAccountUsageResult>,
    completed_at: Option<Instant>,
    last_attempt_at: Option<i64>,
    desktop_lease_until: Option<Instant>,
    config_generation: u64,
    config_token: AccountUsageConfigToken,
    in_flight_generation: Option<u64>,
    completion_generation: u64,
    force_refresh_pending: bool,
}

struct ProviderAccountUsageRefreshSchedule {
    timed_refresh_enabled: bool,
    refresh_interval_seconds: i64,
}
```

`config_generation` 是运行时 checked-increment，不能复用秒级数据库 `updated_at`。`config_token` 由规范化的 Provider 身份、auth/source、Base Origin、adapter/mode/interval 与有效 custom 授权指纹派生，不含 API Key 或原始脚本；它用于把后端快照与请求时 Provider 配置对齐。

运行时同时维护 generation 校验后的只读快照投影。公开读取只返回当前 generation、config token、单调完成时刻、展示时间戳和归一化结果，不含凭据、脚本、Origin、请求/响应或上游错误。后续 route gate 使用同步 try-read/fail-open 读取面，不得另建远端缓存；本任务中尚无路由消费者。

### 7.2 刷新规则

- Desktop 租约为有界续期，最后一个消费者过期后 scheduler 停止普通定时刷新。租约 heartbeat 使用独立控制面，不把旧提交的短周期 React Query 远端轮询带回主线。
- 首次无结果立即 due；同 Provider 已有 in-flight 时手动/自动请求订阅 completion 并合并。
- 手动强刷绕过新鲜缓存；若同 Provider 已有 in-flight，所有并发强刷合并为当前请求后至多一次尾随刷新，强刷调用者等待尾随完成。
- 成功显示缓存硬 TTL 为 60 分钟，未来时间戳无效；失败结果按保存的展示刷新间隔重试。
- `timedRefreshEnabled=false` 时展示消费者不按普通间隔刷新，但首次、手动强刷和 60 分钟硬过期仍可触发。
- 全局同时最多四个 Provider fetch；scheduler 只有在 permit 可用时才 spawn，并用每 Provider 一个 due/force 状态合并待办，避免为大量 Provider 创建 semaphore 等待任务。
- 每个 mutation 先比较规范化查询语义与凭据身份。Base URL/auth/source/API Key、adapter/mode/interval/custom 授权变化增加 generation 并清空旧结果；delete/disable 与配置导入成功提交/reset 清理对应状态。name/note 与仅展示的 `timedRefreshEnabled` 只更新各自元数据，不误清有效结果。完整导入即使复用 Provider ID，旧完成也只能唤醒等待者，不能提交。

### 7.3 前端与 IPC

- IPC 将 `provider_account_usage_snapshot(provider_id)`、`provider_account_usage_refresh(provider_id)` 和 Desktop lease 控制分开；草稿测试另用专用命令。
- React Query 保持当前单一前端镜像 owner 和 exact-key cancellation/逆序完成保护，不复制旧提交的 5 秒轮询，也不通过 TanStack Query `meta` 传递 force 命令。
- Provider 页面挂载时取得并有界续期 Desktop lease，卸载时主动 release；TTL 只用于异常退出兜底。
- Provider mutation 和配置导入提交通过语义化 invalidate/reset API 更新 runtime；旧 Promise、旧 generation 和前端 cache 都不能恢复已变更 Provider 的结果。数据库事务回滚不提前清理，成功 commit 后同步执行 reset。

## 8. 可移植性

- 本地 persistence sanitizer 保留脚本草稿和经验证的派生授权，剔除 proof/未知字段。
- persistence、单 Provider share、完整 config bundle 使用三个显式 allowlist sanitizer。后两者都让 custom adapter 变为 disabled，并删除源码、Origin、timeout、enabled、fingerprint、Base Origin 和 proof；导入端再次应用对应 policy。
- 导入端再次 portable sanitize，手工注入字段也不能激活或留下可恢复脚本副本。
- 原生 sub2api/NewAPI 模式、刷新设置与现有私有 NewAPI credentials 策略保持不变；Child 2 会在新增 route gate 后细分分享与完整备份 policy。

## 9. 兼容与回滚

- 扩展 JSON 增量字段不需要 SQLite migration；未知/非法配置由 sanitizer 归一化为禁用或 configuration-required。
- `rquickjs` 已在当前 Cargo 依赖中，实施不应引入第二个 JS 引擎。
- 主二进制入口增加 worker flag 时必须早于 Tauri/extension-host 初始化，并保持现有 extension worker 分支。
- 生成 TypeScript bindings 由 Rust registry 重建，不手改 `src/generated/bindings.ts`。
- 如共享 runtime 接线需要回滚，可恢复 IPC 直接 fetch，但脚本执行仍不得回到 Renderer 或 Tauri 主进程；旧 Observer/TUI 代码不属于回滚面。

## 10. 关键风险

- **确认后竞态**：确认前后必须比较无密钥/含密钥快照，不能仅相信 Provider ID。
- **主进程阻塞**：QuickJS 必须在一次性 child；仅 engine interrupt 不能约束不轮询的 native built-in。
- **回收阻塞**：kill 之后的 wait/reap 同样需要 deadline，不能让超时 worker 把调用线程永久挂住。
- **凭据回显**：worker 永远不接收明文；错误与结果仍要执行字段上限和精确敏感值清理。
- **双缓存覆盖**：React Query 不再拥有远端刷新时序，所有完成都经 runtime generation 提交。
- **秒级 revision 碰撞**：数据库 `updated_at` 只作展示/存储元数据，异步正确性使用进程 generation 与配置 token。
- **便携残留**：不仅 enabled，源码和 Origin 本身也必须从分享/备份/导入完全移除。
