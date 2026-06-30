# Route Template Unification Design

日期：2026-06-25

## Summary

本设计修复 provider 删除后排序模板仍残留供应商的问题，并统一 Default 和自定义排序模板的产品逻辑。当前 PR #307 把供应商路由排序整合进供应商页面，但遗留了两个问题：

1. 删除 provider 后，排序模板里仍可能出现该 provider 的孤儿引用。
2. Default 模板和自定义模板交互不一致：Default 只有排序，没有模板内开关；自定义模板有成员和启用开关；添加 provider 依赖“重写完整顺序”接口，领域动作不清晰。

新的设计把 Default 也视为系统内置路由模板。Default 和自定义模板共用一套双栏编辑器：左侧资源池，右侧当前模板调用顺序。区别只在管理能力：Default 不能删除、不能重命名；自定义模板可以创建、重命名、删除。

实现上采用“后端表保持低风险、前端 service 统一抽象”的最佳实践：不急着把两张路由表合成一张，而是在 domain/service 层提供统一动作，确保 UI 和 gateway 看到的是同一套模板语义。

## Current State

关键代码：

- `src/pages/providers/SortModesView.tsx`
- `src/pages/providers/useSortModesDataModel.ts`
- `src/services/providers/sortModes.ts`
- `src/query/sortModes.ts`
- `src-tauri/src/commands/sort_modes.rs`
- `src-tauri/src/domain/sort_modes.rs`
- `src-tauri/src/domain/providers/queries.rs`
- `src-tauri/src/infra/db/migrations/baseline_v25.rs`
- `src-tauri/src/infra/db/migrations/ensure.rs`

现有模型：

- `default_route_providers` 保存 Default 调用顺序。
- `sort_modes` 保存自定义模板。
- `sort_mode_providers` 保存自定义模板里的 provider 列表、排序和 `enabled`。
- provider 删除依赖外键 `ON DELETE CASCADE`，连接层已启用 `PRAGMA foreign_keys = ON`。

现有问题：

- 读取 `sort_mode_providers` 时直接读关联表，没有 join 当前 `providers`，历史脏数据会显示 `Provider #id`。
- 删除 provider 的领域逻辑只删除 `providers` 和可选 request logs，没有显式清理路由关联表。
- 自定义模板“加入/移除 provider”通过 `set_mode_providers_order` 重写完整列表完成，成员管理和排序动作耦合。
- Default 的排序和 provider 全局启用状态绑定，自定义模板使用模板内 enabled，产品语义割裂。

## Goals

本阶段要达成：

1. 删除 provider 后，Default 和所有自定义模板都不再显示该 provider。
2. 写路径显式清理关联表，读路径 join 当前 providers 防御历史脏数据。
3. Default 作为系统模板进入统一编辑器，支持模板内顺序和启用开关。
4. 自定义模板和 Default 使用一致的“双栏编辑器”心智模型。
5. 添加 provider 到模板使用独立领域动作，不再伪装成“设置完整顺序”。
6. 尽量保留现有页面布局，不进行大范围视觉重做。
7. 明确区分 provider 自身启用状态和模板内启用状态，避免 UI 文案继续混淆。
8. 迁移和 bindings 变化可回滚、可测试，避免一次大改破坏现有 Default 路由。

## Non-Goals

本阶段不做：

- 不重做供应商页面整体信息架构。
- 不引入模板向导或复杂批量操作。
- 不改变 provider 自身 enabled 的含义。
- 不改变 provider pool order 的用途。
- 不做跨 CLI 模板共享。
- 不做模板导入导出。
- 不做账号级权限或协作。

## Product Direction

### 1. 统一术语

用户看到的是“路由模板”，而不是 Default 与排序模板两个割裂概念。

模板类型：

- `Default`：系统内置模板，不能删除、不能重命名。
- 自定义模板：用户创建的命名模板。

模板内容：

- 成员 provider。
- 调用顺序。
- 模板内启用开关。

provider 自身 enabled 与模板内 enabled 是两层：

- provider 自身 disabled：该 provider 不可实际调用，UI 显示“供应商已关闭”。
- 模板内 disabled：该 provider 保留在模板中，但不参与该模板调用。

实际可调用条件：

```text
provider.enabled == true
AND route_template_member.enabled == true
```

UI 文案矩阵：

```text
provider.enabled=true, template.enabled=true
  徽标：可调用
  操作：模板开关为开

provider.enabled=true, template.enabled=false
  徽标：模板关闭
  操作：模板开关为关

provider.enabled=false, template.enabled=true
  徽标：供应商已关闭
  操作：模板开关仍可编辑，但列表说明“开启后仍需启用供应商才会调用”

provider.enabled=false, template.enabled=false
  徽标：供应商已关闭 · 模板关闭
  操作：模板开关为关
```

左侧资源池按钮文案：

- 不在模板：`加入`
- 已在模板且模板内启用：`已加入`
- 已在模板但模板内关闭：`已加入 · 模板关闭`
- provider 自身关闭时仍允许加入模板，但展示 `供应商已关闭`，因为模板成员关系和 provider 全局启用是两层状态。

### 2. 双栏布局

保留现有双栏：

- 左侧：当前 CLI 的资源池。
  - 显示 provider 名称、URL 摘要、自身启用状态。
  - 对当前模板已存在的 provider 显示“已在模板”。
  - 对不在当前模板的 provider 显示“加入”。
- 右侧：当前模板调用顺序。
  - 可拖拽排序。
  - 每项有模板内启用开关。
  - 每项有移除按钮。
  - Default 和自定义模板都使用同一套右侧列表。

Default 选中时：

- 右侧标题为 `调用顺序 · Default`。
- 新建、重命名、删除按钮对 Default 不适用。
- 开关和排序仍可编辑，因为 Default 也是系统模板。
- 空状态文案为：`Default 当前没有模板成员；请从左侧资源池加入 Provider。`

自定义模板选中时：

- 行为与 Default 一致，但允许重命名和删除。
- 空状态文案为：`当前模板没有成员；请从左侧资源池加入 Provider。`

### 3. 添加 provider

添加 provider 是独立动作：

```text
route_template_provider_add(template, cli_key, provider_id)
```

规则：

- provider 必须属于当前 CLI。
- provider 不在模板中时才可添加。
- 默认追加到模板末尾。
- 默认 `enabled = true`。
- 返回更新后的模板成员列表。

这比“把完整 provider id 顺序重新提交一遍”更符合产品意图，也更容易测试。

## Architecture

### 1. 统一模板抽象

前端使用统一类型：

```text
RouteTemplateSelection
  kind: "default" | "mode"
  modeId: number | null

RouteTemplateProviderRow
  provider_id: number
  enabled: boolean
```

后端可以继续保留两张表：

- `default_route_providers`
- `sort_mode_providers`

为了降低迁移风险，不需要立即合并成一张通用表。统一抽象放在 domain/service 层，由命令根据 `kind` 分派到底层表。

### 2. Default 模板启用开关

`default_route_providers` 需要支持模板内 enabled。新增字段：

```text
enabled INTEGER NOT NULL DEFAULT 1
```

迁移策略：

- 新增 ensure patch，为已有 `default_route_providers` 添加 `enabled`。
- 现有 rows 默认 enabled。
- baseline schema 同步新增 `enabled`，保证新安装和升级路径一致。
- gateway 查询 Default 时增加 `drp.enabled = 1` 条件。
- 前端 Default 列表不再只展示 provider.enabled=true 的供应商，而是展示 Default 模板成员；可调用性由 provider.enabled 和 row.enabled 共同决定。
- `default_route_providers` 的 list/set order 返回类型升级为 `{ provider_id, enabled }`，与自定义模板 row 对齐。
- 升级时为当前 CLI 下仍存在、但缺少 Default row 的 provider 补齐 route row，追加在现有 Default 顺序末尾，`enabled = 1`。这保留现有 Default 行为中“未绑定顺序的 provider 仍可作为尾部候选”的语义。
- 新版本完成迁移后，Default 调用只以 `default_route_providers` 为成员来源；后续新增 provider 由用户从左侧资源池显式加入 Default。

兼容策略：

- 迁移后所有历史 Default row 的 `enabled = 1`，保持现有调用行为。
- 旧前端调用 default route order 接口时，如仍只传 provider ids，后端保留已有 enabled 状态；新前端通过统一 service 调用 add/remove/order/enabled。
- bindings 更新后删除或标记旧 service 的直接视图使用点，但保留底层命令可作为兼容层。
- ensure patch 必须幂等：重复运行不会重复插入 Default row，也不会覆盖用户已有 `enabled` 和排序。

### 3. 后端领域动作

前端 service 层暴露统一模板动作，视图只调用统一接口：

```text
routeTemplateProvidersList(selection, cli_key)
routeTemplateProviderAdd(selection, cli_key, provider_id)
routeTemplateProviderRemove(selection, cli_key, provider_id)
routeTemplateProvidersSetOrder(selection, cli_key, ordered_provider_ids)
routeTemplateProviderSetEnabled(selection, cli_key, provider_id, enabled)
```

IPC 命令可以保留现有 Default 与 sort mode 分开的边界，以降低后端迁移风险；统一性必须落在 domain/service 层，而不是让视图组件判断每种模板的命令细节。

关键约束：

- `mode_id` 仅在 `kind = "mode"` 时必填。
- Default 不需要 `mode_id`。
- provider 必须属于 `cli_key`。
- ordered ids 必须等于当前模板成员集合，只允许改变顺序，不隐式添加或删除。
- add/remove 是唯一改变成员集合的动作。

后端命令落地方式：

```text
Default:
  default_route_providers_list(cli_key)
  default_route_provider_add(cli_key, provider_id)
  default_route_provider_remove(cli_key, provider_id)
  default_route_providers_set_order(cli_key, ordered_provider_ids)
  default_route_provider_set_enabled(cli_key, provider_id, enabled)

Custom mode:
  sort_mode_providers_list(mode_id, cli_key)
  sort_mode_provider_add(mode_id, cli_key, provider_id)
  sort_mode_provider_remove(mode_id, cli_key, provider_id)
  sort_mode_providers_set_order(mode_id, cli_key, ordered_provider_ids)
  sort_mode_provider_set_enabled(mode_id, cli_key, provider_id, enabled)
```

前端 `routeTemplate*` service 根据 selection 分派到这些 IPC 命令。`SortModesView` 和 providers data model 不直接调用 default/sort-mode 细分命令。

### 4. 删除 provider 清理

删除 provider 时在同一事务内显式清理：

```sql
DELETE FROM sort_mode_providers WHERE provider_id = ?;
DELETE FROM default_route_providers WHERE provider_id = ?;
DELETE FROM provider_pool_order WHERE provider_id = ?;
DELETE FROM providers WHERE id = ?;
```

如果有 request logs 清理选项，继续保留当前行为。清理关联表应始终执行，不依赖 `clear_usage_stats`。

即使外键级联可用，也保留显式清理：

- 让领域意图清楚。
- 防御历史连接或测试 fixture 未启用外键。
- 更容易写单元测试。

### 5. 读取防御

模板成员列表查询必须 join 当前 providers：

```sql
FROM sort_mode_providers mp
JOIN providers p ON p.id = mp.provider_id
WHERE mp.mode_id = ?
  AND mp.cli_key = ?
  AND p.cli_key = ?
```

Default 查询同理 join `providers`。

读路径不返回孤儿 provider id。本阶段不在读取时做隐式清理；持续干净由写路径和删除 provider 的事务清理负责。

所有模板成员 list 查询返回顺序必须稳定：

```text
ORDER BY route_row.sort_order ASC, route_row.provider_id ASC
```

这样历史重复 sort_order 或迁移边界不会导致 UI 抖动。

### 6. Gateway 调用选择

Default 和自定义模板使用相同可调用条件：

```text
route row exists
route row enabled
provider enabled
provider belongs to cli
```

排序来自模板 row 的 `sort_order`。

如果当前模板没有任何可调用 provider，保持现有“无可用 Provider”错误路径，但 UI 要提前提示。

gateway 查询层要保留“模板成员存在但 provider 自身关闭”的可观测差异：列表编辑页展示成员，gateway 调用页只选择实际可调用 provider。这样用户能先配置模板，再稍后启用 provider。

## Data Flow

1. 供应商页面加载 providers、模板列表、active template、当前模板成员。
2. 用户选择 Default 或自定义模板。
3. 左侧资源池根据当前模板成员集合显示“加入/已在模板”。
4. 点击加入：调用 add 动作，后端追加成员，返回完整成员列表，前端刷新右侧。
5. 点击移除：调用 remove 动作，后端删除成员，返回完整成员列表。
6. 拖拽排序：调用 set order，只允许重排当前成员集合。
7. 切换开关：调用 set enabled。
8. 删除 provider：provider domain 在事务中清理关联表，相关 query invalidation 后模板列表不再显示该 provider。

## Implementation Slices

为降低风险，按以下顺序实现：

1. 后端数据完整性：
   - 为 `default_route_providers` 添加 `enabled`。
   - provider 删除事务显式清理三张关联表。
   - list 查询 join providers，过滤孤儿行。
   - 补后端测试。

2. 后端领域动作：
   - 增加 Default add/remove/set enabled。
   - 增加 sort mode add/remove。
   - 收紧 set order 只允许重排当前成员。
   - 更新 bindings。

3. 前端 service 统一：
   - 增加 `routeTemplate*` service。
   - 让 data model 只依赖统一 service。
   - 保持 query key 至少包含 `cli_key`、`selection.kind`、`modeId`，避免 Default、不同 custom mode、不同 CLI 之间缓存串线。

4. UI 统一：
   - Default 右侧列表改为模板成员列表。
   - Default 和 custom mode 共用开关、移除、拖拽行为。
   - 左侧资源池使用统一按钮/徽标文案矩阵。

5. 验证与清理：
   - 更新 frontend/backend tests。
   - 跑 generated bindings 检查。
   - 删除视图层对旧 default/sort-mode 分支的直接调用。

## Error Handling

- provider 不存在或不属于 CLI：返回 `SEC_INVALID_INPUT` 或 `DB_NOT_FOUND`，前端 toast。
- 重复添加：后端可幂等返回当前列表，前端也应禁用“加入”按钮。
- 添加 provider 时如果 provider 自身 disabled，仍允许加入模板，但返回 row 的可调用性由 UI 根据 provider.enabled 解释。
- 移除不存在成员：幂等返回当前列表，不把重复点击或缓存延迟升级成用户可见错误。
- set order ids 与当前成员集合不一致：返回 `SEC_INVALID_INPUT`，防止隐式增删。
- Default 删除/重命名：UI 不暴露，后端也不提供该能力。
- 删除 provider 后 query cache 失效，模板和 provider 列表同步刷新。
- bindings 变化后，旧 service 只作为兼容包装，不允许新视图代码直接使用旧命令。

## Testing

后端测试：

- 删除 provider 会清理 `sort_mode_providers`、`default_route_providers`、`provider_pool_order`。
- `list_mode_providers` 不返回孤儿 provider id。
- Default members 支持 enabled 字段。
- baseline 和 ensure migration 都包含 `default_route_providers.enabled`。
- ensure migration 会为缺失 Default row 的现有 provider 幂等补齐成员，且不覆盖已有 enabled/order。
- Gateway Default 查询同时要求 route row enabled 和 provider enabled。
- add/remove/order/enabled 动作分别测试，不再通过 set order 隐式改变成员集合。
- set order 传入缺失、额外或重复 ids 会失败。
- 移除不存在成员幂等返回当前列表。
- 添加 disabled provider 到模板成功，但 gateway 不调用它。

前端测试：

- Default 和自定义模板都显示右侧开关。
- Default 不显示删除/重命名能力。
- 左侧资源池对已在模板 provider 显示“已在模板”，未加入显示“加入”。
- 添加 provider 调用 add 动作，而不是 set order。
- 删除 provider 后模板视图不再显示该 provider。
- provider 自身 disabled 与模板内 disabled 的徽标/文案不混淆。
- Default 和 custom mode 切换时 query cache 不串线。
- set order mutation 只在拖拽排序时触发；加入/移除不触发 set order。

验证命令：

- `cd src-tauri && cargo test sort_modes providers --lib`
- `pnpm test:unit -- src/pages/providers/__tests__/SortModesView.test.tsx src/pages/providers/__tests__/ProvidersView.test.tsx`
- `pnpm tauri:check`
- 如 bindings 变化，运行并检查 `pnpm tauri:gen-types` 与 `pnpm check:generated-bindings`

## Success Criteria

- 删除 provider 后，Default 和所有自定义模板都不会再显示该 provider。
- Default 与自定义模板在右侧调用顺序中都有模板内启用开关。
- 添加 provider 到模板是明确的“加入”动作，不再依赖完整顺序重写。
- 排序只排序当前成员，不隐式增删。
- Gateway 实际调用顺序和 UI 模板成员、开关一致。
- provider 自身 disabled 和模板内 disabled 在 UI 中语义清楚，且 gateway 只调用两者都开启的 provider。
- 页面布局仍保持现有双栏结构，没有大范围重做。
