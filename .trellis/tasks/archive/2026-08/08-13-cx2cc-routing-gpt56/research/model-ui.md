# GPT-5.6 模型与 reasoning effort 数据源审计

## 结论摘要

调研基线：当前分支 `81fd6d0860d1a6cc8c053f42d8aa941a0a445e96`，
`upstream/main` 为 `7725effd33ab9d7e1e8c4f9b5bb30c6e5a0ff23e`，共同祖先为
`4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`。

1. 公开 GPT-5.6 型号应覆盖 `gpt-5.6`（`gpt-5.6-sol` 的 alias）、
   `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`。官方 Responses API
   对三款具名型号声明的 `reasoning.effort` 均为
   `none / low / medium / high / xhigh / max`，默认 `medium`。证据：
   OpenAI Model guidance 的 GPT-5.6 Introduction / Update API parameters，及
   Sol/Luna model pages（2026-08-13 读取）。
2. 当前 CX2CC 设置页仅提供“缺省/不注入、low、medium、high、xhigh”，实际缺
   **显式 `none` 与 `max`**；`ultra` 不是 Responses API effort，不能机械加入该控件。
   `Cx2ccTab.tsx:282-300` 的控件最终经 `openai_responses.rs:233-242`
   写入 `reasoning.effort`。
3. 当前 CX2CC 供应商编辑器的静态默认模型预设只有 `gpt-5.5` 与 `gpt-5.4`：
   `providerEditorUtils.ts:31-34`、`Cx2ccSection.tsx:15-16`。因此缺少上述四个
   GPT-5.6 可选 ID；手动输入虽可绕过，但不满足“供应商模型选择中可选”。
4. 普通 Codex CLI 模型选择并不缺 GPT-5.6 静态项：它通过 Codex app-server
   `model/list` 动态取得型号及逐模型 effort，见 `protocol.rs:203-284`，前端直接消费
   生成绑定，见 `cliManager.ts:15-17,42-44`。这里不应新增 GPT-5.6 硬编码目录。
5. fork 特有的 provider model catalog 也不维护型号白名单；`/models` 发现只解析
   `data[].id`（`provider_models.rs:1029-1057`），能力需用户逐模型配置。因此它能发现
   或手动录入 GPT-5.6，但不会自动得到正确 effort。其通用能力枚举已经能表达
   `none / minimal / low / medium / high / xhigh / max / ultra`。
6. 存在多份静态 effort 与默认模型常量，且语义已漂移。最明显的是：CX2CC UI、
   provider catalog Rust/TS enum、Codex fallback/rank、请求日志 Rust/TS 默认映射各自
   维护列表。它们不能全部合成一个全集，因为 Responses API effort、Codex CLI
   effort 和通用供应商可配置 effort 是不同协议域；应按协议建立少量事实源，并让
   UI/校验/生成绑定派生，而不是继续复制数组。

## 已确认事实

### 1. GPT-5.6 型号与能力矩阵

| 语境                              | 型号/alias                                                | effort                                                          | 默认                     | 证据与含义                                                            |
| --------------------------------- | --------------------------------------------------------- | --------------------------------------------------------------- | ------------------------ | --------------------------------------------------------------------- |
| OpenAI Responses API / CX2CC 出站 | `gpt-5.6`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna` | `none, low, medium, high, xhigh, max`                           | `medium`                 | 官方 GPT-5.6 Model guidance；`gpt-5.6` alias 指向 Sol                 |
| Codex CLI 普通模型                | 由本机 `model/list` 返回                                  | 必须逐模型使用 `supportedReasoningEfforts`，不可按 API 全集猜测 | `defaultReasoningEffort` | `protocol.rs:203-284`; `codexModelCapabilities.ts:158-198`            |
| AIO managed alias                 | `aio/<profile>`，目标是任意远端模型                       | 用户对该 provider model 保存的 capability                       | 用户选择                 | `managed.rs:1166-1204`; 不是 OpenAI GPT-5.6 静态表                    |
| provider model catalog            | 远端 `/models` 或手工 ID                                  | 通用可配置集合 `none..ultra`                                    | 非空集合必须选一个       | `providerModels.ts:14-25,193-232`; `provider_models.rs:26-76,344-391` |

重要边界：仓库 Codex 协议测试中的 `gpt-5.6-sol + max/ultra/future-effort` 与
`gpt-5.6-luna + low/max`（`protocol.rs:1062-1086`）是验证“未知值透传、分页、逐模型
能力不被补齐”的协议夹具，不是官方型号能力表。对应前端测试
`codexModelCapabilities.test.ts:56-92` 也在验证“目录声明什么就展示什么”。不能据此
推导 Luna 只有两档，也不能把 Codex 的 `ultra` 写入 CX2CC Responses API 控件。

### 2. CX2CC 设置页：静态 effort 缺项且设置值会进入运行时

- 控件：`src/components/cli-manager/tabs/Cx2ccTab.tsx:48,127,136-149,282-300`。
  当前选项为 `"" / low / medium / high / xhigh`，缺显式 `none`、`max`。
- 前端持久化映射：`src/services/settings/settings.ts:112-174` 中
  `cx2CcModelReasoningEffort -> cx2cc_model_reasoning_effort`，并由映射的静态断言约束
  settings view/update 完整性。
- 后端输入/投影：`src-tauri/src/app/settings_service.rs:94,181,321,451,536,1003-1006`；
  `src-tauri/src/infra/settings/types.rs:490-500,570-583` 默认值为空字符串。
- 持久化只做通用字符串边界与 trim，而不做枚举校验：
  `src-tauri/src/infra/settings/persistence.rs:354-362,538-539,1212-1224`。
- 运行时设置把空串转 `None`，其他任意非空值透传：
  `src-tauri/src/gateway/proxy/cx2cc/settings.rs:7-35,57-63`。
- 出站语义：请求 IR 已有 `metadata.extra.reasoning` 时优先保留；仅缺失时才用设置值，
  `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:233-242`。
  现有测试 `:1480-1511` 覆盖请求值保留，`:1514-1541` 覆盖设置 fallback 注入。
- 当前前端测试只验证 `high` 回显与 `medium -> low` 保存：
  `Cx2ccTab.test.tsx:244-299`，未覆盖 `none/max`、未知旧值兼容和 API 集合边界。

因此 R3 的“请求显式 thinking/reasoning 透传”不要求删除所有设置字段；就当前转换器
行为而言，该字段已经是“请求未携带 reasoning 时的 fallback”。但本任务若决定完全移除
固定默认覆盖，UI 仍不应再维护一个与 API 能力不一致的静态数组。至少应把可选集合改成
Responses API 事实源，并为请求优先级保留后端回归测试。

### 3. CX2CC 供应商编辑/模型选择：实际缺 GPT-5.6 预设

- `src/pages/providers/providerEditorUtils.ts:31-34,87-118`：
  `CX2CC_DEFAULT_MODEL = "gpt-5.5"`，新建/空配置会把五个 Claude tier 都填成它。
- `src/pages/providers/Cx2ccSection.tsx:15-47,100-131`：下拉静态项只有默认
  `gpt-5.5` 和 `gpt-5.4`，另有“手动”；已有未知值会临时插回列表，兼容旧配置。
- `src/pages/providers/ClaudeModelSection.tsx:13-17,40-131`：五个映射字段是自由输入，
  placeholder 仍是 `gpt-5.4 / o3`；这不是型号白名单。
- `useProviderEditorEffects.ts:163,217` 与 `useProviderEditorForm.ts:451` 调用
  `withCx2ccDefaultModel`，所以静态默认同时影响创建、编辑恢复与切换到 CX2CC。
- 现有 `ProviderEditorDialog.test.tsx:1031,1084-1122` 明确锁定 `gpt-5.5/gpt-5.4`
  的选项和五字段联动，更新事实源时必须同步这些断言并增加四个 GPT-5.6 ID。

这里不应把 provider `/models` catalog 强行接进 CX2CC 默认下拉：CX2CC 可以选择“当前
AIO Codex 网关”，其可用型号取决于当前动态分流，并不归属于单个 source provider。
最小方案是一个小型 Responses API 推荐型号常量（含 alias 与三种具名型号），保留手动
输入；默认模型是否从 `gpt-5.5` 改为 `gpt-5.6` 是产品/迁移决策，不能仅因“补可选项”
隐式改写既有配置。

### 4. Provider model catalog：型号动态，能力手工，前后端枚举重复

- 型号来源：`provider_models.rs:973-1057,1074-1117` 构造 OpenAI-compatible
  `/models` 请求并严格解析 `data[].id`；`apply_refresh_success` 在
  `:1203-1249` 只 upsert ID，不推断 capability。手工型号入口为 `:776-817`。
- 前端能力全集：`src/services/providers/providerModels.ts:14-25`。
- 后端能力全集：`src-tauri/src/domain/provider_models.rs:26-76`。
- 生成绑定的第三份产物：`src/generated/bindings.ts:4053-4083`，其中 union 由 Rust
  `specta::Type` 生成；它应是派生产物，不应再手工定义业务语义。
- UI：`ProviderModelCatalogDialog.tsx:84-90,193-232,536-605` 用 TS 常量渲染勾选，
  非空集合必须选默认值。后端 `normalize_capabilities` 在
  `provider_models.rs:344-391` 重做去重、排序和默认值一致性校验。
- SQLite schema 再复制了一次枚举：
  `infra/db/migrations/v40_to_v41.rs:25-41` 与
  `infra/db/migrations/baseline_v25.rs:256-261`。这是持久化 CHECK 所需的有意重复，
  但应有跨层合同测试确保与 Rust enum 同步。
- v40->v41 对所有历史模型统一回填 `low/medium/high`、默认 `medium`
  (`v40_to_v41.rs:37-41`)；这只是向后兼容基线，不证明任何 GPT-5.6 型号的真实能力。
  新发现模型仍是 capability 未配置。

所以 provider catalog 并非“缺 GPT-5.6 型号”；它会发现或允许手工添加任意合法 ID。
真正风险是用户必须手工选择能力，且 UI/Rust/DB 三处集合可能漂移。若任务只要求 GPT-5.6
出现在供应商选择器，修改 CX2CC 预设即可，不应增加 provider model 的型号白名单。

### 5. Codex managed catalog：已有正确的数据流，不应硬编码 GPT-5.6

- 本机 Codex catalog 拉取：`src-tauri/src/infra/codex_model_catalog/mod.rs:51-80`
  解析 launch/config snapshot 后调用 app-server；协议对象在 `protocol.rs:203-284`。
- parser 保留 `supportedReasoningEfforts` 的 missing/empty/non-empty 三态，并允许未来
  effort 字符串，测试见 `protocol.rs:930-970`。这是正确的前向兼容设计。
- 前端匹配和展示：`codexModelCapabilities.ts:64-96,121-198`；命中型号时完全使用
  catalog，catalog 缺失/退化才用 fallback。
- 模型切换迁移：`useCodexModelMigration.ts:41-101,104-157`；仅 catalog ready 且能力
  可确认时把不受支持的 `max/ultra` 降到目标模型声明的最高已知档位，退化时保留并等待
  一次 reconciliation。测试集中在 `useCodexModelMigration.test.ts:118-357`。
- managed provider model 先要求 capability 已配置，再创建 profile：
  `codex_managed_profiles.rs:657-743`。生成 alias 把数据库中的 effort/default 原样投影为
  Codex `supported_reasoning_levels/default_reasoning_level`：
  `managed.rs:1166-1204`；更新测试见
  `codex_managed_profiles.rs:1260-1324`，安装 Codex smoke test 在 `:1470-1502`。

这条链已有单一运行时事实源：普通模型由 Codex 自身目录提供，AIO alias 由 provider
model capability 提供。不要为 `sol/terra/luna` 增加第二份 managed 静态表。

### 6. 默认、校验、迁移和日志中的重复常量

已确认的重复/漂移点：

| 位置/符号                                                       | 当前集合或默认                                 | 判定                                                                                        |
| --------------------------------------------------------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `Cx2ccTab.tsx:293-299` inline options                           | empty, low..xhigh                              | Responses API 集合的错误副本，缺 none/max                                                   |
| `providerModels.ts:14-25` `PROVIDER_MODEL_REASONING_EFFORTS`    | none, minimal, low..ultra                      | 通用 provider UI 事实副本                                                                   |
| `provider_models.rs:28-76` `ProviderModelReasoningEffort`       | none, minimal, low..ultra                      | provider/managed 后端规范源候选                                                             |
| `codexModelCapabilities.ts:7-27` `KNOWN_...`/`FALLBACK_...`     | rank 含 minimal..ultra；fallback 为 low..ultra | Codex UI fallback/rank，不是 Responses API 表；`none` 不在 rank，但当前迁移只处理 max/ultra |
| `model_inference.rs:269-274` `normalize_codex_reasoning_effort` | none..ultra                                    | 日志识别白名单；与前端又重复                                                                |
| `requestLogSpecialSettings.ts:19-28,81-102`                     | none..ultra；默认模型只到 5.5/5.4              | 前端日志默认推断表缺 GPT-5.6                                                                |
| `model_route_mapping.rs:5-12,288-301`                           | 同一 5.5/5.4 默认表与 none..ultra              | 后端日志/路由审计重复；同样缺 GPT-5.6                                                       |
| `defaults.rs:17` `DEFAULT_CX2CC_FALLBACK_MODEL`                 | gpt-5.4                                        | 全局 CX2CC runtime fallback                                                                 |
| `providerEditorUtils.ts:33` `CX2CC_DEFAULT_MODEL`               | gpt-5.5                                        | 新建 CX2CC provider UI 默认；与 runtime fallback 不同                                       |

日志默认表不应凭 UI 预设直接补齐。已有历史只读证据记录本机 Codex 0.146.0 bundled
catalog 的 `gpt-5.6-sol.default_reasoning_level = low`，而官方 API 默认是 `medium`；两者
再次说明 Codex 运行时默认与 API 默认是不同语境。若日志没有请求显式 effort，理想来源
是同一次 Codex catalog snapshot，而不是在 Rust/TS 各追加一张 GPT-5.6 默认表。若本任务
不扩展日志数据流，则把该问题列为同源风险并保留 `unknown`，不要猜测。

## Upstream 对比与分类

### 可移植

- **本子范围没有发现 upstream 独有的 GPT-5.6 型号/effort 修复可直接移植。**
  `Cx2ccTab.tsx`、`gateway/proxy/cx2cc/settings.rs` 在共同祖先、HEAD、upstream 三者的
  blob 完全相同；upstream 也只有 CX2CC `low..xhigh`，因此这是共享缺口，不是漂移修复。
- `upstream/main` 的 `protocol.rs` 相对共同祖先只做进程工具复用，未改变
  `RawCodexModel` 或 effort 解析；不属于本调研目标。

### 当前分支已有，必须保留

- fork 已通过 `bbe92016`、`50db2a18` 等提交支持 Codex `max/ultra`、动态 capability
  与前端迁移。当前 `codexModelCapabilities.ts` fallback 为 `low..ultra`，而
  upstream/common ancestor 仍为 `minimal..xhigh`。这是 fork 明确决策，不应按 upstream
  回退；差异见 `codexModelCapabilities.ts:7-27`。
- 普通 Codex `model/list` 的 `sol/luna` 协议夹具在共同祖先已存在；不是本轮新增。
- provider model catalog、managed profiles、能力配置、生成 alias、bindings 是 fork 在
  共同祖先后的新增模块（主要提交 `b444b981`, `f59cb458`, `ee50ac8c`, `85ea3fbb`）；
  upstream 没有对应文件，必须作为 fork 特有能力维护。

### Fork 特有、需本任务自行修复

- `Cx2ccSection.tsx` 与 `providerEditorUtils.ts` 的静态默认下拉是 fork 后续改动；
  upstream 仍等于共同祖先。补 GPT-5.6 型号必须在 fork 实现，不能等待移植。
- provider capability 的 TS/Rust/DB 重复集合、managed alias 投影和生成绑定均只存在于
  fork；需要本地合同测试防漂移。
- Rust/TS 两份请求日志默认 effort 表与 configurable/managed route 审计也是 fork 特有；
  upstream 没有可移植的 GPT-5.6 默认映射。

### 范围外

- upstream `6007d7a0` 增加请求日志 reasoning 展示、价格别名和 provider policy UX；它未
  定义 GPT-5.6 capability，且大部分与本任务的型号/设置源无关。本报告只记录其与日志
  默认表的邻接风险，不建议整提交移植。
- 官方价格、上下文窗口、pro mode、prompt caching 不是本任务实现范围。
- 不应顺手修复 upstream 仍存在的 `minimal..xhigh` fallback 或其他 pinned upstream
  原生缺陷；仓库规则要求另行授权。

## 单一事实源建议

不要建立一个“所有地方通用”的超集；应建立三个明确的协议事实源：

1. **OpenAI Responses API / CX2CC 事实源（前端共享常量）**：
   `OPENAI_RESPONSES_REASONING_EFFORTS = [none, low, medium, high, xhigh, max]`，以及
   `GPT_5_6_MODEL_OPTIONS = [gpt-5.6, gpt-5.6-sol, gpt-5.6-terra, gpt-5.6-luna]`。
   CX2CC effort 控件和 CX2CC provider 推荐模型下拉消费它；保留“空/不注入”为单独 UI
   sentinel，不把空串混入协议 enum。不要包含 `minimal` 或 `ultra`。
2. **Codex CLI 模型能力事实源（运行时）**：继续以 app-server `model/list` 的每个
   `CodexModelCapability` 为准。fallback 只用于 catalog 不可用时，且不得覆盖已声明的
   missing/empty/non-empty 语义。`ultra`、未来 effort 和逐型号差异均来自此处。
3. **AIO provider/managed capability 事实源（Rust enum）**：以
   `ProviderModelReasoningEffort` 为规范，生成 TS union；前端显示顺序可从生成 union 无法
   保序这一限制出发保留一份 order 数组，但必须有跨层合同测试与 Rust enum/DB CHECK
   精确一致。managed catalog 继续只投影数据库 capability。

日志默认推断应长期改为消费已捕获的 Codex catalog/default，或随请求记录实际 default
来源；在此之前宁可显示 `unknown`，不要再给 Rust/TS 静态表补 GPT-5.6 猜测值。

## 最小改动清单

1. 新增一个小型前端模块（可放在 `src/constants/` 或 cli-manager/providers 共同可访问的
   service 层），定义 Responses API effort 与 GPT-5.6 推荐型号；不要依赖
   `codexModelCapabilities.ts` 的 Codex-only fallback。
2. `Cx2ccTab.tsx`：用共享 Responses effort 生成 radio，保留空 sentinel；增加 `none`、
   `max`，不加 `ultra`。若 R3 实现改为完全透传并删除 fallback 设置，则相应删除此控件
   与 settings 字段，而不是留下无效常量。
3. `Cx2ccSection.tsx` / `providerEditorUtils.ts`：下拉加入 `gpt-5.6`、Sol、Terra、Luna；
   保留 `gpt-5.5`、`gpt-5.4` 与手动/已有未知值兼容。除非设计明确要求，不在迁移中自动
   把已保存或默认 `gpt-5.5` 改成 GPT-5.6。
4. 不改普通 Codex model picker 的型号来源；只补合同测试，证明 Sol/Terra/Luna 和未来
   型号由动态 catalog 原样呈现，且逐模型 effort 不被静态全集补齐。
5. provider catalog 若触及 effort enum：以 Rust enum 为规范重生成
   `src/generated/bindings.ts`，运行 `pnpm check:generated-bindings`；增加 TS/Rust/DB CHECK
   一致性测试。不要为 GPT-5.6 增加 provider 型号白名单或自动覆盖用户 capability。
6. managed alias 不需要型号代码改动；增加一个 GPT-5.6-shaped provider model 测试，证明
   `none..max/default medium` 经 DB -> profile -> generated catalog 完整投影，同时旧模型
   `minimal`/无 reasoning 配置仍兼容。
7. 请求日志默认 effort 的两份静态表暂不添加 GPT-5.6 猜测值；若实现者决定同任务解决，
   应改为复用 Codex catalog snapshot/default，并同时删除 Rust/TS 重复表及补降级测试。

## 建议回归测试

### 前端

- `src/components/cli-manager/tabs/__tests__/Cx2ccTab.test.tsx`
  - 精确断言选项为：不注入、none、low、medium、high、xhigh、max；无 minimal/ultra。
  - `none`、`max` 保存成功；服务返回未知旧值时不崩溃且不静默改写（若控件需要兼容，
    显示“当前未知值”或保留直到用户选择）。
  - 请求设置透传策略若变化，调整为验证设置不再覆盖显式请求的跨层 Rust 测试。
- `src/pages/providers/__tests__/ProviderEditorDialog.test.tsx`
  - 新建 CX2CC 下拉包含 alias/Sol/Terra/Luna 和旧 `gpt-5.5/gpt-5.4`。
  - 选择每个 GPT-5.6 项会原子更新 main/reasoning/haiku/sonnet/opus 五字段。
  - 既有手动/未知模型仍回显；编辑旧 provider 不自动迁移。
- 新共享常量单测或 `src/constants/__tests__/crossLayerContracts.test.ts`
  - Responses effort 集不含 `minimal/ultra`；GPT-5.6 型号四项无重复。
- `codexModelCapabilities.test.ts` / `useCodexModelMigration.test.ts`
  - 增加 Terra 动态目录样例；Sol/Terra/Luna 各自只显示 catalog 声明值。
  - 旧模型 `minimal`、未来 effort、empty/missing capability 和 catalog degraded 行为不回归。
- `ProviderModelCatalogDialog.test.tsx` 与 `providerModels.service.test.ts`
  - `none/max` 能保存；generic `minimal/ultra` 仍可保存（证明未误用 Responses 子集）。

### Rust

- `gateway/proxy/protocol_bridge/outbound/openai_responses.rs`
  - table test 覆盖 GPT-5.6 的 `none..max`；显式 request `reasoning` 对每个 fallback 值都
    优先，缺失时才注入；`ultra` 不由 CX2CC 设置校验接受（若新增后端 enum 校验）。
- `gateway/proxy/cx2cc/settings.rs`
  - 空串 -> `None`；合法 Responses effort trim；非法/minimal/ultra 的 fail-closed 行为
    （若本任务增加枚举校验）。
- `domain/provider_models.rs`
  - capability enum、去重/排序/default 包含关系；新发现 `gpt-5.6-*` 仍为未配置，不自动
    猜能力。
- `domain/codex_managed_profiles.rs` / `infra/codex_model_catalog/managed.rs`
  - GPT-5.6-shaped 远端模型的 `none..max/default medium` 精确生成；旧 `minimal/ultra` 和
    reasoning disabled 兼容。
- `infra/codex_model_catalog/protocol.rs`
  - 加 Terra 到 fixture；继续验证未知 effort、missing/empty 列表透传，避免静态补齐。
- `infra/db/migrations/tests.rs`
  - baseline 与 v40->v41 CHECK 接受 provider 通用全集；旧回填保持原契约，不以 GPT-5.6
    新能力重写历史用户选择。

### 质量门

- 定向 Vitest：上述 CX2CC、provider editor、Codex capability/migration、provider catalog。
- `pnpm typecheck`、`pnpm check:generated-bindings`（若 Rust type/binding 有变）。
- Rust 定向测试：`cx2cc::settings`、`openai_responses`、`provider_models`、
  `codex_managed_profiles`、`codex_model_catalog`、DB migrations。
- 受影响的完整前端测试与 Rust library test，最后 `git diff --check`。

## 风险与范围外发现

- **高风险**：把 Codex `ultra` 当作 Responses `reasoning.effort`。官方只把它描述为
  Codex multi-agent mode；CX2CC 当前写的是 Responses JSON，因此必须隔离枚举。
- **高风险**：把协议 fixture 当真实型号能力。fixture 故意包含 `future-effort`，其目的
  正是验证未知值不丢失。
- **中风险**：只补下拉文案，不补 `none/max` 运行时/校验测试；或反过来只改 enum，
  未让用户选择 GPT-5.6 型号。
- **中风险**：改默认模型会影响新建 CX2CC provider 与五个 tier 映射；若要从 5.5 迁到
  5.6，应单独形成产品决策与迁移测试。
- **范围外但相关**：请求日志 Rust/TS 默认 effort 表缺 GPT-5.6，已知会使无显式 effort
  的 5.6 日志显示 unknown。正确修复需要 catalog/default 数据流，不应在本集成任务里
  用静态猜测掩盖。
