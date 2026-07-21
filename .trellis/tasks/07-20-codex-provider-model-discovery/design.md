# Codex 供应商模型发现与受管 Profile 设计

## 目标与边界

首版交付一条完整但受控的链路：

```text
Codex 直连供应商
  -> 固定供应商获取 /v1/models 或手工录入
  -> provider-scoped 持久化模型
  -> 用户按需创建 <name>.config.toml
  -> Codex 以 model_provider = "aio" 请求 aio/<model_uuid>
  -> AIO 精确查表并固定供应商
  -> 上游收到 remote_model_id
  -> 日志保留 canonical alias，路由检测比较 wire model 与原始响应 model
```

首版不实现 native Anthropic/Gemini 发现、自动定时刷新、模型级跨供应商故障转移、WSL profile 同步或 profile 跨机器迁移。

## 决策记录

1. **Codex 始终只有 `aio` 一个 model provider。** 不生成 `aio-provider-*`，避免主 `config.toml` 的 provider 表、CLI proxy、remote compaction 与 profile 生命周期相互耦合。
2. **alias 使用 opaque model UUID。** 格式固定为 `aio/<canonical-model-uuid>`。UUID 只用于精确查表，不能从 alias 推导、猜测或信任 provider。
3. **provider 使用不可变 UUID。** 数值 `providers.id` 仍是当前数据库内部 ID；`provider_uuid` 用于配置导入和长期关联。
4. **首版自动发现仅为 OpenAI-compatible。** 只对 `cli_key = codex` 且没有 `source_provider_id` / `bridge_type` 的直连供应商开放；其他供应商使用手工模型。
5. **模型目录和 profile 是本机状态。** 配置包 v4 只携带 provider UUID，不携带模型缓存、手工模型、profile 元数据或 profile 文件。相同 UUID 的本机数据可在同机完整导入后继续使用；跨机器需要重新刷新和创建。
6. **数据库记录就是 profile ownership manifest。** 记录生成内容 SHA-256；未知同名文件不认领、不覆盖，已修改文件不删除。无需再维护第二份 sidecar manifest。
7. **profile 首版创建/删除，不做原地改名或重绑。** 用户通过删除后重建完成变更，减少双文件重命名与回滚状态。
8. **受管模型绑定覆盖普通排序，但不绕过健康门。** alias 查到的 provider 是唯一候选；它仍必须启用，并继续经过 circuit/cooldown/limit/auth 等公共 gate。同供应商内部重试保留，跨供应商 failover 禁止。
9. **路由检测从比较语义上修复。** 不能靠 `aio/` 前缀或前端字符串特判消警；所有响应路径统一比较最终 wire model 与原始上游响应 model。

## 数据模型

数据库 schema 从 v39 升至 v40。UUID 由共享 Rust helper 生成规范小写 UUIDv4，不新增第三方依赖；导入值必须通过相同的 canonical 校验。

### `providers.provider_uuid`

```sql
provider_uuid TEXT NOT NULL
UNIQUE(provider_uuid)
```

- v39 -> v40 在事务内为每个现有 provider 生成不同 UUID，再建立唯一约束/索引和非空保护。
- 新增、OAuth 新增、复制、单供应商分享导入统一生成新 UUID。
- 普通编辑不接受客户端 UUID，也不修改 UUID。
- 完整配置 v4 导出/导入原样保留；包内非法或重复 UUID 在任何删除前失败。

### `provider_model_catalogs`

```sql
provider_id         INTEGER PRIMARY KEY
protocol            TEXT NOT NULL CHECK(protocol = 'openai_compatible')
stale               INTEGER NOT NULL DEFAULT 1
last_attempt_at     INTEGER
last_success_at     INTEGER
last_error_code     TEXT
FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
```

只持久化安全、结构化的错误码，不保存上游消息、body、URL 或凭据。

### `provider_models`

```sql
model_uuid          TEXT PRIMARY KEY
provider_id         INTEGER NOT NULL
remote_model_id     TEXT NOT NULL
source              TEXT NOT NULL CHECK(source IN ('discovered', 'manual'))
stale               INTEGER NOT NULL DEFAULT 0
last_seen_at        INTEGER
created_at          INTEGER NOT NULL
updated_at          INTEGER NOT NULL
UNIQUE(provider_id, remote_model_id)
FOREIGN KEY(provider_id) REFERENCES providers(id) ON DELETE CASCADE
```

- 同一 provider 内去重，不跨 provider 合并同名模型。
- 手工添加一个已发现模型时将其提升为 `manual`，后续刷新不得把它标记为远端消失。
- 成功刷新后，本次未出现的 `discovered` 行标记 stale；不删除。`manual` 行保持有效。
- provider 连接字段变化后 catalog 和全部 discovered 行标记 stale。

### `codex_managed_profiles`

```sql
profile_uuid        TEXT PRIMARY KEY
profile_name        TEXT NOT NULL
profile_name_key    TEXT NOT NULL UNIQUE
model_uuid          TEXT NOT NULL
content_sha256      TEXT NOT NULL
codex_home_path     TEXT NOT NULL
created_at          INTEGER NOT NULL
updated_at          INTEGER NOT NULL
FOREIGN KEY(model_uuid) REFERENCES provider_models(model_uuid)
  ON DELETE RESTRICT
```

Profile 名称限制为 1..64 个 ASCII 字符，格式 `[A-Za-z0-9][A-Za-z0-9_-]*`；`profile_name_key` 使用小写值，确保 Windows/macOS 大小写不敏感文件系统不会出现别名冲突。

## 配置导入、分享与删除

### 完整配置 v4

- `ProviderExport` 增加必填 `provider_uuid` 和可选 `source_provider_uuid`；v4 bridge 引用只按 UUID 恢复。
- v1-v3 在没有本机受管 profile 时继续兼容，为导入 provider 生成新 UUID并沿用旧数值引用映射。
- 若本机存在受管 profile，导入 v1-v3 因无法证明同一逻辑 provider 而在破坏性操作前拒绝。
- 导入 v4 前收集本机 profile 引用的 provider UUID。包内缺少任一 UUID 时拒绝并列出有界 profile 名称。
- 拿到 import lock 后、清表前，在同一数据库事务捕获本机 catalog/models/profiles 及其 provider UUID；通过预检后临时移除 profile 元数据以解除 model FK 限制，再执行现有 provider 替换。
- v4 provider 插入完成后建立 `provider_uuid -> new provider_id` 映射，只为包内仍存在的 UUID 重插本机 catalog/models，最后重插 profiles。包中移除且未被 profile 引用的本机 catalog/models 不恢复。
- 完整导入后所有保留的 discovered catalog 标记 stale，避免把不同连接配置下的历史结果宣称为新鲜。
- 若 v4 把被 profile 引用的 provider 改成非 Codex 直连供应商，或有效 Codex home 与 profile 创建时记录的 home 不一致，同样在清表前拒绝。

### 复制和单供应商分享

- 不修改 provider-share JSON schema，不携带 provider UUID、模型或 profile。
- 两条新增副本路径都通过后端生成新 provider UUID。
- 复制/分享后的模型目录为空，用户重新刷新或手工录入。

### 删除 provider / 模型

- provider 删除先查询 profile -> model -> provider 引用。存在引用时返回 `PROVIDER_MANAGED_PROFILE_REFERENCED` 和最多 20 个 profile 名，不级联删除。
- 无 profile 引用时，在同一事务删除 catalog/models 后删除 provider。
- 被 profile 引用的手工模型不能删除；远端发现模型刷新时只标 stale。

## 模型发现

### IPC

```text
provider_models_get(provider_id)
provider_models_refresh(provider_id)
provider_model_manual_upsert(provider_id, remote_model_id)
provider_model_manual_delete(provider_id, model_uuid)
```

返回独立的 `ProviderModelCatalog` DTO，包含 provider ID、刷新状态/时间、安全错误码和模型数组；不扩充高频 `ProviderSummary`。

### 传输和边界

- Rust 根据已保存 provider ID 读取 Base URL 与有效凭据，React 永不接收明文凭据。
- 逐个尝试该 provider 自身配置的 Base URL；不会调用公共 `/v1/models` failover 路径，也不会访问其他 provider。
- 使用现有上游 URL 规范化 helper，`.../v1` + `/v1/models` 只产生一个 `/v1`。
- `Authorization: Bearer <effective credential>`；禁止自动 redirect，避免跨 origin 传递凭据。
- connect timeout 5 秒，总刷新 deadline 15 秒；响应上限 2 MiB；最多 2048 个模型；ID 最多 256 字符且禁止控制字符；额外字段忽略。
- 顶层必须是 object 且 `data` 必须是 array。任一 item 缺少合法 `id`、结果为空、超限或 JSON 畸形时整次刷新失败，最近成功模型不变。
- 401、403、404/405、timeout/network、invalid response、empty、limit 分别映射为稳定错误码。错误和日志不得包含 API key、OAuth token、完整 URL 或原始响应 body。
- 成功解析完全部响应后才开启一次数据库事务更新模型和 catalog；失败只更新 catalog 的安全 attempt/error 状态。

## Codex Profile 文件

### IPC 与 DTO

```text
codex_managed_profiles_list()
codex_managed_profile_create(profile_name, model_uuid)
codex_managed_profile_delete(profile_uuid)
```

列表通过 join 投影 profile、当前 provider ID/name、model UUID、remote model ID、文件状态；前端不自行拼接路径或 TOML。数据库保存创建时解析出的 canonical Codex home，后续只操作该 home 下的目标，环境变量漂移不能把删除/更新指向另一个目录。

### 生成格式

目标为当前 Codex home 下的 `<profile_name>.config.toml`：

```toml
model = "aio/<model_uuid>"
model_provider = "aio"
```

使用 `toml_edit` 生成并通过 TOML 解析回读，不做字符串插值。写入流程由进程级锁串行化，并遵守：

1. 新建时目标必须不存在；先写同目录 writer-owned 临时文件，再以 no-clobber 方式激活。
2. 文件激活后插入 DB；DB 失败时只在磁盘 hash 仍等于本次生成 hash 时删除本次文件。
3. 删除时若文件缺失，直接删除 DB 记录；若 hash 匹配，移除文件和记录；若 hash 不匹配，保留文件但解除 DB 管理，并在结果中告知 UI `external_file_preserved = true`。
4. 列表把磁盘状态投影为 `managed | missing | modified`。任何未知同名文件都不能被自动认领或覆盖。

主 `config.toml` 中现有 `[model_providers.aio]` 仍由 CLI proxy 负责；profile 服务不写任意 `model_providers` 表。

## 网关受管路由

### 解析与选择

在 `ModelInferenceMiddleware` 之后增加受管模型解析步骤：

1. 非 `aio/` 模型原样进入现有链路。
2. `aio/` 开头但 UUID 非 canonical、查无模型、provider/模型关系失效或 provider 已变成 bridge/source 时，以稳定 4xx 错误失败关闭。
3. 查表成功后保存 `ManagedModelRoute { canonical_model, model_uuid, provider_id, remote_model_id }`，并将请求 JSON 的 model 改为 remote ID；`requested_model` 始终保留 canonical alias。
4. 外部 forced-provider 与绑定 provider 不同则拒绝；相同可继续。
5. provider resolution 直接加载唯一的当前 Codex provider，忽略普通 sort/session 候选顺序，但继续执行公共 provider gate。禁用、缺失、circuit/cooldown/limit/auth 失败时不切换其他 provider。
6. 同一 provider 的显式重试策略仍生效；候选 provider 数固定为 1。

查表结果必须来自服务端持久化行。前缀、模型名、provider 显示名和客户端提交的 provider ID 都不是授权依据。

### 三层模型与路由检测

```text
canonical requested model: request_logs.requested_model = aio/<model_uuid>
wire model per attempt:    最终序列化给本次上游的 model
observed upstream model:   bridge/fixer/plugin 之前原始响应中的 model
```

- 四类响应路径（完整非流式、body-buffer、正常 SSE、SSE 提前终结）调用同一比较 helper。
- wire 与 observed 都存在且相同：不生成异常。
- 都存在且不同：生成 `model_route_mapping` 严重告警。
- observed 缺失/不可解析/因有界读取不可见：`unobserved`，不告警也不宣称匹配。
- SSE 出现多个不同模型时保存 conflict 证据；任何一个不同值都不能被后续相同值覆盖。
- alias 还原只写中性、provider-scoped 的 `aio_managed_model_route` special setting，不写异常 mapping。

### 日志与成本

`aio_managed_model_route` 至少包含 canonical model、provider ID、remote/wire/priced model 和 applied 状态。前端将其作为中性路由信息；不新增 `aio/` 字符串白名单。

计价模型优先级：

1. 与最终 provider 匹配的现有 bridge/cx2cc cost basis；
2. 与最终 provider 匹配的 `aio_managed_model_route.pricedModel`；
3. 兼容回退 `request_logs.requested_model`。

只有成功终态 provider 可选择第二项，失败 attempt 不得污染成本。

## 前端交互

- 新增/编辑 Codex 直连 provider 提供“保存”和“保存并获取模型”。后者先完成本地保存，再刷新；刷新失败显示“供应商已保存，模型获取失败”，不回滚 provider。
- 现有 Codex 直连 provider 卡片增加模型目录入口。模型对话框提供刷新、最后成功时间、stale/error 状态、手工添加/删除和模型行。
- 每个模型行按需提供创建 profile 操作；已有 profile 在同一行展示并可删除。Profile 名称在小型确认对话框中输入。
- 列表 key 使用 `model_uuid`，同名模型不会跨 provider 合并。发现目录与 Codex app-server 能力目录使用不同 service、query key 和 DTO。
- 所有 mutation 只失效对应 provider model key及 managed profiles key；不把网络状态塞回 provider 列表快照。
- 中性 `aio_managed_model_route` 可在请求详情显示普通路由说明，但不得使用严重告警样式。

## 并发、失败与回滚

- provider refresh 以 provider UUID 为锁粒度；同 provider 并发刷新后发起者可以覆盖前一次，手工模型始终保留。
- profile 文件操作使用全局进程锁；DB 与文件的跨边界补偿只删除/恢复本次拥有且 hash 匹配的字节。
- 完整配置导入继续使用现有全局 import lock 和 SQLite transaction；UUID/profile 可重绑定预检发生在清库前。
- provider 普通保存不等待发现；“保存并获取”明确是两阶段 UI 操作。

## 兼容与回滚

- 非 `aio/` 请求完全沿用现有排序、会话和 failover。
- 既有用户若手工使用 `aio/...` 作为普通上游模型，升级后会因保留前缀而失败关闭；这是防伪合同，需在 release notes 中说明。
- schema v40 migration 事务失败时保持 v39 数据库；旧版应用不能读取新能力，但既有 provider 字段不被重写。
- UI/IPC 上线顺序由同一桌面版本保证；生成 bindings 必须与 Rust 一起提交。

## 验证矩阵

- migration：fresh/v39 backfill、UUID 唯一、重复迁移、provider CRUD/复制/分享/v1-v4 config import。
- discovery：URL 去重、同 provider 多 Base URL、无跨 provider、401/403/404/405、timeout、redirect、非 JSON、空/非法/超限、历史保留、手工保留、stale。
- profile：当前文件格式、TOML escaping、同名外部文件、缺失/修改文件删除、DB 失败补偿、provider/model 引用阻止。
- gateway：正常 alias、伪造/不存在/冲突 forced provider、禁用/bridge provider、无 failover、同 provider retry、四类响应检测、真实 mismatch、unobserved、SSE conflict、canonical log 与最终 provider 计价。
- frontend：保存与保存并获取的两阶段结果、provider cache 隔离、同名模型、手工回退、profile 创建/删除和 modified-file 提示。
