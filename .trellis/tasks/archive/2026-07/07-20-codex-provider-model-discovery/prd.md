# Codex 供应商模型发现与配置

## Goal

为 AIO Coding Hub 实现一套稳定的 Codex 第三方供应商模型发现与配置能力：用户保存 Codex 直连供应商后，可以主动获取或手工录入该供应商的模型，并按需创建 Codex profile。Codex 始终通过唯一的 `model_provider = "aio"` 连接 AIO 网关，再由网关根据服务端受管模型关联把请求固定发往对应的 AIO 供应商。

## User Value

- 减少手工查找和输入第三方模型 ID 的成本。
- 即使不同供应商暴露同名模型，也能看出并选择真实的上游供应商。
- 模型列表接口不可用时，供应商仍可保存、已有目录仍可使用，并可手工补录模型。
- 生成的配置符合当前 Codex 版本，而不是继续写已废弃的 profile 格式。
- 创建 Profile 后，用户可以在新建或重启的 Codex 会话中直接通过 `/model` 看见并选择该模型。

## Confirmed Product Decisions

- Codex 侧只保留一个 AIO model provider：`model_provider = "aio"`。
- 不为每个 AIO 供应商生成 `aio-provider-<id>` 等独立 Codex model provider。
- 模型发现后只更新该供应商的模型目录，不自动为所有模型批量生成 profile。
- 用户从某个供应商的模型目录中按需创建 profile；该 profile 的请求先进入 AIO 网关，再由 AIO 路由到用户选择的供应商。
- 受管 profile 使用 AIO 生成的稳定、供应商限定模型标识；网关解析该标识并向上游恢复真实模型 ID，以支持不同供应商的同名模型。
- 预期的“AIO 模型标识 -> 上游真实模型 ID”还原不得触发现有模型路由检测误报。
- 供应商仍被一个或多个受管 profile 引用时阻止删除，并向用户列出引用项；用户必须先删除 profile 或重新绑定供应商。
- 首版自动发现只支持 Codex 直连供应商的 OpenAI-compatible `GET /v1/models`；Anthropic/Gemini native 统一使用手工录入回退。
- 新增不可变 `provider_uuid` 作为供应商跨完整配置导入的稳定身份；普通编辑和完整配置 v4 导入保留 UUID，供应商复制和单供应商分享导入生成新 UUID。
- 每个供应商模型拥有独立、不可猜测的 `model_uuid`；新 Codex profile 写入可读的 `aio/<profile_name_key>`，服务端先精确查 Profile 再恢复 `model_uuid + provider + remote_model_id`。旧 `aio/<model_uuid>` 继续精确兼容，两种 alias 都不暴露或信任数值 `provider_id`。
- 模型目录和受管 profile 首版保持为本机状态，不进入完整配置包或单供应商分享包。完整配置导入通过 `provider_uuid` 保留可重绑定的本机数据；若导入会让现有受管 profile 失去供应商则在破坏性操作前整体拒绝。
- 受管 profile 元数据和生成内容哈希保存在数据库；`$CODEX_HOME/<name>.config.toml` 是派生文件。创建时不覆盖同名外部文件，删除时不删除已被外部修改的文件。
- AIO 在 Codex CLI 代理启用期间维护完整合并模型目录；Codex 仍只有 `aio` 一个供应商，模型选择器中的可读 alias 由服务端 Profile 绑定精确解析。
- 新 Profile alias 使用 `aio/<profile_name_key>`；旧测试版的 `aio/<model_uuid>` 继续兼容，不能因可读 alias 改造破坏已有 Profile。
- 模型能力配置归属 provider-scoped 模型条目，而不是 Profile；同一模型创建的多个 Profile 共享推理强度和上下文窗口配置。
- 新发现或新手工添加的模型必须先明确配置能力再创建 Profile；推理能力可明确配置为“不发送推理强度”，上下文窗口可明确保留为未知。
- v40 已有模型升级到 v41 时回填当前 `low / medium / high`、默认 `medium` 的兼容基线，避免现有受管 Profile 升级后失效；不根据供应商名或模型名推断新模型能力。
- `codex-auto-review` 等未使用受管 `aio/<profile_name_key>` / `aio/<model_uuid>` 的 Codex 系统模型请求不绑定当前业务 Profile 的供应商；它们仍通过唯一的 `aio` provider 进入网关，沿用普通供应商路由和跨供应商 failover。路由到其他支持该系统模型的供应商属于预期行为。

## Confirmed Facts

### Codex 配置

- Codex 原生将 `model` 与 `model_provider` 分开配置；自定义 `model_provider` 指向用户级 `[model_providers.<id>]` 连接定义。
- Codex `0.134.0+` 不再读取主 `config.toml` 中的 `[profiles.<name>]`。当前 profile 是 `$CODEX_HOME/<name>.config.toml` 独立文件，其中直接写顶层配置：

  ```toml
  model = "grok-4.5"
  model_provider = "aio"
  ```

- `model_provider = "aio"` 只表示请求发给 AIO 聚合网关，不代表某个 AIO 数据库供应商，也不能解决同名模型归属。
- `model_provider` / `model_providers` 必须位于用户级 Codex 配置；项目级 `.codex/config.toml` 会忽略这些键。
- 本机 Codex `0.144.6` 回环实验确认，自定义 provider 会把 profile 中的任意模型字符串原样用于 `POST /v1/responses`。

### 仓库现状

- 当前供应商保存链路只持久化本地数据，不执行模型发现。
- AIO 网关已代理 `GET /v1/models` 与 `/models`，但会在供应商之间 failover 并返回首个成功响应；结果不带 AIO `provider_id`，不能用于采集某个供应商的目录。
- 现有 `codex_models_list` 来自 Codex App Server 的 `model/list`，是 Codex 能力目录，不是逐 AIO 供应商的上游模型列表，其 DTO 也没有 `provider_id`。
- `availability_test_model` 只用于探活；`model_mapping` 只用于运行时模型名改写；二者都不是供应商模型目录。
- 当前网关按 CLI、排序模板、会话和启用状态选供应商，不按请求模型归属筛选。
- 网关已有 `/:cli_key/_aio/provider/:provider_id/*path` 强制供应商路由，可作为未来显式绑定方案的基础，但当前 Codex 配置同步只管理单一 `aio` provider。
- 现有 Codex 模型路由检测直接比较请求模型与原始上游响应模型；除 `codex-auto-review*` 展示特例外，任何不一致都会生成 `model_route_mapping` 并在前端显示为严重告警。
- 路由检测覆盖完整非流式、body-buffer 非流式、正常 SSE 和 SSE 提前终结四类响应路径；新受管模型语义必须在四条路径上保持一致。
- 数据库遗留 `supported_models_json` 没有稳定 schema，普通 provider upsert 会把它重置为 `{}`；不能未经迁移和生命周期设计直接复用。
- 前端、生成绑定、service 与 query 层目前都没有 provider-scoped 模型目录契约，也没有当前 Codex profile 文件管理能力。

### 发现协议

- OpenAI、xAI、NewAPI 默认模式及 Gemini OpenAI compatibility 可通过 OpenAI 风格模型列表纳入首版候选。
- Anthropic native 与 Gemini native 虽然支持列举模型，但鉴权、响应结构和分页都不同，需要独立 adapter。
- 第三方 `/models` 结果只能证明模型 ID 可见，不能证明该模型兼容 Codex Responses API，也不能伪装成带推理档位等元数据的 Codex 能力目录。
- 模型身份必须是 `(provider_id, remote_model_id)`；模型前缀、`owned_by`、显示名和供应商名称都不能用于反推归属。
- 当前完整配置导入会删除并重新插入 provider，自增 `provider_id` 不稳定；长期 alias 必须使用不可变 UUID 查表，而不是数值 ID。

## Requirements

### R1. Provider-scoped 模型目录

- 每个目录条目必须显式携带 `provider_id` 与 `remote_model_id`。
- 每个目录条目还必须拥有稳定 `model_uuid`；新 `aio/<profile_name_key>` 和旧 `aio/<model_uuid>` 都只接受服务端已有的精确绑定，不能从字符串猜测供应商。
- 同一供应商内按模型 ID 去重；不同供应商的同名模型必须作为不同条目保留和展示。
- 目录必须区分远端发现项与手工项，并记录最近一次成功刷新状态；完整模型数组不得塞入高频读取的 `ProviderSummary`。

### R2. 固定供应商发现

- 发现动作只接受已保存的 `provider_id`，由 Rust 后端读取该供应商的 Base URL 和凭据，前端不得接触明文凭据。
- 发现请求不得跨 AIO 供应商 failover；否则会污染模型归属。
- 请求构造必须正确处理 Base URL 已包含版本路径的情况，不能盲目拼接出 `/v1/v1/models`。
- 首版发现协议应显式选择或持久化，不能通过并发尝试多个厂商协议后按首个 `200` 猜测类型。
- 首版协议固定为 `openai_compatible`，且只对 `cli_key = codex`、无 bridge/source 引用的直连供应商开放。

### R3. 保存、刷新与手工回退

- 普通“保存供应商”保持确定性的本地动作，不以远端模型接口成功为前提。
- UI 提供独立的“保存并获取模型”和已有供应商“刷新模型”动作。
- 发现失败、鉴权失败、超时、接口缺失、畸形响应或空列表都不得清空最近一次成功目录，也不得删除手工项。
- 用户始终可以手工添加模型；修改 Base URL 或认证信息后，旧发现结果应标记为过期而不是静默删除。

### R4. 安全与资源边界

- 发现请求必须限制连接时间、总时长、响应体大小、模型数量、字符串长度和分页次数。
- 跨 origin 重定向不得携带供应商凭据。
- 日志和错误不得包含 API key、OAuth token 或完整原始响应体。
- 错误至少区分未授权、禁止访问、接口不支持、网络/超时、响应畸形和空列表。

### R5. Codex profile 与供应商归属

- 首版按用户操作创建 profile，不根据发现目录自动批量生成。
- Profile 必须写当前 `$CODEX_HOME/<name>.config.toml` 格式，不能生成旧式 `[profiles.<name>]`。
- 所有受管 profile 都写 `model_provider = "aio"`；不得为单个 AIO 供应商生成额外的 Codex model provider 定义。
- Profile 的产品数据必须显式保存 profile 名和 `model_uuid`，查询 DTO 必须投影当前 `provider_id + remote_model_id`，不能只保存模型名后再猜供应商。
- AIO 不得覆盖无法验证为自身管理的外部 profile 或自定义 `model_providers` 配置。
- Profile 首版只支持创建和删除；改名或重新绑定通过删除后重新创建完成。

### R6. 受管 profile 路由

- 受管 profile 发出的请求必须先进入现有 AIO Codex 网关，再由网关把该模型解析为创建 profile 时选择的 `provider_id` 与远端 `model_id`。
- AIO 模型标识只有在服务端查到有效、持久化且 provider-scoped 的模型绑定时才是受管标识；不得仅根据 `aio/` 等字符串前缀信任客户端或抑制告警。
- 网关必须只在已启用且有效的目标供应商上执行该受管路由，不能因目标失败而静默落到未声明支持该模型的供应商。
- 受管 profile 对 Codex 暴露稳定的 AIO 供应商限定模型标识；网关必须在转发前恢复正确的远端模型 ID。
- 请求审计必须区分三层模型：Codex 请求的 canonical AIO 标识、每次 attempt 最终实际发送给上游的模型、从 bridge/fixer/plugin 前原始上游响应观察到的模型。
- `request_logs.requested_model` 保留 canonical AIO 标识；不能通过把它替换成远端模型来规避误报。
- `model_route_mapping` 只比较“本次最终发送的上游模型”和“原始上游观察模型”。预期的 canonical AIO 标识还原使用独立、provider-scoped 的中性受管路由审计记录，不得生成路由异常。
- 上游真实返回其他模型时仍须生成严重路由告警；响应缺少可验证的模型字段、解析失败或有界缓冲被截断时属于 `unobserved`，既不告警，也不宣称匹配已验证。
- 成本定价必须优先使用与最终供应商匹配的受管路由远端计价模型，同时保留 canonical AIO 标识用于请求审计和模型维度展示。
- 未由 AIO 管理的 profile 和普通 Codex 模型请求继续沿用现有排序、会话绑定和 failover 行为。

### R7. 兼容性与生命周期

- 供应商模型目录与 Codex 能力目录必须保持独立类型、IPC、缓存键和失效逻辑。
- 删除供应商前必须在同一后端操作中检查受管 profile 引用；存在引用时拒绝删除并返回有界的 profile 引用信息，不得级联删除 profile 或留下失联 profile。
- 供应商禁用时保留模型和 profile 但受管请求失败关闭；复制和单供应商分享不复制模型/profile，且生成新 `provider_uuid`。
- 完整配置 v4 保留 `provider_uuid`，同机导入后本机模型/profile 按 UUID 继续关联；包内 UUID 非法或重复、旧版包与本机受管 profile 无法安全重绑定时整体拒绝。
- 模型目录与 profile 首版不随配置包跨机器迁移；新机器导入供应商后需要重新刷新模型并创建 profile。
- 新能力不得改变未选择模型绑定时的现有供应商排序、会话绑定和 failover 行为。

### R8. Codex 内模型可见性

- 创建受管 Profile 后，AIO 必须将它加入 Codex 可读取的完整模型目录，不能只生成需要 `--profile` 才能使用的文件。
- 用户已有 `model_catalog_json` 时必须以该目录为基础；否则使用当前已安装 Codex 的版本匹配 bundled 目录。不能用 AIO 编译时固定快照覆盖用户或未来 Codex 字段。
- 合并目录必须保留基础目录全部模型和未知字段，并追加可见的 `aio/<profile_name_key>` 条目；不同 Profile 名必须映射到不同 slug。
- 根 `config.toml` 只在 Codex CLI 代理启用期间指向 AIO 生成目录；关闭代理后恢复原 `model_catalog_json` 值或缺失状态。
- 生成目录和配置写入必须有所有权验证、并发漂移检测和补偿。外部修改后失败关闭，不得静默覆盖或删除。
- 删除 Profile 必须同步移除目录条目；删除最后一个 Profile 后恢复基础目录配置，不留下失效的 picker 项。
- 目录是启动时配置，UI 必须提示“新建或重启 Codex 会话后生效”，不得宣称当前会话已热更新。

### R9. Provider-model 能力配置

- 每个 `provider_models` 条目必须持久化能力是否已配置、支持的 reasoning effort 集合、默认 effort 和可选上下文窗口；多个 Profile 只引用该模型级配置，不复制能力字段。
- 首版支持 Codex 当前稳定识别的 `none / minimal / low / medium / high / xhigh / max / ultra` 档位；集合不得重复，非空集合必须选择其中一个默认值，空集合表示不发送 reasoning。
- 上下文窗口为可选 token 数；填写时必须在首版有界范围内。生成 Codex 目录时将 `context_window` 与 `max_context_window` 写为相同值，`auto_compact_token_limit` 保持为空，由 Codex 自行派生。
- 新发现和新手工模型不得自动获得猜测能力；能力未配置时保留模型行，但后端和 UI 都必须阻止创建 Profile。
- 已有受管 Profile 的模型能力变更必须在同一所有权锁下重建受管 Codex 目录；外部目录/config 漂移时能力更新失败关闭，不能留下数据库与当前目录不一致。
- 能力更新不改变 canonical/wire/observed 模型比较，也不增加 reasoning effort 告警豁免：上游未回显 effort 仍不告警，明确回显不同 effort 仍按现有规则告警。

## Proposed MVP Boundary

- Codex 直连供应商的 provider-scoped 持久化模型目录。
- 用户主动刷新，以及不依赖发现成功的手工录入。
- OpenAI-compatible 列表解析；Anthropic/Gemini native 暂用手工回退。
- 刷新失败保留最近一次成功目录，远端消失项标记过期而不立即删除。
- 用户从指定供应商的目录按需创建受管 profile，不自动批量创建。
- 受管 profile 始终使用单一 `model_provider = "aio"`，由 AIO 网关把请求固定到所选供应商。
- 完整配置包只迁移稳定供应商 UUID，不迁移模型缓存、手工模型或 profile 文件；该取舍作为首版决策记录，后续再评估跨机器迁移。

## Acceptance Criteria

- [ ] 官方 Codex 配置结论有官方文档或可复现实验证据，生成示例使用当前独立 profile 文件格式。
- [ ] 固定刷新供应商 A 时，即使 A 失败也不会把供应商 B 的模型写入 A 的目录。
- [ ] 两个供应商返回相同模型 ID 时，查询、缓存、UI 选择和后续配置仍保留两个不同的供应商归属。
- [ ] OpenAI-compatible `data[].id` 可被有界解析，额外字段不会导致失败；缺失/非法 ID 会产生结构化错误。
- [ ] 401、403、404/405、超时、非 JSON、空列表和超限响应均不阻止供应商保存，也不清空历史目录或手工模型。
- [ ] 修改供应商连接信息后，已发现目录可见但被标为过期；手工项不被重置。
- [ ] 模型目录通过独立 DTO/query 获取，供应商列表不携带无界模型数组，前端不接收供应商凭据。
- [ ] 用户可从指定供应商的模型目录按需创建 profile，发现或刷新模型不会自动批量创建 profile。
- [ ] 受管 profile 的配置使用当前独立文件格式和唯一的 `model_provider = "aio"`，产品记录显式关联 `provider_id + model_id`。
- [ ] 通过集成测试证明受管 profile 请求经过 AIO 网关后只落到所选供应商，并向上游发送正确的远端模型 ID。
- [ ] 完整非流式、body-buffer 非流式、正常 SSE 和 SSE 提前终结四条路径中，AIO 标识被正常还原且上游返回预期模型时均不生成 `model_route_mapping` 误报。
- [ ] 上游返回其他非预期模型时仍生成严重告警，且告警比较的是最终上游请求模型与原始上游响应模型，不是 canonical AIO 标识。
- [ ] 非法/伪造 AIO 前缀、绑定不存在或供应商不匹配时不会获得受管路由或告警豁免。
- [ ] 上游没有模型证据时记录为未观察，不误报、不宣称已验证；同一 SSE 出现冲突模型时采用确定性规则且不能被最后一个相同值掩盖。
- [ ] 终态日志保留 canonical AIO 标识，成本计算使用与 `final_provider_id` 匹配的远端计价模型；失败 attempt 的模型不能污染计价。
- [ ] 删除仍被受管 profile 引用的供应商会被后端拒绝，并返回引用的 profile；删除或重新绑定全部引用后才能删除供应商。
- [ ] 新建、编辑、复制、单供应商分享导入和完整配置导入分别满足既定 UUID 生命周期；v4 bundle 的非法/重复 UUID 在删除当前配置前失败。
- [ ] 同机完整配置 v4 导入保留仍存在供应商的本机模型/profile 关联；会造成受管 profile 悬空的导入在写数据库前失败，旧 v1-v3 包在存在本机受管 profile 时同样失败关闭。
- [ ] 同名未受管 profile 文件不会被覆盖；受管文件被外部修改后不会被更新或删除，解除管理时保留外部文件。
- [ ] 创建 Profile 后，新 Codex 进程的 `/model` 和 app-server `model/list` 可见 `aio/<profile_name_key>`，选择后请求仍固定落到绑定供应商。
- [ ] 用户已有模型目录时其全部模型和未知字段仍存在；没有自定义目录时使用当前安装 Codex 的 bundled 目录，而不是 AIO 固定快照。
- [ ] 关闭 Codex CLI 代理会恢复原 `model_catalog_json`；生成目录被外部修改或配置发生并发漂移时失败关闭且不覆盖用户状态。
- [ ] 旧 `aio/<model_uuid>` 和新 `aio/<profile_name_key>` 都能解析到同一 provider-scoped 绑定，正常请求不产生路由误报，真实 mismatch 仍告警。
- [ ] 创建、删除和应用启动同步失败时不会留下 DB、Profile 文件、模型目录或根配置的部分成功状态。
- [ ] 未启用受管 profile/模型路由的现有 Codex 请求保持当前排序、会话绑定与 failover 语义。
- [ ] 新模型在能力未配置时不能创建 Profile；可明确保存“无 reasoning + 未知上下文”的配置后创建。
- [ ] 支持档位、默认档位和上下文窗口可在 provider 模型目录中编辑、持久化并经 IPC 往返，默认档位必须属于支持集合。
- [ ] 已有 Profile 的模型能力变更会重建完整受管目录；Codex 新会话读取到对应档位和上下文窗口，外部目录/config 漂移时数据库更新回滚。
- [ ] v40 -> v41 保留已有模型/Profile 行并回填兼容 reasoning 基线；新发现/手工模型保持未配置，不按名称猜测。

## Out of Scope for the Proposed MVP

- 后台定时刷新、启动时自动刷新和跨多个 Base URL 合并目录。
- Anthropic native、Gemini native 等独立协议 adapter。
- 根据名称或不完整元数据自动判断 Responses 兼容性。
- 一次刷新后自动删除远端暂时消失的模型。
- 为每个 AIO 供应商生成独立的 Codex `model_provider` 定义。
- 将所有普通、非受管 Codex 模型请求改成强制模型感知路由。
- 未经单独产品决策的同名模型故障转移池。
- 模型目录、手工模型和受管 profile 的跨机器配置包迁移。
- 受管 profile 原地改名、原地重新绑定和任意附加 TOML 字段编辑。
- WSL 内独立 Codex home 的 profile 文件同步。

## Research Evidence

- `research/codex-provider-config-semantics.md`
- `research/backend-provider-model-flow.md`
- `research/frontend-provider-model-flow.md`
- `research/model-discovery-protocols.md`
- `research/model-route-detection-alias-compat.md`
- `research/provider-identity-import-lifecycle.md`
- `research/codex-managed-model-picker.md`
