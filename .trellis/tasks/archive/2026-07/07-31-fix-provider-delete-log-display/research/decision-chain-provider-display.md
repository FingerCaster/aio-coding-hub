# 请求日志详情决策链 Provider 展示研究

## 结论

当前数据链已经足以在请求日志详情的每个决策链条目中展示“人类可读名称 + 稳定数字 ID”，无需回查当前 `providers` 表，也无需新增 Rust DTO、数据库列或生成 TypeScript 字段：

- 请求发生时，网关把 `provider_id`、`provider_name` 和 `base_url` 一起写入 `request_logs.attempts_json`，它们是历史快照（`src-tauri/src/gateway/events.rs:72-76`、`src-tauri/src/gateway/proxy/request_end.rs:350-355`）。
- 详情查询和 attempts 兼容接口都直接从该快照解码名称、ID、URL，不靠 provider 当前是否仍存在（`src-tauri/src/infra/request_logs/queries.rs:394-432`、`src-tauri/src/infra/request_attempt_logs.rs:55-97`）。
- 生成绑定和前端适配器已经携带这三个字段（`src/generated/bindings.ts:4120-4132`、`src/services/gateway/requestLogs.ts:38-43`、`src/services/gateway/requestLogs.ts:120-124`）。
- 实际缺口只在展示层：`ProviderChainView` 已合并出 `provider_name`，但单次尝试卡片正文只渲染 `provider_id`（`src/components/ProviderChainView.tsx:105-134`、`src/components/ProviderChainView.tsx:324-340`）。

推荐采用前端最小改动：以持久化 attempt 快照为权威，在每个 attempt 卡片的可折叠标题中始终显示 `名称 (#ID)`；名称缺失时稳定回退为 `未知供应商 (#ID)`。不要为了判断“已删除”而加入当前 provider 列表查询，因为当前数据无法区分“已删除、旧日志缺字段、损坏数据或其他不可解析”这些原因。

## 已读取规范

- `.trellis/spec/aio-coding-hub/cross-layer/index.md:14` 将 gateway failover route 定义为跨 Rust、生成绑定、适配器和 UI 的契约；其质量门禁要求重新校验生成绑定、前端空值/未知值以及聚焦测试（`:157-168`）。
- `.trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md:23-35` 定义 route/attempt 展示字段，`:55-60` 明确 provider hop 数与 attempt 数不可混用，`:127` 要求前端覆盖 provider/transition/attempt 计数。
- `.trellis/spec/aio-coding-hub/cross-layer/usage-insights-contract.md:14` 给出 request log 的 Rust -> binding -> frontend 数据流；`:77-78` 已采用“当前 provider 名称，失败后回退到持久化 attempt 名称”的历史身份原则。
- `.trellis/spec/aio-coding-hub/backend/index.md:7-37` 要求共享 failover 输入变更时保持 attempt/route 行为并运行完整 Rust 验证。本研究建议不改 failover 输入或路由语义。
- `.trellis/spec/guides/cross-layer-thinking-guide.md:21-44` 要求先映射完整数据流并定义边界契约；`:74-100` 要求结构化 payload 只有一个解析/归一化所有者，`:116` 要求覆盖 null、empty、invalid。

## 完整数据链

### 1. 网关日志记录与序列化

`FailoverAttempt` 是 `attempts_json` 的源结构，包含：

| 字段 | 来源 | 语义 |
| --- | --- | --- |
| `provider_id: i64` | `src-tauri/src/gateway/events.rs:73-74` | provider 的本地数据库数字 ID，决策链分组也按此 ID 进行。 |
| `provider_name: String` | `src-tauri/src/gateway/events.rs:75` | 请求发生时的名称快照。 |
| `base_url: String` | `src-tauri/src/gateway/events.rs:76` | 本次 attempt 的 URL 快照；准备前被跳过时可能只是首个配置 URL或空串。 |
| provider 类型 | 无逐 attempt 字段 | `FailoverAttempt` 不保存 `cli_key`、`auth_mode`、`oauth_provider_type` 或 `bridge_type`。 |

正常 provider 名称若为空，准备阶段会生成 `Provider #<id> (auto-fixed)`，因此新日志通常有非空名称（`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:98-108`）。熔断/限流等 gate skip 也会把 ID、名称和展示 URL写入 attempt（`src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs:70-106`、`src-tauri/src/gateway/proxy/handler/failover_loop/loop_helpers.rs:48-80`）。

请求结束时，attempt 名称最多保留 512 字符、URL最多保留 2048 字符，然后整体序列化（`src-tauri/src/gateway/proxy/request_end.rs:316-355`）。`request_log_insert_from_args` 把序列化结果原样放入 `RequestLogInsert.attempts_json`（`src-tauri/src/gateway/proxy/logging.rs:114-183`），最终写入 `request_logs`（`src-tauri/src/infra/request_logs.rs:629-700`）。

还有一个较瘦的 `provider_chain_json`，只快照 `provider_id`、`provider_name`、status/outcome/decision 等，不含 URL 或 provider 类型（`src-tauri/src/gateway/proxy/request_end.rs:358-385`）。当前详情决策链不消费它，因此不应为本修复改用它。

### 2. Rust 查询与 DTO

`RequestLogDetail` 暴露原始 `attempts_json`、最终 provider ID/名称和请求级 `cli_key`，但不暴露 `route`（`src-tauri/src/infra/request_logs/types.rs:112-153`）。`RequestLogSummary` 才暴露按连续 provider ID 折叠后的 `route`，其 hop 只有 ID/名称及结果字段（`src-tauri/src/infra/request_logs/types.rs:38-55`、`:61-109`）。

查询行为：

1. `row_to_detail` 从 `attempts_json` 计算最终 provider ID/名称，并保留原 JSON（`src-tauri/src/infra/request_logs/queries.rs:394-443`）。
2. `request_attempt_logs_by_trace_id` 是兼容接口，不是另一张 attempt 表；它再次读取同一 `request_logs.attempts_json`，投影成 `RequestAttemptLog { provider_id, provider_name, base_url, ... }`（`src-tauri/src/infra/request_attempt_logs.rs:8-22`、`:55-97`）。
3. `route_from_attempts` 按正数 `provider_id` 折叠连续 retries，名称取该 attempt 快照，不回查 provider（`src-tauri/src/infra/request_logs/queries.rs:140-206`）。
4. 唯一会查当前 `providers` 表的是 bridge/source provider 补充信息；provider 被删后 `final_provider_source_id/name` 会变为 `None`，但决策链 attempt 自身的名称/ID不受影响（`src-tauri/src/infra/request_logs/queries.rs:228-293`、`:446-470`）。

注意旧/异常数据：详情查询的内部 `AttemptRow` 要求 `provider_id`、`provider_name`、`outcome` 存在；任一行反序列化失败会让整份 attempts 投影退化为空数组（`src-tauri/src/infra/request_logs/queries.rs:90-104`）。兼容接口则给 `provider_name`、`base_url`、`outcome` 设置空串默认值，但 `provider_id` 仍是必需字段（`src-tauri/src/infra/request_attempt_logs.rs:24-40`）。

### 3. Tauri 命令与生成 TypeScript 绑定

- `request_log_get` 返回 `RequestLogDetail`；`request_attempt_logs_by_trace_id` 返回 `Vec<RequestAttemptLog>`，两者均有 `#[specta::specta]`（`src-tauri/src/commands/request_logs.rs:93-137`）。
- 两个命令都在共享 registry 注册（`src-tauri/src/commands/registry.rs:212-220`）；导出器从同一 registry 生成 TypeScript（`:290-319`）。
- 生成结果中 `RequestAttemptLog` 已有必需的 `provider_id`、`provider_name`、`base_url`，`RequestLogDetail` 已有 `cli_key`、`attempts_json`、`final_provider_id/name`（`src/generated/bindings.ts:4120-4172`）。

因此，仅做“名称 + ID”展示时不应手改 `src/generated/bindings.ts`，也不需要触发绑定差异；仍须运行 `pnpm check:generated-bindings` 证明无漂移。若未来真要增加逐 attempt 类型，必须先给 Rust `FailoverAttempt` 和 `RequestAttemptLog` 增加可选快照字段，再重新生成绑定，不能只改 TypeScript。

### 4. 前端适配器与合并

`src/services/gateway/requestLogs.ts:31-43` 只收窄请求/attempt 的 `cli_key`；`toRequestLogDetail` 与 `toRequestAttemptLog` 对 provider 字段不做改变（`:113-124`）。

`ProviderChainView` 同时接收：

- `attemptLogs`：生成 DTO 的兼容投影；
- `attemptsJson`：完整结构化 payload。

有兼容投影时，ID/名称/URL优先使用 `attemptLogs`，空名称或空 URL再回退到相同索引的 raw JSON；其他 decision/circuit 字段来自 raw JSON（`src/components/ProviderChainView.tsx:64-138`）。raw parser 当前只检查顶层是否数组，随后直接类型断言，没有逐字段校验（`src/services/gateway/attemptsJson.ts:35-42`）。本次展示修复可在 formatter 层容忍缺失字段，无需顺带扩大为完整 schema 重构；但若未来增强 raw contract，应在该单一 parser 中集中完成。

### 5. 详情 UI 当前行为

`RequestLogDetailDialog` 并行获取 detail 和 attempts 兼容投影，并把两者传给 chain tab（`src/components/home/RequestLogDetailDialog.tsx:68-86`、`:212-219`）。chain tab 已在顶栏展示请求级 CLI 类型 badge（`src/components/home/RequestLogDetailChainTab.tsx:23-46`）。

当前可见身份不满足“每项名称 + ID”：

- 起始/最终 pill 有名称时只显示名称，名称未知时才拼 ID（`src/components/ProviderChainView.tsx:168-194`）。
- 每个 attempt 的折叠标题只显示结果、耗时、HTTP 状态，不显示 provider 身份（`:282-322`）。
- 展开后只显示 `Provider ID`，虽然 `attempt.provider_name` 已在内存中（`:324-340`）。
- 未知哨兵不统一：链组件只特殊处理中文 `"未知"`（`:170-179`），通用 `resolveProviderLabel` 只特殊处理英文 `"Unknown"`（`src/pages/providers/baseUrl.ts:27-34`）。旧日志可能因此显示裸 `Unknown` 而丢失 ID。

## 字段可用性与删除后的行为

| 身份信息 | 决策链现在是否可用 | provider 删除后 | 是否适合作为身份 |
| --- | --- | --- | --- |
| 名称 | 是，`attempts_json.provider_name` 和 `RequestAttemptLog.provider_name` | 保留请求时快照；provider 后续改名也不会改历史日志 | 人类可读主标签，但不能单独保证唯一。 |
| 数字 ID | 是，`provider_id` | 保留；本地日志/DB范围内稳定 | 必须始终可见，用于同名、同域名精确区分。 |
| URL | 是，`base_url`；某些 OAuth/准备前 skip 可为空 | 快照保留 | 仅作二级上下文；同域名可重复，不能作唯一标签。 |
| 请求类型 | 是，请求级 `RequestLogDetail.cli_key`/`RequestAttemptLog.cli_key` | 保留 | 整条链同一 CLI family，现有 tab badge 足够。 |
| auth/OAuth/bridge 子类型 | 当前 provider 表有 `auth_mode`、`oauth_provider_type`、`bridge_type`（`src-tauri/src/domain/providers/types.rs:287-325`） | provider 删除后不可解析 | 不应为本需求临时回查；如产品确需，必须新增可选历史快照。 |
| provider UUID | 当前 provider 表有不可变 `provider_uuid`（`src-tauri/src/domain/providers/types.rs:288-293`） | 未写入日志，删除后不可用 | 本次不能展示；现有稳定身份契约是数字 `provider_id`。 |

provider 删除的数据库语义也支持历史快照：`request_logs.final_provider_id` 没有 provider 外键（`src-tauri/src/infra/db/migrations/baseline_v25.rs:72-104`）；默认删除只删 provider/model 数据，不更新日志 JSON（`src-tauri/src/domain/providers/queries.rs:1848-1920`），已有测试明确验证默认保留日志（`src-tauri/src/domain/providers/tests.rs:1143-1155`）。

例外是用户显式选择 `clear_usage_stats=true`：后端会删除 `final_provider_id` 匹配的 request logs（`src-tauri/src/domain/providers/queries.rs:1912-1917`，测试见 `src-tauri/src/domain/providers/tests.rs:1157-1172`）。日志本身被删后不存在详情页回退，这属于既有“清除使用统计”语义，不应由展示修复改变。

## 推荐展示契约

### 格式

统一用一个 chain-local identity formatter：

| 输入 | 输出 |
| --- | --- |
| `name="Provider A", id=7` | `Provider A (#7)` |
| `name="  Provider A  ", id=7` | `Provider A (#7)` |
| `name="" / whitespace / "Unknown" / "未知", id=7` | `未知供应商 (#7)` |
| 名称可用、ID无效/非正数 | `Provider A (ID 不可用)` |
| 名称和ID都不可用 | `未知供应商 (ID 不可用)` |

约束：

1. 名称只做 `trim` 和明确 unknown 哨兵归一化，不用 URL 或当前 provider 表“猜”名称。
2. 名称与 ID 同时可用时始终同时显示，不能因名称存在而隐藏 ID。
3. attempt 卡片折叠时也必须可见身份，建议把 `名称 (#ID)` 放进卡片 header；expanded body 可保留端点和诊断字段，独立 `Provider ID` 行可删除或保留为辅助信息。
4. 起始/最终 pill 复用同一 formatter，避免同名 provider 在摘要处再次失去唯一性。
5. `base_url` 保持二级“端点”信息；不拿域名替代名称，也不拿 URL判断同一 provider。
6. 请求级类型继续使用 chain tab 的 `cli_key` badge。没有逐 attempt subtype 快照时，不制造“类型”字段。
7. provider 名称快照存在时，即使当前 provider 已删除，也展示历史名称 + ID；不要标记“已删除”，因为当前链路没有可靠存在性证据。名称不可用时使用原因中立的 `未知供应商`。

### 最小实现边界

推荐只改：

- `src/components/ProviderChainView.tsx`：新增局部纯 formatter，并在 attempt header、起始/最终 pill 使用；
- `src/components/__tests__/ProviderChainView.test.tsx`：补充已知、同名/同 URL、未知/旧日志和兼容接口场景。

不推荐改：Rust `FailoverAttempt`、`RequestLogDetail`、`RequestAttemptLog`、数据库 schema、`provider_chain_json` 或生成绑定。这样不会改变路由、attempt 持久化或凭据边界，也天然保留删除后的历史快照。

## 兼容性风险

1. **旧日志缺名称**：兼容接口会给空名称，formatter 必须回退到 `未知供应商 (#ID)`；不能要求新增非空字段后才渲染。
2. **英文/中文 unknown 哨兵**：至少统一处理空串、全空白、`Unknown`、`未知`。建议英文比较忽略大小写；不要把任意用户自定义名称模糊判为 unknown。
3. **raw JSON损坏**：只要 attempts 兼容接口仍返回数据，就应展示其名称/ID；现有“JSON解析失败”提示保留。两路都不可用时维持空态，不抛渲染异常。
4. **provider 改名**：显示请求时名称快照，而非当前名称；这是审计日志预期。ID让用户能与当前/历史配置精确区分。
5. **同名、同 URL、同类型**：不得按这些属性去重或 join；只按 `provider_id` 区分。不同 ID必须产生不同身份文本。
6. **URL缺失**：OAuth 或 gate skip 可以没有 URL；身份展示不能依赖端点是否存在。
7. **超旧/异常日志缺 ID**：显示 `ID 不可用`，不能生成 `#undefined`、`#null` 或空白；真实 provider attempts 应继续要求正数 ID。
8. **绑定兼容**：本次前端-only 方案没有生成绑定变化。若未来加逐 attempt 类型，新字段必须是 optional/nullable，历史 JSON不会补写。
9. **bridge source 元数据**：`final_provider_source_*` 依赖当前 provider 表，删除后可为空；不要把它当作 decision-chain provider 名称的回退来源。

## 测试点

### 前端聚焦测试

在 `src/components/__tests__/ProviderChainView.test.tsx` 增加或收紧：

1. raw `attempts_json` 单源：已知名称 `Provider A`、ID `7`，折叠 header、起始和最终摘要均可见 `Provider A (#7)`。
2. attempts 兼容接口单源：raw JSON损坏时仍显示 `Provider A (#7)`，保留“尝试 JSON解析失败”。现有基础场景位于 `:6-103`。
3. 两个 provider 同名且 `base_url` 相同，但 ID分别为 7/8：两项分别显示 `Provider A (#7)`、`Provider A (#8)`，不得按名称/URL合并。
4. 空串、空白、`Unknown`、`unknown`、`未知` + 正数 ID：稳定显示 `未知供应商 (#ID)`；现有测试仅覆盖中文 `未知`（`:63-103`）与空串 + ID 0（`:105-136`）。
5. 名称存在但 ID 0/null 的兼容 payload：显示 `名称 (ID 不可用)`，不崩溃。
6. 名称/ID可用但 URL为空：身份仍显示，端点块隐藏。
7. 卡片折叠后身份仍可见；不能把唯一的人类可读信息只放在 expanded body。
8. skipped/circuit/retry 场景保持现有 decision、熔断和计数展示（现有覆盖 `:138-237`、`:239-314`）。

### Rust/跨层防回归测试

尽管推荐实现不改 Rust，任务的“删除后历史日志”验收最好增加一条集成/查询测试：

1. 插入包含 `provider_id`、`provider_name`、`base_url` 的 request log；用 `clear_usage_stats=false` 删除 provider；再次 `request_log_get` 与 `request_attempt_logs_by_trace_id`，断言名称、ID、URL快照仍存在。
2. 同名、同 URL的两个 provider attempts 保持不同 ID，并在 `route_from_attempts` 中形成独立 hops；现有 route 测试入口在 `src-tauri/src/infra/request_logs/queries.rs:718-834`。
3. provider 作为非最终 hop 被删除时，日志详情仍保留该 hop 快照。
4. 明确保留 `clear_usage_stats=true` 删除日志的既有语义，不把它误判为展示回退失败。
5. 若未来触碰 DTO，运行 `pnpm tauri:gen-types`、`pnpm check:generated-bindings` 并核对 `RequestAttemptLog`/`RequestLogDetail` 兼容字段；无 DTO改动时至少运行校验命令证明生成文件无漂移。

### 建议验证命令

```text
pnpm vitest run src/components/__tests__/ProviderChainView.test.tsx
pnpm typecheck
pnpm lint
pnpm check:generated-bindings
```

如增加 Rust 删除后读取测试，再运行对应 focused Cargo test、`pnpm tauri:fmt`，并按跨层规范执行相关 Rust 检查。
