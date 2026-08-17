# Research: Codex GPT-5.6 372K runtime dataflow

> 2026-08-17 最终产品决策：跟随 Codex 上游 `272000` 的十进制目录口径，372K 的实现与验收值为 `372000`。下文关于 `380928` / `372 * 1024` 的数值建议属于已被覆盖的历史分析，不得作为实现要求。

- Query: 确认 Codex CLI 0.147.0 对 GPT-5.6 上下文窗口的真实解析、启动加载和缓存路径，判断 `372 * 1024 = 380928` 开关如何在 AIO 中落地，同时保持 bundled 默认、用户目录、`aio/*` 能力和并发回滚契约。
- Scope: mixed（仓库内部实现、OpenAI Codex 0.147.0 源码与官方配置参考）
- Date: 2026-08-16

## Findings

### 1. 结论

1. 目标值必须始终写成精确十进制 `380928`。`372000` 既不是 372 Ki token，也不得被 UI 识别为本功能开启。
2. 只写根配置 `model_context_window = 380928` **不能生效到 380928**。Codex 会用目录项的 `max_context_window` 截断配置覆盖；当前 GPT-5.6 三个 bundled 条目均为 `272000`，所以单字段方案最终仍是 `272000`。
3. 开启态至少需要两部分保持一致：
   - 根配置：`model_context_window = 380928`，作为用户可见、可逆且满足 R6 的功能状态；
   - 派生完整目录：仅把三个精确 GPT-5.6 条目的 `context_window` 与 `max_context_window` 同时改成 `380928`。
4. 关闭态只在当前根值**精确等于** `380928` 时删除本功能拥有的覆盖，并恢复未改写的 bundled/用户目录。`372000`、其他整数及缺省值都不是本功能所有，不得自动删除或改写。
5. 保留目录原有 `effective_context_window_percent = 95` 与 `auto_compact_token_limit = null` 时：
   - 名义窗口：`380928`；
   - Codex 实际输入可用窗口：`floor(380928 * 95 / 100) = 361881`；
   - 自动压缩阈值：`floor(380928 * 9 / 10) = 342835`。
   这不是 380928 全量可用输入空间。若验收中的“使用 380928 token 上下文”指名义目录/配置值，则保留 95% 是正确且最小的行为；若要求 380928 全量可用，必须另行授权把百分比改成 100，这会改变 Codex 的安全余量，不应暗中完成。
6. 当前 AIO managed catalog 管线只有“Codex 代理已启用”时才活动，并且只有 `profiles` 非空才生成和绑定目录。PRD 未限定必须开启代理，因此直接复用 `prepare_for_profiles` 仍有功能缺口：代理关闭时会 no-op，零 managed profile 时也会移除目录。完整实现必须泛化该目录所有权，或明确把产品范围收窄为代理启用态；不能把现状误认为已覆盖普通 Codex。

### 2. 本机与精确模型集合

本机命令实测：

```text
codex-cli 0.147.0

slug             context_window  max_context_window  auto_compact_token_limit  effective_context_window_percent  comp_hash
gpt-5.6-sol      272000          272000              null                      95                                "3000"
gpt-5.6-terra    272000          272000              null                      95                                "3000"
gpt-5.6-luna     272000          272000              null                      95                                "3000"
```

`codex debug models --bundled` 中只有以上三个 `^gpt-5\.6` slug，没有裸 `gpt-5.6` 条目。因此建议使用固定 allowlist：

```text
gpt-5.6-sol
gpt-5.6-terra
gpt-5.6-luna
```

不要用 `starts_with("gpt-5.6")` 改写未来模型，也不要合成裸 `gpt-5.6` 条目。Codex 自身已支持最长前缀匹配，以及单层命名空间去除后再匹配；兼容 alias 应沿用该解析，不扩大目录改写集合。`aio/<profile_name_key>` 有自己的精确目录项，必须继续由 provider-model capability 决定。

### 3. Codex 运行时字段语义

- `ModelInfo.context_window` 是名义窗口；`resolved_context_window()` 优先使用它，缺失时才回退 `max_context_window`（OpenAI Codex `protocol/src/openai_models.rs:413-469`）。
- `max_context_window` 的注释就是“配置覆盖允许的最大上下文窗口”。`with_config_overrides` 对 `model_context_window` 执行 `min(requested, max_context_window)`（`models-manager/src/model_info.rs:25-33`）。因此必须同时放大目录上限。
- `auto_compact_token_limit = null` 不是禁用压缩，而是由 `resolved_context_window * 9 / 10` 派生（`protocol/src/openai_models.rs:466-476`）。保持 null 能自然随 380928 变为 342835。
- 真正字段名为 `effective_context_window_percent`，没有独立的 `context_window_percent`。turn context 用整数运算 `context_window * percent / 100`（`core/src/session/turn_context.rs:238-245`）。
- 未命中任何目录候选时，Codex fallback 仍硬编码 `context_window = max_context_window = 272000`、百分比 95（`models-manager/src/model_info.rs:137-175`）。AIO 无法在不修改 Codex 二进制的情况下改变全局 fallback；只能保证受支持的精确目录项存在并被选中。
- 候选选择先做最长前缀，再对恰好一层 `namespace/suffix` 做 suffix 最长前缀匹配（`models-manager/src/manager.rs:617-672`）。这解释了兼容 alias 行为，也解释了为何不应宽泛改写所有前缀 slug。

### 4. Codex 启动、目录与缓存数据流

```text
$CODEX_HOME/config.toml
  -> 启动时读取 model_catalog_json
  -> 解析完整 ModelsResponse
  -> 有自定义目录：StaticModelsManager（权威目录，无 remote/cache 合并）
  -> 无自定义目录：OpenAiModelsManager
       -> bundled models
       -> $CODEX_HOME/models_cache.json（TTL 300 秒）
       -> 必要时远端 /models，按 slug 替换 bundled
  -> 选择模型元数据
  -> 应用 model_context_window，但受 max_context_window 截断
  -> 按 effective_context_window_percent 计算 turn 可用窗口
```

证据：

- 官方配置参考把 `model_catalog_json` 定义为“启动时加载的可选 JSON 模型目录路径”，把 `model_context_window` 定义为“活动模型可用的上下文 token 数”。
- Codex 在构造 Config 时读取并解析完整 `ModelsResponse`；空模型数组或非法 JSON 会失败（`core/src/config/mod.rs:2057-2085,3947-3951,4204`）。自定义目录替换当前进程 bundled catalog（`core/src/config/mod.rs:978-980`）。
- provider 有目录时创建 `StaticModelsManager`；无目录时创建带缓存的 `OpenAiModelsManager`（`model-provider/src/provider.rs:390-456`）。
- 普通 manager 的缓存文件为 `models_cache.json`，TTL 300 秒；`OnlineIfUncached` 先尝试缓存，再访问远端，远端同 slug 覆盖 bundled（`models-manager/src/manager.rs:29-30,374-475`）。app-server `model/list` 使用该策略（`app-server/src/models.rs:13-24`）。
- `ThreadManager` 在创建时持有 `SharedModelsManager`，后续仅克隆同一 manager（`core/src/thread_manager.rs:376-445,675-687`）。修改磁盘上的 catalog 不会让已运行 Codex 进程热加载；开关只保证**新启动进程/新 app-server**使用新目录，旧会话必须重启。
- AIO 构建 managed base 时显式运行 `codex debug models --bundled`，因此绕过远端和 `models_cache.json`（`src-tauri/src/infra/codex_model_catalog/protocol.rs:109-116`）。这是可复现基线，但也意味着绑定派生目录后，当前进程使用的是静态快照，直到 AIO 重新生成并重启 Codex。

### 5. AIO 当前普通 catalog 数据流

```text
Codex 页面进入/刷新
  -> codex_model_catalog_get
  -> 解析当前 Codex executable、version、CODEX_HOME
  -> 启动 codex app-server --stdio
  -> JSON-RPC initialize
  -> model/list(includeHidden=true)
  -> 前端 DTO（picker/reasoning 字段）
  -> TanStack Query 以 configPath/executablePath/cliVersion 缓存 5 分钟
```

- 后端入口与启动路径见 `src-tauri/src/infra/codex_model_catalog/mod.rs:59-115`、`protocol.rs:99-191,298-307`。
- `RawCodexModel` / `CodexModelCapability` 只传 id、model、显示、隐藏、默认和 reasoning effort，不传上下文字段（`protocol.rs:203-284`、`mod.rs:33-42`）。前端 catalog 不能证明 380928 已实际加载。
- 前端缓存 5 分钟，key 为 config path、executable path、CLI version；手动刷新会 invalidate 后 prefetch（`src/query/cliManager.ts:38-59,89-114`、`src/query/keys.ts:303-323`、`src/pages/cli-manager/useCliManagerPageDataModel.ts:465-481`）。
- 因为 key 不含 catalog 内容 hash 或开关状态，切换后若要立即更新 picker 状态，应显式刷新/失效查询；但验收运行时数值仍应由生成 JSON 或新的 `codex debug models` 进程验证，不能只依赖该 DTO。

### 6. AIO 当前 managed catalog 数据流

```text
代理基线 config.toml
  -> 有绝对 user model_catalog_json：读取用户完整目录
  -> 否则：当前 Codex debug models --bundled
  -> 校验完整 base 与 owner metadata
  -> 原样保留 base models/未知字段
  -> 追加 aio/<profile> entries
  -> 写 app_data/cli-proxy/codex/managed-model-catalog.json
  -> 再把 live config.toml.model_catalog_json 指向生成文件
```

- 生成路径见 `src-tauri/src/infra/codex_model_catalog/managed.rs:580-597`。
- base 选择与当前 CLI 指纹见 `managed.rs:937-990`；用户目录按原始 bytes/hash 纳入指纹，bundled 指纹含 executable、version、文件长度和 mtime。
- `generate_catalog` 保留完整 base，验证 slug 唯一性，再追加 managed profile（`managed.rs:1020-1164`）。
- `aio/*` 的上下文来自数据库 `provider_models.context_window`，生成时同时投影到 `context_window/max_context_window`，并保留 null auto-compact 与 95%（`managed.rs:401-455,992-1008,1166-1249`）。它不从绑定的 `remote_model_id` 继承普通 GPT-5.6 的窗口。
- owner cache 目前只比较 `profile_set_sha256 + base_source_fingerprint`（`managed.rs:395-399,499-518,1276-1321`）。如果加入 372K 策略但不把策略状态/版本纳入所有权 hash，开关切换会错误复用旧生成文件。
- 当前 `profiles.is_empty()` 时不生成，且绑定条件也是 `!profiles.is_empty()`（`managed.rs:499-526`）。若在该管线落地，至少要抽象成 `needs_generated_catalog = !profiles.is_empty() || gpt56_372k_enabled`，并让开关状态进入 policy/profile hash 与 owner metadata。
- 写入顺序是 baseline repair -> generated catalog -> live config；先做快照漂移检查，原子替换，失败时反向补偿（`managed.rs:119-165,168-243,284-347,661-735`）。新功能应复用这一事务顺序，不能先让 live config 指向尚不存在/不完整的目录。
- 关闭或无生成需求时，`model_catalog_json` 恢复为原用户值或移除（`managed.rs:791-910`）。

### 7. 推荐实现边界

#### 7.1 状态判定

建议不新增第二个互相漂移的 boolean SSOT。开关状态由现有 Codex 根配置派生：

```text
selected model 属于受支持的 GPT-5.6 解析集合
AND model_context_window == 380928
```

- `380928`：开启；
- null/缺失：关闭；
- `372000` 或任何其他值：关闭，但显示/保留现有手工值；
- 用户明确点开：把现有值替换为 380928；
- 用户明确点关：仅当前值为 380928 时删除该字段。

这与现有 `CodexConfigState/CodexConfigPatch` 的 nullable 数字契约一致（`src-tauri/src/infra/codex_config/types.rs:21-39,52-66`），无需把目录字段暴露进普通 picker DTO。

#### 7.2 目录投影

在 base 的 `models` 数组中只对固定 allowlist 的**精确 slug**执行：

```json
{
  "context_window": 380928,
  "max_context_window": 380928
}
```

其余字段（包括 `auto_compact_token_limit`、`effective_context_window_percent`、`comp_hash`、messages、tools、reasoning、未知字段）保持 base 原值。关闭时重新从 base 生成，不在上次派生输出上做反向猜测。

对 `aio/*` 条目继续调用现有 `build_managed_model`，其 context 只来自显式 provider capability。不要按其 `remote_model_id` 套用 380928，否则违反 `.trellis/spec/.../codex-managed-model-route-contract.md:157-180,201-239` 的能力所有权，也违反 PRD 的 out-of-scope。

#### 7.3 保存与原子性

结构化保存与 raw TOML 保存都已经获取 managed-profile lifecycle lock（`src-tauri/src/infra/codex_config/mod.rs:293-310,388-405`）。应在同一锁内走一个共同 reconciler：

```text
读取 current/baseline
  -> 计算 next root config 与 feature mode
  -> 准备 base + 派生 catalog + owner hash
  -> 校验所有快照
  -> 写/修复 baseline backup
  -> 写 generated catalog
  -> 写 live config（model_context_window + model_catalog_json 同时可达）
  -> 任一步失败，按已应用顺序反向 CAS/快照补偿
  -> 返回重新读取的 canonical CodexConfigState
```

不能先由普通 config setter提交 `model_context_window`，再 best-effort 调 catalog sync；这样目录生成失败会留下 UI 开启、运行时仍被 272000 clamp 的分裂状态。raw TOML 中新增/删除精确 380928 也必须触发相同 reconciler，否则高级编辑器可制造同样的不一致。

模型切换也要共同处理：如果离开受支持 GPT-5.6 时根值仍是功能自有的 380928，应在该次显式模型保存事务内删除它并恢复目录，避免该全局覆盖把其他大窗口模型压到 380928。其他手工值不得随模型切换清理。

#### 7.4 代理启用范围缺口

当前 `prepare_for_profiles` 在没有 enabled proxy baseline 时直接返回 inactive（`managed.rs:458-470`）。因此有两个可选方向：

1. **满足当前 PRD的方向**：把“完整目录派生/所有权”从“代理专属”泛化成 Codex 配置级能力，代理只是其中一个 baseline/projection 消费者。代理关闭时也要安全保存原 `model_catalog_json` 路径/absence，绑定派生目录，并在关闭功能时恢复；代理后续启停、CODEX_HOME 重绑和应用退出都要与该所有权合并。
2. **较小但不满足当前文字验收的方向**：只扩展现有 managed 管线，要求 Codex 代理已启用。若选此方向，必须先修改 PRD/验收明确限制，而不能静默交付。

代理关闭态没有现成 baseline manifest 可恢复用户目录，因此方向 1 不是简单改一个条件：必须定义独立、hash-owned 的原目录路径/absence 和崩溃恢复状态。不要把用户原 catalog 本体改写；只生成 AIO 派生副本并切换根路径。

### 8. 刷新、重启与并发

- 现有 catalog 刷新入口包括代理启用（`src-tauri/src/infra/cli_proxy/mod.rs:1194-1349`）、网关启动/代理同步（`cli_proxy/mod.rs:1640-1830`、`src-tauri/src/app/gateway_service/lifecycle.rs:16-48`）、profile create/delete（`src-tauri/src/domain/codex_managed_profiles.rs:650-940`）、provider capability 更新（`src-tauri/src/domain/provider_models.rs:871-970`）和 CODEX_HOME 重绑（`src-tauri/src/infra/cli_proxy/codex.rs:73-215`）。新功能的 canonical reconciler必须覆盖这些入口。
- 应用退出当前恢复直连配置但保留 enabled manifest 状态；若 372K 目录需要在代理关闭/应用退出后继续有效，不能无条件随 proxy restore 一起撤销。该所有权优先级必须明确。
- 生命周期锁是进程内 `OnceLock<Mutex<()>>`（`codex_managed_profiles.rs:105-116`）；桌面构建注册 single-instance（`src-tauri/src/app/plugin_registry.rs:135-148`），可防普通双开，但不能防开发版/portable/另一 AIO 进程共享同一 `CODEX_HOME`。
- 文件快照和原子替换能发现一部分外部漂移，但“最后一次快照检查 -> replace”仍有跨进程 TOCTOU。所有写者必须校验 owner hash并在不确定时 fail closed；不要承诺跨进程强串行化，除非新增 OS 级文件锁。
- 多个已经运行的 Codex 进程各自持有启动快照。开关切换后允许旧进程保持旧值，但 UI 文案/成功提示必须说清需要重启 Codex 会话；测试要同时证明旧进程不热变、新进程读取新值。

### 9. 建议影响文件

最小核心改动面（实现代理无关所有权时可能继续扩展）：

| 文件 | 责任 |
| --- | --- |
| `src-tauri/src/infra/codex_model_catalog/managed.rs` | 精确 slug 投影、feature policy hash/owner metadata、零 profile 生成条件、base/用户目录保真与事务 |
| `src-tauri/src/infra/codex_config/mod.rs` | structured/raw 保存共同 reconciler，确保 config 与 catalog 原子一致 |
| `src-tauri/src/infra/codex_config/types.rs` | 复用现有 nullable `model_context_window`；只有确需虚拟状态 DTO 时才扩展 |
| `src/components/cli-manager/tabs/CodexTab.tsx` | Switch、精确状态判定、支持模型 gating、模型切换清理和重启提示 |
| `src/pages/cli-manager/useCliManagerPageDataModel.ts` / `src/query/cliManager.ts` | 成功后失效 config/catalog；保持失败时 canonical 回滚显示 |
| `src-tauri/src/infra/codex_model_catalog/managed.rs` 内 tests、`src-tauri/src/infra/codex_config/tests.rs`、`src-tauri/src/infra/cli_proxy/tests.rs` | Rust 目录、配置、代理生命周期和补偿回归 |
| `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx`、`src/services/cli/__tests__/cliManager.service.test.ts` | Switch/手工值/模型切换/patch 编码回归 |

若复用现有字段而不新增 IPC 字段，`src/generated/bindings.ts` 不需要语义变化；仍应运行生成绑定检查。`protocol.rs` 无需为了本功能扩展 context DTO，除非产品明确要求 UI 展示“运行时已加载值”。

### 10. 测试矩阵

| 维度 | 必须证明 |
| --- | --- |
| 数值 | 只接受/识别 `380928`；`372000` 明确为普通手工值 |
| 默认关闭 | bundled 三个条目仍是 272000；不写 380928，不绑定无必要派生目录 |
| 开启目录 | 三个精确 slug 的 `context_window/max_context_window` 都为 380928；95/null/`comp_hash` 和所有其他字段保真 |
| 精确范围 | 非 GPT-5.6、裸/未来未知 slug 不被改写；不做宽前缀批量替换 |
| `aio/*` | profile context 继续等于 DB capability；remote slug 即使是 GPT-5.6 也不被全局开关覆盖 |
| 手工根值 | null、372000、其他值保持；只在明确开启时替换，只在精确自有值关闭时删除 |
| 模型切换 | 支持型号之间保持开启；离开支持集合时只清理自有 380928，其他手工值保留，普通模型不被全局压低 |
| 零/一个 profile | 零 profile 开启仍按产品范围生成；一个 profile 时 base 三项与 `aio/*` 同时正确；关闭后 profile catalog 仍按原契约存在 |
| 用户 catalog | 原文件零写入；未知根/模型字段保留；只改派生副本精确条目；目标缺失时 fail closed 或按预先定义策略处理，不能静默 fallback 272000 |
| hash/cache | 相同 CLI/base 下 off -> on -> off 每次重建正确；policy 值/版本进入 owner hash；CLI version/executable 变化触发重建 |
| structured/raw | Switch patch与 raw TOML 写入精确值走同一事务并得到同一结果；raw 写其他值不会误开 |
| 失败补偿 | bundled 超时/非法、生成失败、generated write 失败、config write 失败、DB/backup 失败都恢复精确旧 bytes 与 UI canonical 状态 |
| 代理生命周期 | enable、sync、disable、gateway restart、应用退出、CODEX_HOME rebind 后目录所有权符合已选范围，不泄漏旧路径 |
| 查询刷新 | 切换后 config 与 catalog query 主动失效；永不完成/失败刷新不能让保存 promise 假成功或卡死 |
| 进程快照 | 已运行 Codex 保持旧值；新 `codex debug models`/app-server 进程从生成目录看到 380928；关闭后的新进程看到 272000/base 值 |
| 并发 | 同进程锁内 serialize；外部修改 owner/config 时 fail closed；确定性 race 不覆盖较新的用户 catalog 或 config |
| WSL | Windows 功能测试不得声称覆盖 WSL；若纳入范围，Linux 侧单独生成可访问目录并测试 |

建议增加一个真实 Codex 0.147.0 集成断言：在隔离 `CODEX_HOME` 写入生成目录绑定，然后运行 `codex debug models`（**不加** `--bundled`）检查三个字段；另以 `-c model_context_window=380928` 或隔离配置解析受支持模型，证明不再被 272000 clamp。`--bundled` 只用于验证关闭基线，不能用于验证派生目录已被加载。

## Files Found

### 仓库内部

- `src-tauri/src/infra/codex_model_catalog/managed.rs`：完整 managed catalog 的 base 选择、生成、owner hash、绑定、事务和 `aio/*` capability 投影。
- `src-tauri/src/infra/codex_model_catalog/protocol.rs`：启动 `app-server --stdio` 与 `debug models --bundled` 的受控子进程协议。
- `src-tauri/src/infra/codex_model_catalog/mod.rs`：普通 picker catalog IPC DTO 与入口。
- `src-tauri/src/infra/codex_config/mod.rs`：structured/raw Codex config 保存、proxy baseline 合并和原子写入。
- `src-tauri/src/infra/codex_config/types.rs`：现有 nullable `model_context_window` state/patch 类型。
- `src-tauri/src/infra/codex_config/patching.rs`：根 TOML 数字字段的写入/删除。
- `src/components/cli-manager/tabs/CodexTab.tsx`：当前模型、上下文数字输入和 Switch 交互模式。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`：Codex config mutation、toast 与 catalog 手动刷新。
- `src/query/cliManager.ts`、`src/query/keys.ts`：catalog 5 分钟缓存及 query identity。
- `src-tauri/src/domain/codex_managed_profiles.rs`：进程内 lifecycle lock 与 profile/catalog/DB 补偿。
- `src-tauri/src/domain/provider_models.rs`：capability 更新时的 catalog rebuild/rollback。
- `src-tauri/src/infra/cli_proxy/mod.rs`、`src-tauri/src/infra/cli_proxy/codex.rs`：代理启停、同步、CODEX_HOME 重绑和 catalog refresh 入口。
- `src-tauri/src/infra/wsl/config_codex.rs`：WSL 只投影 provider/auth；当前没有 catalog 生成或绑定。
- `src-tauri/src/shared/fs.rs`：原子替换实现；不能提供跨进程事务锁。

### 外部 OpenAI Codex 0.147.0（固定 commit）

- `codex-rs/models-manager/models.json`：三个 GPT-5.6 bundled model 的 272000 元数据（行 4/27-30、119/142-145、232/255-258）。
- `codex-rs/models-manager/src/model_info.rs`：config override clamp 与未知模型 272000 fallback。
- `codex-rs/protocol/src/openai_models.rs`：目录字段定义、resolved window 与 90% auto-compact。
- `codex-rs/core/src/session/turn_context.rs`：95% effective window 计算。
- `codex-rs/core/src/config/mod.rs`：`model_catalog_json` 启动加载并替换 bundled。
- `codex-rs/model-provider/src/provider.rs`：自定义目录选择 StaticModelsManager。
- `codex-rs/models-manager/src/manager.rs`：bundled/cache/remote 合并、最长前缀与 namespace alias 解析。
- `codex-rs/app-server/src/models.rs`：`model/list` 的 `OnlineIfUncached` 策略。
- `codex-rs/core/src/thread_manager.rs`：进程创建时持有 models manager，不热读磁盘目录。

## Code Patterns

- **完整副本投影，不改源文件**：用户 catalog 或 installed bundled 是 base，生成 AIO-owned 完整副本；见 `managed.rs:937-990,1020-1164`。
- **精确字段双写**：现有 `aio/*` 已把已知 capability 同时写入 `context_window/max_context_window`；见 `managed.rs:1232-1241`。GPT-5.6 投影应沿用这一必要模式，但数据源不同。
- **hash ownership + fail closed**：生成 bytes 带 payload/profile/base hash，外部漂移拒绝覆盖；见 `managed.rs:1276-1321`。
- **prepare/apply/rollback**：写前快照，先 generated 后 live binding，失败反向补偿；见 `managed.rs:119-347`。
- **nullable field ownership**：`Option<Option<u64>>` 区分 patch 未提供与显式删除；见 `codex_config/types.rs:52-66`、`patching.rs:689-697`。
- **前端 canonical 回读**：mutation 返回后端 `CodexConfigState`，失败 toast且不自行宣称成功；见 `useCliManagerPageDataModel.ts:558-582`。

## External References

- OpenAI Docs，Codex Config Reference：`https://learn.chatgpt.com/docs/config-file/config-reference`。2026-08-16 读取；确认 `model_context_window` 与 `model_catalog_json` 的官方定义，后者在启动时加载。
- OpenAI Codex source，tag `rust-v0.147.0` 对应 commit `be6e8eac029b183056b7e4402879f15d2c85f61b`。以上外部源码行号均固定到该 commit。
- 本机实测：`codex-cli 0.147.0`；`codex debug models --bundled` 输出与固定源码一致。

## Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md`：managed profile capability、完整 catalog、hash ownership、proxy-time activation、零 profile restore。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`：结构化/原始 Codex TOML 字段、proxy-owned field 和保存事务。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md`：字段所有权、changed-key patch、外部副作用失败后的 owned CAS/收敛。
- `.trellis/spec/aio-coding-hub/cross-layer/index.md:68-88,210-215,315-320`：Codex config、managed model 与 setting writer 的变更检查表。

本功能若让零 profile 仍绑定派生目录，或让 catalog 在 proxy disabled 时继续由 AIO 管理，会改变现有 managed route spec 的“proxy-time activation / zero profiles no generated catalog”契约；实现前应同步更新 spec，而不是把行为差异藏在代码中。`aio/*` capability 权威规则不应修改。

## Caveats / Not Found

- PRD 没有说明 380928 是“名义窗口”还是“95% 后的真实可用输入窗口”。按 Codex 当前语义，二者分别是 380928 与 361881；这是实现前唯一需要产品明确理解的数值差异。
- 当前普通 `model/list` DTO 不暴露 context/max/effective 字段，前端 picker 不能作为运行时数值证明。
- 用户自定义 catalog 缺少所选精确 GPT-5.6 条目时，没有现成安全补全契约。推荐 fail closed 并保留全部 bytes，或单独设计“从 bundled 精确补入缺失目标”的明确策略；不得静默落入 272000 fallback。
- WSL 路径明确未覆盖：`configure_wsl_codex` 只写 Linux `config.toml` 的 provider/auth，Windows app-data 生成路径也不能直接当作 Linux Codex 的可靠路径。若产品验收包含 WSL，需要额外实现与测试。
- Codex 0.147.0 的未知模型 fallback 固定 272000，AIO 派生 catalog 无法改变二进制 fallback；未来 CLI 升级必须重新验证字段和源码语义。
- 进程内锁与桌面 single-instance 不能防所有共享 `CODEX_HOME` 的跨进程 writer；现有机制只能 fail closed，不能宣称严格跨进程序列化。
