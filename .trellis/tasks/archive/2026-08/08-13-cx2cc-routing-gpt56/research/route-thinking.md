# CX2CC 模型路由与思考参数端到端调研

## 1. 基线、范围与结论

- 当前分支：`81fd6d0860d1a6cc8c053f42d8aa941a0a445e96`
- 对比基线：`upstream/main` = `7725effd33ab9d7e1e8c4f9b5bb30c6e5a0ff23e`
- 共同祖先：`4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`
- 本报告只审计 CX2CC 请求的模型与思考参数数据流；不重复全量 upstream commit 审计，也不覆盖网关回环和完整 GPT-5.6 目录审计。

已确认的核心结论：

1. **CX2CC 当前存在第二次模型路由。** CX2CC bridge 先通过 `CX2CCModelMapper` 把 Claude 模型映射成来源模型，随后 `prepare_provider` 又用原始 Claude CLI、原始 `/messages` 路径和原始模型解析通用 `configured_model_route`。该通用路由在 plugin hook 之后、实际发送之前，按最终 `/responses` 形状再次改写 `model` 和 `reasoning.effort`。
2. **设置页的 reasoning effort 当前不是“缺省值”，而是无条件覆盖值。** outbound 已有“IR 请求元数据优先、设置兜底”的代码骨架，但 Anthropic inbound 始终写入空 `IRMetadata`；bridge 返回后，`apply_cx2cc_request_settings` 又无条件替换整个 `reasoning` 对象。因此该优先级对真实 CX2CC 请求不可达。
3. **调用方显式思考配置当前不会端到端透传。** `/output_config/effort`、顶层 `thinking` 以及其他顶层 reasoning 形状均未被 Anthropic inbound 读取。历史 assistant 消息中的 `thinking` 内容块虽能进入 IR，但 Responses outbound 会主动跳过它；它与本次请求的思考配置不是同一语义。
4. **未知 effort 字符串在设置链路被后端接受并原样发送，但 UI 无法正确表示；调用方请求中的未知字符串则会被丢弃。** 当前共享能力表已包含 `minimal/max/ultra`，CX2CC UI 仍只列出空值、`low/medium/high/xhigh`。
5. **最小修复不是改通用路由器的写入逻辑，而是在 CX2CC provider preparation 阶段不解析通用路由，并把思考参数的唯一所有权收敛到 Anthropic inbound -> IR -> Responses outbound。** 这样普通 Claude/Codex 的通用配置路由仍保持原行为。

## 2. 端到端数据流

### 2.1 前端设置到 Rust settings

1. `Cx2ccTab` 从 `AppSettings.cx2cc_model_reasoning_effort` 建立 draft，并在 radio 变更时调用 `persistReasoningEffort`：
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:127`，符号 `reasoningEffortText`
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:136`，符号 `persistReasoningEffort`
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:142`，写入 patch `{ cx2cc_model_reasoning_effort: value }`
2. UI 文案声明“默认表示不注入”，但选项硬编码为 `""/low/medium/high/xhigh`：
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:282`，思考强度设置项
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:284`，当前“注入”语义文案
   - `src/components/cli-manager/tabs/Cx2ccTab.tsx:293`，硬编码选项
3. 页面层通过 `persistCommonSettings` 调用普通 settings mutation，并用 Rust 返回的 settings 快照回填：
   - `src/pages/cli-manager/useCliManagerPageDataModel.ts:394`，符号 `persistCommonSettings`
   - `src/pages/cli-manager/useCliManagerPageDataModel.ts:407`，调用 `commonSettingsMutation.mutateAsync`
   - `src/pages/cli-manager/useCliManagerPageDataModel.ts:416`，读取返回快照
   - `src/pages/cli-manager/useCliManagerPageDataModel.ts:774`，注入 `Cx2ccTab`
4. 前端字段映射为生成 IPC 的 `cx2CcModelReasoningEffort`，最终调用 `settings_set/settings_patch`：
   - `src/services/settings/settings.ts:158`，view -> update 字段映射
   - `src/services/settings/settings.ts:291`，符号 `toGeneratedSettingsUpdate`
   - `src/services/settings/settings.ts:394`，符号 `settingsPatch`
   - `src/generated/bindings.ts:17`，生成命令 `commands.settingsSet`
5. 前端只校验 trim 后长度不超过 64 且无控制字符，不做枚举校验：
   - `src/services/settings/settingsValidation.ts:41`，`MAX_CX2CC_OPTIONAL_FIELD_LEN = 64`
   - `src/services/settings/settingsValidation.ts:362`，符号 `validateCx2ccOptionalField`

Rust settings 链路：

1. `SettingsUpdate` 把该字段定义为 `Option<String>`：`src-tauri/src/app/settings_service.rs:92`。
2. 未提供 patch 时沿用旧值；提供任意字符串时先进入 settings 聚合：`src-tauri/src/app/settings_service.rs:1003`，并在 `src-tauri/src/app/settings_service.rs:1144` 装配。
3. 持久化层仍只做有界可选字符串校验，随后 trim：
   - `src-tauri/src/infra/settings/persistence.rs:359`
   - `src-tauri/src/infra/settings/persistence.rs:538`
4. `AppSettings` 默认值为空字符串：
   - `src-tauri/src/infra/settings/types.rs:495`
   - `src-tauri/src/infra/settings/types.rs:579`
5. 每次网关请求加载运行时设置时，`Cx2ccSettings::from_app_settings` 把空/全空白转为 `None`，其他字符串 trim 后保留：
   - `src-tauri/src/gateway/proxy/handler/runtime_settings.rs:57`
   - `src-tauri/src/gateway/proxy/cx2cc/settings.rs:21`，符号 `from_app_settings`
   - `src-tauri/src/gateway/proxy/cx2cc/settings.rs:28`，赋值 `model_reasoning_effort`
   - `src-tauri/src/gateway/proxy/cx2cc/settings.rs:57`，符号 `non_empty`

因此设置链路当前语义是：空字符串 = `None`；任何其他有界字符串，包括后端尚未知的 future value = `Some(value)`。

### 2.2 Provider preparation 与 CX2CC 自身模型映射

`prepare_provider` 先识别 CX2CC bridge 并调用 `cx2cc_preparation::prepare`：

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:258`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:264`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:280`，记录 `cx2cc_active`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:281`，记录 `active_bridge_type = cx2cc`

来源有两种：

- 指定 `source_provider_id` 时，读取来源 provider 的 CLI、鉴权和 base URL：`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:54`、`:83`、`:116`。
- 未指定来源时，指向当前 AIO 的 Codex `/v1` 入口，CLI 标为 `codex`：`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:160`、`:171`、`:176`。此处只是数据流事实；回环合法性由本任务的另一条调研处理。

CX2CC preparation 解析原始 Claude body，构造 `BridgeContext`，再调用 registry 中的 `cx2cc` bridge：

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:182`，解析原 body
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:186`，构造 `BridgeContext`
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:201`，调用 `translate_request`
- `src-tauri/src/gateway/proxy/protocol_bridge/registry.rs:57`，`cx2cc_factory`
- `src-tauri/src/gateway/proxy/protocol_bridge/registry.rs:60`，Anthropic Messages inbound
- `src-tauri/src/gateway/proxy/protocol_bridge/registry.rs:61`，OpenAI Responses outbound
- `src-tauri/src/gateway/proxy/protocol_bridge/registry.rs:62`，`CX2CCModelMapper`

Bridge 固定按 `client JSON -> IR -> model mapper -> provider JSON` 执行：

- `src-tauri/src/gateway/proxy/protocol_bridge/bridge.rs:31`，符号 `translate_request`
- `src-tauri/src/gateway/proxy/protocol_bridge/bridge.rs:37`，inbound -> IR
- `src-tauri/src/gateway/proxy/protocol_bridge/bridge.rs:41`，模型映射
- `src-tauri/src/gateway/proxy/protocol_bridge/bridge.rs:44`，IR -> Responses body

CX2CC 自身的模型契约是按原 Claude 模型名选择 `opus/haiku/sonnet/main` 槽位，再用 CX2CC fallback：

- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:22`，`impl ModelMapper for CX2CCModelMapper`
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:28`，符号 `map_claude_to_openai`
- `src-tauri/src/gateway/proxy/protocol_bridge/cx2cc/mod.rs:29`、`:36`、`:43`、`:50`，四类分支

该 mapper 不读取通用 `ModelRoutingPolicy`，也不使用普通 Claude provider 的 `ClaudeModels::map_model(..., has_thinking)`。后者位于 `src-tauri/src/domain/providers/types.rs:158`，只由普通 Claude legacy mapping 使用；CX2CC 已在 `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:340` 通过 `should_apply_claude_model_mapping(cx2cc_active, ...)` 排除这一路径。这证明项目已经接受“CX2CC 不叠加 legacy Claude mapping”，但遗漏了新通用 configured route 的同等保护。

### 2.3 IR 中的思考参数实际状态

Anthropic inbound 的 `parse_request` 只读取 model、system、messages、tools、tool choice、token/temperature/top_p、stream 和 stop sequences：

- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:51`，符号 `parse_request`
- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:52` 至 `:79`，实际读取字段
- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:92`，始终 `metadata: IRMetadata::default()`

IR 已有协议扩展槽 `IRMetadata.extra: HashMap<String, Value>`，但此 inbound 没有填它：

- `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs:26`，`InternalRequest.metadata`
- `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs:136`，`IRMetadata`
- `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs:139`，`extra`

消息历史中的 `{type: "thinking", thinking: ...}` 仅转换成 `IRContentBlock::Thinking`：`src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:223`。Responses outbound 在输入构造时明确跳过该块：`src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:143`。所以“历史思考内容块”和“本次请求的顶层思考配置”必须分开测试，不能把前者的解析测试当作后者透传。

### 2.4 Responses outbound、固定 effort 与实际发送

Responses outbound 已有请求优先骨架：

- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:228`，调用 `apply_responses_metadata`
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:238`，若 IR 有 `reasoning` 则复制请求值
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:240`，否则用 CX2CC setting
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:244`、`:250`，`service_tier/store` 使用同一优先级结构

但当前真实 Anthropic IR 元数据恒为空，而且 preparation 紧接着再次调用设置注入：

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:228`，取得 translated body
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:229`，调用 `apply_cx2cc_request_settings`
- `src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:144`，该 helper 定义
- `src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:148`，只要 setting 为 `Some` 就执行
- `src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:149`，用新对象整体替换 `responses_body["reasoning"]`

这次整体替换不仅覆盖 `effort`，还会丢掉既有 `reasoning` 的任何 sibling（例如未来加入的 summary 配置）。`service_tier/store` 也在同一个 helper 中重复写入，见 `loop_helpers.rs:151` 至 `:155`。

随后发生第二次通用模型路由：

1. `prepare_provider` 在 CX2CC 已翻译完成后，仍用**原始** `input.cli_key`、`input.forwarded_path`、`input.requested_model` 解析通用规则：`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:377` 至 `:387`。
2. `configured_model_route::resolve` 把 Claude `POST /messages` 视为受支持推理请求，并精确匹配原模型：
   - `src-tauri/src/gateway/configured_model_route.rs:41`，符号 `resolve`
   - `src-tauri/src/gateway/configured_model_route.rs:52`，只有 managed route/非推理请求会提前跳过
   - `src-tauri/src/gateway/configured_model_route.rs:68`，按原模型找规则
   - `src-tauri/src/gateway/configured_model_route.rs:91`，Claude `/messages` 被支持
3. plugin `beforeSend` 完成后，attempt executor 对**最终** `/responses` path/body 应用这条原始 Claude 规则：
   - `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:315` 至 `:327`，plugin hook 已完成
   - `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:331`，读取 prepared route
   - `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:338`，调用 `configured_model_route::apply`
   - `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:340`，传入最终 path
4. `apply` 根据最终 path 识别 Responses，并重写 model/effort：
   - `src-tauri/src/gateway/configured_model_route.rs:116`，最终 wire protocol 分类
   - `src-tauri/src/gateway/configured_model_route.rs:159`，改 target model
   - `src-tauri/src/gateway/configured_model_route.rs:182`，改 reasoning effort
   - `src-tauri/src/gateway/configured_model_route.rs:224`，写入 `/reasoning/effort`
5. 改写后的 body 在 `attempt_executor.rs:454` finalize，并在 `attempt_executor.rs:474` 交给 transport，故这不是日志层误差，而是实际请求。

无 plugin 时，当前最终优先级可近似写成：

```text
final model  = matching configured route target
               ?? CX2CCModelMapper(原 Claude model)

final effort = matching configured route effort
               ?? CX2CC settings effort
               ?? absent

caller explicit effort/thinking = 未进入上述优先级
```

plugin 可以在 helper 之后修改 body，但 configured route 又位于 plugin 之后，因此匹配规则的 target/effort 仍能覆盖 plugin 输出。

另一个一致性风险是，CX2CC 在第二次路由之前已用 bridge 产出的 `openai_model` 记录 `cx2cc_cost_basis.priced_model`：`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:230`、`:285`、`:294`。若通用路由稍后再改模型，该 marker 不再描述最终 wire model。阻止二次路由可同时消除此阶段错位。

## 3. 显式、缺省与未知值兼容矩阵

下表描述**无 plugin**时的当前行为和目标兼容行为；stream 标志不会改变请求参数优先级，因此 S/NS 都适用。

| 调用方输入 | CX2CC setting | 匹配通用规则 | 当前最终 Responses | 目标行为 |
|---|---|---|---|---|
| `/output_config/effort = "high"` | `medium` | target=`other`, effort=`low` | model=`other`，effort=`low`；显式值丢失 | model 只按 CX2CC mapper；effort=`high` |
| `/output_config/effort = "ultra"` 或 future string | `medium` | 无 | 显式值丢失，effort=`medium` | future string 语义等价透传；不被 setting 降级 |
| 无显式思考字段 | `medium` | 无 | effort=`medium` | 保持兼容：setting 作为缺省值注入 `medium` |
| 无显式思考字段 | 空字符串/`None` | 无 | 不生成 `reasoning` | 保持不生成 |
| `thinking.type = "disabled"` | `high` | 无 | `thinking` 被丢弃，反而注入 `high` | 调用方显式禁用必须抑制 setting；不生成 `reasoning` |
| `thinking.type = "enabled"/"adaptive"`，无可等价 effort | `high` | 无 | `thinking` 被丢弃并注入 `high` | 不发明 effort；至少记录调用方已显式控制并抑制 setting |
| `thinking.budget_tokens = N` | 任意 | 无 | 被丢弃 | 当前代码无可证实的 budget -> effort 等价表；不得猜测分桶。保持非破坏兼容时应抑制默认覆盖并明确标记“不支持精确透传” |
| 显式 effort 为空/全空白 | `medium` | 无 | 当前整体字段被忽略，注入 `medium` | 按缺省处理，注入 `medium`；需测试 trim 规则 |
| 显式 effort 为非字符串 | `medium` | 无 | 被忽略，注入 `medium` | 不是受支持参数；应有确定的 fail-closed 或按缺省策略，不能被误报为已透传 |
| 无显式值 | setting 为后端未知非空字符串 | 无 | 后端原样发送；UI 无匹配 radio | 保持 future-value 透传并在 UI 显示“当前未知值”，不自动改写 |
| 普通 Claude/Codex 请求 | 任意 | 有 | 通用 configured route 正常改 model/effort | 完全保持当前行为 |

“受支持的显式 effort”应以项目已有协议证据为准：普通 Claude Messages 的最终 effort 字段是 `/output_config/effort`，当前通用路由也写该字段（`src-tauri/src/gateway/configured_model_route.rs:221` 至 `:223`）。不要把 Codex 请求的 `/reasoning/effort` 或 `/reasoning_effort` 不加区分地当成 Anthropic Messages 主契约。

未知值的现有证据：

- provider 模型能力的共享枚举包含 `none/minimal/low/medium/high/xhigh/max/ultra`：`src/services/providers/providerModels.ts:14` 至 `:23`。
- Codex capability UI 的已知列表包含 `minimal` 至 `ultra`，且有保留当前未知值的视图类型和 helper：`src/components/cli-manager/tabs/codexModelCapabilities.ts:7` 至 `:15`、`:52` 至 `:55`、`:132` 至 `:152`。
- usage tracker 明确保留有界 future effort 字符串：`src-tauri/src/domain/usage.rs:430` 至 `:442`。

因此不能在 CX2CC Rust settings 层新增封闭枚举并静默把 future value 降级为 `medium/high`。UI 可以按当前模型能力限制新选择，但已持久化未知值必须可见、可保留。

## 4. 对应 upstream 实现（限定范围）

### 4.1 应移植的上游不变量

`upstream/main` 的旧 `provider_model_policy` 与 fork 的 `configured_model_route` 不是同一实现，不能逐行 cherry-pick；但 upstream 已明确建立本任务需要的单一路由不变量：

- `upstream/main:src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:185` 注释说明 CX2CC 用自己的 `claude_models` 映射，不再叠加 generic policy。
- 同文件 `:187` 至 `:191` 对 `provider.is_cx2cc_bridge()` 返回 `policy_target_model = None`。
- `upstream/main:src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_model_policy.rs:63` 再次声明 CX2CC 不会进入通用 policy apply。
- 这些行来自 upstream commit `6007d7a09dace7a775a2fb5300c05e165050b340`，该提交不在当前 HEAD ancestry 中。

当前 fork 的通用 configured route 来自 fork commit `f6773c152ce90afc47dc0eab8c1fcd59628e3c31`。它不仅遗漏 CX2CC guard，还在 `src-tauri/src/gateway/configured_model_route.rs:542` 的测试 `cx2cc_final_responses_shape_is_rewritten` 中显式断言“bridge 后继续改写”。该测试编码的是本 PRD 已废止的旧决策，实施时必须删除或反转，而不能把它当回归保护保留。

结论：**移植 upstream 的行为不变量，适配到 fork 的 configured route 解析点；不移植已被 fork 替换的旧 provider policy 模块。**

### 4.2 upstream 没有解决的部分

Pinned upstream 的 Anthropic inbound 同样在 `upstream/main:src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:51` 至 `:93` 忽略顶层思考配置并生成空 metadata。其 `apply_cx2cc_request_settings` 也在 `upstream/main:src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:105` 至 `:116` 无条件写 setting。因此显式思考透传不是可直接移植的上游修复，而是 pinned upstream 也存在、但被本任务 R3 明确授权修复的问题。

相反，当前 fork 的 `openai_responses::apply_responses_metadata`（`openai_responses.rs:233` 至 `:255`）是 upstream 没有的 fork 特有骨架；它已表达正确的“请求优先、设置兜底”方向，但真实 inbound 未填 metadata，且后续 helper 又覆盖，故不能据此判定 R3 已实现。

Upstream 新增的最终 wire effort 提取器提供了协议和 future-value 证据，但不构成 bridge 修复：

- `upstream/main:src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs:37` 至 `:49` 分别从 Responses、Chat Completions、Claude Messages、Gemini 的最终字段读取 effort。
- 同文件 `:42` 至 `:43` 确认 Claude Messages 使用 `/output_config/effort`。
- 同文件 `:139` 至 `:145` 测试保留 future explicit string `ultra`。

### 4.3 分类摘要

| 分类 | 项目 | 处理 |
|---|---|---|
| 移植（语义适配） | upstream 的 CX2CC 不叠加 generic provider policy | 在 fork `prepare_provider` 中让 CX2CC 的 `configured_model_route = None` |
| 当前已有 | CX2CC 自身 mapper；legacy Claude mapping 已排除 CX2CC；Responses outbound 的 request-first 骨架 | 保留并补齐真实 inbound 数据 |
| fork 特有且需修正 | `configured_model_route` bridge 后二次重写及其正向测试 | 只改解析/选择边界，不削弱普通路径 apply 逻辑 |
| upstream 原生但任务内 | 顶层思考配置丢失；setting helper 无条件覆盖 | 因 R3 明确授权，做最小链路修复 |
| 范围外 | 全量 upstream commit 审计、网关回环、GPT-5.6 完整模型目录、budget 数值到 effort 的新产品策略 | 由并行报告/后续明确决策处理 |

## 5. 最小变更边界

### A. Provider preparation：只阻断 CX2CC 的通用 route 解析

位置：`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:377`，符号 `prepare_provider`。

建议以已知的 `is_cx2cc_bridge`（或等价且更窄的 active bridge 判定）包住 `configured_model_route::resolve`：CX2CC 直接得到 `None`，其他 provider 保持原调用参数和优先级。不要伪造 `managed_model_route = true`，因为那会把两个独立概念混在审计和日志中；也不需要修改 `configured_model_route::apply` 对普通 Responses 的支持。

同时反转/删除 `configured_model_route.rs:542` 的旧 CX2CC 改写测试，新增 preparation 级测试证明：即使原 Claude model 精确命中 global/provider override，CX2CC prepared route 仍为 `None`；普通 Claude/Codex 仍为 `Some`。

### B. Protocol bridge：让调用方控制进入 IR

位置：

- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:51`，`parse_request`
- `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs:136`，`IRMetadata`
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:233`，`apply_responses_metadata`

最小方案可复用 `IRMetadata.extra`，避免给所有 IR 构造点新增必填字段：

1. 把受支持的 Claude `/output_config/effort` 非空字符串映射为 IR `reasoning = { effort: value }`。
2. 另用明确的内部 presence/suppression 标记表示调用方提供了顶层 `thinking`，即使它没有可等价 effort。该标记只控制“是否允许 settings fallback”，不能把 Anthropic `thinking` 对象原样塞进 OpenAI `reasoning`。
3. future 非空字符串按现有有界字符串约束透传，不在 bridge 中降级。
4. `thinking.type=disabled` 必须抑制设置注入；`enabled/adaptive` 和 `budget_tokens` 在没有证据支持精确映射时也不得被固定 setting 覆盖。不要发明 budget 分桶。

若实现者选择 typed IR 字段而非 `extra`，仍须保持同一优先级和 presence 语义；不能只增加 `Option<String>`，否则无法区分“调用方未提供”与“调用方显式 disabled/已控制但没有等价 effort”。

### C. 只保留一个 settings 注入所有者

当前 `openai_responses::apply_responses_metadata` 已能在 IR 缺省时注入 setting，故 `cx2cc_preparation.rs:229` 的第二次 `apply_cx2cc_request_settings` 是重复所有者。最小选择是删除该第二次写入，或把 helper 改为严格 fill-if-absent 且识别显式 suppression；不能继续整体替换 `reasoning`。

同一 helper 还重复处理 `service_tier/store`。收敛所有权时必须回归：请求未指定时，现有 settings 仍生效；请求 metadata 存在时不被 helper 覆盖；ChatGPT compat 过滤、`store=false` 默认和 schema 清理行为不变。

### D. UI 与 settings 兼容

保留 `cx2cc_model_reasoning_effort: String` 和空字符串缺省语义，不做数据迁移。CX2CC 选项不应继续维护第三份硬编码列表，应从任务统一后的模型能力/effort 数据源生成；至少要覆盖适用的 `minimal/max/ultra`。对已保存但当前 capability 未识别的值，采用 `codexModelCapabilities.ts:132` 的“current unknown”模式显示并保留，不在加载或保存其他设置时自动改写。

### E. 明确不改的边界

- 不改变 `CX2CCModelMapper` 的 `opus/haiku/sonnet/main` 契约，也不趁机改用普通 Claude 的 `reasoning_model` 槽位。
- 不改变普通 Claude/Codex/Grok/Gemini 的 `configured_model_route::resolve/apply`、鉴权、plugin、故障转移与 retry 行为。
- 不把历史 assistant `thinking` 内容块当成本次请求配置。
- 不在本修复中定义 `budget_tokens -> effort` 数值映射。
- 不在本报告处理当前 AIO Codex 入口的回环 guard；只要求上述 route/effort 测试能在合法来源场景复用。

## 6. 必须测试的矩阵

### 6.1 参数优先级与模型路由

| ID | 路径 | 输入与冲突设置 | 必须断言 |
|---|---|---|---|
| P1 | CX2CC | sonnet + 显式 `output_config.effort=high` + setting=`medium` + matching global route target/low | 最终 wire model 为 CX2CC sonnet 槽；effort=`high`；无 configured route marker |
| P2 | CX2CC | opus/haiku/sonnet/main 各模型 + provider-specific route override | 四类仍按 CX2CC mapper/fallback；provider override 不叠加 |
| P3 | CX2CC | 显式 future effort + setting=`medium` | future 字符串原样到 `/reasoning/effort`，不降级 |
| P4 | CX2CC | 无显式值 + setting=`medium` | 兼容保留 `/reasoning/effort=medium` |
| P5 | CX2CC | 无显式值 + setting 空 | 不生成 `reasoning` |
| P6 | CX2CC | `thinking.type=disabled` + setting=`high` | 不生成 `reasoning`，证明 presence 抑制 fallback |
| P7 | CX2CC | `thinking.type=enabled/adaptive` 或 budget，无等价 effort + setting=`high` | 不凭空注入 high；行为按已定义 unsupported/presence 契约稳定 |
| P8 | CX2CC failover | 首个 CX2CC 来源失败，后备 CX2CC 有 matching provider route | 每个 attempt 都不解析通用 route；第二个来源的 CX2CC mapper 独立生效 |
| P9 | 普通 Claude | matching global/provider route，含 target + effort | 仍改 `/messages` model 和 `/output_config/effort`；provider override 仍替代 global |
| P10 | 普通 Codex Responses | matching route，含 target + effort | 仍改 `model` 和 `/reasoning/effort`；managed `aio/` 排除逻辑不回归 |
| P11 | plugin | CX2CC plugin 修改非路由字段；普通路径 plugin + configured route | CX2CC 不再被 route 后处理覆盖 model/effort；普通路径仍维持现有 plugin 后 route 顺序 |
| P12 | 观测/计费 | CX2CC 映射后无通用 route | `active_requested_model`、实际 wire model、`cx2cc_cost_basis.priced_model` 一致；无伪 configured route marker |

Bridge 层现有 `openai_responses.rs:1481` 的 metadata-preservation 单测和 `:1515` 的 setting-injection 单测只覆盖手工 IR，必须新增从 Anthropic JSON 进入的 e2e 测试。现有 `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs:36` 只验证基本 model/body，尚未覆盖显式 effort 与冲突 setting。

### 6.2 Stream / non-stream 与响应形状

请求改写发生在响应分流之前，stream 和 non-stream 必须各自捕获**最终 transport body**，不能只测 bridge 返回值。

| ID | 客户端请求 | 来源响应 | 路径 | 必须断言 |
|---|---|---|---|---|
| S1 | CX2CC `stream=false` | Responses JSON | non-stream handler | transport body 使用 CX2CC model + 正确 effort；返回 Anthropic JSON |
| S2 | CX2CC `stream=true` | Responses SSE | event-stream handler | 同一 model/effort 优先级；`BridgeStream` 正常转 Anthropic SSE（入口见 `success_event_stream.rs:1598`） |
| S3 | CX2CC `stream=false` | 来源意外返回 SSE | non-stream recovery | SSE 被 buffer 成 JSON（`success_non_stream.rs:173`、`:227`），路由/effort 不变化 |
| S4 | CX2CC `stream=true` | 来源返回 JSON | non-stream-to-SSE | `translate_response_to_sse` 生效（`success_non_stream.rs:544` 至 `:554`），model/usage 不丢 |
| S5 | 普通 Claude `stream=false/true` | JSON/SSE | 两个 handler | configured route 仍命中，鉴权/响应 passthrough 不变 |
| S6 | 普通 Codex `stream=false/true` | JSON/SSE | 两个 handler | configured route、usage observer、retry/failover 不回归 |

现有响应测试可作为基础但不能替代 transport 请求断言：

- `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs:63`，CX2CC non-stream round trip
- `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs:103`，CX2CC SSE usage round trip
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs:2153`，client stream + upstream JSON 的包装测试
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:1598`，实际 streaming bridge 入口

### 6.3 前端与 settings

1. `Cx2ccTab` 显示任务统一后的全部适用 effort（含 GPT-5.6 所需等级），选择后只提交对应 patch。
2. settings 为空时选中“默认”；选择后清空能回到空字符串。
3. Rust/导入中已有 future unknown value 时，UI 显示当前未知值且其他设置保存不会覆写它。
4. `Cx2ccSettings::from_app_settings` 覆盖空白 -> `None`、known -> `Some`、future string -> `Some`。
5. 前端、Rust settings、Responses outbound 对同一 effort 集合/unknown policy 的测试保持一致；不能只测 UI 文案。

## 7. 风险与实施检查点

- **最容易出现的假修复**：只让 `apply_responses_metadata` 请求优先，却不填 Anthropic IR metadata，或填了 metadata但保留 `cx2cc_preparation.rs:229` 的无条件 helper；两者都会让端到端行为仍然错误。
- **最容易漏掉的路由点**：只跳过 legacy Claude mapping。该路径当前已经跳过；真正的二次改写在 `provider_iterator.rs:377` 解析、`attempt_executor.rs:338` 应用。
- **测试层级风险**：`configured_model_route::apply` 的纯函数测试可以证明它能改 Responses，却不能证明 CX2CC 不会选择该 route。必须在 provider preparation 或最终 transport capture 层断言 route 为 `None`。
- **future value 风险**：provider 可能拒绝未来 effort，但网关职责是语义透传并让真实来源返回协议错误，不应静默降级。错误处理和 failover 仍按现有路径执行。
- **budget 语义风险**：OpenAI effort 与 Anthropic token budget 不存在本仓库可证实的一一对应。若产品后续要求精确支持，应单独定义协议合同；本次不要用经验阈值猜测。
- **共享字段风险**：移除重复 helper 时同时影响 `service_tier/store` 的所有权。对缺省请求其 wire 值必须与当前一致，并为请求优先分支补回归测试。

## 8. 建议验收判据

实现完成后，从实际来源捕获的最终请求应满足：

```text
CX2CC model: 仅由 CX2CCModelMapper / claude_models / CX2CC fallback 决定
CX2CC effort: 调用方受支持显式值 > 调用方显式控制的 suppression > CX2CC setting 缺省 > absent
普通路径: 继续执行既有 configured model route
```

同时 `configured_model_route` 特殊设置、`active_requested_model`、CX2CC cost basis 和实际 wire body 必须描述同一个最终模型；stream/non-stream 只改变响应承载，不改变上述请求优先级。
