# 余额门控与恢复回切

## Goal

在用户显式开启后，让余额已耗尽或账户已过期的 Provider 在共同发送前门控点以 `skipped` 形式跳过，并在共享账户用量快照确认恢复后接入现有高优先级回切与 Session 收敛机制；未配置余额查询的供应商保持当前行为。

## Dependency

- 依赖子任务 `08-08-custom-account-usage-script` 提供后端拥有、带 generation 和新鲜度的 Provider 级共享账户用量快照。
- 本任务不直接运行 JavaScript、不请求余额接口，也不建立第二份余额缓存。

## Requirements

### R1. 配置与资格

- 增加独立、显式、默认关闭的“余额影响路由”配置。现有配置迁移后默认关闭，升级不得改变任何 Provider 的路由资格。
- 配置统一覆盖 `sub2api`、NewAPI billing、NewAPI account 和自定义 JavaScript；所有适配器只通过共享归一化快照参与门控。
- 只有账户用量适配器已配置且路由门控开启时，运行时才维持 Gateway 消费租约和后台刷新。
- 路由门控开启后忽略纯展示的 `timedRefreshEnabled` 开关，始终沿用 Provider 已保存并规范化到 `60–300` 秒的 `refreshIntervalSeconds`；不新增路由专用间隔，也不在阻断状态强制固定 60 秒轮询。
- 未配置账户用量、适配器关闭、适配器不支持、路由门控关闭或 Provider 本身未启用时，不为余额门控刷新，也不改变现有选择结果。
- 新鲜且可信的 `zero_balance` 或 `expired` 才能进入余额阻断状态；现金余额与套餐剩余并存时，任一可消费额度为正都必须保持可路由。
- `zero_balance` 同时携带任一正可消费额度，或 `expired` 携带未来过期时间时，结果视为矛盾并 fail-open；矛盾结果不得作为确认恢复。
- 路由新鲜度上限为 `2 × refreshIntervalSeconds`，随已规范化间隔得到 `120–600` 秒范围；年龄达到上限、未来时间戳或时间计算异常一律不可信并 fail-open。展示运行时的 60 分钟成功缓存不得用于延长路由门控。
- 快照缺失、超过路由新鲜度上限，或刷新结果为 `query_failed`、`auth_failed`、`configuration_required` 时必须 fail-open 并继续后台重试；不得继续沿用旧的余额阻断。

### R2. 发送前门控与审计

- 余额门控加入现有 `provider_checks::run_gates` 共同发送前检查，位于实际上游调用和 Ready Provider 计数之前。
- 拒绝时生成稳定结构化 reason code 的普通 `skipped` attempt，保留 Provider 身份、路由顺序和审计可见性，且零上游调用。
- 只有候选被现有 planner/route 实际计划并进入共同 gate 时才生成该 attempt；不为持续阻断的高优先级 Provider 在每个稳态请求中合成 observation 或强制 Direct target。
- 新 Session、无绑定请求和普通候选选择仍保留原候选集合；首次实际选中余额阻断 Provider 时必须经过共同 gate 并留下一个可审计的 `skipped`，不能在 resolution 阶段静默删除。
- 余额跳过不计供应商失败、不修改健康度、不打开/关闭熔断、不发布探针成功、不消费 Ready Provider 或同 Provider 重试预算。
- 全部候选被门控时继续返回当前 `GW_ALL_PROVIDERS_UNAVAILABLE` 语义，并保留所有已检查候选；不得改写为无启用供应商或静默删除候选。
- 余额阻断使用独立稳定的 error category、Gateway error code 和 `zero_balance`/`expired` reason code，不能伪装成 OAuth/本地消费限额。
- 余额刷新计划不是权威恢复时间，不得写入 `earliest_available_unix` 或单独产生 `Retry-After`；混合门控只使用其他 gate 已有的可信时间。
- 只要本次终态包含余额 `skipped`，即使混合门控保留其他 gate 的 `Retry-After`，也不得把 503 写入 recent-error cache；余额恢复、查询失败或快照过期后的下一请求必须重新进入 Provider 选择。
- forced Provider、managed model、Session 优先、普通路由和回切目标均经过同一余额复检，不允许旁路。

### R3. 阻断刷新与恢复信号

- 余额阻断期间，即使没有桌面消费者，Gateway 租约仍驱动受控后台刷新；请求路径只读取本地快照，不等待远端查询。
- 每个 Provider 维护单调的余额资格 generation/恢复代次。只有同一有效配置 generation 的新鲜成功快照明确从阻断变为 `available` 时发布新恢复代次。
- 同 generation 的失败/过期可保留上次确认阻断作为转换记忆，但 gate 必须立即 fail-open；后续新鲜 `available` 才发布一次恢复。再次确认阻断必须清除该 Provider 的旧恢复代次。
- 快照缺失、过期或失败所产生的 fail-open 只解除共同发送前门控，不发布主动回切代次；新 Session 和普通 fallback 可尝试该 Provider，既有 Session 不因不确定状态集体回切。
- 配置变化、脚本授权撤销、Provider 删除、运行时硬过期和后续再次阻断必须使不再有效的快照或恢复信号不可用。查询失败或快照过期可以保留同 generation 的转换记忆和已发布 epoch，但只有当前再次为新鲜 `available` 时该 epoch 才可供 planner 观察。
- 只有查询语义、凭据身份、刷新间隔或 gate 资格变化才重置相应 generation/恢复状态；name/note 与纯展示 `timedRefreshEnabled` 变化不重置转换记忆。完整配置导入成功提交后必须同步 reset 账户运行时、live Session、recent errors 并立即 reconcile Gateway targets。
- 多个 Session 各自维护已观察的恢复基线；一个 Session 完成回切不能全局消费其他 Session 的恢复机会。

### R4. 现有回切集成

- 对已绑定 fallback 的 Session，新鲜可信的余额阻断是 failback planner 的抑制提示：高优先级 Provider 不得被自然、compaction、route-change 或 aggressive 回切反复计划，也不得为其创建 Direct target、`not_triggered` observation 或 dispatch reservation；这不改变普通候选资格，发送前共同 gate 仍是权威判定。
- 首次实际进入共同 gate 的余额阻断候选保留结构化 `skipped`；同一 Session 随后的稳态请求应直接使用已绑定 fallback，只记录真实发送的 attempt。若 planner 读取后、发送前状态转为阻断，共同 gate 仍记录一次 `skipped` 并继续 fallback，下一请求再进入稳态抑制。
- 被抑制的自然/compaction 触发保持待处理，不通过余额 skip 伪造消费；Provider 确认恢复并发布新 recovery epoch 后，下一次符合条件的请求重新计划高优先级 Direct target。
- 恢复代次比 Session 基线更新时，现有回切规划按原路由优先级将 Provider 作为 direct 目标，不把余额刷新当作网络探针成功。
- direct 目标在发送前重新执行余额、熔断、冷却、OAuth 配额和本地消费上限门控。若再次余额不足，则记录 `skipped` 并继续当前 Provider/fallback。
- 只有真实、完整的模型请求成功才能沿用现有单调绑定 token 提交 Session 回切；余额查询成功本身不得改绑 Session。
- 非流与流式成功、旧请求晚完成、同 Session 并发以及多个高优先级 Provider 恢复时，继续遵守现有顺序和单调提交规则。

### R5. 可观测性与安全

- 路由详情应把余额门控与 circuit、cooldown、OAuth quota、消费上限区分，不记录金额、脚本、Origin、响应或账户身份。
- 后台刷新和门控诊断只记录 Provider ID、稳定状态类别、generation 和时间，不记录真实余额或上游内容。
- 余额查询故障不得改变熔断、Provider 启停、排序或其他额度系统。
- 完整配置备份/恢复与本机 Provider 复制保留 `routeGateEnabled`；单 Provider 分享/导入强制重置为 `false`。自定义适配器的任何导入同时保持无脚本、适配器禁用和门控关闭。
- 本机复制自定义 Provider 可保留脚本草稿与 gate 偏好，但新 Provider 身份不得继承确认。源 custom 已启用时复制前重新原生确认，取消则不创建副本；未确认的 custom 不建立 Gateway 租约或执行门控。
- 用户显式关闭正在生效的 gate 或对应 adapter 后，下一请求按最新配置重新选择；实现可清理该 CLI 的 live route runtime，但不得发布余额/circuit recovery epoch。

## Acceptance Criteria

- [x] 升级后所有现有 Provider 默认保持原路由行为；未配置余额查询的 Provider 不产生额外刷新或门控。
- [x] `sub2api`、两种 NewAPI 模式和自定义 JavaScript 对同一归一化状态执行一致的门控、新鲜度与恢复规则。
- [x] 显式开启后，新鲜 `zero_balance` / `expired` Provider 产生可区分的 `skipped` attempt、零上游调用和零 Ready 尝试消耗。
- [x] 已知阻断不会让 planner 每次强制制造 Direct attempt；实际被既有路由/回切计划检查时仍以普通 `skipped` 可见。
- [x] 同一 Session 首次命中阻断的高优先级 Provider 时记录 `skipped + fallback success`；绑定 fallback 后，在阻断快照未变化的连续请求中只记录实际 fallback success，且高优先级 Provider 零上游调用。
- [x] Codex compaction fingerprint 等自然回切触发可保持待处理，但不得使已知阻断 Provider 每请求重复出现；确认恢复的 recovery epoch 会让下一次符合条件的请求重新尝试并完成回切。
- [x] planner 读取 `available` 后、发送前转为阻断的竞态只在该请求记录一次余额 `skipped` 并 fallback；下一请求读取新投影后不再重复计划。
- [x] 快照缺失、过期、查询失败、认证失败或配置不完整时立即恢复路由资格，后台重试不阻塞网关请求。
- [x] 不确定状态的 fail-open 不发布主动回切信号；后续新鲜 `available` 才使现有 Session 获得余额恢复回切机会。
- [x] 现金余额为零但套餐剩余为正的 Provider 可路由；所有已知可消费额度耗尽时才阻断。
- [x] 矛盾的 `zero_balance + positive amount` 与 `expired + future expiry` fail-open 且不发布恢复代次。
- [x] 全候选余额不足及余额不足与熔断/冷却混合时，错误码、attempts、route 和 Retry-After 行为符合现有共同门控规范。
- [x] 阻断期间无 UI 时仍会有界刷新；恢复后高优先级 Provider 在下一次符合条件的请求中按原顺序被直接尝试。
- [x] 路由门控开启时，展示定时刷新即使关闭也不停止 Gateway 刷新；实际调度沿用已配置的 `60–300` 秒间隔且同 Provider 请求继续合并。
- [x] 路由快照在 `2 × refreshIntervalSeconds` 边界失效；未来时间戳和显示层仍可展示但不再可路由信任的旧快照不会阻断 Provider。
- [x] 恢复刷新不改绑 Session；只有真实请求成功才完成回切，真实失败按现有 retry/failover 处理。
- [x] 恢复后发送前再次变为阻断时记录余额 `skipped` 并继续 fallback，不产生上游调用或错误回切。
- [x] 多 Session 各自观察同一恢复代次；旧响应不能反转较新的 Session 绑定。
- [x] 再次确认阻断会使尚未观察的旧恢复信号失效；查询失败/过期不会延续阻断，但后续新鲜 `available` 仍可确认一次恢复。
- [x] forced Provider、managed model、普通 API Key、OAuth、非流、流式和应用重启后的首次状态均有测试。
- [x] 路由日志、普通日志、IPC 和测试产物不包含余额、脚本、Origin、凭据或上游响应。
- [x] 完整备份和本机复制保留门控开关；单 Provider 分享导入不会替接收方开启余额路由门控。
- [x] 纯余额阻断无基于刷新计划的 `Retry-After`；混合门控只保留其他 gate 的可信时间。
- [x] 含余额 skipped 的混合 503 不写 recent-error cache；余额恢复、失败或 stale 后的下一请求不会被旧缓存短路。
- [x] 显式关闭 gate 后旧 fallback Session 在下一请求重新按当前路由选择，且两个 recovery epoch 均不因此增加。

## Out of Scope

- 在 Provider resolution 阶段静默删除余额不足候选。
- 自动修改 Provider 启用状态、排序或配置路由。
- 让账户查询失败直接记为模型请求失败。
- 在网关请求路径同步刷新余额。
