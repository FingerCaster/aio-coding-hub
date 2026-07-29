# 前端供应商模型发现与 Codex 配置流调研

## 结论摘要

当前前端已经有完整的供应商 CRUD、按 CLI 的供应商调用顺序、Codex 根级 `model`
编辑、原始 `config.toml` 编辑，以及通过本机 Codex `app-server` 读取模型能力目录的能力；
但这些能力之间没有“某个模型属于某个 AIO 供应商”的共享契约。

具体而言：

- Codex 直连供应商目前只保存一个 `availability_test_model`，它只用于探活请求，不是
  可用模型目录。
- `ProviderSummary` / `ProviderUpsertInput` 没有 provider-scoped `models` 字段，也没有
  单独的模型发现 IPC、service 或 TanStack Query。
- 供应商页面的“调用顺序”只按 `cli_key`、模板和会话选择候选供应商，不按模型选择。
- Codex 页的模型建议来自本机 Codex `app-server` 的 `model/list`，DTO 没有 AIO
  `provider_id`；该目录不是第三方供应商 `/models` 发现结果。
- Codex 结构化配置只读写根级 `model` 等字段，不读写 `model_provider`，也没有 profile
  文件管理。高级编辑器只编辑主 `config.toml`。
- 当前 Codex 已不再使用用户示例中的 `[profiles.grok]`。详细版本和官方证据见同任务的
  [codex-provider-config-semantics.md](./codex-provider-config-semantics.md)。面向当前版本应生成
  `$CODEX_HOME/grok.config.toml`，文件中使用顶层 `model` 与 `model_provider`。

因此，不能把现有三个“模型”字段或现有 Codex 模型下拉直接复用为第三方模型发现。
MVP 至少需要一份以 `provider_id` 为归属主键的供应商模型目录，以及一条把 Codex profile
明确绑定到该 `provider_id` 的配置路径。

## 一、供应商新增与编辑现状

### 1. UI 入口和表单分支

- `src/pages/providers/ProvidersView.tsx:279-288` 打开新增供应商对话框；
  `src/pages/providers/ProvidersView.tsx:546-577` 分别挂载新增和编辑模式的
  `ProviderEditorDialog`。
- `src/pages/providers/ProviderEditorDialog.tsx:45-132` 根据认证/桥接类型切换
  `OAuthSection`、`Cx2ccSection`、`CodexBridgeSection`、`ApiKeySection`，然后渲染公共的
  超时、用量、重试、限额和 Claude 模型映射区域。
- `src/pages/providers/ApiKeySection.tsx:92-125` 编辑 Base URL 和 API Key；
  `src/pages/providers/ApiKeySection.tsx:164-175` 对 Codex 供应商额外显示自由文本“测试模型”。
  页面没有“获取模型”、模型列表、刷新时间或手工模型目录控件。
- `src/pages/providers/useProviderEditorForm.ts:272-277` 的模型相关本地状态只有
  `claudeModels`、`modelMapping` 和 `testModel`。
- `src/pages/providers/useProviderEditorEffects.ts:155-184`、`:197-237` 分别从新增初值和
  `ProviderSummary` 恢复上述状态。

### 2. 保存数据流

```text
ProviderEditorDialog
  -> useProviderEditorForm.buildPayloadContext()
  -> buildProviderEditorUpsertInput()
  -> runProviderEditorSave()
  -> useProviderUpsertMutation()
  -> services/providers.providerUpsert()
  -> generated commands.providerUpsert()
  -> Rust provider_upsert
```

- `src/pages/providers/providerEditorSubmitModel.ts:19-145` 先校验表单、URL、Claude 模型和
  Codex bridge 映射；`:161-199` 构造 `ProviderUpsertInput`。
- 其中 `availabilityTestModel` 只在 `cliKey === "codex"` 时提交（`:174`）；
  `claudeModels` 只在 Claude 下提交（`:188`）；`modelMapping` 只在 Codex CX2CC bridge
  下提交（`:189-191`）。普通 Codex provider 没有模型目录字段。
- `src/pages/providers/providerEditorSaveRunner.ts:7-44` 保存后拿到完整
  `ProviderSummary`，但 `:66-69` 立即通知成功并关闭对话框。若 MVP 采用“保存并获取模型”，
  这里需要允许在获得新 `provider.id` 后继续发现，并在失败时保留已保存状态与对话框。
- `src/query/providers.ts:241-267` 的 mutation 只更新/失效供应商列表缓存；当前没有
  provider-model query cache。
- `src/services/providers/providers.ts:91-145` 明确映射生成的 upsert 字段；
  `:179-229` 规范化并生成 IPC payload；`:262-292` 调用生成命令并对日志中的凭据脱敏。

### 3. 当前模型相关字段不是模型目录

| 字段 | UI | 语义 | 是否可作为供应商模型目录 |
| --- | --- | --- | --- |
| `claude_models` | `ClaudeModelSection` 五个自由文本槽位 | Claude 请求模型到上游模型的分层映射 | 否 |
| `model_mapping` | `CodexBridgeSection` 默认/精确映射 | Codex bridge 请求模型到另一个协议上游模型的转换 | 否 |
| `availability_test_model` | Codex API Key 表单“测试模型” | `/v1/chat/completions` 探活时发送的模型 | 否 |

证据：

- `src/pages/providers/ClaudeModelSection.tsx:6-17` 限定为 Claude，并说明五个模型槽位；
  `:34-114` 全部为手工映射输入。
- `src/pages/providers/CodexBridgeSection.tsx:83-157` 只配置 bridge 的默认上游模型和精确映射。
- `src-tauri/src/domain/provider_availability.rs:278-285` 按供应商覆盖、全局设置、默认值解析
  Codex 探活模型；`:310-323` 将其发送到 `/v1/chat/completions`。该流程不会列举模型。
- `src/pages/providers/SortableProviderCard.tsx:415-425` 仅对 Claude 模型映射显示 badge；
  Codex provider card 没有模型目录摘要。

## 二、当前生成绑定与前端契约

`src/generated/bindings.ts` 是 Rust/Specta 生成边界：

- `:3855-3891` 的 `ProviderSummary` 含 `claude_models`、`model_mapping`、
  `availability_test_model`，但无 `models`、模型数量或模型目录刷新状态。
- `:3892-3922` 的 `ProviderUpsertInput` 同样没有模型目录字段。
- `:657-668` 的命令只有 `providersList` / `providerUpsert`；`:781-788` 的
  `providerTestAvailability` 是探活，不是发现。
- `src/services/providers/providers.ts:73-79` 只窄化 `cli_key` 和认证模式；若新增模型 DTO，
  仍应由 service adapter 验证 `provider_id`、状态枚举和模型字符串边界，React 不应直接
  cast 生成 payload。

不建议把完整模型数组塞进 `ProviderSummary`。供应商列表和调用顺序会频繁读取该 DTO，
第三方目录可能很大。更合适的前端契约是：

```ts
type ProviderModelEntry = {
  provider_id: number;
  model_id: string;
  display_name: string | null;
  origin: "discovered" | "manual";
  last_seen_at: number | null;
};

type ProviderModelCatalogState = {
  provider_id: number;
  status: "ready" | "manual_only" | "unsupported" | "error";
  refreshed_at: number | null;
  stale: boolean;
  models: ProviderModelEntry[];
  warning: string | null;
};
```

建议新增独立命令/adapter，而非扩大 provider list：

- `provider_models_get(providerId)`：读取持久化/缓存目录，不发网络请求。
- `provider_models_refresh(providerId)`：由 Rust 根据已保存供应商加载 Base URL 和凭据并发现；
  前端绝不接收明文凭据。
- `provider_model_manual_upsert(providerId, modelId)` / `provider_model_manual_delete(...)`：
  为无 `/models` 接口的供应商提供回退。
- `ProviderSummary` 最多增加 `model_count`、`model_catalog_updated_at` 等小型摘要字段，供卡片显示。

TanStack Query 应增加 `providersKeys.models(providerId)`，而不是复用 Codex CLI 的
`cliManagerKeys.codexModelCatalog(...)`。刷新 mutation 只能更新对应 `provider_id` 的缓存；
同名模型也必须保留为两个不同条目。

## 三、模型下拉和 Codex 模型能力目录现状

### 1. 当前下拉来源

- `src/services/cli/cliManager.ts:174-183` 的 `cliManagerCodexModelCatalogGet()` 无参数；
  它不能指定 AIO provider。
- `src/query/cliManager.ts:37-58` 使用 5 分钟 stale time；`:88-113` 的 key 只包含
  Codex 配置路径、可执行文件路径和 CLI 版本。
- `src-tauri/src/infra/codex_model_catalog/mod.rs:58-93` 以当前 `CODEX_HOME` 启动本机 Codex；
  `src-tauri/src/infra/codex_model_catalog/protocol.rs:97-180` 调用 app-server
  `model/list`。
- 生成 DTO `src/generated/bindings.ts:2741-2767` 只有 `id/model/display_name/hidden/`
  推理档位等 Codex 能力元数据，没有 AIO `provider_id`。
- `src/components/cli-manager/tabs/CodexTab.tsx:943-971` 用该目录给根级 `model` 输入提供
  `datalist` 建议；自由文本仍可保存。
- `src/components/cli-manager/tabs/CodexTab.tsx:1662-1685` 对目录去重时只按模型字符串，
  无 provider 维度。
- `src/components/cli-manager/tabs/codexModelCapabilities.ts:79-97` 也只按 `model` 或 `id`
  匹配。`src/components/cli-manager/tabs/useCodexModelMigration.ts:53-99` 可能根据该匹配结果
  自动调整推理强度。

同任务的官方/本机实验已经证明，自定义 provider 不会因此自动调用其 `/v1/models`。
所以第三方发现结果只能提供“该上游声称的模型 ID/显示名”，不能伪装成包含 Codex 推理档位
的 `CodexModelCapability`。否则同名模型可能错误继承 OpenAI/Codex 能力，并触发不正确的
推理强度迁移。

### 2. 缓存缺口

`src/query/keys.ts:277-297` 的 Codex 目录 key 不含配置内容或当前 `model_provider`。
`src/query/cliManager.ts:200-230` 在结构化/原始 TOML 保存后只失效 config 查询，不失效
模型目录；`src/pages/cli-manager/useCliManagerPageDataModel.ts:440-456` 只有手动刷新路径会
显式刷新目录。未来若 provider/profile 改动影响 Codex 能力读取，必须显式失效目录，或把
稳定配置版本/hash 纳入 key。

## 四、供应商路由 UI 与实际选择语义

- `src/pages/providers/ProvidersView.tsx:390-422` 的“调用顺序”只选择 Default 或排序模板；
  `:467-539` 只维护 provider ID 顺序和启用状态。
- `src/pages/providers/hooks/useProvidersViewDataModel.ts:1062-1118` 把路由保存为一串
  `provider_id`，没有模型条件。
- `src-tauri/src/gateway/proxy/handler/provider_selection.rs:15-43` 实际候选选择只接收
  `cli_key`、会话和活动 sort mode；不接收 requested model。
- `src-tauri/src/gateway/model_route_mapping.rs:25-65` 的 `model_route_mapping` 是请求结束后的
  requested/actual model 审计记录，不是可配置的模型路由表。

但网关已有一个可复用的显式绑定能力：

- `src-tauri/src/gateway/routes.rs:53-73` 的
  `/:cli_key/_aio/provider/:provider_id/*path` 会把路径中的 ID 转成内部
  `x-aio-provider-id`；`:103-118` 注册该路由。
- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs:80-102` 在正常候选列表
  上应用 forced provider。

这意味着 Codex profile 若要稳定指向某个 AIO provider，不必按模型名反向猜测。可以在主
`config.toml` 中为每个受管理供应商写独立的 Codex model-provider 定义，例如概念上：

```toml
[model_providers."aio-provider-17"]
name = "Grok via AIO"
base_url = "http://127.0.0.1:<port>/codex/_aio/provider/17/v1"
wire_api = "responses"
requires_openai_auth = true
```

对应的 `grok.config.toml` 使用：

```toml
model = "grok-4.5"
model_provider = "aio-provider-17"
```

这是前端/配置层的首选方案，因为绑定键来自稳定的 `provider_id`，同名模型不会冲突，
且复用现有 forced-provider 路由。备选方案是继续统一写 `model_provider = "aio"`，再由
网关按请求模型查 provider；该方案必须额外定义模型别名和同名冲突规则，改动更大，也更
容易出现错误归属。

需要在设计阶段进一步确认：数据库 ID 在配置迁移/导入后的稳定性、provider 删除后的
profile 处理、CLI proxy 对受管理 `model_providers.*` 表的所有权/回滚，以及
`remote_compaction` 对 provider 命名的约束。

## 五、Codex config 与 profile UI 现状

### 1. 结构化配置没有 provider/profile

- Rust `src-tauri/src/infra/codex_config/types.rs:5-39` 的 `CodexConfigState` 与
  `:52-79` 的 `CodexConfigPatch` 只包含根级 `model` 等字段，没有 `model_provider`、
  `model_providers` 或 profile。
- 生成边界 `src/generated/bindings.ts:2676-2729` 保持同样限制。
- 前端 adapter `src/services/cli/cliManager.ts:55-75` 的默认 patch 也没有上述字段；
  `:185-250` 只有主配置 get/set/raw TOML/provider history sync。
- `src/components/cli-manager/tabs/CodexTab.tsx:1210-1265` 名为“AIO Provider”的区域实际上
  只管理探活默认模型和历史 Provider Sync，不选择 AIO 供应商。

### 2. 原始 TOML 是唯一手工逃生口，但只覆盖主文件

- `src/components/cli-manager/tabs/CodexTab.tsx:1388-1563` 提供主 `config.toml` 的原始编辑、
  校验和保存。
- `src-tauri/src/infra/codex_config/parsing.rs:310-360` 的结构化读取忽略未知表和
  `model_provider`；所以手工写入后，结构化 UI 看不到这些值。
- `src-tauri/src/infra/codex_config/parsing.rs:474-570` 的 raw 校验会解析完整 TOML，并只对
  已知根字段做额外枚举校验；语法正确的其他表可保留，但不会验证 model/profile 引用关系。
- `src-tauri/src/infra/codex_paths.rs:107-110` 只解析主 `config.toml` 路径。仓库中没有
  `<profile>.config.toml` 的枚举、读取、写入或删除服务。

因此，MVP profile UI 不能继续往主文件追加 `[profiles.<name>]`。需要新的后端文件 API，
前端只消费受限 DTO，不能拼路径或直接写文件。

## 六、建议的最小 UI 流程

### 1. 供应商编辑器：保存后发现，始终允许手工录入

仅在 `cliKey === "codex"` 且供应商为可直接访问的 API Key/OAuth provider 时显示
“模型目录”区域。bridge provider 应明确显示“模型来自上游来源/映射”，避免对 bridge
端点重复发现。

建议交互：

1. 新增时保留现有名称、Base URL、凭据字段，并提供“保存”和“保存并获取模型”两个明确
   动作。发现命令只接受保存后返回的 `provider.id`，由 Rust 读取凭据。
2. “保存并获取模型”先完成 provider upsert，再调用 refresh；两步不是原子操作。发现失败
   时明确提示“供应商已保存，模型获取失败”，对话框保持打开。
3. 编辑已有 provider 时显示“刷新模型”图标按钮、最后刷新时间、模型数量和状态。
4. 成功结果按 `model_id` 在当前 `provider_id` 内去重；不跨 provider 去重。
5. 无接口、鉴权失败、超时或返回空列表时保留上次成功目录，并提供“手工添加模型”。
6. 修改 Base URL、认证方式或凭据后将已有目录标为 stale；不要静默清空手工模型。

不建议在“保存供应商”时无条件自动探测：这会让一个本来确定性的本地保存动作依赖远端
网络，并使错误语义和回滚难以理解。可以在首版后增加用户可选的自动刷新设置。

### 2. Codex 页：独立的“Profiles”管理区

建议放在现有 `CodexProviderSection` 之后、原始 TOML 之前，按行显示：

- profile 名称；
- AIO provider 选择器（数据来自 `providersList("codex")`，值是 `provider.id`）；
- 模型 combobox（只展示该 provider 的目录，同时允许自由文本）；
- 配置状态、编辑和删除动作。

保存 DTO 应保留显式三元组：

```ts
type CodexManagedProfileInput = {
  name: string;
  provider_id: number;
  model_id: string;
};
```

后端在一次受控操作中：

1. 确保主 `config.toml` 中存在该 `provider_id` 对应的受管理
   `[model_providers.<managed-key>]`，base URL 指向 forced-provider 路径；
2. 写入 `<name>.config.toml` 的顶层 `model` 和 `model_provider`；
3. 任一步失败时回滚，不让前端分别提交两个文件。

读取时只有当 model-provider key 和受管理 base URL 均能严格验证到同一 `provider_id` 时，
才将其标为 AIO managed profile。未知/外部 profile 必须原样展示或标为只读，不能按
`model_id` 猜归属。

模型 combobox 的 React key 应使用 `(provider_id, model_id)` 组合。若提供“全部模型”搜索，
同名项必须显示供应商名，例如 `grok-4.5 · Provider A`，而不是合并成一个选项。

### 3. 根级默认模型保持兼容

MVP 可先不改现有根级“默认模型”输入，避免与 CLI proxy 当前管理的根
`model_provider = "aio"` 冲突。若后续要让根默认模型也绑定 AIO provider，应先把
`model_provider` 加入 `CodexConfigState/Patch`，并让 `useCodexModelMigration` 只在可信的
provider-aware 能力目录上做自动推理强度调整。

## 七、前端实现边界与缺口清单

### 必需

- provider-scoped 模型 DTO、生成绑定、service adapter、query key 和刷新/manual mutation。
- `ProviderEditorDialog` 的模型目录区域，以及保存成功后可继续发现的 runner 状态机。
- profile 列表/upsert/delete 的生成绑定和 `cliManager` adapter。
- Codex profile UI 使用 `provider_id + model_id`；不从模型名推断 provider。
- 后端组合写主配置与 profile 文件；前端不拼 TOML、不处理凭据、不拼文件路径。
- provider 变更/删除、模型目录刷新和 profile 写入后正确失效各自 query。

### 不应复用或混用

- 不把 `availability_test_model` 当成模型目录。
- 不把 `model_mapping` 当成模型归属关系。
- 不把 Codex `app-server model/list` 当成第三方 `/models` 发现。
- 不把 `model_provider = "aio"` 当成某个 AIO provider 的标识。
- 不生成旧式 `[profiles.<name>]`。

### 仍需产品决策

- 发现结果是否持久化。建议持久化最近成功目录和手工条目，否则 profile 编辑在离线或接口
  失败时不可用。
- “保存并获取模型”是否作为新增时主按钮，还是次要动作。建议次要动作，保留纯本地保存。
- 删除 provider 时是阻止删除、级联删除受管理 profile，还是把 profile 转成失联状态。
  建议先阻止并列出引用，避免静默破坏用户配置。
- provider ID 是否足够作为长期 managed key。若配置导入需要跨机器稳定引用，应先为
  provider 增加不可变 UUID，而不是把显示名作为键。
- 非 Responses 兼容 provider 是否允许建 profile。Codex 官方已弃用 Chat Completions；
  MVP 建议只对 Responses 兼容路径开放自动生成，其余保留手工配置。

## 八、建议验证范围

- `src/pages/providers/__tests__/ProviderEditorDialog.test.tsx`：新增/编辑刷新、保存后发现、
  失败后已保存提示、手工录入、stale 状态、同名模型不跨 provider 合并。
- `src/pages/providers/__tests__/providerEditorSaveRunner.test.ts`：upsert 成功而发现失败的非原子
  结果，以及是否关闭对话框。
- `src/services/providers/__tests__/providers.service.test.ts` 和
  `providers.contract.test.ts`：provider ID/模型 ID 边界、生成命令参数、无凭据泄漏。
- `src/query/__tests__/providers.test.tsx`：按 provider ID 缓存隔离、刷新失效和晚到响应不覆盖
  新结果。
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx`：provider-filtered combobox、
  手工模型、同名项、未知外部 profile、删除引用和 profile 文件新格式。
- `src/services/cli/__tests__/cliManager.service.test.ts`、
  `src/query/__tests__/cliManager.test.tsx`：profile adapter、组合写结果和模型目录失效。
- Rust/Specta 类型改动后运行 `pnpm tauri:gen-types` 与
  `pnpm check:generated-bindings`，不能手改生成类型作为源事实。

## 相关规范

- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`
- `.trellis/spec/aio-coding-hub/cross-layer/index.md`
- `.trellis/spec/guides/cross-layer-thinking-guide.md`
