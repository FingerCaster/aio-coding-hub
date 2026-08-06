# 排查非 AIO Provider 思考等级显示 unknown

## Goal

定位“只要请求使用的 provider 不是 AIO，最近代理记录中的思考等级就显示为 `unknown`”这一现象的直接原因与数据链路，为后续独立修复提供可复核证据。

## Background

- 用户提供的截图中，多条 Codex 请求显示为 `Codex / gpt-5.6-sol-unknown`。
- 待分析日志数据库位于 `E:\Download\Telegram Desktop\aio-coding-hub.db`。
- 本任务仅做诊断，不修改产品代码、数据库内容或运行时配置。

## Confirmed Facts

- `attempts_json[].reason` 是供应商尝试/路由原因字段，不是 Codex 思考等级字段；成功或无需归因的尝试可为 `null`。
- Codex 思考等级在请求模型解析中从 `/reasoning/effort`、`/reasoning_effort`、`/reasoningEffort` 依次提取，并以 `type = codex_reasoning_effort` 写入 `request_logs.special_settings_json`（`src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs:58`, `:200`, `:237`）。
- 模型解析是中间件第 5 步，AIO 托管模型路由是第 6 步，Provider 选择是第 12 步（`src-tauri/src/gateway/proxy/handler/mod.rs:240`, `:246`, `:282`）。等级采集函数没有 Provider 参数，因此最终 Provider 不能直接决定是否采集等级。
- `special_settings_json` 序列化只做有界编码，不按 Provider 或设置类型过滤（`src-tauri/src/gateway/response_fixer/mod.rs:45`, `:69`）。异常区间内每条日志最多 2 个设置项，且截断标记数为 0，排除“已采集但持久化时被截断”。
- 异常区间的 `gpt-5.6-sol/luna` 样本均为 `/v1/responses`，`query` 为空，路径不含 `/models/...`，但数据库中的模型名不能证明请求体解析成功：流式响应结束时，若请求侧模型为空，日志会使用响应中的首个模型回填 `requested_model`（`src-tauri/src/gateway/streams/usage_tee.rs:575`, `:1150`）。因此 zstd 请求体完全无法解析时，数据库仍可显示 `gpt-5.6-sol/luna`。
- `record_codex_reasoning_effort` 只在同一解析 JSON 的三个位置发现字符串时写入记录：`/reasoning/effort`、根级 `reasoning_effort`、根级 `reasoningEffort`（`src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs:200`, `:237`）。因此异常日志能确定的是“没有捕获到可识别的 effort”，而不是“原始请求必然没有携带 effort”。
- 前端从 `special_settings_json` 解析显式等级；缺少显式条目时，仅对已知模型使用硬编码默认值，其余回退 `unknown`（`src/services/gateway/requestLogSpecialSettings.ts:146`）。列表模型文本和详情“请求等级”均复用该解析结果（`src/components/home/requestLogPresentation.ts:253`, `src/components/home/RequestLogDetailSummaryTab.tsx:50`）。
- 默认映射只包含 `gpt-5.4*` 与 `gpt-5.5*`，不包含截图中的 `gpt-5.6-sol` 或同期的 `gpt-5.6-luna`（`src/services/gateway/requestLogSpecialSettings.ts:68`）。现有单元测试也明确约定：没有显式等级且模型无已知默认值时返回 `unknown`（`src/services/gateway/__tests__/requestLogSpecialSettings.test.ts:98`）。
- 只读数据库样本 `request_logs.id = 21879` 的 attempt `reason` 为 `NULL`，但 `special_settings_json` 中的等级为 `xhigh`、来源指针为 `/reasoning/effort`，所以界面可正确显示；样本 `id = 23165` 没有 `codex_reasoning_effort` 条目，且模型 `gpt-5.6-sol` 不在默认映射中，因此前端回退 `unknown`。
- 数据库共有 22,985 条 Codex 日志。`id <= 21879` 的 21,713 条中有 2,813 条显式等级；`id >= 21880` 的 1,272 条中为 0。最后一条显式等级日志为 2026-07-27 的 `id = 21879`，2026-07-29 起的样本全部缺失等级。
- 这不是某个非 AIO Provider 的固定分支：较早日志中 `CIII codex` 有 1,715 条显式等级，`AI INPUT` 有 901 条，`CIII codex DLC` 有 2 条；2026-07-29 后上述 Provider 均为 0。
- 当前数据库的 `provider_models` 与 `codex_managed_profiles` 为空，且 `aio_managed_model_route` 日志数为 0，因此无法用该文件直接做一条真实 AIO 托管请求与普通请求的同版本对照。
- AIO 托管模型目录会写入等级能力元数据，但本机在移除显式等级和目录绑定后仍能正常捕获 effort；结合 zstd 证据，该目录差异已排除为本问题的主要原因（`src-tauri/src/infra/codex_model_catalog/managed.rs:1182`）。
- 本机只读对照库的最近 2,000 条 Codex 日志全部为 `gpt-5.6-sol`，分别落到 `ai.input.im-Plus`、`ai.input.im-Air`、`Company`，2,000 条均带 `codex_reasoning_effort = max`。这直接证明同一模型在非 AIO Provider 下也能正常显示。
- 本机显式配置 `model_reasoning_effort = "max"` 时，请求记录为 `/reasoning/effort = max`；用户删除该配置后的实测请求 `id = 252972` 仍记录 `/reasoning/effort = low`。因此显式配置不是本机正常显示的必要条件。
- 本机 `codex-cli 0.146.0` 的内置目录（`codex debug models --bundled`）已明确声明 `gpt-5.6-sol.default_reasoning_level = low`。AIO 生成目录以该内置目录为基础，原样保留普通模型条目并只追加 `aio/*` 托管别名（`src-tauri/src/infra/codex_model_catalog/managed.rs:937`, `:1020`）。因此注释 `model_catalog_json` 后会回退到内容相同的内置默认值，正常显示不受影响；该配置项本身不是分叉点。
- 本机最新只读样本在没有顶层 `model_reasoning_effort` 配置时仍记录了 `low`、`medium`、`max` 等显式请求值，进一步说明等级可以来自模型默认值或线程/会话选择，而不依赖这两个顶层配置键。
- 两个数据库的 `PRAGMA user_version` 均为 42，因此已观察到的差异不是数据库 schema 版本造成的。
- 后端已经能从 JSON/SSE 响应解析 `reasoning.effort`（`src-tauri/src/domain/usage.rs:493`），但该证据仅用于构造“路由不一致”设置。请求等级缺失、响应等级存在时，`effort_mismatch` 被判为 `false`；若模型也相同，函数直接返回 `None`，响应等级不会持久化（`src-tauri/src/gateway/model_route_mapping.rs:89`, `:94`）。
- Provider 模型表具备 `default_reasoning_effort` 能力元数据，但当前请求日志等级解析链路没有读取最终 Provider 的这份数据。本机有 87 条 Provider 模型记录、其中 39 条配置了默认等级；对方库为 0 条。
- 本机有 3 个 Codex 托管 Profile，`config.toml` 的 `model_catalog_json` 指向 AIO 生成目录；对方库有 0 个托管 Profile。目录同步代码在 Profile 为空时不生成目录并移除生成目录绑定，在非空时才安装（`src-tauri/src/infra/codex_model_catalog/managed.rs:499`, `:521`）。
- 异常边界同时伴随所有请求体派生信息丢失：最后一批正常记录是 `gpt-5.5` 且会话标识来自 `body_prompt_cache_key`；2026-07-29 起 effort 与 `body_prompt_cache_key` 同时消失，会话标识改由 `fingerprint_cache` 补全。这与请求体因 zstd 无法解析完全一致，不支持“最终 Provider 单独抹掉 effort”。
- 异常区间 1,272 条日志中有 1,203 条响应用量包含 `reasoning_tokens`，但没有任何列保存响应实际 `effort`；是否产生推理 token 不能反推出 `low/medium/high`，所以历史数据库无法补回具体等级。
- Codex 的 `enable_request_compression` 默认开启，但实际 zstd 压缩还要求认证使用 Codex backend 且模型 Provider 被识别为 OpenAI；官方覆盖同时明确 API Key 认证不会压缩。故“feature 开启”不等于所有请求都使用 zstd。
- `aio-coding-hub-v0.60.36` 的 `GatewayRequestBody` 只区分 `Identity`、`Gzip`、`Unsupported`；`Content-Encoding: zstd` 会进入 `Unsupported` 并把压缩原始字节当作 `decoded` 内容（`src-tauri/src/gateway/proxy/request_body.rs:8`, `:52`, `:142`）。`BodyReaderMiddleware` 随后直接对这些字节执行 JSON 解析并得到 `None`（`src-tauri/src/gateway/proxy/handler/middleware/body_reader.rs:64`）。当前 HEAD 与 `0.60.36` 在这些文件上无差异。
- 请求体未被 AIO 修改时，`finalize_for_upstream` 会恢复原 `Content-Encoding` 并原样转发压缩字节（`src-tauri/src/gateway/proxy/request_body.rs:97`），所以能出现“上游请求成功，但 AIO 无法检查请求体字段”的结果。
- 数据边界与压缩失读高度吻合：边界前抽取的 380 条记录中，380 条同时有显式 effort、`body_prompt_cache_key` 和模型名；边界后抽取的 357 条中，effort 与 `body_prompt_cache_key` 都为 0，330 条转为 `fingerprint_cache`，335 条仍有可由响应回填的模型名。
- 本机 `enable_request_compression` 的有效状态同样为 `true`，但 `codex login status` 显示使用 API Key；按 Codex 的压缩条件不会发送 zstd，所以本机非 AIO 下仍能正常捕获 effort。这解释了本机对照没有复现。

## Diagnosis

- **直接原因（高置信）**：zstd 请求体未被解码，`introspection_json` 为 `None`，所以 `special_settings_json` 没有 `codex_reasoning_effort`；`gpt-5.6-sol/luna` 又不在前端默认映射中，`resolveCodexReasoningEffort` 最终返回 `unknown`。
- **产品根因（高置信）**：AIO `0.60.36` 不支持解码 Codex 发送的 zstd 请求体，导致请求 JSON introspection 整体缺失；effort、`prompt_cache_key` 等所有请求体派生证据同时丢失。日志展示链路又仅用静态模型表补少数默认值，因此 `gpt-5.6-*` 最终显示 `unknown`。
- **本机为何正常（高置信）**：不是因为显式 `model_reasoning_effort` 或 `model_catalog_json`。本机使用 API Key 认证，Codex 官方实现不会在该认证模式下启用请求压缩，AIO 因而收到普通 JSON 并能捕获 effort。
- **“非 AIO 才出现”解释（中高置信）**：最终下游 Provider 仍不是等级采集分支；真正分叉点是 Codex 入站请求的认证/Provider 模式是否满足 zstd 压缩条件。AIO UI 中的 Provider 切换若同时切换 Codex 认证或模型 Provider 配置，就会形成表面上的 AIO/非 AIO 相关性。
- **证据边界**：数据库不保存请求头或原始请求体，不能直接看到 `Content-Encoding: zstd`，所以现有结论是机制、版本代码和数据共变共同支持的高置信归因，而非单条请求的直接包证据。让对方设置 `features.enable_request_compression = false`、完全重启 Codex 后用同一 Provider 发一条请求；若 effort 与 `body_prompt_cache_key` 同时恢复，即完成因果 A/B 验证。

## Suggested Fix Direction

- 根本修复应让 `GatewayRequestBody` 有界解码并按需重新编码 zstd，请求体检查、插件和转发继续共享明确的 raw/decoded 语义；仅补前端默认值无法恢复 session、插件和其他请求体派生功能。
- 临时规避可在 Codex `[features]` 下设置 `enable_request_compression = false`；这会牺牲请求压缩收益，但能让当前 AIO 版本重新读取 JSON。
- 若只做最小展示修复，可补充 `gpt-5.6-*` 默认映射，但该方案会掩盖请求体整体失读，且随模型能力变化而失真，不建议作为根因修复。
- 修复时还应保存同模型响应中解析到的 effort，而不是仅在路由不一致时保存。
- 修复验证应至少覆盖：identity、gzip、zstd 请求体的读取与原样/重编码转发，显式 request effort、`prompt_cache_key`、插件无修改和有修改路径，以及日志列表和详情的一致性。

## Requirements

- 只读检查 SQLite 数据库的表结构及相关请求记录，确认 provider、模型、思考等级等字段的实际存储值。
- 沿现有代码追踪思考等级从请求接入、代理转发、日志持久化到最近代理记录展示的完整数据流。
- 对比 AIO provider 与非 AIO provider 的处理分支，找出 `unknown` 的产生位置及触发条件。
- 区分数据库原始数据缺失、字段映射错误、provider 特定解析遗漏和纯前端展示问题。
- 输出根因、影响范围、证据位置和建议修复方向，但不实施任何修复。

## Acceptance Criteria

- [x] 给出数据库中能复现该现象的代表性记录及其关键字段，不泄露密钥或完整敏感请求内容。
- [x] 给出从输入到展示的代码路径，并提供关键文件与行号。
- [x] 明确指出 `unknown` 是在哪一层生成或回退得到的，并纠正“由非 AIO Provider 分支导致”的归因。
- [x] 说明结论的确定性、仍存在的未知项和最小修复建议。
- [x] 除本任务的 Trellis 规划/记录文件外，产品代码和用户数据库保持不变。

## Out Of Scope

- 修改代码、数据库、配置或历史日志。
- 实施或验证修复。
- 与该思考等级问题无关的性能、计费或代理错误排查。

## Notes

- 数据库位于仓库外，仅允许只读访问。
- 展示截图可作为现象佐证，根因判断必须以数据库和代码证据为准。
