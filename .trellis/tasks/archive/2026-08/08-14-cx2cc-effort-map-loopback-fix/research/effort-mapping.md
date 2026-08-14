# Research: CX2CC 全局思考强度映射

- Query: 为 CX2CC 增加可新增、编辑、删除、恢复默认的全局思考强度映射；默认 `low -> low`、`medium -> medium`、`high -> high`、`xhigh -> xhigh`、`max -> max`、`ultra -> max`；缺省 effort 保持缺省，Disabled 状态固定输出 `none` 且不受配置影响，未命中值原样透传。查清最小持久化、默认值、校验、迁移、settings service、生成绑定、配置包、`Cx2ccTab`、协议应用点、日志和测试范围。
- Scope: internal
- Date: 2026-08-14

## Findings

### 1. 推荐的最小数据与行为契约

建议新增一个有序、带类型的列表，而不是 JSON 字符串或对象映射：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default, deny_unknown_fields)]
pub struct Cx2ccReasoningEffortMapping {
    pub source_effort: String,
    pub target_effort: String,
}

pub cx2cc_reasoning_effort_mapping: Vec<Cx2ccReasoningEffortMapping>
```

选择 `Vec<...>` 的理由：UI 需要稳定展示和逐行增删改；顺序可持久化；后端可以在规范化后明确拒绝重复 `source_effort`。若使用 JSON object / `HashMap`，重复 key 会在解析阶段折叠，无法向用户报告歧义，且展示顺序不再是契约。

默认值必须只有以下六条可编辑规则，顺序固定：

| source | target |
| --- | --- |
| `low` | `low` |
| `medium` | `medium` |
| `high` | `high` |
| `xhigh` | `xhigh` |
| `max` | `max` |
| `ultra` | `max` |

运行时应执行一次、大小写敏感、非递归的精确查找：

```text
input effort
  -> 第一条 source_effort == input 的规则
  -> 命中则返回该条 target_effort
  -> 未命中则返回原 input
```

特别要用 `ultra -> max, max -> low` 证明 `ultra` 最终只得到 `max`，不能继续链式得到 `low`；环形规则也不能循环。

当前 IR 已经区分 presence/state：`Absent`、`Disabled`、`Enabled(Option<String>)`、`Adaptive(Option<String>)`、`Effort(String)`，见 `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs:136`。因此行为矩阵应是：

| IR 状态 | Responses 输出 |
| --- | --- |
| `Absent` | 不写 `reasoning` |
| `Enabled(None)` / `Adaptive(None)` | 不写 `reasoning` |
| `Disabled` | 固定 `reasoning.effort = "none"`，完全绕过映射 |
| `Enabled(Some(v))` / `Adaptive(Some(v))` / `Effort(v)` | 对 `v` 做一次映射；未命中原样输出 |

这里的 Disabled 是 `thinking.type = "disabled"` 对应的语义状态，而不是字符串 effort。当前解析在 `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:101`，并在 `:107-111` 先识别 Disabled。因此固定规则不应写入可编辑列表；UI 如需展示，应以只读、不可删除的一行显示 `disabled -> none`。字面量 `output_config.effort = "disabled"` 在当前协议模型中仍是普通显式字符串，不等同于 Disabled 状态。

空映射列表必须是合法用户配置：表示所有显式 effort 原样透传，但 Disabled 仍固定为 `none`。不能把“空列表”当作“缺字段”或“损坏后恢复默认”。

### 2. 持久化、默认值、校验与迁移

#### 当前所有权

- 设置持久化是 `settings.json`，不是 SQLite。`AppSettings` 在 `src-tauri/src/infra/settings/types.rs:527-529` 使用结构级 `#[serde(default)]`；缺失字段从 `AppSettings::default()` 补齐。
- 当前 settings schema 是 62，见 `src-tauri/src/infra/settings/defaults.rs:5`；最后一个命名版本常量在 `:65`。
- `AppSettings` 的 CX2CC 字段集中在 `src-tauri/src/infra/settings/types.rs:632-643`，默认值集中在 `:719-729`。
- 读取先反序列化、迁移/修复、边界校验，再在需要时规范化回写，见 `src-tauri/src/infra/settings/persistence.rs:192-199` 和 `:250-260`。
- 写入会 clone、规范化、校验并原子替换，见 `src-tauri/src/infra/settings/persistence.rs:533-572`；共享 RMW 锁入口是 `settings::update`，见 `:664-675`。
- settings 文件总上限为 1 MiB，见 `src-tauri/src/infra/settings/defaults.rs:144`。

#### 最小 schema 方案

1. 把 `SCHEMA_VERSION` 提升到 63，新增类似 `SCHEMA_VERSION_ADD_CX2CC_REASONING_EFFORT_MAPPING` 的常量。
2. 在 `defaults.rs` 保存一份 Rust 默认 pair 常量；在 `types.rs` 提供构造 `Vec<Cx2ccReasoningEffortMapping>` 的默认函数，并让 `AppSettings::default()` 使用它。
3. 在 `AppSettings` 上新增列表字段。结构级 serde default 已能让 schema 62 / 无 schema JSON 的缺字段得到六条默认值，但仍应增加 62 -> 63 的显式迁移以记录持久化契约。
4. 迁移函数只负责版本推进，不应使用 `if mapping.is_empty() { mapping = defaults }`。通用 bump 模式见 `src-tauri/src/infra/settings/migration.rs:921-941`，迁移注册表见 `:1561-1606`。

需要锁定以下迁移语义：

| 输入 | 结果 |
| --- | --- |
| schema <= 62 且字段缺失 | serde default 补六条，再 bump 到 63 |
| schema <= 62 且已有自定义字段 | 保留并规范化该自定义值，再 bump |
| schema 63 且字段为 `[]` | 必须保持 `[]` |
| schema 63 且字段缺失 | serde default 补六条；canonical repair 回写 |
| 缺少 schema 且字段缺失 | 补六条并迁移到 63 |

旧 CX2CC 迁移会在版本低于 26 时强制若干默认字段，见 `src-tauri/src/infra/settings/migration.rs:1140-1182`。该模式不能直接照搬到新列表，因为用户删除全部规则是有意义的已保存状态。

#### 严格写入校验与读时修复

建议新增 CX2CC 专属限制常量，即使数值复用现有成熟上限：最多 128 条、source/target 各最多 64 个 Unicode 字符。现有模型路由已有 128 条和 effort 64 字符的先例，见 `src-tauri/src/infra/settings/defaults.rs:67-69`；请求日志也把 effort 证据限制在 64 字符，见 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs:4`。

严格 writer canonicalizer 应：

- 保持列表顺序；trim source 和 target。
- source、target 都必须非空。
- 各自最多 64 个 Unicode 字符，且不能含控制字符。
- 在 trim 后按大小写敏感的 source 精确去重；重复应返回 `SEC_INVALID_INPUT`。
- 空列表合法。
- 不做 closed enum 校验；未来 effort 字符串和用户自定义目标必须可保存。

现成模式是模型路由的 strict normalize + read sanitizer：写入规范化、重复拒绝见 `src-tauri/src/infra/settings/migration.rs:394-476`，读时丢弃非法项见 `:479-515`，并由 `repair_settings` 在 `:1619-1644` 调用。新映射可按相同模式增加：

- `normalize_cx2cc_reasoning_effort_mapping_for_write(&mut Vec<_>) -> AppResult<bool>`：供 settings service 和 config import 使用。
- `sanitize_cx2cc_reasoning_effort_mapping(&mut Vec<_>) -> bool`：仅用于磁盘读修复；不能把合法空列表改回默认。
- `validate_bounds` 仍做最终防线。当前 CX2CC 边界检查位于 `src-tauri/src/infra/settings/persistence.rs:342-377`。

若给复杂字段添加 lossy deserializer，应明确失败策略。现有模型路由对 malformed 整体回退默认，见 `src-tauri/src/infra/settings/types.rs:429-445`；上游错误规则则逐项过滤，见 `:493-507`。对该列表最稳妥的是 production writer 严格拒绝，读时仅修复可解析项并记录 settings warning；绝不能因 malformed 回退为空后与“用户主动删除全部”混淆。无论采用哪种反序列化策略，都要有 malformed 文件测试。

### 3. Settings service、普通 owner 与回滚

这是普通 settings 页面/CLI Manager 所有的字段，必须完整穿过普通 owner；不能只加到 `AppSettings`。规范明确未来 `AppSettings` 字段不会自动成为普通 writer 所有字段，见 `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md:88-95`。

需要同步的具体位置：

| 位置 | 必需改动 |
| --- | --- |
| `src-tauri/src/app/settings_service.rs:31-119` | `SettingsUpdate` 增加 `Option<Vec<...>>`，显式 serde/specta 名建议 `cx2CcReasoningEffortMapping` |
| `src-tauri/src/app/settings_service.rs:121-208` | `SettingsPatch` 增加同名 optional 字段 |
| `src-tauri/src/app/settings_service.rs:210-277` | `SettingsPatch::to_update` 传递该字段 |
| `src-tauri/src/app/settings_service.rs:280-340` | `SettingsServiceOwnedToken` 增加完整 Vec |
| `src-tauri/src/app/settings_service.rs:342-404` | token 从 canonical settings clone 字段 |
| `src-tauri/src/app/settings_service.rs:414-474` | token apply/rollback 恢复字段 |
| `src-tauri/src/app/settings_service.rs:485-561` | `SettingsView` 暴露 snake_case Vec |
| `src-tauri/src/app/settings_service.rs:614-695` | `From<&AppSettings> for SettingsView` clone 字段 |
| `src-tauri/src/app/settings_service.rs:989-1047` | 从 update 或 previous token 取候选值 |
| `src-tauri/src/app/settings_service.rs:1097-1113` | 在提交前调用 strict normalizer；这是其他结构化策略的既有位置 |
| `src-tauri/src/app/settings_service.rs:1121-1179` | committed token 纳入 canonical Vec |
| `src-tauri/src/app/settings_service.rs:1181-1188` | apply 后继续由 `validate_bounds` 做最终校验 |

普通 writer 必须继续使用 lock 内 patch merge。`settings::update` 的锁内 read/mutate/write 边界见 `src-tauri/src/app/settings_service.rs:16-29`；前端只发 changed keys 的契约见 `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md:78-87`。若漏掉 owner token，运行时同步失败的 rollback 或并发 winner 会丢失/错误恢复这份映射。

后端重点测试位置：

- Specta key 反序列化测试：`src-tauri/src/app/settings_service.rs:2599-2639`。
- 旧前端缺字段仍得到 `None`：`:2641-2655`。
- 完整 ordinary update fixture：`:2657-2731`。
- 并发 patch merge：`:2784-2815`。
- 另加 runtime sync 失败 rollback 和并发 winner 测试，确认映射属于 token 且只恢复 owned field。

### 4. 生成绑定与前端 settings adapter

`SettingsPatch`、`SettingsUpdate`、`SettingsView` 当前生成在 `src/generated/bindings.ts:4388`、`:4453`、`:4516`；CX2CC view 字段在 `:4576-4586`。新增 Rust entry type及三处字段后必须重新生成，不应手改生成文件。

生成入口和检查：

- `src-tauri/examples/export-bindings.rs:1-7` 调用 Rust exporter。
- `src-tauri/src/lib.rs:51-65` 暴露 exporter 和手动测试入口。
- `scripts/tauri-gen-types.mjs:27-41` 运行 locked Cargo example。
- `package.json:59` 是 `tauri:gen-types`，`:63` 是 `check:generated-bindings`。

前端 adapter 必须同步：

- `src/services/settings/settings.ts:1-22` 导入生成的 entry type，并在 `:33-48` re-export 供 UI 使用。
- `SETTINGS_VIEW_TO_UPDATE_FIELD_MAP` 在 `src/services/settings/settings.ts:111-169` 加 `cx2CcReasoningEffortMapping -> cx2cc_reasoning_effort_mapping`。
- `toGeneratedSettingsUpdate` 在 `src/services/settings/settings.ts:244-305` 编码该 Vec。
- `createGeneratedSettingsPatch` 已按 changed-key 映射生成 patch，见 `:324-345`；补映射后即可只发送该 key。
- `__AssertNoUnhandledSettingsViewKeys` 在 `src/services/settings/settings.ts:175-209` 会在漏接新 view 字段时让 typecheck 失败，应保留这道约束。
- `settingsPatch` 会先验证 candidate 再调用 `settings_patch`，见 `src/services/settings/settings.ts:385-398`。

前端校验目前只接收单值 CX2CC 字段，见 `src/services/settings/settingsValidation.ts:431-436`，主校验在 `:571-588`。应把新 Vec 加进输入，并调用与后端一致的 map validator。现有无控制字符、trim、CX2CC 长度校验模式见 `:342-370`；列表/重复规则可参考 `src/services/gateway/modelRoutingPolicy.ts:42-96`。前端只为即时反馈，Rust 仍是权威边界。

需要更新的完整 SettingsView fixtures 至少包括：

- `src/test/fixtures/settings.ts:69-79`
- `src/test/msw/state.ts:87-97`
- `src/__tests__/msw-default-settings.test.ts:87-97`
- `src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx:9-38`

### 5. Config bundle

不需要提高 config bundle schema：

- bundle schema 当前为 v4，`ConfigBundle.settings` 是一个完整 settings JSON 字符串，见 `src-tauri/src/infra/config_migrate/mod.rs:18-25` 和 `:55-75`。
- export 读取并直接序列化完整 `AppSettings`，见 `src-tauri/src/infra/config_migrate/mod.rs:399-419`，新字段会自动随 settings 输出。
- import 将内嵌 JSON 反序列化为 `AppSettings` 并执行 settings schema migration，见 `src-tauri/src/infra/config_migrate/mod.rs:558-576`。

应在 import preflight 的 settings migration 后无条件调用新 strict normalizer，类似 `model_routing_policy` 的 `src-tauri/src/infra/config_migrate/mod.rs:566-569`。不应按 config bundle v4 gate 这项字段，因为兼容版本由内嵌 settings schema 63 管理；旧 v1-v4 bundle 缺字段时自然得到默认映射。

测试入口 `src-tauri/src/infra/config_migrate/tests.rs:196-202` 用 `AppSettings::default()` 建 bundle；迁移案例见 `:236-301`，export settings 案例从 `:348` 开始。新增测试应覆盖：

- 自定义映射 export/import round-trip。
- schema 62 / 缺 schema bundle 得到六条默认值。
- schema 63 的空列表保持空。
- 当前 schema 的重复、空 source/target、超限、控制字符在 preflight 阶段失败，且不产生部分写入。

没有发现需要数据库 schema 迁移或 Provider share schema 迁移的证据；该配置属于全局 `AppSettings`，不属于 Provider 行。

### 6. Runtime 传播与协议应用点

#### 设置进入 bridge

- 每次请求由 `RuntimeSettingsMiddleware` 读取当前 settings，见 `src-tauri/src/gateway/proxy/handler/middleware/runtime_settings_reader.rs:10-26`。
- `handler_runtime_settings` 把 `AppSettings` 转为 `Cx2ccSettings`，见 `src-tauri/src/gateway/proxy/handler/runtime_settings.rs:39-62`。
- `Cx2ccSettings` 当前字段、`from_app_settings`、runtime default 分别在 `src-tauri/src/gateway/proxy/cx2cc/settings.rs:7-19`、`:21-36`、`:39-54`。新 Vec 必须在三处 clone/default，并可在这里提供单次 exact lookup helper。
- `BridgeContext` 已持有完整 `Cx2ccSettings`，见 `src-tauri/src/gateway/proxy/protocol_bridge/traits.rs:95-109`。
- CX2CC preparation clone 该 runtime settings 进 bridge，再调用真实 translation，见 `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:253-300`。

#### 唯一正确的协议落点

Inbound 必须继续只解析和保留源语义：`output_config.effort` 在 `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:101-112` 进入 IR；不要在这里映射，否则会丢失源值，并让其他潜在 outbound 无法区分 source/target。

映射应加在 Responses outbound 的 `apply_responses_metadata`：

- `ResponsesOutboundSettings` 当前从 `Cx2ccSettings` 投影引用/布尔设置，见 `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:15-31`；增加一个 mapping slice 引用仍可保持轻量、只读。
- `ir_to_request` 在 `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:226` 调用 metadata writer。
- `apply_responses_metadata` 在 `:231-247` 已经精确区分 Absent、Disabled、有显式 effort 和 enabled/adaptive 无 effort。只修改 `:241-245` 的显式 effort 分支为一次 lookup；`:237`、`:238-240`、`:246` 保持不变。

后续 `apply_cx2cc_request_settings` 只处理 service tier/store，见 `src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:146-156`；不要在那里再次映射。其现有测试 `:162-180` 也在保护 reasoning 不被 legacy fixed setting覆盖。

#### 回环和重复映射风险

CX2CC bridge 的全流程是 client body -> IR -> model map -> provider body，见 `src-tauri/src/gateway/proxy/protocol_bridge/bridge.rs:30-50`。无显式 source provider 时，翻译后的 Responses body 会作为受信 Codex 请求回到本地 gateway；映射仍应只在第一跳 CX2CC outbound 执行一次。

配置模型路由已有双重隔离：`configured_model_route_for_request` 在 `cx2cc_active` 或 `trusted_internal_reentry` 时直接返回 `None`，见 `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:90-99`，实际调用点见 `:392-408`。因此不要把映射放到通用 `configured_model_route::apply` 或通用 Responses 发送层，否则目标 effort 在本地 Codex 第二跳可能再次被当成 source 处理。

### 7. 日志和可观测性

不需要新增请求日志 schema、DTO、badge 或“source -> target”字段。现有契约要求记录最终语义 outbound body 上的实际 effort，而不是原始意图：

- Responses/CX2CC 从 `reasoning.effort` 读取，见 `.trellis/spec/aio-coding-hub/cross-layer/reasoning-effort-observability-contract.md:48-63`。
- 必须在所有协议/请求变换之后、编码和发送之前提取，见同文件 `:67-70`。
- `attempt_executor` 目前正是在最终 attempt body 上提取，见 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:469-475`，并把值放进 attempt timing，见 `:507-512`。
- extractor 的 Responses 路径是 `reasoning.effort`，见 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs:14-59`。
- attempt 持久化字段在 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_record.rs:153-195`。
- 最终请求选择“最后成功，否则最后已发送 attempt”，见 `src-tauri/src/infra/request_logs/semantics.rs:27-39`。

所以 `ultra -> max` 默认映射后，日志自然应记录 `max`；未命中的 future effort 仍记录原值（日志最多 64 字符）。现有真实 CX2CC bridge + extractor 集成测试在 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs:187-236`，应改/扩为断言映射后的 target，同时保留 thinking-only 得到 `None`。

注意插件 `RequestBeforeSend` 可在 bridge 后继续修改最终 body，日志仍会忠实记录插件后的 wire 值；这符合 final-wire observability 契约，映射代码不应反向修改日志 extractor。

### 8. `Cx2ccTab` UI

`Cx2ccTab` 当前只有五个文本 draft key，见 `src/components/cli-manager/tabs/Cx2ccTab.tsx:21-63`；通过 reducer 跟随 canonical `appSettings`，见 `:66-77` 和 `:111-125`。单值保存失败时恢复 previous canonical 值，见 `:131-173`。新列表应沿用这个 canonical-response/rollback 语义，但使用独立列表 draft。

建议在“模型 Fallback 映射”和“上游请求注入”之间增加一个同级 `Card`“思考强度映射”，不要嵌套 card。交互：

- 受控的 source/target 双列行编辑器。
- 新增使用 `Plus`，删除使用 `Trash2`，图标按钮带 Tooltip/aria-label；现有成熟样式见 `src/components/gateway/ModelRoutingPolicyFields.tsx:16-80` 和 `:114-148`。
- 显示一条只读 `disabled -> none` 固定行，但不把它放进持久化 Vec，也不给编辑/删除入口。
- 多行编辑产生临时不完整状态，因此不要每次 keystroke/onBlur 写后端；使用本地 draft + 明确“保存”命令。
- “恢复默认”直接 clone 六条默认值并持久化；成功后使用返回的 canonical Vec，返回 `null`/失败则恢复之前 canonical Vec。
- 删除最后一条并保存 `[]` 是合法操作。
- `commonSettingsSaving || !appSettings` 时禁用所有 input、增删、保存、恢复默认。当前 disable 基线在 `src/components/cli-manager/tabs/Cx2ccTab.tsx:125`。
- `appSettings` 被外部刷新或 canonical save 返回时重置列表 draft；不要让旧 draft 覆盖新 snapshot。页面把 `appSettings`、saving 和 patch callback 传入 tab，见 `src/pages/cli-manager/useCliManagerPageDataModel.ts:771-775`。
- `persistCommonSettings` 返回 canonical settings 或 `null`，并在异常时内部 toast，见 `src/pages/cli-manager/useCliManagerPageDataModel.ts:394-457`；tab 仍要像现有字段一样以返回值决定保留/回退 draft。

恢复默认需要前端默认常量。`src/constants/cx2cc.ts:1-12` 已是 CX2CC 跨层默认/预设位置，建议新增 typed `DEFAULT_CX2CC_REASONING_EFFORT_MAPPING` 并深 clone 后使用。Rust/TS 会各有一份默认列表；应扩展 `src/constants/__tests__/crossLayerContracts.test.ts:170-195` 的源码契约测试来防漂移，而不是只在两边各测一次。

UI 测试基线在 `src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx:41-59`、`:244-265`、`:362-397`、`:399-428`。应增加：默认六条、add/edit/delete、删除到空、重复/空/控制字符/64 字符上限、save exact patch、restore defaults、保存 `null` 回滚、canonical response 归一化、saving/null settings disable、prop refresh 重置 draft。现有“legacy fixed reasoning effort control 不出现”的测试必须继续通过；不要复活 `cx2cc_model_reasoning_effort` UI。

### 9. 建议测试矩阵

#### Rust settings / migration

- `AppSettings::default()` 精确六条、有序、schema 63。
- schema 62 缺字段、缺 schema 缺字段得到默认；schema 63 `[]` 保持空；schema 63 custom 保持 custom；迁移幂等。
- strict normalize trim 成功；拒绝空 source/target、trim 后重复、控制字符、超过 64 字符、超过 128 条；允许空 Vec 和 future strings。
- read sanitizer 不把合法空 Vec 恢复默认；malformed JSON 策略有显式测试。
- settings service Update/Patch Specta key、缺字段兼容、changed-key merge、owner token rollback、并发 winner。
- settings file canonical serialization 包含新字段；1 MiB 总边界仍生效。

#### Rust runtime / protocol / logs

- `Cx2ccSettings::default/from_app_settings` 携带默认/custom/empty map。
- outbound 默认 `none/low/medium/high/xhigh/max` identity，`ultra -> max`。
- custom edit/delete；未命中 exact passthrough；大小写敏感。
- `Absent`、`Enabled(None)`、`Adaptive(None)` 不写 effort；Disabled 无视 `disabled`/`none` 相关自定义规则并固定 `none`。
- single-hop test：`ultra -> max` 与 `max -> low` 同时存在时输出 `max`；环形 map 不循环。
- E2E 真实 bridge 测试更新 `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs:72-139`，当前 `ultra` 期望在 `:130-133` 仍是 `ultra`，必须改为默认 `max`。
- outbound 单测更新 `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:1519-1601`，保留 legacy field 不回填、unknown passthrough。
- 日志集成断言最终 mapped target；缺省仍无 badge/evidence。
- CX2CC first hop 与 trusted internal reentry 继续不调用 configured route resolver。

#### Config / bindings / frontend

- Config bundle custom/default/empty/malformed round-trip 与原子拒绝。
- 重新生成 bindings 并运行 drift check。
- `settings.ts` set/patch 的新字段映射、只发 changed key、校验失败不 invoke。
- 更新 MSW/fixture 默认快照和跨 Rust/TS 默认列表一致性测试。
- `Cx2ccTab` 完整交互、回滚、disable、canonical resync 测试。

建议验证命令：

```text
cd src-tauri && cargo fmt -- --check
cd src-tauri && cargo test --locked --lib cx2cc
cd src-tauri && cargo test --locked --lib reasoning_effort
cd src-tauri && cargo test --locked --lib settings
cd src-tauri && cargo test --locked --lib config_migrate
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm exec vitest run src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx src/services/settings/__tests__/settings.test.ts src/__tests__/msw-default-settings.test.ts src/constants/__tests__/crossLayerContracts.test.ts
pnpm check:prepush
```

## Files Found

- `.trellis/spec/aio-coding-hub/cross-layer/cx2cc-routing-contract.md` — CX2CC 模型、reasoning presence、双跳隔离和测试契约；当前含与 `ultra -> max` 冲突的旧条款。
- `.trellis/spec/aio-coding-hub/cross-layer/reasoning-effort-observability-contract.md` — 最终 wire effort 的提取、attempt 保存和 UI 投影契约。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md` — 普通 settings writer、changed-key patch 和 owned-token rollback 契约。
- `src-tauri/src/infra/settings/defaults.rs` — settings schema、CX2CC 默认值和边界常量。
- `src-tauri/src/infra/settings/types.rs` — `AppSettings`、serde defaults、复杂字段反序列化模式。
- `src-tauri/src/infra/settings/migration.rs` — schema migration、strict normalizer、read sanitizer 和 repair pipeline。
- `src-tauri/src/infra/settings/persistence.rs` — settings JSON 读取、校验、原子写和共享 update 锁。
- `src-tauri/src/infra/settings/mod.rs` — settings 类型、默认常量和 normalizer 的 re-export 边界。
- `src-tauri/src/app/settings_service.rs` — ordinary Update/Patch/View、owned token、应用/回滚和 IPC 测试。
- `src-tauri/src/infra/config_migrate/mod.rs` — v4 bundle 中完整 settings JSON 的导入导出。
- `src-tauri/src/infra/config_migrate/tests.rs` — bundle settings migration、export/import 和拒绝测试基线。
- `src-tauri/src/gateway/proxy/cx2cc/settings.rs` — CX2CC runtime settings 投影和默认值。
- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs` — Anthropic effort/thinking presence 到 IR 的解析。
- `src-tauri/src/gateway/proxy/protocol_bridge/ir.rs` — presence-preserving reasoning IR。
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs` — 唯一应应用 CX2CC effort map 的 Responses metadata writer。
- `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs` — 从 Anthropic 到 Responses 的真实 bridge reasoning 回归测试。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs` — CX2CC 翻译与受信本地 Codex reentry 的第一跳准备。
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs` — CX2CC first hop / trusted second hop 的 configured-route 隔离。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs` — 最终 semantic body 的 effort 提取时点。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs` — 协议感知 effort extractor 和真实 CX2CC 集成测试。
- `src-tauri/src/infra/request_logs/semantics.rs` — 最终 attempt effort selector。
- `src/generated/bindings.ts` — 生成的 SettingsPatch/Update/View 和嵌套类型出口。
- `src/services/settings/settings.ts` — generated settings 与前端 snake_case `AppSettings` 的字段映射及 changed-key patch。
- `src/services/settings/settingsValidation.ts` — 设置页前端校验总入口。
- `src/constants/cx2cc.ts` — 前端 CX2CC 默认/预设常量位置。
- `src/components/cli-manager/tabs/Cx2ccTab.tsx` — CX2CC 全局设置 UI 与 canonical save 回退模式。
- `src/components/gateway/ModelRoutingPolicyFields.tsx` — 可复用的行编辑器视觉/无障碍模式。
- `src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx` — CX2CC tab 交互、禁用、同步和 legacy control 回归测试。
- `src/test/fixtures/settings.ts`、`src/test/msw/state.ts`、`src/__tests__/msw-default-settings.test.ts` — 手工维护的 SettingsView 默认 fixtures。

## Code Patterns

- 缺字段默认：`AppSettings` 的结构级 `#[serde(default)]` 在 `src-tauri/src/infra/settings/types.rs:527-529`，canonical repair 在 `src-tauri/src/infra/settings/persistence.rs:250-257`。
- 版本迁移：`migrate_bump_schema_version` 在 `src-tauri/src/infra/settings/migration.rs:921-941`，迁移按注册顺序执行在 `:1608-1616`。
- 结构化规则严格写/宽容读：model routing normalizer/sanitizer 在 `src-tauri/src/infra/settings/migration.rs:394-515`。
- 原子普通设置更新：`settings::update` 在 `src-tauri/src/infra/settings/persistence.rs:664-675`；ordinary owner token 在 `src-tauri/src/app/settings_service.rs:280-474`。
- 生成绑定防漏接：`SETTINGS_VIEW_TO_UPDATE_FIELD_MAP` 与 never assertions 在 `src/services/settings/settings.ts:111-209`。
- presence-preserving bridge：Anthropic parser 在 `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:101-112`，Responses writer 在 `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:231-247`。
- 单跳隔离：CX2CC/trusted reentry 跳过 generic configured route 在 `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:90-99`。
- final-wire 日志：提取发生在 `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:469-475`，最终选择在 `src-tauri/src/infra/request_logs/semantics.rs:27-39`。

## External References

无。此项为仓库内部 settings/bridge 契约变更，项目 Trellis specs 和现有代码是权威来源；未使用外部文档，也未使用 Claude。

## Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/cx2cc-routing-contract.md:92-106`：当前 reasoning presence 契约。
- `.trellis/spec/aio-coding-hub/cross-layer/cx2cc-routing-contract.md:157-168`：当前验证矩阵。
- `.trellis/spec/aio-coding-hub/cross-layer/cx2cc-routing-contract.md:202-208`：协议测试要求。
- `.trellis/spec/aio-coding-hub/cross-layer/reasoning-effort-observability-contract.md:48-70`：最终 wire effort 来源和提取时点。
- `.trellis/spec/aio-coding-hub/cross-layer/reasoning-effort-observability-contract.md:113-128`：日志/前端测试要求。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md:73-98`：字段 owner、patch 和 rollback。
- `.trellis/spec/aio-coding-hub/cross-layer/index.md:127-151`：CX2CC reasoning 与 observability 的 pre-development checklist。

## Caveats / Not Found

1. **现有 spec 有直接冲突。** `cx2cc-routing-contract.md:103-104` 明确写着 unknown future effort 原样保留，并特别禁止把 `ultra` 转成 Responses catalog；新需求把 `ultra` 定义为已知默认映射到 `max`。实现前必须通过 update-spec 修改为“命中可配置映射时输出 target；未命中 future 值原样透传”，并同步 `:167` 的验证矩阵。研究代理未修改 spec。
2. **`disabled` 的含义必须保持 IR 状态。** 当前代码和 spec 的 Disabled 都来自 `thinking.type = "disabled"`，不是 effort 字符串。本文按此实现固定、不可配置语义；若产品实际还想保留字面量 source key `"disabled"`，PRD 需另行明确是否拒绝该 key。无论如何，IR Disabled 分支必须绕过列表。
3. **空列表不能被默认化。** 这是“删除所有映射”与“旧版本缺字段”的兼容分界；任何无条件 `empty -> defaults` 都会让删除功能无法持久化。
4. **不得做递归/链式映射。** 本地 Codex reentry 和 source/target 重叠会放大二次映射错误；映射应只在 CX2CC Responses outbound 的一次显式 effort 分支执行。
5. **默认值有 Rust/TS 双份。** 恢复默认 UI 需要前端常量；必须增加跨层源码契约测试，否则以后单边修改会漂移。
6. **日志记录 target，不记录 source。** 当前 schema 只表示真正发出的 effort；新增 source/target 审计字段不是本需求的最小范围，也会扩散到 DB、事件、bindings 和 UI。
7. **legacy 字段继续保留但不能参与 fallback。** `cx2cc_model_reasoning_effort` 仍需 schema/IPC 兼容；当前回归测试明确隐藏 UI 且不重写它，见 `src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx:244-265`。
8. **任务 PRD 仍是 TBD，且 Goal 同时包含自回环/120 秒慢请求。** 见 `.trellis/tasks/08-14-cx2cc-effort-map-loopback-fix/prd.md:1-13`。本文只覆盖父任务指定的 effort mapping；没有对慢请求修复给出结论。
9. 未发现该字段需要 SQLite、Provider share、请求日志 schema 或 config bundle schema 版本升级。
