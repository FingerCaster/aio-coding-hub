# 自定义余额查询与路由恢复集成 - 技术设计

## 1. 设计目标

本设计把“如何取得可信账户用量”和“何时让余额影响网关发送”拆成两个顺序实施、独立验收的子任务：

1. `08-08-custom-account-usage-script` 选择性移植自定义 JavaScript 适配器与后端共享账户用量运行时，保持纯展示语义。
2. `08-08-account-usage-route-gate` 在共享快照之上增加显式门控、后台 Gateway 消费租约和 Session 恢复回切。

父任务不直接拥有业务实现，只负责依赖顺序、跨层契约和最终集成验收。

```text
Provider 本地配置 / 私有凭据
        -> 后端共享账户用量运行时
        -> generation 校验后的 Provider 快照
        +-> Desktop 展示
        +-> 路由新鲜度投影（仅显式开启）
              -> 共同发送前 gate
              -> skipped attempt / 零上游调用
              -> fresh Blocked -> Available 恢复代次
              -> 既有 failback planner + Session 单调绑定
```

## 2. 核心所有权

### 2.1 账户用量运行时是唯一远端查询所有者

- React Query 和 Provider 页面只通过独立 Desktop 租约及快照/刷新 IPC 镜像后端结果，不接管远端刷新时序。
- 同一 Provider 的手动、桌面和 Gateway 刷新合并为一个 in-flight 请求；不同 Provider 最多四个并发，待办以 Provider 级 due 位合并，不创建无界 semaphore 等待任务。
- 运行时使用 checked-increment 的进程内 generation，并为当前规范化配置生成不含明文密钥/脚本的比较 token；不能把秒级数据库 `updated_at` 当 correctness revision。旧 generation 或 token 不匹配的异步结果不得写入显示快照、路由投影或恢复代次。
- 显示成功缓存寿命与路由可信寿命是两个派生规则，不建立两份独立远端缓存。

### 2.2 共同发送前 gate 是唯一拒绝所有者

- Provider resolution 保留完整候选，不按余额静默删除 Provider。
- `failover_loop/prepare/provider_checks::run_gates` 在 Ready 计数和 transport send 之前读取本地路由投影。
- 新鲜、可信的 `zero_balance` 或 `expired` 才产生普通 `skipped` attempt；快照缺失、过期、失败或配置不完整均 fail-open。
- 余额 gate 不查询数据库、不等待远端、不运行 JavaScript、不领取或完成 circuit probe，也不改变 Provider 排序/启用状态。

### 2.3 余额恢复是独立资格信号

- 共享运行时维护账户用量专用的全局单调 epoch 和 Provider 最新恢复 epoch。
- Session binding 除现有 circuit recovery baseline 外，再捕获 account-usage recovery baseline；两者不互相覆盖。
- 只有同一配置 generation 中曾有可信阻断，随后取得新鲜 `available` 成功结果，才发布余额恢复 epoch。
- circuit 为 `CLOSED` 时，任一成功恢复 epoch 比 Session 对应 baseline 新即可成为现有 planner 的 `Direct` 高优先级目标。
- circuit 为 `OPEN`/`HALF_OPEN` 时，余额恢复不得绕过 circuit；仍由现有 cooldown、single-flight 和 probe trigger 决定何时可发送。
- 快照失败/过期造成的 fail-open 不发布 epoch；余额查询成功也不关闭 circuit 或改绑 Session。

## 3. 共享数据契约

### 3.1 配置

账户用量扩展继续使用 `core.provider-account-usage/accountUsage`，新增字段均由共享 sanitizer 规范化：

```text
adapterKind: disabled | sub2api | newapi | custom
newApiQueryMode: billing | account
timedRefreshEnabled: boolean
refreshIntervalSeconds: integer 60..300
routeGateEnabled: boolean, default false
custom*: local-only custom adapter fields
```

`routeGateEnabled` 是独立选择，不由 `adapterKind`、展示刷新或历史配置推导。只有 Provider 已启用、适配器有效且该开关为 `true` 时，Gateway 才成为账户用量消费者。

### 3.2 可移植性矩阵

| 操作 | 原生适配器配置 | `routeGateEnabled` | 自定义脚本 / Origin / 授权 |
| --- | --- | --- | --- |
| 本地持久化 | 保留 | 保留 | 保留本机值 |
| 本机 Provider 复制 | 保留 | 保留 | 可复制源码；源 custom 已启用时新身份必须先重新确认，取消则不创建副本 |
| 单 Provider 分享/导入 | 保留模式与刷新设置 | 强制 `false` | 删除字段，适配器归一化为 disabled |
| 完整配置备份/恢复 | 原生适配器保留 | 原生适配器保留 | 删除字段；自定义适配器归一化为 disabled 且 gate 强制 `false` |

分享与完整备份不得复用一个含糊的 portable sanitizer。实现使用 persistence、provider-share、config-bundle 三个命名清楚的 allowlist 入口，导入端再次应用对应策略，防止手工注入字段。

### 3.3 路由投影

路由只读取不含敏感数据的派生状态：

```rust
enum AccountUsageRouteState {
    ConfirmedAvailable,
    BlockedZeroBalance,
    BlockedExpired,
    UnknownAllow,
}
```

- 成功快照必须属于当前 generation，配置 token 与请求时 Provider 配置一致，`freshness=Fresh`，带非未来的展示时间戳，且运行时单调完成时刻的年龄严格小于 `2 * refreshIntervalSeconds`。
- `Available` 映射为 `ConfirmedAvailable`。
- `ZeroBalance` 在 `balance` 或 `plan_remaining` 任一有限值大于零时映射为 `UnknownAllow`；否则即使没有金额字段也可按显式状态阻断。
- `Expired` 若携带未来 `expires_at` 则视为矛盾并 `UnknownAllow`；否则按显式过期状态阻断。过期优先于仍显示的余额。
- 非成功状态、异常时间、矛盾输出和不可分类值都映射为 `UnknownAllow`，不得沿用旧阻断。

### 3.4 审计分类

余额阻断新增独立分类，而不是伪装成 OAuth/消费限额：

```text
error_category = account_usage
error_code     = GW_PROVIDER_ACCOUNT_USAGE_BLOCKED
reason_code    = account_usage_zero_balance | account_usage_expired
outcome        = skipped
```

attempt 只包含 Provider 请求时身份和稳定类别，不包含金额、脚本、Origin、响应或账户字段。余额接口没有权威恢复时间，因此不写 `earliest_available_unix`；全余额阻断时不凭后台刷新时间伪造 `Retry-After`。若其他 gate 提供可信时间，现有混合路径仍可输出其 `Retry-After`，但只要本次存在余额 skip 就不写 recent-error 503 cache，确保资格变化后的下一请求重新选择。

## 4. 后台刷新与生命周期

- Child 1 建立 Desktop 有界租约和 60 分钟显示成功缓存；现有主线没有旧提交中的 TUI/Observer 模块，不移植相关代码。
- Child 2 在 Gateway 生命周期内增加专用租约协调器：启动后立即从数据库投影所有已启用且 gate 有效的目标，之后以有界心跳做精确集合替换。
- Gateway 消费者忽略 `timedRefreshEnabled`，但复用 `refreshIntervalSeconds`；Desktop 仍遵守展示语义。
- 关闭 gate、禁用/删除 Provider 或更改配置 token 后，目标从 Gateway 集合移除并使旧 generation 失效；其他显示消费者存在时仍可继续展示刷新。
- name/note 与纯展示 `timedRefreshEnabled` 变化不重置 query generation 或余额转换记忆；Base URL/auth/source/API Key、adapter/mode/interval/custom 授权和 gate 资格变化按各自语义失效。
- 完整配置导入只在事务成功 commit 后同步 reset account runtime、live route runtime、recent errors，并立即替换 Gateway targets；不能依赖 5 秒 reconcile 修复可能复用的 Provider ID。
- Gateway 请求路径只做同步、短时的本地快照读取。运行时状态缺失、锁异常或读失败均 fail-open，并由后台协调器继续修复。
- 应用重启后没有可持久化的余额快照或恢复 epoch。首轮请求 fail-open，后台立即刷新；Gateway 重启同时清空 live Session，因此无需迁移恢复 cursor。

## 5. Session 回切数据流

```text
刷新提交 fresh Blocked
  -> gate 阻断，Provider recovery_epoch 清零
  -> 当前请求记录 skipped，继续 fallback 并绑定成功 Provider

后续刷新提交 fresh Available（同 generation，last confirmed 为 Blocked）
  -> 发布 Provider account_usage_recovery_epoch
  -> 不修改 circuit，不改 Session

Session 下一次正常可回切请求
  -> planner 发现 epoch > session baseline 且 circuit CLOSED
  -> 按最新路由顺序放入 Direct target
  -> 发送前再次执行余额/circuit/limit gate
  -> 真实模型请求完整成功后才以现有 request token 提交绑定
```

刷新失败、快照过期或配置缺失只解除 gate，不发布恢复。`last_confirmed` 可在同一 generation 内跨一次查询失败保留，用于后续可信 `Available` 识别恢复；实际 gate 永远不使用失败期间的旧阻断。

## 6. 子任务边界与顺序

### 6.1 Child 1：自定义查询与共享运行时

- 选择性移植 `c75150897145420d630f9927519493f154032227` 的脚本、确认、网络和本地净化设计。
- 选择性移植 `3cc35e4920b98f7f29cbe575cfb04c542ec3f95d` 的后端共享缓存、租约、合并和 generation 提交保护，不移植已从当前主线删除的 Observer/TUI 投影或 React Query 轮询。
- 暴露 generation 校验后的后端快照读取面，但不添加任何路由消费者或 gate。
- 完成后更新账户用量规范中的自定义适配器和共享运行时部分，仍保留“默认纯展示”。

### 6.2 Child 2：门控与恢复

- 增加 `routeGateEnabled`、Gateway 租约、路由投影、共同 gate 和稳定审计码。
- 扩展 Session baseline 与现有 failback planner，仅增加一种成功资格来源，不建立第二套排序器。
- 完成后修订账户用量“纯展示”绝对表述为“默认纯展示；仅显式 gate 消费路由投影”，并同步 gateway、share 和 config-bundle 契约。

Child 2 只有在 Child 1 的共享运行时、generation、失效和快照测试全部通过后才能启动。

## 7. 兼容、发布与回滚

- 扩展 JSON 新字段默认 `false`，无需 SQLite schema 版本迁移；旧 Provider 升级后行为不变。
- 完整配置 bundle 保持 schema v4。v1/v2 继续忽略账户用量快照，v3/v4 通过 sanitizer 读取新字段；旧版 v4 应用可能在重导出时丢弃未知 gate 字段，但缺失值安全回落为 false，因此不为此升级 v5。
- 新 Gateway error code 必须同步 Rust/TypeScript 常量检查；生成 IPC 绑定只能由 Rust 导出重建。
- Child 1 可在未接 Child 2 时独立发布，行为仍为展示能力。
- Child 2 如需行为回退，可先把所有 `routeGateEnabled` 置为 false；运行时与自定义查询仍可保留。代码回退前不涉及数据库降级。
- 父任务最终验收必须同时覆盖 Desktop、Gateway、Session、分享和完整备份，避免子任务各自通过但跨层策略不一致。

## 8. 主要风险

- **旧阻断误用**：显示 60 分钟 TTL 不能进入路由；边界年龄必须用严格 `<`。
- **错误恢复信号**：查询失败、配置切换、矛盾输出和普通模型成功都不能发布余额恢复 epoch。
- **绕过 circuit**：余额恢复只让 `CLOSED` Provider 直接回切，不能把 `OPEN` 当健康。
- **请求路径阻塞**：gate 不得等待 Tokio mutex、数据库、脚本或网络。
- **便携泄密**：分享与完整备份对自定义字段都必须双向剔除，错误/日志也不得回显载荷。
- **Session 竞态**：恢复后仍由既有单调 binding request token 决定最终绑定，旧响应不能覆盖新回切。
