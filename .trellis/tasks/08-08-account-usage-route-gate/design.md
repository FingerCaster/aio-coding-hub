# 余额门控与恢复回切 - 技术设计

## 1. 设计目标

本任务在 `08-08-custom-account-usage-script` 已完成的共享账户用量运行时之上增加一个默认关闭的路由消费者。它只做三件事：

1. 后台维持启用 gate 的 Provider 快照新鲜度；
2. 在现有共同发送前 gate 中把可信余额不足投影为可审计的 `skipped`；
3. 把可信余额恢复作为现有 failback planner 可观察的成功资格信号。

Provider resolution、路由排序、circuit、Session binding 和 attempt budget 继续由现有模块拥有。本任务不建立“余额路由列表”、不静默预过滤、不把查询失败记为模型失败。

## 2. 配置与目标资格

### 2.1 配置字段

在 `core.provider-account-usage/accountUsage` 增加：

```text
routeGateEnabled: boolean, default false
```

Rust 与 TypeScript sanitizer 都将缺失、未知类型修复为 `false`。它与 `timedRefreshEnabled` 独立：前者决定 Gateway 是否消费快照，后者只决定展示消费者的普通定时刷新。

Child 2 将 `routeGateEnabled` 纳入 Child 1 的非敏感 `AccountUsageConfigToken`；`timedRefreshEnabled` 不纳入。因而 gate 资格变化会失效旧 route snapshot/transition，而纯展示定时开关不会。

### 2.2 Gateway 目标

一个 Provider 仅在以下条件全部满足时成为 Gateway account-usage target：

- Provider `enabled=true`；
- `auth_mode=api_key` 且无 `source_provider_id`；
- account-usage adapter 被有效配置；custom 还必须有当前有效的本机确认；
- `routeGateEnabled=true`；
- `refreshIntervalSeconds` 已规范化为 60..300 秒。

未配置、adapter disabled/invalid、gate 关闭、OAuth/来源 Provider 或 Provider disabled 都不是目标，不产生 Gateway 专用刷新，也不改变 gate 结果。

### 2.3 可移植性

使用显式 portability policy：

| 路径 | 原生 adapter | Custom adapter |
| --- | --- | --- |
| local persistence | 保留 gate | 保留 gate；授权规则不变 |
| local duplicate | 保留 gate | 保留 gate 和本机脚本草稿；源 custom 已启用时新身份先重新确认，取消则不创建副本 |
| provider share/export/import | gate 强制 false | 删除 custom，adapter disabled，gate false |
| config bundle v3/v4 export/import | 保留 gate | 删除 custom，adapter disabled，gate false |
| config bundle v1/v2 | 继续忽略整个账户用量 snapshot | 同左 |

扩展 JSON 足以承载该字段，不增加 SQLite 或 config-bundle schema 版本。

## 3. Gateway 消费租约

### 3.1 生命周期

在 Gateway background tasks 中增加账户用量租约协调器：

- Gateway 启动后立即读取当前 enabled Provider 投影并调用 runtime 的 `replace_gateway_targets`。
- 协调器按固定、有界心跳重新读取配置并精确替换目标集合；复用 Child 1 的 15 秒 lease，续期间隔取 5 秒。
- 精确替换会立即移除已关闭 gate/禁用/删除的目标，而不是等待旧 additive touch 自然过期；lease TTL 仍处理 Gateway 异常退出。
- Gateway stop 取消协调器；Gateway lease 最迟在 TTL 后消失。Desktop lease 独立存在时仍可继续显示刷新。
- target config token 变化调用同一 generation invalidation；不为 Gateway 建立第二个缓存或 in-flight 请求。

协调器只投影账户用量 target 所需的 Provider ID、enabled/auth/source、adapter 资格和由 domain 生成的非敏感 config token，不读取 API Key、原始脚本或私有 NewAPI token。token 不写日志；真实凭据只在 runtime 选中 due target 后由现有 uncached fetch 边界加载。Provider mutation hook 提供即时 invalidate，5 秒 reconcile 负责修复漏失事件和精确集合收敛。

### 3.2 调度

- 任一有效 Gateway target 都使 scheduler 保持运行，即使没有 Desktop 页面。
- Gateway target 的 due 规则始终使用保存的 `refreshIntervalSeconds`，忽略 `timedRefreshEnabled`。
- 阻断与非阻断状态使用同一间隔，不强制 60 秒，不新增 route-only interval。
- 同 Provider 刷新与 Desktop/manual 合并；不同 Provider 继续受全局四并发限制和 Provider 级有界待办规则。
- 请求路径从不触发同步 refresh。启动/配置变化期间无快照时先 fail-open，由后台首轮立即 due。

## 4. 路由快照与纯投影

### 4.1 只读快照

Child 1 runtime 的 generation 校验后快照是唯一输入：

```rust
struct ProviderAccountUsageCachedSnapshot {
    config_generation: u64,
    config_token: AccountUsageConfigToken,
    refresh_interval_seconds: i64,
    completed_at: Instant,
    result: ProviderAccountUsageResult,
}
```

Gateway 使用同步 `RwLock` 的 try-read/clone。无 runtime state、锁竞争/poison、请求时 config token 不匹配或快照不存在均返回 `UnknownAllow`；不得在 async 请求路径等待 runtime mutex。token 在 Provider 加载时解析一次，热路径不反复解析扩展 JSON。

### 4.2 新鲜度

可信路由快照同时满足：

- 结果属于当前配置 generation，且 config token 与请求时 Provider 当前配置一致；
- `freshness == Fresh`；
- 后端展示时间戳存在且不晚于当前请求墙钟；
- `age = monotonic_now - completed_at` 严格满足 `0 <= age < 2 * refreshIntervalSeconds`；
- 状态与字段可以按下述规则一致分类。

年龄恰好等于阈值即失效。Child 1 的 60 分钟 display TTL 不进入该计算。

### 4.3 分类规则

```rust
enum AccountUsageRouteProjection {
    ConfirmedAvailable,
    Blocked(AccountUsageBlockReason),
    UnknownAllow,
}

enum AccountUsageBlockReason {
    ZeroBalance,
    Expired,
}
```

| 结果 | 投影 |
| --- | --- |
| `available` 且快照可信 | `ConfirmedAvailable` |
| `zero_balance`，`balance`/`plan_remaining` 任一有限正数 | `UnknownAllow`，防止矛盾状态误阻断，也不视为确认恢复 |
| `zero_balance`，没有正数（含没有金额字段） | `Blocked(ZeroBalance)` |
| `expired` 且 `expires_at` 为未来 | `UnknownAllow`，也不视为确认恢复 |
| 其他可信 `expired` | `Blocked(Expired)`；过期可覆盖仍显示的正余额 |
| `auth_failed/query_failed/configuration_required/unsupported` | `UnknownAllow` |
| 缺失、过期、未来时间、非法字段或任何不可分类值 | `UnknownAllow` |

金额永远不进入路由日志。Custom adapter 的显式无金额 `zero_balance/expired` 仍可表达布尔型供应商状态；用户的本机确认是该脚本语义的信任边界。

## 5. 共同发送前 Gate

### 5.1 顺序

`provider_checks::run_gates` 顺序调整为：

```text
account-usage route gate
-> circuit/cooldown/probe lease gate
-> OAuth quota / local spend limits
-> Ready preparation
```

余额先于 circuit 的原因是：已知余额阻断时无需领取随后立即 abandon 的 probe lease，也不能让余额 skip 改变 circuit snapshot。其他 gate 仍是各自唯一所有者；一个 Provider 本轮只记录第一个权威拒绝原因。

### 5.2 拒绝结果

新增稳定常量：

```text
GatewayErrorCode::ProviderAccountUsageBlocked
  = GW_PROVIDER_ACCOUNT_USAGE_BLOCKED

reason_code = account_usage_zero_balance
            | account_usage_expired
error_category = account_usage
outcome = skipped
```

拒绝时：

- 生成一个普通 Provider attempt，保留 request-time Provider ID/name/base URL，沿用 `decision=skip`、`selection=filtered`，Provider/retry attempt index 与 circuit/probe 字段保持空；
- `providers_tried`、同 Provider retry index 和 Ready-provider cap 均不增加；
- 不调用上游、不加载模型凭据、不提交 dispatch intent/session reservation；
- 不记录 circuit failure/success，不发布 probe result，不修改健康度；
- request-scoped failback reservation 仍可供后续计划目标使用，全部目标均未发送时由现有 drop 逻辑释放。

`IterationCounters` 增加独立 `skipped_account_usage`，终态诊断与日志显示该数量。余额快照没有权威恢复时间，绝不把“下一次刷新”写入 `earliest_available_unix`；因此纯余额阻断没有 `Retry-After`，混合 gate 仍可使用 circuit/limit 提供的可信时间。

现有 recent-error cache 会在 Provider selection 前直接短路缓存的 503。只要本轮 `skipped_account_usage > 0`，finalizer 就不得写该 cache，即使混合门控响应带其他 gate 的 `Retry-After`；否则余额恢复、查询失败或快照 stale 后的下一请求无法按 fail-open 契约重新选择。

### 5.3 全部不可用

余额 skipped 继续满足现有 `should_finalize_as_all_providers_unavailable`：

- 全部候选余额阻断返回 HTTP 503 / `GW_ALL_PROVIDERS_UNAVAILABLE`；
- verbose attempts 和 persisted route 保留全部 Provider；
- 不改写为 `GW_NO_ENABLED_PROVIDER`；
- 普通 skipped 仍计入 attempt/route，只有既有精确 `probe_result=not_triggered` observation 被摘要排除。

forced Provider、managed route、Session rotated route、普通 fallback、planned direct/probe target 和 model-discovery 只要进入共同 `run_gates` 都执行同一规则；本任务不改变各路径原有请求分类、strict attempt 数或候选集合。

可见性边界是“候选已被现有 planner/route 计划”。当前 planner 不会为持续余额阻断的高优先级 Provider 在每个稳态请求制造 Direct target；本任务不新增合成 observation。候选一旦实际进入 `run_gates`，就必须产生上述普通 `skipped`，不能在 resolution 阶段消失。

## 6. 余额恢复 Epoch

### 6.1 运行时状态

每个 runtime entry 额外维护：

```rust
enum LastConfirmedRouteState { Available, Blocked }

struct RouteRecoveryState {
    generation: u64,
    last_confirmed: Option<LastConfirmedRouteState>,
    provider_recovery_epoch: u64,
}
```

runtime 另有进程级、checked-increment 的 `global_account_usage_recovery_epoch`。

提交规则：

- fresh confirmed Blocked：更新 `last_confirmed=Blocked`，把 Provider recovery epoch 清零，使旧恢复信号立即不可用。
- 同 generation 的 fresh confirmed Available 且上一个 confirmed 为 Blocked：在 runtime inner write lock 内从全局 epoch 计算 checked next，先原子更新 Provider snapshot、Provider recovery epoch 和 `last_confirmed=Available`，最后以 Release store 发布全局 epoch。
- 初次 Available、连续 Available、连续 Blocked、矛盾成功、失败结果和时间过期不发布。
- 查询失败/过期期间 gate fail-open，但不清除同 generation 的 `last_confirmed=Blocked`；后续 fresh Available 仍能确认一次真实恢复。
- adapter/config/permission token 变化、gate 关闭、Provider 删除或 runtime invalidate 重置 Provider confirmed state/epoch；旧 in-flight completion 因 generation/token 不匹配无法发布。
- 查询失败或快照过期时保留同 generation 的 `last_confirmed` 和已发布 epoch，但对 planner 暂时投影为零；只有当前快照再次为 fresh `ConfirmedAvailable` 时旧 epoch 才重新可见，失败/过期本身不生成新 epoch。
- 全局 epoch 溢出时继续放行 fresh Available，但不发布可复用标记，保持主动回切 fail-closed。

新 Session 捕获 baseline 时以 Acquire load 读取全局 epoch。发布顺序必须保证读者不能先看到全局 epoch、后看到仍无 Provider epoch 的不一致状态；不得用先暴露全局值的 `fetch_add/fetch_update`。

### 6.2 Session baseline

`SessionBinding` 和 `SessionRoutingSnapshot` 增加：

```text
account_usage_recovery_epoch_baseline: u64
```

- 创建新 binding 时同时以对应 Acquire 语义捕获 circuit global epoch 和 account-usage global epoch。
- sliding TTL、同 Provider 成功、其他 Session 收敛和旧 in-flight completion 都不推进 baseline。
- binding clear/expiry 后的新 incarnation 捕获当时的两个全局 baseline。
- 继续使用现有 `SessionBindingRequest` floor/last-success token；不增加无版本写入路径。

### 6.3 Failback planner

Planner 每个候选读取 `(CircuitSnapshot, current route projection, account_usage_recovery_epoch)`：

- 自然模式中，circuit 为 `CLOSED`，且 circuit epoch 较新，或当前投影为 `ConfirmedAvailable` 且 account-usage Provider epoch 比 Session baseline 新时，生成现有 `Direct` target。
- account-usage epoch 为零、旧于/equal baseline 或 Provider 当前已再次 confirmed Blocked 时，不因余额来源生成 target。
- circuit 为 `OPEN/HALF_OPEN` 时，account-usage recovery 不生成 direct，也不新增 probe trigger；现有 natural/open deadline、explicit trigger 和 single-flight 继续决定 probe。
- route-change、compaction、aggressive 等既有显式规划不被余额 epoch 改写；它们选中的 Provider 最终仍由共同 gate 复检。
- 多个恢复候选保持最新 route 的完整有序 prefix，不固定 P1/P2 长度。

余额刷新本身不创建 `RequestDispatchIntent`、不消费 Session trigger、也不绑定 Provider。只有真实模型请求完整成功，才沿用现有非流/流 finalizer 的 token-aware binding commit。

## 7. 配置变化与显式关闭

- Provider 保存成功后比较 query/route 语义：Base URL/auth/source/API Key、adapter/mode/interval/custom 授权变化 invalidate query generation；gate 资格变化重置 route recovery state。name/note 与纯展示 `timedRefreshEnabled` 不清有效快照或转换记忆。
- 当 `routeGateEnabled` 从 true 变 false，或一个正在生效的 gate 因 adapter/auth/source/permission 配置被显式关闭时，沿用现有 CLI route-runtime clear 入口清理 live Session 和相关 recent errors，使下一请求按最新配置重新选择；这不是余额恢复 epoch，也不修改 circuit。Provider upsert/duplicate/enable/delete 的成功 mutation 路径都必须调用语义化 invalidate，不能只依赖秒级 `updated_at`。
- config import 可能复用 Provider ID。只有事务成功 commit 后，才同步 `reset_all` account runtime、清 live route runtime/recent errors 并立即 reconcile Gateway targets；回滚路径不发布这些状态变化，也不能等待 5 秒 heartbeat 才修复。
- refresh interval/脚本权限变化由 generation 失效和 gate fail-open 处理；后续真实 Blocked -> Available 再发布 epoch。
- Provider delete/disable 和 config import 继续走现有 route-runtime clear/restart 生命周期，并同步移除 Gateway target。

## 8. UI 与可观察性

- 在现有 Provider account-usage section 中增加一个 switch，不新增独立页面或全局开关。
- adapter disabled 时不创建 gate 配置行；已有 `routeGateEnabled=true` 但 custom 等待重新确认时保留用户选择，运行时显示配置未就绪且 gate 不生效。
- Provider card/账户用量展示可继续显示 60 分钟内结果；路由详情只显示稳定的“余额不足/账户过期”跳过类别，不显示金额。
- Rust error code 与 `src/constants/gatewayErrorCodes.ts` 同步；attempt/live event/request-log JSON 复用现有可选字段，不新增敏感 DTO。

## 9. 兼容与回滚

- 旧配置缺字段即 false，升级零行为变化。
- 完整配置 bundle 保持 schema v4；v1/v2 忽略账户用量配置，v3/v4 使用 config-bundle policy。旧版 v4 可能在重导出时丢掉未知 gate 字段，但缺失安全回落为 false，不因此升级 v5。
- 关闭所有 gate 可恢复当前路由行为；共享运行时和显示仍可工作。
- 无 SQLite migration，无旧二进制数据库降级问题。
- 运行时不可用或故障统一 fail-open；但任何 fail-open 都不得伪造恢复 epoch。
- 若新增 error code 需要回滚，必须同时回滚 Rust/TypeScript 常量和 attempt fixtures，不能留下未知代码。

## 10. 关键风险

- **gate 顺序副作用**：余额拒绝必须发生在 circuit probe lease 前，且不能消费 request reservation。
- **陈旧恢复**：再次 Blocked 必须清 Provider epoch；配置 generation 变化必须拒绝迟到 Available。
- **不确定状态集体回切**：缺失/失败/过期仅 fail-open，不发布 epoch。
- **Session 双 cursor 混淆**：circuit/account baseline 分开比较，不能用一个全局 revision 替代成功信号。
- **后台刷新泄漏**：目标集合必须精确替换，关闭 gate 后不能因旧 lease 永久轮询。
- **Retry-After 误导**：刷新计划不是余额恢复时间，不写 header。
- **503 缓存压住恢复**：含余额 skip 的终态绝不写 recent-error cache，混合门控恢复用例必须覆盖下一请求。
