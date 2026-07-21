# 第三方供应商模型发现协议调研

调研日期：2026-07-20

## 结论摘要

可以做，但“发现模型”“让 Codex 显示模型”“把模型稳定路由到某个供应商”是三个不同问题，不能只增加一个 `GET /v1/models` 请求就视为完成。

1. 首版可以覆盖 OpenAI-compatible 模型发现：OpenAI、xAI/Grok、NewAPI，以及 Gemini 的 OpenAI 兼容入口都提供 OpenAI 风格的模型列表。Anthropic 和 Gemini native 也能发现，但需要独立鉴权、分页和响应适配器。
2. 必须把模型身份保存为 `(provider_id, remote_model_id)`，不能从 `grok-4.5`、`claude-*` 等名字反推供应商。不同供应商可能暴露同一个模型 ID，代理还可能重命名模型。
3. 当前仓库的 Codex provider 选择只按 `cli_key = "codex"` 和资源池顺序进行，不按请求模型过滤。因此即使 profile 写了 `model = "grok-4.5"`，也不能保证请求落到用户选择的 Grok provider。若产品目标包含“模型对应供应商”，还需要唯一的公开路由模型 ID，或新增模型到 provider 的路由关系。
4. 用户示例中的内嵌 `[profiles.grok]` 已是旧语法。Codex 0.134.0 起应使用 `~/.codex/grok.config.toml`，并在该文件顶层写 `model` 与 `model_provider`：

   ```toml
   # ~/.codex/grok.config.toml
   model = "grok-4.5"
   model_provider = "aio"
   ```

   启动时使用 `codex --profile grok`。`model_provider = "aio"` 表示 Codex 连接 AIO 网关，不表示上游模型来自名为 `aio` 的厂商。
5. 自动发现永远不能成为保存 provider 的硬前置条件。接口缺失、401/403、网络失败、空列表、模型不在列表中时，都必须允许手工录入并保留既有数据。

## 证据口径

- **[仓库]**：当前工作区产品代码，说明现状。
- **[外部事实]**：厂商官方文档或官方 SDK/仓库当前实现。
- **[建议/推断]**：由现状和协议差异推导出的设计建议，不声称是现有行为。

## Codex 配置约束

### 官方事实

- **[外部事实]** Codex 自定义 provider 由 `model_provider` 和 `[model_providers.<id>]` 定义，provider 描述 base URL、wire API、认证和可选请求头；自定义 ID 不能占用 `openai`、`ollama`、`lmstudio`。参见 [Codex Advanced Configuration - Custom model providers](https://learn.chatgpt.com/docs/config-file/config-advanced#custom-model-providers)。
- **[外部事实]** 当前自定义 provider 的 `wire_api` 只支持 `responses`。因此 Anthropic Messages、Gemini native 等协议不能直接因为“有模型列表”就成为 Codex wire provider；仍需 AIO 网关桥接为 Responses。
- **[外部事实]** Codex 0.134.0 起，`--profile` 不再读取 `config.toml` 内的 `[profiles.<name>]`，而是叠加 `~/.codex/<name>.config.toml`。参见 [Codex Advanced Configuration - Profiles](https://learn.chatgpt.com/docs/config-file/config-advanced#profiles)。
- **[外部事实]** `model_catalog_json` 是启动时加载的模型目录覆盖，profile 文件也可以覆盖它；这能影响 Codex 可见模型元数据，但它不表达 AIO 内部 provider 路由。参见同一页面的 Profiles 示例和 [Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)。
- **[外部事实]** project-local `.codex/config.toml` 不能覆盖 `model_provider`、`model_providers` 等会改变认证/上游的配置；这类配置必须在用户级 Codex 配置中。参见 [Project config files](https://learn.chatgpt.com/docs/config-file/config-advanced#project-config-files-codexconfigtoml)。

### 仓库现状

- **[仓库]** AIO 启用 Codex 代理时固定写入 `model_provider = "aio"`，并生成 `[model_providers.aio]`、网关 `base_url`、`wire_api = "responses"`、`requires_openai_auth = true`：[src-tauri/src/infra/cli_proxy/codex.rs](../../../../src-tauri/src/infra/cli_proxy/codex.rs#L421-L434)、[同文件](../../../../src-tauri/src/infra/cli_proxy/codex.rs#L691-L715)。
- **[仓库]** 已有 Codex 模型目录读取是启动本机 Codex app-server，再调用 JSON-RPC `model/list`，支持 cursor 分页；它读取“Codex 当前看到的 catalog”，不是直接查询某个 AIO provider 的上游 `/models`：[src-tauri/src/infra/codex_model_catalog/protocol.rs](../../../../src-tauri/src/infra/codex_model_catalog/protocol.rs#L97-L180)。
- **[仓库]** provider upsert/summary 当前没有“发现模型列表”字段，仅有 Claude 模型槽、Codex bridge 映射和 `availability_test_model`：[src-tauri/src/domain/providers/types.rs](../../../../src-tauri/src/domain/providers/types.rs#L85-L118)、[同文件](../../../../src-tauri/src/domain/providers/types.rs#L287-L324)。
- **[仓库]** SQLite 仍有历史列 `supported_models_json`，但创建时固定 `{}`，更新 provider 时也重置为 `{}`，且列表 DTO 不读取它；不能把它当成可复用的现成契约：[src-tauri/src/infra/db/migrations/baseline_v25.rs](../../../../src-tauri/src/infra/db/migrations/baseline_v25.rs#L22-L40)、[src-tauri/src/domain/providers/queries.rs](../../../../src-tauri/src/domain/providers/queries.rs#L1381-L1415)、[同文件](../../../../src-tauri/src/domain/providers/queries.rs#L1641-L1653)。
- **[仓库]** 普通 Codex provider 的编辑器只有 Base URLs、API Key 和手填测试模型，没有协议种类或发现模型选择器：[src/pages/providers/ApiKeySection.tsx](../../../../src/pages/providers/ApiKeySection.tsx#L92-L175)。
- **[仓库]** 网关候选 provider 查询只按 `cli_key`、启用状态和资源池/排序模式选择，未读取请求模型或 `supported_models_json`：[src-tauri/src/domain/providers/queries.rs](../../../../src-tauri/src/domain/providers/queries.rs#L686-L805)、[src-tauri/src/gateway/proxy/handler/provider_selection.rs](../../../../src-tauri/src/gateway/proxy/handler/provider_selection.rs#L15-L72)。
- **[仓库]** Codex bridge 已能把公开 Codex 模型精确映射到上游模型，但映射只属于 bridge provider；普通 provider 没有同等的 provider/model 归属目录：[src/pages/providers/CodexBridgeSection.tsx](../../../../src/pages/providers/CodexBridgeSection.tsx#L83-L157)、[src-tauri/src/domain/providers/types.rs](../../../../src-tauri/src/domain/providers/types.rs#L207-L243)。

## 模型发现协议对比

| 类型 | 列表请求 | 鉴权 | 响应/分页 | 与 OpenAI `GET /v1/models` 的兼容性 | MVP 判断 |
| --- | --- | --- | --- | --- | --- |
| OpenAI | API base `https://api.openai.com/v1` 下 `GET /models`，即 `GET /v1/models` | `Authorization: Bearer <key>` | `data[]`，每项至少有 `id/created/object/owned_by`；当前官方 SDK 的 list 无分页参数 | 基准协议 | 支持 |
| xAI / Grok | `https://api.x.ai/v1/models` | `Authorization: Bearer <xAI key>` | `object: "list"` + `data[]`；还包含 aliases、价格、context 等；文档未定义列表分页 | OpenAI 兼容，额外字段应忽略；列表含图像等非 Codex 文本模型 | 支持，但发现后仍需可用性验证/筛选 |
| NewAPI | `GET /v1/models`；另有 `/v1beta/openai/models` 和 Gemini native `/v1beta/models` | TokenAuth 接受 Bearer；也会把 Anthropic `x-api-key`、Gemini `x-goog-api-key`/`key` 转成 token | 默认 OpenAI 风格 `data[]`，实现额外返回 `success: true`；根据请求头还可返回 Anthropic 或 Gemini 形状；当前实现不分页 | 默认 Bearer 请求兼容；解析器必须容忍额外顶层字段，不能看到 NewAPI 域名就假定响应形状 | 作为 OpenAI-compatible 支持；无需首版单独协议，保留显式覆盖 |
| Gemini OpenAI compatibility | base `https://generativelanguage.googleapis.com/v1beta/openai/` 下 `GET models` | `Authorization: Bearer <Gemini key>` | 官方示例通过 OpenAI SDK `models.list()` 迭代；兼容页面未声明独立分页参数 | 官方明确提供 OpenAI models list，但整个兼容层仍标为 beta | 支持为独立 URL 模板；失败可切 Gemini native |
| Gemini native | `GET https://generativelanguage.googleapis.com/v1beta/models` | 官方 REST 示例使用 `?key=<key>`；API key 指南也支持 `x-goog-api-key` | `models[]` + `nextPageToken`；请求 `pageSize`（默认 50、最大 1000）和 `pageToken`；模型名为 `models/{model}`，还带 supported actions | 不兼容：数组键、分页和模型 ID 都不同 | 可做第二适配器；保存 remote ID 时规范化去掉 `models/`，同时保留原始 name |
| Anthropic native | `GET https://api.anthropic.com/v1/models` | `x-api-key` + `anthropic-version: 2023-06-01`（官方 SDK也支持 bearer token，但 API key 是普通场景） | `data[]` + `has_more/first_id/last_id`；请求 `after_id`、`before_id`、`limit`（默认 20，1..1000） | 路径相同但不是 OpenAI 协议：鉴权、模型对象和 cursor 都不同 | 需要显式 Anthropic 适配器；不能用 Bearer/OpenAI parser 猜 |
| 无列表接口或代理禁用列表 | 无稳定端点 | 不适用 | 常见 404/405/403、空列表或非标准 JSON | 不兼容/不可发现 | 始终允许手工录入 |

### 外部来源

- OpenAI：官方 SDK当前 `Models.list()` 请求 `/models`、声明 bearer auth，并返回 `SyncPage[Model]`；`Model` 含 `id/created/object/owned_by`：[openai-python models.py（固定提交）](https://github.com/openai/openai-python/blob/d4dceb221b9a92c55c232d5b330ae89beb539415/src/openai/resources/models.py)、[model.py](https://github.com/openai/openai-python/blob/d4dceb221b9a92c55c232d5b330ae89beb539415/src/openai/types/model.py)。
- Anthropic：官方 SDK（由其 OpenAPI 生成）列出 `/v1/models`、`after_id/before_id/limit`：[models.py](https://github.com/anthropics/anthropic-sdk-python/blob/3c8bdf14bc55377262f11d6c34b893834a02b3fc/src/anthropic/resources/models.py)、[model_list_params.py](https://github.com/anthropics/anthropic-sdk-python/blob/3c8bdf14bc55377262f11d6c34b893834a02b3fc/src/anthropic/types/model_list_params.py)、[pagination.py](https://github.com/anthropics/anthropic-sdk-python/blob/3c8bdf14bc55377262f11d6c34b893834a02b3fc/src/anthropic/pagination.py)、[_client.py](https://github.com/anthropics/anthropic-sdk-python/blob/3c8bdf14bc55377262f11d6c34b893834a02b3fc/src/anthropic/_client.py)。
- Gemini native：[Models API](https://ai.google.dev/api/models)；Gemini OpenAI models list、base URL 与 bearer 示例：[OpenAI compatibility](https://ai.google.dev/gemini-api/docs/openai)；API key header 指南：[Using Gemini API keys](https://ai.google.dev/gemini-api/docs/api-key)。
- xAI：[Inference API Overview](https://docs.x.ai/developers/rest-api-reference/inference.md) 明确 OpenAI REST 兼容、base 与 Bearer；[Models](https://docs.x.ai/developers/rest-api-reference/inference/models.md) 定义 `GET /v1/models` 和响应。
- NewAPI 官方文档：[Native OpenAI Format](https://docs2.newapi.pro/en/llms.mdx/api/ai-model/models/list/listmodels)、[Native Gemini Format](https://docs2.newapi.pro/en/llms.mdx/api/ai-model/models/list/listmodelsgemini)。固定提交代码证明路由按请求头切换响应类型：[relay-router.go](https://github.com/QuantumNous/new-api/blob/4aa08f917eedecf77cef387f2337af88277fbbd0/router/relay-router.go)、[controller/model.go](https://github.com/QuantumNous/new-api/blob/4aa08f917eedecf77cef387f2337af88277fbbd0/controller/model.go)、[middleware/auth.go](https://github.com/QuantumNous/new-api/blob/4aa08f917eedecf77cef387f2337af88277fbbd0/middleware/auth.go)。

## Base URL 与请求构造

**[建议] 不要使用 `base_url + "/v1/models"`。** 仓库 UI 已鼓励用户把 `/v1` 放进 Base URL；盲拼会得到 `/v1/v1/models`。应为每种 adapter 定义相对于“API base”的模型路径，并用 URL parser 合并：

- `openai` / `xai` / `newapi-openai`：`https://host/v1` + `models` -> `https://host/v1/models`。
- `gemini-openai`：`https://generativelanguage.googleapis.com/v1beta/openai/` + `models`。
- `gemini-native`：固定/显式 API base `.../v1beta` + `models`。
- `anthropic`：`https://api.anthropic.com/v1` + `models`。

对用户可能填入的 origin-only URL，可由显式 adapter 补默认版本路径；不要并发试遍所有厂商协议后凭第一个 200 自动认定类型。自动建议可依据已知 host/path，但保存时应持久化用户确认的 `discovery_kind`。

多 Base URL 的首版建议只查询当前主 URL（列表第一项，或让用户在刷新时选择）；把多个镜像返回值直接求并集会掩盖节点配置漂移。请求应限制超时、最大响应字节、最大模型数与最大页数；重定向不得把 credential 带到不同 origin，日志不得记录 key 或原始响应体。

## 模型归属与重名

### 推荐最小数据契约

```text
ProviderModel
  provider_id        // 强归属；外键
  remote_model_id    // 实际发给该上游的 model
  display_name       // 可选；不参与身份
  source             // discovered | manual
  last_seen_at       // 可空；手工项为空
  enabled            // 用户是否允许选择
  raw_name           // 可选，例如 Gemini 的 models/gemini-...

UNIQUE(provider_id, remote_model_id)
```

`owned_by`、模型前缀、provider 名称都只能作展示提示，不能作为归属键。若未来一个公开模型要路由到多个 provider，另建 `ModelRoute(public_model_id, provider_model_id, priority/weight)`，不要把 `public_model_id` 与 `remote_model_id` 混为一列。

### 当前必须显式记录的路由缺口

**[推断]** 现代 profile 中以下配置只让 Codex 把 `grok-4.5` 发给 AIO 网关：

```toml
model = "grok-4.5"
model_provider = "aio"
```

当前网关仍会从所有 Codex 候选 provider 按资源池选择，无法由这两个字段确定某个 `provider_id`。要保证“这个模型对应这个供应商”，后续实现至少选择一种：

1. 给 provider-model 生成唯一 `public_model_id`，网关据此筛选 provider 并在转发前改写为 `remote_model_id`；或
2. 引入按模型配置的 provider pool，让相同公开 ID 可以对应一组明确支持它的 provider。

不建议把 provider 名字直接拼进远端模型 ID 后原样发送；应在网关内解开别名并恢复远端 ID。

## OpenAI-compatible MVP 边界

### 建议纳入

1. Provider 编辑器增加显式“模型列表协议”：`OpenAI 兼容` / `Gemini OpenAI` / `不自动获取`。URL 可给建议，用户可覆盖。
2. 用户主动点击“获取模型”；保存 provider 不自动阻塞等待网络。首版不做后台定时刷新。
3. OpenAI parser 接受 `data[]`，只要求非空字符串 `id`，忽略 `object/owned_by/aliases/pricing/success` 等额外字段；单次最多保留受限数量。
4. 模型选择和手工输入共用同一控件。发现项仅代表“列表可见”，不是“Codex Responses 一定可用”；选择后复用现有可用性测试做真实请求验证。
5. 按 `(provider_id, remote_model_id)` 持久化；刷新做 upsert，不清除手工项。远端消失的发现项标为 stale，不能在一次失败或空响应后删除。
6. 错误分类至少区分 `unauthorized`、`forbidden`、`not_supported`（404/405）、`timeout/network`、`malformed`、`empty`；所有错误都显示“可手工添加”。

### 建议后续再做

- Anthropic native 和 Gemini native adapter 及完整 cursor/pageToken 翻页。
- 根据厂商 capability 自动过滤文本/Responses 可用模型。xAI `/v1/models` 会返回图像模型，而 OpenAI 风格 `Model` 本身通常不足以证明 Responses 能力，首版不应靠名称猜测。
- 定时同步、跨多个 Base URL 合并、自动删除下线模型。
- 自动生成/维护 `model_catalog_json` 与多个 Codex profile 文件；这涉及 Codex 版本兼容和用户配置所有权，应与 provider discovery 解耦评审。
- 按模型选择 provider 的网关路由与唯一公开别名。若用户验收要求“选择 Grok 后一定走 Grok provider”，此项必须提升到 MVP，而不是只做 UI 列表。

## 手工回退规则

- “获取失败”不得阻止 provider 保存，也不得清空上次成功结果。
- 用户始终可输入任意远端模型 ID；手工项与发现项同名时合并为同一个 `(provider_id, remote_model_id)`，保留用户启用状态。
- 401/403 提示检查该 provider 自己的凭据，不要回退使用 Codex/OpenAI 全局凭据。
- 404/405 标记为“该端点不支持模型发现”，不要把 provider 标记为整体不可用。
- 空列表应单独显示，不能等价于“删除全部模型”。
- 响应包含重复 ID 时在单个 provider 内去重；不同 provider 的同名 ID必须同时保留并显示 provider 名。
- 手工模型也应允许调用现有 availability probe；probe 成功只更新验证状态，不改变发现来源。

## 待产品决策

1. 本任务只提供 provider 内模型下拉，还是必须保证 profile 选中的模型定向到该 provider？后者需要网关路由改造。
2. profile 由 AIO 写入独立 `~/.codex/<name>.config.toml`，还是只生成可复制预览？这涉及用户级配置所有权和旧 Codex 版本兼容。
3. 同一远端 ID 被多个 provider 提供时，产品语义是“用户明确选 provider”还是“组成一个可故障转移的模型池”？数据层应在实现前确定。
4. 首版是否接受仅 OpenAI-compatible + 手工回退。协议证据支持这是最小且可验证的范围；Anthropic/Gemini native 可独立增量交付。
