# 供应商删除链路研究

## 结论

删除后的“调用顺序残留”是前端缓存一致性缺口，不是当前 SQLite 持久化遗漏：

- Rust 删除在一个事务中删除 `providers` 主行；三个含 `provider_id` 的顺序表都声明了
  `ON DELETE CASCADE`，且连接池的每个连接都启用 `PRAGMA foreign_keys = ON`。事务提交后，
  资源池顺序、Default 调用顺序和所有排序模板中的该 ID 都已从磁盘删除。
- `useProviderDeleteMutation` 成功后只从供应商列表缓存移除该 ID，并清理账号用量/模型目录缓存；
  它没有取消、更新或失效 Default 调用顺序缓存以及任意排序模板成员缓存。
- 右侧面板直接消费这些独立缓存。供应商列表缓存已移除对象、调用顺序缓存仍保留 ID 时，
  UI 会按既有回退逻辑渲染 `未知 Provider #<id>`，所以用户看到“删除后仍在调用顺序中”。
- 排序模板成员还被复制到 `ModeProvidersState` 本地状态；只更新供应商列表不会改变其 reset key，
  因而模板视图同样保留旧行。
- 删除成功时未先取消在途供应商/顺序查询。旧 IPC 查询若在删除后才完成，还可能把删除前快照
  重新写回缓存，使残留显示名称而不只是“未知 Provider”。

最小修复应位于前端 query mutation：删除成功后按稳定 `provider_id` 取消并同步过滤供应商列表、
Default 路由和该 CLI 的所有排序模板成员缓存，再失效这些 key 与后端真值对账。后端产品逻辑
无需为此重复手写顺序表 DELETE，但必须补一个正常删除的级联回归测试。

## 已读规范

- `.trellis/workflow.md`，研究产物必须写入任务的 `research/`。
- `.trellis/spec/aio-coding-hub/cross-layer/index.md:14`：路由选择/呈现应读取
  `gateway-failover-route-contract.md`。
- `.trellis/spec/aio-coding-hub/cross-layer/index.md:17`：供应商删除缓存清理涉及
  `provider-account-usage-query-contract.md`。
- `.trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md:38`：会话绑定只拥有
  复用偏好和顺序；候选/路由真值仍由共同选择链负责。
- `.trellis/spec/aio-coding-hub/cross-layer/provider-account-usage-query-contract.md:42`：Tauri IPC Promise
  可能无法物理中止，逻辑取消必须阻止旧完成覆盖新状态。
- `.trellis/spec/aio-coding-hub/cross-layer/provider-account-usage-query-contract.md:48`：删除按 provider ID
  清理该供应商自己的缓存，不得影响其他供应商。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md`：已完整核对；本链路
  不读写 `AppSettings`，所以其 RMW/CAS 规则不是本修复的落点。
- `.trellis/spec/aio-coding-hub/backend/index.md`、`.trellis/spec/guides/index.md` 与
  `.trellis/spec/guides/cross-layer-thinking-guide.md:21`：按 Source -> Store -> Retrieve -> Display
  追踪跨层数据流。

## 完整删除链路

| 层 | 位置 | 当前行为 |
| --- | --- | --- |
| 删除入口 | `src/pages/providers/SortableProviderCard.tsx:654` | 删除按钮调用 `onDelete(provider)`。 |
| 打开确认框 | `src/pages/providers/ProvidersView.tsx:174` | 将完整 `ProviderSummary` 保存为 `deleteTarget`。 |
| 确认动作 | `src/pages/providers/ProvidersView.tsx:650` | 把是否清理历史用量/日志传给 `confirmRemoveProvider`。 |
| 页面数据模型 | `src/pages/providers/hooks/useProvidersViewDataModel.ts:823` | 调用 deletion mutation；成功后仅记录日志、toast、关闭对话框。页面层不清理顺序状态。 |
| Query mutation | `src/query/providers.ts:331` | 调用 service；成功后只改供应商列表、账号用量和模型目录缓存。缺少 Default/模板路由缓存处理。 |
| TS service | `src/services/providers/providers.ts:326` | 校验 ID 并调用生成的 `commands.providerDelete`。 |
| 生成绑定 | `src/generated/bindings.ts:699` | `TAURI_INVOKE("provider_delete", { providerId, clearUsageStats })`。 |
| Tauri command | `src-tauri/src/commands/providers/crud.rs:49` | 无额外逻辑，委托 `provider_service::provider_delete`。 |
| 应用服务 | `src-tauri/src/app/provider_service.rs:388` | 删除前读取 `cli_key`，调用领域删除；成功后清空该 CLI 的网关路由运行时状态。 |
| 领域事务 | `src-tauri/src/domain/providers/queries.rs:1848` | 开启事务，先拒绝仍被 managed profile 引用的 provider，再删除模型目录/模型、provider 主行，并按选项删除请求日志。 |
| 事务提交 | `src-tauri/src/domain/providers/queries.rs:1920` | provider 主行及外键级联在同一事务提交。 |
| 运行时清理 | `src-tauri/src/app/provider_service.rs:408` | 成功后调用 `app_gateway_clear_cli_route_runtime_state`。 |
| 会话/错误缓存 | `src-tauri/src/gateway/runtime.rs:186` | 清空该 CLI 的 session bindings，并清空 recent-error cache，避免继续使用已删除顺序快照。 |

错误路径也是原子的：managed profile 检查发生在任何 provider 删除之前
（`src-tauri/src/domain/providers/queries.rs:1857`、`:1879`），失败会随未提交事务退出，不会先清掉
调用顺序再保留 provider。

## 持久化与运行时数据结构

### SQLite 是顺序真值，AppSettings 不是

`AppSettings` 只有 `provider_cooldown_seconds`、`failover_max_attempts_per_provider`、
`failover_max_providers_to_try` 等网关参数（`src-tauri/src/infra/settings/types.rs:277`），没有 provider ID
顺序字段。本链路也没有 `settings::read/update/write` 调用。

实际 provider 引用位于三个 SQLite 表：

| 表 | 用途 | 删除语义 |
| --- | --- | --- |
| `provider_pool_order` | 左侧资源池显示顺序；`list_by_cli` 用它排序（`src-tauri/src/domain/providers/queries.rs:641`）。 | `provider_id -> providers(id) ON DELETE CASCADE`（`src-tauri/src/infra/db/migrations/baseline_v25.rs:290`）。 |
| `default_route_providers` | 右侧 Default 调用顺序；DTO 只有 `{ provider_id }`（`src/services/providers/providers.ts:82`）。 | `ON DELETE CASCADE`（`src-tauri/src/infra/db/migrations/baseline_v25.rs:303`）。 |
| `sort_mode_providers` | 所有命名模板的调用顺序；DTO 是 `{ provider_id, enabled }`（`src/services/providers/sortModes.ts:22`）。 | provider 和 mode 均级联（`src-tauri/src/infra/db/migrations/baseline_v25.rs:324`）。 |

`sort_mode_active` 只保存 `cli_key -> mode_id`，不引用 provider，删除 provider 时无需修改。

右侧顺序的正常读写链也独立于删除命令：Default 由 TS service
`src/services/providers/providers.ts:360` 调用 Tauri command
`src-tauri/src/commands/providers/crud.rs:73`，再进入应用服务
`src-tauri/src/app/provider_service.rs:448` 和领域函数
`src-tauri/src/domain/providers/queries.rs:2055`/`:2087`；模板成员由
`src-tauri/src/commands/sort_modes.rs:106`/`:122` 进入
`src-tauri/src/domain/sort_modes.rs:347`/`:446`。Default/模板写成功都会清对应 CLI 的路由运行时状态
（`src-tauri/src/app/provider_service.rs:475`、`src-tauri/src/commands/sort_modes.rs:138`），但这些后端
运行时清理不会主动通知或修改浏览器中的 TanStack Query 缓存。

外键不是仅在迁移连接上开启：连接池 manager 对每个连接执行 `configure_connection`
（`src-tauri/src/infra/db/mod.rs:205`），其中明确设置 `PRAGMA foreign_keys = ON`
（`src-tauri/src/infra/db/mod.rs:322`、`:326`）。因此 `DELETE FROM providers`
（`src-tauri/src/domain/providers/queries.rs:1905`）会在同一事务级联三个顺序表。

网关读取也不会路由到已删除 provider：模板路径用 inner join
`sort_mode_providers -> providers`（`src-tauri/src/domain/providers/queries.rs:739`），Default 路径从
`providers` 主表出发并以 `EXISTS(default_route_providers)` 选成员
（`src-tauri/src/domain/providers/queries.rs:796`）。应用服务随后清空该 CLI 的 session-bound 顺序快照。

## 前端右侧调用顺序结构

### 三个互相独立的缓存域

1. 供应商列表：`["providers", "list", cliKey]`
   （`src/query/keys.ts:31`、`src/query/providers.ts:55`）。
2. Default 调用顺序：`["providers", "defaultRoute", cliKey]`
   （`src/query/keys.ts:36`、`src/query/providers.ts:66`）。
3. 模板成员：`["sortModes", "providers", cliKey, modeId]`
   （`src/query/sortModes.ts:19`、`:48`）。每个 mode 是独立 key，删除必须覆盖未激活但已缓存的模板。

页面同时订阅供应商列表与 Default 顺序（`src/pages/providers/hooks/useProvidersViewDataModel.ts:268`、
`:300`），并根据当前选择按需订阅模板成员（`:587`）。`providersById` 由供应商列表构造
（`:580`），而 `routeRows` 直接取 Default 或模板缓存（`:1062`）；两个来源不会自动互相约束。

模板查询结果还复制进 `ModeProvidersState`。其 reset key 只包含 CLI、mode ID 和模板行的
`provider_id:enabled`（`src/pages/providers/hooks/useProvidersViewDataModel.ts:137`）；供应商列表删除不会
改变这个 key，所以本地 `modeProviders` 不会重置（`:592`-`:603`）。

### 可复现的状态序列

1. 删除前：供应商列表和某个/多个顺序缓存都含 ID `P`。
2. Rust 删除 `P`，SQLite 级联成功并返回 `true`。
3. `useProviderDeleteMutation.onSuccess` 只执行
   `providersKeys.list(cliKey).filter(id !== P)`（`src/query/providers.ts:341`），随后只清账号用量和模型
   缓存（`:345`-`:352`）。
4. Default/模板 route rows 仍含 `P`，`providersById[P]` 已变成 `null`。
5. 右侧面板仍遍历旧 `routeRows`（`src/pages/providers/ProvidersView.tsx:496`），并显示
   `未知 Provider #P`（`:497`-`:500`；相同回退也在
   `src/pages/providers/SortableProviderOrderItem.tsx:29`）。
6. 全局查询 `staleTime` 是 5 分钟且关闭 window-focus refetch
   （`src/query/queryClient.ts:3`-`:9`）；活动页面没有任何 mutation invalidation，因此残留不是一次
   瞬时重绘，可一直存在到手工移出、切页后重新取数或重启/刷新。

### 在途查询竞态

页面允许手动 `providersQuery.refetch()`（`src/pages/providers/hooks/useProvidersViewDataModel.ts:672`）。
删除 mutation 只取消 provider-model 查询（`src/query/providers.ts:347`），不取消供应商列表、Default
或模板查询。若这些 IPC 在删除前读取旧快照、删除后才完成，旧结果可以覆盖第 3 步的缓存修正。
这与账号用量规范中“逻辑取消定义新写入边界”的要求相同。

### 演进根因

`git blame` 显示删除 mutation 最早来自 `c674fa5c5`（2026-02-01）；模板 provider query 后由
`dff4733d9`/`13bba9b04` 接入，Default route query 又在 `d2e4465c6`（2026-06-22）接入。新增的两个
顺序缓存没有加入既有删除 mutation 的缓存所有权清单；后续只继续补了账号用量和模型目录清理。

## 建议的最小修复

### 前端：在 mutation 层统一清理，推荐

修改 `src/query/providers.ts:331` 的成功路径；不要把主修复放到 `ProvidersView` 渲染过滤中。

1. `ok === true` 且规范化 `cliKey` 后，先取消以下可能在途的查询：
   - `providersKeys.list(cliKey)`；
   - `providersKeys.defaultRoute(cliKey)`；
   - 该 CLI 的模板 provider key 前缀
     `[...sortModesKeys.all, "providers", cliKey]`。
2. 按 `row.id !== providerId` 同步过滤供应商列表缓存。
3. 按 `row.provider_id !== providerId` 同步过滤 Default route exact key。
4. 用 `queryClient.setQueriesData` 对模板前缀下所有 mode 缓存做相同 ID 过滤；不能只清当前激活
   或当前打开的模板。
5. 再失效上述 route keys（可同时失效列表 key）以从已完成级联的 SQLite 获取权威结果。
6. 保留现有账号用量、模型 generation/cancel/remove 逻辑；`false` 或错误路径不得改任何缓存。

建议在 `src/query/sortModes.ts` 导出一个 `sortModeProvidersQueryPrefix(cliKey)`，并让完整 key helper
基于该前缀构造，避免 `providers.ts` 私自复制 key 布局。只按数值 `provider_id` 过滤；名称、域名、
CLI 类型、标签和 URL 都不能参与匹配。

同步改 query cache 后，`useSortModeProvidersListQuery` 会产生新 rows，现有 reset-key 机制会把
`ModeProvidersState` 更新为过滤后的结果，无需在页面层再维护第二份删除逻辑。

仅在渲染时执行 `routeRows.filter(row => providersById[row.provider_id])` 不是充分修复：它只隐藏了
不一致，缓存仍可在后续拖拽/保存时把已删除 ID 重新提交，也无法证明前端状态与持久化状态一致。
仅做 `invalidateQueries` 也不满足“立即消失”，且会保留 refetch 完成前的未知行；应先同步过滤再失效。

### 后端：保留级联，补契约测试

不建议在 `providers::delete` 中重复手写三个顺序表的 `DELETE`。当前 FK 级联已与 provider 主行处于
同一 SQLite 事务，语义更小且不会遗漏将来的普通删除入口。若测试暴露某类受支持旧数据库缺少 FK，
应单独修复迁移/ensure schema，而不是在应用服务层做非事务补偿；静态检查未发现这种正常迁移路径。

`provider_delete` 继续返回 `bool` 即可；前端已有 `cliKey` 和 `providerId`，无需扩大 IPC DTO 或重生成
绑定来完成本缺陷修复。

## 现有测试与缺口

| 现有测试 | 已覆盖 | 未覆盖 |
| --- | --- | --- |
| `src/query/__tests__/providers.test.tsx:1349` | 删除成功后列表缓存、账号用量缓存、目标/其他模型缓存。 | 未 seed/assert Default route 或任意模板 route cache；未覆盖在途列表/路由旧结果。 |
| `src/query/__tests__/providers.test.tsx:1503` | service 返回 `false` 时列表不变。 | 其他 route caches 的失败不变性。 |
| `src/pages/providers/__tests__/ProvidersView.test.tsx:652` | 对话框和 mutation 参数。 | mutation 被 mock；没有证明删除后右侧行立即消失，也没有切换模板检查。 |
| `src-tauri/tests/providers_crud.rs:184` | reorder、删除、随后 provider 列表只剩一项。 | 删除前未建立 Default/模板顺序，未断言级联持久化。 |
| `src-tauri/src/domain/providers/tests.rs:1144` | 默认保留日志；可选清理目标日志且保留其他 provider 日志。 | 三个顺序表和重启后的状态。 |
| `src-tauri/src/domain/providers/tests.rs:623` | managed profile 引用阻止删除。 | 被阻止删除时既有顺序行保持不变。 |
| `src-tauri/tests/sort_modes_crud.rs:184` | 模板顺序 ID 的非法、重复、超限校验。 | provider 正常删除跨多个 mode 的级联。 |
| `src-tauri/src/infra/db/migrations/tests.rs:779` | v27->v28 删除 official provider 时一个模板 FK 行级联。 | 当前普通删除、`provider_pool_order`、Default route、多个模板和保留邻接 provider。 |
| `src-tauri/src/gateway/manager.rs:171` | route runtime clear helper 清 session binding/recent errors。 | `provider_delete` 成功路径确实调用 helper 的集成连接。 |

## 建议测试点

### Query 单元测试

1. seed 两个 provider；Default key 和同 CLI 的两个 mode key 都包含目标 ID 与保留 ID。删除目标后断言：
   - 供应商列表、Default、两个 mode 缓存均移除目标；
   - 另一个 provider 的行、顺序和模板 `enabled` 值逐字不变；
   - 其他 CLI 的列表/Default/mode caches 逐字不变。
2. 两个 provider 使用相同 base URL、相同 provider 类型和近似名称，证明只按 ID 删除。
3. seed `null`、缺失和未激活 mode caches，证明 helper 不创建虚假缓存且会清所有已缓存 mode。
4. service 返回 `false`、抛错时，所有缓存保持原样。
5. 用 deferred Promise 模拟供应商列表、Default 和 mode 查询先读取旧值、删除后才 resolve；断言旧完成
   不能恢复目标 ID，并检查取消发生在同步 cache 写入之前。

### 页面测试

1. 尽量使用真实 QueryClient/mutation，而不是完全 mock `useProviderDeleteMutation`；删除 Default 成员后，
   同一帧/settled 后既看不到名称，也看不到 `未知 Provider #id`。
2. provider 同时位于 Default、当前模板和未激活模板；删除后逐一切换，任何方案都没有该行。
3. 删除其他共享域名/类型的 provider 后，保留项仍可拖拽、切换 enabled 和移出。

### Rust 领域/集成测试

1. 建立 provider A/B；把二者写入 `provider_pool_order`、Default 和至少两个 sort modes，其中 mode 的
   `enabled` 值不同。调用普通 `providers::delete(A)` 后逐表/通过 list API 断言所有 A 行为 0、B 行
   仍在且相对顺序/状态不变。
2. drop/reopen DB 后重复断言，覆盖“刷新或重启不复现”；最后运行 `PRAGMA foreign_key_check`。
3. 给 A 建 managed-profile 引用并预置全部顺序行；删除应失败，provider 与全部顺序行原样保留，
   证明事务失败不做部分清理。
4. 通过 Tauri/app-service 路径补一项运行时测试：删除当前 session-bound 顺序中的 A 后，该 CLI 的
   binding 被清，其他 CLI binding 保留；下一请求从数据库重新选择且不含 A。

## 验证说明

本次为只读研究，没有修改或执行产品源码，也没有运行测试；唯一产物是本报告。结论基于当前工作树
的源码、schema、现有测试和本地 Git 历史。
