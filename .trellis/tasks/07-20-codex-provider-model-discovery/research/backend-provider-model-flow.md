# Codex 供应商模型发现：Rust 后端现状与实现边界

## 调研范围与结论

本文件只覆盖仓库内 Rust 后端：供应商 CRUD、数据库字段、网关 `/models`、Codex 模型目录、Codex 配置同步和模型路由。前端交互与 Codex 官方配置语义由同任务的其他研究文件负责。

结论如下：

1. 当前“保存供应商”只写数据库并清理相关运行时状态，不会向上游发现模型。
2. 网关已经能代理 Codex 的 `GET /v1/models`，但它返回第一个成功供应商的原始响应，失败时还可能切换到另一个供应商；因此它不能作为带供应商归属的模型目录。
3. 现有 `codex_models_list` 来自 Codex App Server 的 `model/list`，表示“当前 Codex 配置可见的模型快照”，不是按 AIO 供应商采集的目录。
4. 当前路由不会根据请求模型筛选 AIO 供应商；`model_mapping` 仅做模型名改写，不能表达归属。
5. `[profiles.grok] model_provider = "aio"` 只表示请求发往 AIO 这个 Codex 传输后端，不能唯一指向某个数据库 `provider_id`。若多个 AIO 供应商存在同名模型，仅靠该 profile 无法消歧。
6. 后端 MVP 应显式以 `provider_id + model_id` 保存归属，模型发现必须固定到被刷新的供应商；不应复用当前会跨供应商 failover 的公共 `/v1/models` 结果来反推归属。

## 术语边界

| 名称 | 当前含义 | 是否能表示模型归属 |
| --- | --- | --- |
| Codex `model_provider` | `config.toml` 中的传输后端键，目前代理通常写为 `aio` | 否；`aio` 只指向聚合网关 |
| AIO `provider_id` | `providers` 表中实际上游供应商的主键 | 是 |
| `model_mapping` | 将请求模型名改写为上游模型名 | 否 |
| `supported_models_json` | 数据库遗留字段，当前业务未接入 | 理论可存，但没有稳定 schema 和生命周期 |
| `CodexModelCatalog` | Codex App Server `model/list` 的临时能力快照 | 否；DTO 不带 AIO `provider_id` |

## 当前数据流

### 供应商保存

```text
ProviderUpsertInput
  -> Tauri provider_upsert
  -> app::provider_service::provider_upsert
  -> domain::providers::upsert_provider
  -> providers 表事务
  -> 清理路由/健康度等运行时状态
```

- IPC 输入位于 `src-tauri/src/app/provider_service.rs:5`，已有 `model_mapping`、`availability_test_model` 等字段，没有模型目录或发现选项。
- Tauri 命令只是转发：`src-tauri/src/commands/providers/crud.rs:18`。
- 服务入口位于 `src-tauri/src/app/provider_service.rs:134`，数据库调用从该文件 `:176` 开始；该链路没有网络请求。
- 持久化实现位于 `src-tauri/src/domain/providers/queries.rs:1110` 附近。
- 命令注册表只有 `providers_list`、`provider_upsert`、`provider_duplicate`、`provider_delete` 等供应商 CRUD，见 `src-tauri/src/commands/registry.rs:96`；全仓不存在 `provider_models_*` 命令。

### 供应商返回模型

- `ProviderSummary` 定义于 `src-tauri/src/domain/providers/types.rs:288`，暴露 `model_mapping` 和 `availability_test_model`（`:295-296`），不暴露 `supported_models_json`。
- 网关使用的供应商结构同样只有 `model_mapping` 等运行字段，见 `src-tauri/src/domain/providers/types.rs:337` 附近。
- 因此即使数据库内存在模型数据，当前列表、详情、生成绑定和网关候选均无法读取它。

## `supported_models_json` 是遗留字段，不是现成目录

- 字段在基线 schema 中定义为 `TEXT NOT NULL DEFAULT '{}'`：`src-tauri/src/infra/db/migrations/baseline_v25.rs:36`。
- 新建供应商固定写入 `{}`：`src-tauri/src/domain/providers/queries.rs:1381-1392`。
- 普通供应商更新会无条件执行 `supported_models_json = '{}'`：`src-tauri/src/domain/providers/queries.rs:1649-1653`。
- `ProviderSummary`、`ProviderForGateway` 均不解析该字段。
- 完整配置导入导出仍会原样搬运它：`src-tauri/src/infra/config_migrate/export.rs:55`、`src-tauri/src/infra/config_migrate/import.rs:95`。
- 单供应商分享会明确写 `{}`：`src-tauri/src/domain/providers/share.rs:1556`。
- v25 到 v26 迁移曾清除 Claude 的旧模型配置：`src-tauri/src/infra/db/migrations/v25_to_v26.rs:217`。

因此不能把该列直接接到新 UI。若复用，至少要先定义版本化 JSON schema，并同时修复创建、普通更新、复制、分享、完整配置迁移和旧数据迁移，否则一次普通编辑就会静默清空模型目录。若模型目录将参与查询、消歧或路由，更适合新增规范化表。

## 三种“模型列表”能力不是同一件事

| 能力 | 来源 | 当前行为 | 主要问题 |
| --- | --- | --- | --- |
| 网关 `GET /v1/models` | 某个上游供应商 | 普通代理，返回首个成功响应 | 可跨供应商 failover，响应没有 AIO 归属 |
| `codex_models_list` | 已安装 Codex App Server | JSON-RPC `model/list`，分页返回 Codex 可见目录 | 依赖当前 Codex 配置，不按 AIO 供应商分组 |
| `supported_models_json` | `providers` 表 | 无业务读写，upsert 会清空 | schema 和生命周期均未定义 |

### 网关 `/v1/models`

- 请求识别支持 `/v1/models` 与 `/models`：`src-tauri/src/gateway/proxy/mod.rs:56-61`。
- 模型发现请求不写普通请求日志，并保持健康度中立：`src-tauri/src/gateway/proxy/handler/mod.rs:159` 附近。
- 单个供应商严格尝试一次，但仍允许换到下一个供应商：`src-tauri/src/gateway/proxy/handler/runtime_settings.rs:76`、`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:159`。
- 路由测试覆盖了“首个供应商失败后切换到第二个”“每个供应商只请求一次”“不修改熔断状态”：`src-tauri/src/gateway/routes.rs:5291`、`:5341`、`:5419`。
- URL 拼接可避免 base URL 已含 `/v1` 时重复追加：`src-tauri/src/gateway/util.rs:501`。
- Codex API key 请求使用 Bearer：`src-tauri/src/gateway/cli_auth/codex.rs:8`。

这条代理路径适合满足 Codex 客户端自身的模型请求，不适合持久化“供应商 A 支持哪些模型”。一旦发生 failover，请求目标和保存归属就会不一致。

### Codex App Server 目录

- IPC 命令位于 `src-tauri/src/commands/cli_manager.rs:32`。
- 实现从 `src-tauri/src/infra/codex_model_catalog/mod.rs:58` 启动已安装的 Codex App Server。
- 协议层调用 JSON-RPC `model/list` 并处理分页：`src-tauri/src/infra/codex_model_catalog/protocol.rs:107`、`:154`。
- 模型 DTO 只有 `id/model/display_name/hidden/is_default/reasoning efforts` 等 Codex 能力字段，见 `src-tauri/src/infra/codex_model_catalog/mod.rs:26` 附近，没有 AIO `provider_id`，结果也不持久化。

它可继续用于 Codex 设置页的“当前客户端可选模型”，但不能替代供应商目录。

## 当前模型路由不会识别供应商归属

- 候选供应商只按 `cli_key`、启用状态、默认路由/排序模式和会话绑定选择：`src-tauri/src/gateway/proxy/handler/provider_selection.rs:15`、`src-tauri/src/domain/providers/queries.rs:686`。
- `requested_model` 在中间件中被提取，用于诊断和日志：`src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs:31`，没有参与初始候选过滤。
- `model_mapping` 在协议桥接阶段改写模型名：`src-tauri/src/gateway/proxy/protocol_bridge/registry.rs:108`，语义不是“模型到供应商”。
- `src-tauri/src/gateway/model_route_mapping.rs:25` 记录请求模型与实际响应模型不一致的审计信息，也不参与选择。

已有一条可复用的强制供应商路由：`/:cli_key/_aio/provider/:provider_id/*path`，见 `src-tauri/src/gateway/routes.rs:53`。它通过 `x-aio-provider-id` 注入目标，之后只保留该候选：`src-tauri/src/gateway/proxy/handler/early_error.rs:91`。但目标仍须处于当前启用/路由候选集合。

## Codex 配置与 profile 边界

- `CodexConfigState` / `CodexConfigPatch` 定义于 `src-tauri/src/infra/codex_config/types.rs:5`、`:53`，结构化字段没有 `model_provider`、`model_providers` 或 `profiles`。
- 结构化读取只解析根级 `model` 等白名单字段：`src-tauri/src/infra/codex_config/parsing.rs:210`。
- 结构化 patch 只写这些根字段；仅 remote compaction 开关会在 `aio` 与 `OpenAI` 之间改写 `model_provider` 和 provider 表：`src-tauri/src/infra/codex_config/patching.rs:814-834`。
- raw TOML 读取、验证和保存允许未知表，因此 `[profiles.grok]` 可以通过 raw 编辑保留；验证入口见 `src-tauri/src/infra/codex_config/parsing.rs:474`。
- raw save 不触发 provider sync；结构化 patch 也只有 remote compaction 变化才触发：`src-tauri/src/infra/codex_config/mod.rs:199`、`:328-340`。
- provider sync 只读取根级 `model_provider`：`src-tauri/src/infra/codex_provider_sync.rs:305`，允许的托管目标仅为 `aio` 或 `OpenAI`；对应拒绝其他 key 的测试见 `src-tauri/src/infra/codex_config/tests.rs:879-901`。它不会遍历 `[profiles.*]`。
- CLI proxy 固定维护单一 `[model_providers.aio]`：`src-tauri/src/infra/cli_proxy/codex.rs:421`、`:708-714`；WSL 另有一套 shell/awk 写入路径：`src-tauri/src/infra/wsl/config_codex.rs:50` 起。

因此示例：

```toml
[profiles.grok]
model = "grok-4.5"
model_provider = "aio"
```

在现有架构中是合法的 raw TOML 内容，但其准确语义是“使用模型名 `grok-4.5`，请求交给 AIO 聚合代理”。它并不表达“必须选择数据库中的 Grok 供应商”。

## 可复用后端能力

- `ProviderTransportContext` 可提供供应商传输上下文：`src-tauri/src/domain/providers/types.rs:365`。
- API key / OAuth 凭据解析及 OAuth 刷新逻辑：`src-tauri/src/domain/providers/queries.rs:970`。
- 上游 base URL 解析：`src-tauri/src/gateway/proxy/failover.rs:146`。
- 规范化 URL 拼接：`src-tauri/src/gateway/util.rs:501`。
- 可用性测试已有连接超时、总超时和 bounded body 模式：`src-tauri/src/domain/provider_availability.rs:14`、`:287`。相关 loader/reader 当前为私有，实施时应抽出共享能力，避免复制鉴权和读取限制。

## 推荐 MVP 后端边界

### 1. 发现动作显式绑定供应商

新增独立命令，而不是把网络请求塞进 `provider_upsert`：

- `provider_models_refresh(provider_id)`：只访问该 `provider_id` 的上游；首版只适配 OpenAI 兼容 `GET /v1/models`。
- `provider_models_list(provider_id)`：返回最近成功目录及刷新状态。
- 可选 `provider_models_set_manual(provider_id, models)`：为没有模型接口或不兼容的供应商提供手工录入。

保存供应商后可由 UI 提示或主动触发刷新，但保存事务本身不应等待外部网络。刷新失败应保留上次成功目录并返回可诊断错误，不能清空已有数据。

发现请求不得跨 `provider_id` failover。一个供应商内部若有多个 base URL，可以遵守该供应商的 base URL 策略，但最终结果的归属始终是该 `provider_id`。

### 2. 使用稳定复合身份

建议新建规范化 `provider_models` 表，最小字段为：

```text
provider_id          FK -> providers.id
model_id             上游返回的原始 ID
display_name         可空
source               discovered | manual
last_seen_at         可空
metadata_json        有界的原始扩展信息，可空
UNIQUE(provider_id, model_id)
```

所有 IPC 和路由引用都使用 `provider_id + model_id`，绝不根据 `grok-*`、`claude-*` 等名称前缀猜供应商。若只做短期展示缓存，也至少应定义带版本号的 JSON schema；但只要后续需要筛选、同名消歧和路由，规范化表更稳妥。

### 3. 首版保持单一 Codex `aio` provider

推荐先继续生成 `model_provider = "aio"`，把 profile 当作用户可选的模型预设。若要让 profile 中的模型自动选定 AIO 供应商，应另加显式的模型路由策略：

```text
(cli_key, requested_model)
  -> 匹配声明支持该模型的 provider_id 集合
  -> 0 个：沿用现有路由或明确报错（产品决策）
  -> 1 个：固定该供应商
  -> 多个：按用户配置的优先级/默认项选择，不能按名字猜测
```

不要在模型发现 MVP 中顺带改变现有 failover 语义。目录展示和模型感知路由应作为两个可独立验收的阶段。

### 4. 暂不为每个 AIO 供应商生成 Codex provider key

另一种方案是为每个 AIO `provider_id` 生成独立 Codex `[model_providers.<key>]`，其 base URL 指向已有强制供应商路由。它能天然消歧，但会带来以下非 MVP 影响：

- 绕开或改变聚合 failover；
- 供应商重命名、删除、禁用时需要维护 Codex 配置生命周期；
- 当前 provider sync 只接受 `aio` / `OpenAI`，结构化 config DTO 也不支持任意 provider/profile；
- Windows 与 WSL 两套配置写入路径都要同步改造；
- 需要定义稳定 key，不能直接依赖可变名称。

除非产品明确要求“Codex profile 直接锁定供应商”，否则应保留为后续架构选项。

## 风险与验证重点

1. **归属污染**：发现接口任何跨供应商 failover 都会把 B 的模型写到 A 名下。
2. **同名模型**：模型 ID 不是全局主键，UI、IPC 和路由必须携带 `provider_id`。
3. **普通编辑清空**：若复用 `supported_models_json`，现有 upsert 会清空它。
4. **凭据与日志**：发现应复用现有传输上下文，不把 API key、OAuth token 或完整原始响应写日志。
5. **资源限制**：模型响应必须有连接/总超时、重定向策略和 body/item/string 上限；不能无界解析第三方 JSON。
6. **兼容响应**：首版只承诺 OpenAI 兼容 `{ data: [{ id }] }`；缺少接口、401/403、非 JSON、缺少 `id` 等应返回结构化错误并允许手工录入。
7. **删除与导入导出**：新表需外键级联或显式清理，并决定完整配置迁移、单供应商分享是否携带目录缓存。
8. **路由回归**：增加模型感知路由时必须覆盖无目录、目录过期、同名、禁用供应商、会话绑定和现有排序/failover 的组合。

## 后端建议拆分

1. **模型目录阶段**：schema/DTO、固定供应商刷新、手工录入、缓存保留和边界测试；不改变请求路由。
2. **配置呈现阶段**：前端选择模型并生成/编辑 profile；仍使用单一 `aio` Codex provider。
3. **模型感知路由阶段**：经单独产品决策后，引入显式 `(cli_key, model_id) -> provider_id` 策略和同名消歧。
4. **独立 Codex provider 阶段（可选）**：只有确需 profile 锁定上游时，再扩展 config DTO、provider sync、CLI proxy 与 WSL 生命周期。

这一拆分能先交付可靠的“供应商模型获取与选择”，同时避免把目录采集、Codex TOML 管理和网关路由三类风险绑在一次改动里。
