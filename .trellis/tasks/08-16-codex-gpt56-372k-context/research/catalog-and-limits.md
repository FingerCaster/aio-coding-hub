# Research: Codex model catalog and context limits

> 2026-08-17 最终产品决策：跟随 Codex 上游 `272000` 的十进制目录口径，372K 的实现与验收值为 `372000`。下文关于 `380928` / `372 * 1024` 的数值建议属于已被覆盖的历史分析，不得作为实现要求。

- Query: 确认 Codex GPT-5.6 模型目录和上下文上限的真实来源，判断 `model_context_window = 380928` 是否会生效，并界定 AIO Coding Hub 实现 372 Ki 上下文开关所需的最小安全改动与测试范围。
- Scope: mixed
- Date: 2026-08-16

## Findings

### 结论摘要

1. 仓库没有内置默认 Codex model catalog JSON。AIO 在需要生成托管目录时，优先读取用户配置的绝对 `model_catalog_json`，否则调用当前安装的 Codex CLI 的 `codex debug models --bundled`。因此默认值跟随用户机器上的 Codex 版本，而不是 AIO 源码常量。
2. 本机 `codex-cli 0.147.0` 的 bundled catalog 对 `gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 都声明 `context_window = 272000`、`max_context_window = 272000`、`effective_context_window_percent = 95`，且没有显式 `auto_compact_token_limit`。
3. 只写顶层 `model_context_window = 380928` 不会达到目标。Codex 0.147.0 会执行 `min(config.model_context_window, model.max_context_window)`，所以在原目录下会被截回 `272000`。
4. PRD 的精确产品值是 `372 * 1024 = 380928`，不是 `372000`。`372000` 和其他手工值都不能被当作本开关的开启状态。
5. 最小可工作的双层机制是：仅对三个精确 GPT-5.6 slug，把 AIO 派生目录中的 `max_context_window` 提升到 `380928`，同时在开启状态写顶层 `model_context_window = 380928`。保留基础条目的 `context_window = 272000`，从而在关闭覆盖后恢复 Codex 原始默认。
6. 目录中的 `effective_context_window_percent = 95` 还有一层运行时语义。Codex 把会话硬上限计算为 `resolved_context_window * 95 / 100`。因此原样保留 95% 时，配置值 `380928` 对应的有效硬上限是 `361881`，不是字面上的 `380928`。这与 PRD 中“新会话精确使用 380,928 token 上下文”的表述存在验收口径歧义，实施前必须明确。
7. 省略 `model_auto_compact_token_limit` 时，Codex 按原始 resolved context 的 90% 向下取整。对 `380928`，默认阈值是 `342835`。仓库中出现的 `32768`/`32 * 1024` 均与模型上下文或自动压缩无关，本功能不应联动写入 `32768`。
8. 设置 `model_catalog_json` 会令 Codex 使用 `StaticModelsManager`，不再采用远端 `/models` 动态目录。这是该功能最大的兼容性成本，尤其是当前目录生成只在“代理已启用且至少一个 managed profile”时发生；若普通 GPT-5.6 用户也可使用开关，目录生命周期必须扩展，而不能只改 `build_managed_model`。

### 数值和运行时语义

| 状态/层次 | 默认关闭 | 开启 372 Ki 覆盖 | 说明 |
| --- | ---: | ---: | --- |
| bundled/base `context_window` | 272000 | 仍为 272000 | 不直接改默认，保证关闭后回到 Codex 原行为 |
| 派生目录 `max_context_window` | 272000 | 380928 | 只修改三个精确 GPT-5.6 slug |
| 顶层 `model_context_window` | 不由本功能写入 | 380928 | Codex 配置覆盖值；精确值也是 UI ownership 判据 |
| `effective_context_window_percent` | 95 | 若保留上游语义则仍为 95 | Codex 0.147.0 的默认值 |
| 会话有效硬上限 | 258400 | 361881 | `floor(context_window * 95 / 100)` |
| 缺省自动压缩阈值 | 244800 | 342835 | `floor(context_window * 9 / 10)` |

这里的“会话有效硬上限”来自 Codex 核心运行时，并非 AIO 自行推断。上游 `TurnContext::model_context_window()` 对 resolved context 应用 95%，随后 `context_window.rs` 把该结果作为 full-context hard cap。

若验收真正要求“可用硬上限恰好为 380928”，存在两个方案，但都超出单纯提升 `max_context_window` 的语义：

- 将三个目标条目的 `effective_context_window_percent` 在开启状态改为 100。这样配置值和硬上限均为 `380928`，但会移除 Codex 上游保留的 5% 余量。
- 保留 95%，把原始配置值提高到 `400977`，因为 `floor(400977 * 95 / 100) = 380928`。这直接违反 R1/R3/R6 对持久化精确值 `380928` 的要求，不建议采用。

基于 PRD 对持久化值的硬约束，研究建议先按“模型描述符/配置窗口为 `380928`，Codex 继续应用自身 95% 有效窗口”实现，并把验收措辞改清楚；若产品要求实际硬上限 380928，则应显式批准同时把目标条目的 effective percent 改为 100，并补充相应风险测试。

### 默认目录来源

AIO 的基础目录选择流程如下：

1. `prepare_for_profiles` 读取代理 baseline 和当前 live config。
2. 若原始配置含绝对 `model_catalog_json`，读取该用户目录，并以内容生成 fingerprint。
3. 否则解析已安装 Codex 的启动规格，以可执行文件路径、CLI 版本、文件长度和 mtime 生成 source fingerprint。
4. 调用已安装 CLI 的 `codex debug models --bundled` 取得原始 JSON。
5. `generate_catalog` 保留基础 JSON 的顶层、模型条目和未知字段，再追加 `aio/*` managed entries 与 AIO ownership metadata。

本机观测：

```text
codex-cli 0.147.0
gpt-5.6-sol:   context=272000, max=272000, effective=95, auto_compact=null
gpt-5.6-terra: context=272000, max=272000, effective=95, auto_compact=null
gpt-5.6-luna:  context=272000, max=272000, effective=95, auto_compact=null
```

Codex npm 包最终启动平台原生 `codex.exe`。0.147.0 上游 tag `rust-v0.147.0` 对应 commit `be6e8eac029b183056b7e4402879f15d2c85f61b`；真正的 bundled 源是 `codex-rs/models-manager/models.json`，并由 `models-manager/src/lib.rs` 的 `include_str!("../models.json")` 编入二进制。

### 为什么只改 TOML 不够

Codex 的覆盖次序是：

```text
bundled/custom ModelInfo
  -> with_config_overrides(model_context_window)
  -> min(requested context, model.max_context_window)
  -> effective_context_window_percent
  -> session full-context hard cap
```

在未修改目录时：

```text
min(380928, 272000) = 272000
floor(272000 * 95 / 100) = 258400
```

所以仅复用现有 CodexTab 数字输入、Rust patch 字段或服务 DTO，只会成功写 TOML，不会使目标上下文生效。

### AIO 当前目录 ownership 和事务

现有 managed catalog 实现已有可复用的安全属性：

- 生成路径固定在应用数据目录的 `cli-proxy/codex/managed-model-catalog.json`，单文件限制 4 MiB。
- 用户原始 `model_catalog_json` 必须是绝对路径；激活 AIO 目录时保留原路径，停用后恢复原路径或删除 key。
- 当前 binding 只能是原目录或 AIO 目录，其他漂移 fail closed。
- ownership metadata 和 SHA-256 会检测外部修改、profile 集合变化和基础目录来源变化。
- 应用顺序覆盖 proxy baseline backup、生成目录和 live `config.toml`，任一阶段失败会回滚已写阶段；回滚时若发现外部漂移则返回 recovery-required，而不是覆盖用户新改动。
- 常规结构化配置保存也会在代理开启时合并用户对 live config 的外部修改、同步 backup，再投影代理配置；失败会恢复 backup snapshot。

这些能力意味着新开关不应另写一个旁路 JSON 文件，也不应直接修改 Codex 安装目录。应把“GPT-5.6 long-context override enabled”作为现有派生目录的第二种生成原因，并进入同一套 prepare/apply/rollback 流程。

### 当前生命周期缺口

现有目录生命周期与 managed profiles 绑定：

- 代理未启用时，`prepare_for_profiles` 返回 inactive。
- `profiles.is_empty()` 时不生成或删除 AIO 目录，并把 `model_catalog_json` 恢复为原值。
- 创建/删除 managed profile、provider model capability 更新、代理启用/重绑/启动同步会触发目录重建。

因此，只在 `build_managed_model` 中修改三个基础条目的上限不能满足 PRD：

- 没有 managed profile 时根本不会激活生成目录。
- 代理关闭时也不会激活生成目录。
- 若开关关闭但 managed profiles 仍存在，生成目录应继续存在，只把三个基础条目的上限恢复到 base 值。
- 若最后一个 managed profile 删除但 long-context 仍开启，生成目录不能被删除。

目录是否存在应由两个独立原因决定：

```text
needs_generated_catalog = has_managed_profiles || gpt56_long_context_enabled
```

如果产品明确把开关限定为“仅代理开启且至少一个 managed profile 时可用”，可以不扩展生命周期，但 UI 必须在其他状态禁用并解释。该限制与当前 PRD 的普通 GPT-5.6 目标不一致，不推荐。

### 建议的最小实现边界

1. 定义单一常量 `GPT56_LONG_CONTEXT_TOKENS = 372 * 1024 = 380928`，Rust 和 TypeScript 通过既有生成绑定或单一后端契约共享，避免各层分别写 magic number。
2. UI 只在当前模型是明确支持的 GPT-5.6 slug/已确认 alias 时提供开关。开启状态只能由后端确认的 `model_context_window == 380928` 推导；`372000`、其他手工值和空值均不是开启。
3. 开启操作作为一个联合变更处理：从真实 base catalog 派生完整目录，把三个精确 slug 的 `max_context_window` 提升到至少 `380928`，再写 `model_context_window = 380928` 和 AIO 目录 binding。不能先提交 TOML 后异步尝试目录生成。
4. 基础条目的 `context_window` 保持原值 `272000`；只提升 `max_context_window`。这样开关关闭或顶层覆盖删除后，默认行为仍由安装的 Codex bundled catalog 决定。
5. 关闭操作只删除本功能拥有的精确 `380928` 覆盖；若当前值已变成其他手工值，重新加载后应显示关闭且不得静默删除。目录仍按其他 ownership 原因决定保留或恢复。
6. 不写 `model_auto_compact_token_limit`。已有显式用户值必须保持；缺省时让 Codex 自己推导并 clamp 到窗口 90%。
7. 扩展现有 catalog ownership metadata/hash，使其包含 long-context 状态和用于命中的明确模型集合；重复生成必须字节稳定。
8. 把应用启动、Codex CLI 更新、代理开关、managed profile 增删、provider capability 更新和 long-context 开关保存都接入同一重建入口。
9. 保留用户自定义目录的全部未知字段和模型；只对精确目标条目的一个或两个批准字段做结构化 JSON 修改。不要用 AIO 固定快照覆盖用户目录。
10. 开关更改后提示新启动 Codex 会话/进程才会加载目录，因为 `model_catalog_json` 是启动时读取的配置。

### 模型匹配和 alias

当前 bundled catalog 能确认的目标 ID 只有：

- `gpt-5.6-sol`
- `gpt-5.6-terra`
- `gpt-5.6-luna`

当前目录条目没有可供 AIO 使用的通用 alias 字段，仓库的 `CodexModelCapability` DTO 也只暴露模型 ID、显示名、可见性/default 和 reasoning 能力，不暴露 context/max context。因而：

- 目录改写应使用精确 slug 白名单，不使用 `starts_with("gpt-5.6")`，避免未来模型被无意修改。
- `aio/*` managed aliases 继续使用 provider capability 自己的 `context_window`/`max_context_window`，不得被普通 GPT-5.6 开关改写。
- R5 中“兼容 alias”的具体集合在当前代码和 bundled catalog 中未找到。只有在发现稳定、可验证的 alias 到三个目标 slug 的映射后才能加入；否则应把该项作为未决契约，而不是猜测字符串。

### 配置和 UI 现有链路

仓库已经完整支持原始数值字段：

- Rust state 使用 `Option<u64>`，patch 使用 `Option<Option<u64>>` 区分“不修改、写值、删除”。
- parser 读取顶层 `model_context_window` 和 `model_auto_compact_token_limit`。
- patcher 写入或删除两个 key。
- Tauri/TypeScript 服务透传两个 nullable 字段。
- CodexTab 已有两个 number input，模型迁移 hook 在切换模型时主动清除两个覆盖。

但现有 UI catalog 走 app-server 的 `model/list`，而派生基础目录走 `debug models --bundled`，是两条不同的数据路径。开关不能假设模型选择器 DTO 中已经有 `max_context_window`；要么以明确 slug 契约判断，要么扩展后端 DTO 和绑定。

模型切换当前会无条件清除两个覆盖。新开关应沿用这一保存/重对账流程，并明确以下行为：

- 从受支持 GPT-5.6 切到不支持模型时，精确 `380928` 覆盖被撤销，UI 随后以后端返回状态为准。
- 切换失败、二次 reconciliation 失败或并发保存时，不得仅乐观翻转开关。
- 非 `380928` 手工值不能被开关的自动同步清除；如果现有迁移 hook 仍会清除所有手工值，则这是现有行为与 R6 的潜在冲突，需在本功能测试中显式决定。

### 自定义 catalog 的兼容性成本

Codex 0.147.0 在配置中存在 catalog JSON 时，三个 models-manager 构造路径都会选择 `StaticModelsManager`；只有没有自定义 catalog 时才使用 `OpenAiModelsManager`。这意味着：

- 开启开关后，当前进程不再接收远端 `/models` 的动态目录/能力更新。
- AIO 生成目录虽然保留生成时看到的全部 base models 和未知字段，但在下次重建前可能变旧。
- 功能从“只有 managed profile 用户承担静态目录成本”扩大为“任何开启 372 Ki 的普通 GPT-5.6 用户都承担该成本”。
- Codex CLI 升级后必须按 executable fingerprint 重建；还应测试应用启动时对 CLI 小版本变化的检测。
- 模型 picker 必须验证没有意外丢失远端才提供、但 bundled 尚未包含的模型。如果无法保证动态新鲜度，UI 需要披露这一限制。

不建议编辑 npm 包、原生 `codex.exe` 或其安装目录：MSI 无法稳定控制用户安装的 Codex 版本，CLI 升级也会覆盖修改。

## Files Found

- `.trellis/tasks/08-16-codex-gpt56-372k-context/prd.md` - 定义精确目标 `380928`、默认保持 272K、手工值 ownership 和 MSI 验收范围。
- `src-tauri/src/infra/codex_model_catalog/managed.rs` - 用户/bundled 基础目录选择、派生 JSON、ownership metadata、binding 恢复和三文件事务实现。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs` - app-server `model/list` 和 CLI `debug models --bundled` 两条目录读取协议。
- `src-tauri/src/infra/codex_model_catalog/mod.rs` - 暴露给前端的 `CodexModelCapability` DTO；当前不含 context/max context。
- `src-tauri/src/infra/codex_config/types.rs` - Codex 配置 state/patch 对两个模型限制字段的类型定义。
- `src-tauri/src/infra/codex_config/parsing.rs` - 顶层 TOML 数值字段解析。
- `src-tauri/src/infra/codex_config/patching.rs` - 顶层 TOML 数值字段写入和删除。
- `src-tauri/src/infra/codex_config/mod.rs` - 结构化保存、代理 baseline 投影、backup 同步和失败恢复。
- `src-tauri/src/domain/codex_managed_profiles.rs` - managed profile 创建/删除后的 catalog prepare/apply 触发点。
- `src-tauri/src/domain/provider_models.rs` - provider capability 上下文范围及 capability 更新后的 catalog 同步。
- `src-tauri/src/infra/cli_proxy/codex.rs` - Codex 代理启动/重绑时的 catalog 同步。
- `src-tauri/src/infra/cli_proxy/mod.rs` - 代理配置投影保留活动目录 binding，并在生命周期事件中同步目录。
- `src/components/cli-manager/tabs/CodexTab.tsx` - 现有 model context 和 auto-compact 原始数值输入。
- `src/components/cli-manager/tabs/useCodexModelMigration.ts` - 模型切换时清除两个模型相关覆盖并重对账 reasoning effort。
- `src/services/cli/cliManager.ts` - 前端 Codex state/patch 字段及 Tauri 调用透传。
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx` - 现有数字输入写入/清空测试，尚无 372 Ki 联合开关测试。
- `src/components/cli-manager/tabs/__tests__/useCodexModelMigration.test.ts` - 现有模型切换清除限制字段及 reconciliation 测试。
- `src-tauri/src/infra/codex_config/tests.rs` - TOML 两字段写入、删除、JSON null 和解析回归测试。

## Code Patterns

- `src-tauri/src/infra/codex_model_catalog/managed.rs:458-526` - 目录目前只在代理有 baseline 时准备，`profiles.is_empty()` 决定删除生成内容和恢复 binding。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:580-596` - 生成目录路径和父目录创建规则。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:738-910` - 原始路径必须绝对、binding 漂移 fail closed、启停恢复原路径。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:937-989` - 用户目录优先，否则使用安装的 Codex bundled catalog，并构造 source fingerprint。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:992-1017` - 当前 profile hash 覆盖 model/effort/context 等生成输入。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:1020-1163` - 结构化保留基础 JSON/未知字段，追加 AIO metadata 和 managed entries。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:1166-1249` - `aio/*` 条目把 provider capability 同时写入 context/max context，并保持 auto compact 为 null、effective percent 为 95。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:124-337` - prepare 后再次验证 baseline/snapshot，按阶段应用并提供完整 rollback/recovery-required 语义。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:1417-1490` - 现有生成测试验证未知字段保留与 managed capability，但未验证基础 GPT-5.6 上限改写。
- `src-tauri/src/infra/codex_model_catalog/managed.rs:1527-1730` - 原路径 round-trip、三文件事务、失败回滚、回滚漂移和 baseline repair 测试。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs:99-115` - UI 的 app-server model list 与 raw bundled catalog 分别获取。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs:118-285` - app-server 初始化、`model/list` 请求和有限 DTO 解析。
- `src-tauri/src/infra/codex_model_catalog/mod.rs:27-42` - 当前 capability DTO 不暴露 context/max context。
- `src-tauri/src/infra/codex_config/types.rs:24-25`、`:62-65` - state 和三态 patch 类型。
- `src-tauri/src/infra/codex_config/parsing.rs:332-335` - 顶层字段解析。
- `src-tauri/src/infra/codex_config/patching.rs:689-701` - 顶层字段写入/删除。
- `src-tauri/src/infra/codex_config/mod.rs:388-470` - 结构化配置保存沿用 proxy baseline merge/projection 和 backup rollback。
- `src/components/cli-manager/tabs/CodexTab.tsx:146-187`、`:1050-1086` - 数字输入本地文本状态和持久化入口。
- `src/components/cli-manager/tabs/useCodexModelMigration.ts:53-71` - 模型切换保存中清除 context/compact 两项。
- `src-tauri/src/domain/codex_managed_profiles.rs:770`、`:872` - profile 增删同步。
- `src-tauri/src/domain/provider_models.rs:907-917` - capability 变化同步。
- `src-tauri/src/infra/cli_proxy/codex.rs:139`、`:194` 与 `src-tauri/src/infra/cli_proxy/mod.rs:1322`、`:1716`、`:1799` - 代理生命周期同步。
- `src-tauri/src/infra/cli_proxy/mod.rs:559-576` - 配置投影保留 AIO 活动 catalog binding。
- `src-tauri/src/domain/provider_models.rs:23-24` 与 `src/services/providers/providerModels.ts:12-13` - provider-managed context 合法范围为 1024 到 10,000,000；不是普通 GPT-5.6 目录上限。

## External References

### OpenAI 官方文档

- Codex 配置参考（访问于 2026-08-16）：https://developers.openai.com/codex/config-reference/
  - `model_catalog_json`：启动时从 JSON 文件加载模型目录，可由 profile 覆盖。
  - `model_context_window`：当前模型可用的 context token 数。
  - `model_auto_compact_token_limit`：自动压缩阈值；未设置时使用模型默认。
- GPT-5.6 Sol：https://developers.openai.com/api/docs/models/gpt-5.6-sol
- GPT-5.6 Terra：https://developers.openai.com/api/docs/models/gpt-5.6-terra
- GPT-5.6 Luna：https://developers.openai.com/api/docs/models/gpt-5.6-luna
  - 2026-08-16 三个官方模型页均显示 `1,050,000 context window`、`128,000 max output tokens`，并说明输入超过 `272K` 会进入长上下文定价。
  - 页面没有把 `380928`、`372000` 或 `32768` 声明为模型上限。380928 是本任务的产品策略值，不是官方最大上下文。

### Codex 0.147.0 固定版本源码

- Bundled JSON：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/models-manager/models.json
  - Sol slug 在第 4 行，窗口字段在第 27-29 行。
  - Terra slug 在第 119 行，窗口字段在第 142-144 行。
  - Luna slug 在第 232 行，窗口字段在第 255-257 行。
- 编译时嵌入：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/models-manager/src/lib.rs
  - 第 12-15 行用 `include_str!("../models.json")` 嵌入目录。
- 配置覆盖 clamp：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/models-manager/src/model_info.rs
  - 第 25-37 行将配置 context 与 `max_context_window` 取最小值。
- 字段默认与 auto compact：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/protocol/src/openai_models.rs
  - 第 357-359 行把 effective percent 默认成 95。
  - 第 415-422 行定义 max override 和 auto compact 语义。
  - 第 462-476 行按 resolved context 的 90% 推导并 clamp auto compact。
- 有效窗口：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/core/src/session/turn_context.rs
  - 第 238-245 行将 resolved context 乘以 effective percent 后作为 session model context。
- 硬上限：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/core/src/session/context_window.rs
  - 第 53-79 行把有效 model context 作为 full-context hard cap，并与 auto compact 阈值共同触发压缩。
- 静态目录切换：https://raw.githubusercontent.com/openai/codex/rust-v0.147.0/codex-rs/model-provider/src/provider.rs
  - 第 390-457 行表明提供 config catalog 时使用 `StaticModelsManager`，否则使用 `OpenAiModelsManager`。
- 固定 revision：tag `rust-v0.147.0`，commit `be6e8eac029b183056b7e4402879f15d2c85f61b`。

## Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md` - 生成目录必须基于真实 Codex/user catalog、保留未知字段、受 ownership/hash/size/drift 保护，并与 proxy/profile 事务一致。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md` - Codex TOML 修改需贯穿 Rust state/patch、解析/写入、生成绑定、TS service、query/UI 和测试；代理模式下必须同时维护 baseline/live projection。
- `.trellis/spec/aio-coding-hub/backend/index.md` - 后端边界、错误契约和测试入口索引。
- `.trellis/spec/aio-coding-hub/cross-layer/index.md` - 跨层功能需要检查 DTO、服务、UI、缓存/同步和回滚的完整数据流。
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - 不能把只成功写 TOML 当作端到端生效，应验证运行时消费端。
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - 应复用 managed catalog 的结构化 JSON、ownership 和事务能力，而不是创建第二套目录机制。

## Behaviors To Test

### Rust catalog and runtime contract

- 开启时只把三个精确 GPT-5.6 slug 的 `max_context_window` 改为 `380928`；`context_window` 仍为 base 的 `272000`。
- 关闭时三个条目完全恢复基础值；GPT-5.5/5.4、`aio/*`、未知/未来模型、相似前缀均不变。
- 基础目录顶层和模型未知字段完整保留；重复生成字节稳定，ownership hash 包含 long-context 状态。
- 配置 `model_context_window = 380928` 经目标目录 clamp 后仍为 380928；原始 272000 目录下明确证明会被 clamp。
- 若保留 effective 95%，断言 session effective hard cap 为 361881，缺省 auto compact 为 342835；若批准 effective 100，则相应断言硬上限 380928。
- 已有显式 `model_auto_compact_token_limit` 保持不变，并由 Codex 按 90% 上限 clamp；本功能不写 32768。
- zero/one/multiple managed profiles 与 long-context off/on 的四类 ownership 组合。
- 用户已有绝对 catalog、无 catalog、AIO-owned catalog、外部 binding 漂移、生成文件被改、损坏/超限 JSON。
- 代理开/关、应用启动同步、Codex CLI 升级 fingerprint 变化后重建。
- baseline backup、generated JSON、live config 任一步失败时完整回滚；回滚期间外部修改时 fail closed/recovery-required。

### Config ownership and frontend

- `null`/缺失/272000/372000/其他手工值都显示关闭；只有后端确认的精确 380928 显示开启。
- 用户显式开启后联合持久化目录和 TOML；保存失败时开关回滚或重新同步，不能停留在乐观状态。
- 用户显式关闭时只删除当前精确 380928；若保存前已被外部改成其他值，应检测漂移而不是删除。
- 支持模型之间切换、支持到不支持模型、从不支持到支持模型，以及切换后的 reasoning effort reconciliation。
- 并发点击/保存、配置重新加载、第二阶段 reconciliation 失败时以后端返回状态为准。
- 现有 `useCodexModelMigration` 清除任意手工 context 的行为与 R6 是否冲突，需要用明确测试固定最终契约。
- UI 使用现有 Switch/Toggle 模式，unsupported 模型不误导用户；变更后有“新会话生效”和长上下文计价提示。

### Static catalog compatibility

- 开启生成 binding 后，model picker 仍包含生成时 base catalog 的全部模型和能力。
- 远端目录比 bundled 更新时，验证 static manager 的可观察差异并决定 UI 提示/刷新策略。
- 关闭最后一个 catalog ownership 原因时恢复用户原 catalog path；若仍有 managed profiles，则只撤销 long-context 改写而不删除生成目录。

### Quality gate

- 更新 Rust 单测、CodexTab 测试、model migration 测试、服务/绑定测试。
- 运行适用的 Rust tests、前端定向 tests、`pnpm typecheck`、`pnpm lint`、`pnpm tauri:fmt`、`pnpm check:generated-bindings`。
- 新启动真实 `codex-cli 0.147.0` 会话验证所选模型的 resolved context、effective hard cap 和 auto compact 阈值，而不只检查输出 JSON/TOML。

## Caveats / Not Found

- `python ./.trellis/scripts/task.py current --source` 在本研究子会话返回 `Current task: (none)`；父任务已明确指定 `.trellis/tasks/08-16-codex-gpt56-372k-context`，因此研究写入该目录。未猜测或写入其他任务目录。
- PRD 的 372 Ki 精确值是 `380928`。早期讨论中的 `372000` 不是需求值，也不能触发 ownership。
- 当前官方模型页声明的最大 context 是 1,050,000，Codex bundled 的 272,000 是客户端目录默认/上限，官方页的 `>272K` 还是定价阈值。这三个数字不能混为一谈。
- “配置/目录窗口 380928”与“Codex 0.147.0 实际有效硬上限 380928”并不等价；95% 语义使后者为 361881。这是当前最关键的产品验收歧义。
- 官方文档和固定版本源码中未找到 `32768` 与 GPT-5.6 context/auto compact 的关系；仓库命中的 32768 都属于日志 fixture、流/脚本/请求大小等无关限制。
- 当前 bundled catalog 和仓库中未找到 R5 所称“兼容 alias”的明确列表或稳定映射。不要用前缀匹配猜测。
- 自定义 `model_catalog_json` 使 models manager 静态化。即使派生目录正确，仍需接受或缓解远端模型目录不再动态更新的兼容性成本。
- Codex 在启动时读取 catalog；对已经运行的会话/进程不能承诺即时生效。
- 本研究未修改产品代码、spec、Git 状态，也未执行 MSI 构建；这些属于 implement/check 阶段。
