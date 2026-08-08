# 可配置模型路由融合可行性调研

## 1. 调研快照

| 对象 | 快照 | 用途 |
| --- | --- | --- |
| 本次规划 HEAD | `534f878d` | 相对最后产品行为基线只增加 Trellis journal |
| 最后产品行为基线 | `8757d32c` | 已包含账户用量产品提交及重复余额回切抑制修复 |
| 外部原始提交 | `e2a2996265a92baf9d363e8ee8e6370a817f2d62` | 模型路由功能设计来源 |
| 外部最终合入上下文 | PR #14，含后续构建/迁移修复 | 判断原始提交是否可直接使用 |
| 账户用量会话功能提交 | `708d2965`、`ef8892be` | 与模型路由的产品代码交叉基线 |
| 账户用量会话工作树 HEAD | `c2e6b065` | 原会话工作树已干净，但当前 `main` 已继续前进 |
| 账户回切后续修复 | `8757d32c` | 当前 `main` 的最终交叉基线；补充 Blocked 高优先级 failback 抑制 |

`019fda50-f0a6-7650-adb2-6b5d2457ebb5` 未被 `trellis mem` 索引。工具返回当前构建不支持 OpenCode 1.2+ SQLite 会话；这不等于会话没有工作。通过 Orca/Git 只读核对，其工作树位于 `D:/OrcaProjects/aio-coding-hub-fork/account-usage-routing-build`，分支为 `FingerCaster/account-usage-routing-build`，当前干净并指向 `c2e6b065`。产品代码已推进到 `8757d32c`，说明 `708d2965`、`ef8892be`、任务归档及后续回切抑制修复都已成为直接基线；本次规划 HEAD `534f878d` 在其上只新增 Trellis journal。主工作树还有与本任务无关的未提交和未跟踪内容，规划阶段不修改，实现前按实际 dirty paths 重新审计并保留。

## 2. 目标能力还原

目标提交引入 `ModelRoutingPolicy`：

```text
enabled: boolean
rules: [{ source_model, target_model?, reasoning_effort? }]
```

规则只以客户端原始模型做精确、区分大小写的一次匹配；没有级联、通配符或默认规则，限制规则数量和字段长度，拒绝重复 source、空输出和控制字符。`reasoning_effort` 是有界文本而非固定枚举，以同时承载 Claude/Responses/Chat 的文本等级和 Gemini 可解析的数值 budget。Provider 级策略使用三态语义：字段缺失/NULL 继承全局；存在且 enabled 替换全局；存在但 disabled 抑制全局。更新请求带有 `*_override_specified`，以便“未提交该字段”的补丁不清除已有覆盖。

模型路由只作用于 POST 的真实模型推理请求，排除辅助接口和 managed `aio/` 别名；目标提交覆盖 Claude Messages、Codex Responses 与 compact、Grok Chat/Responses、Gemini generateContent。路由在协议桥接、内建映射、清理和 `RequestBeforeSend` 插件之后应用，拥有最终上游模型和 effort。不同协议的 effort 写入位置不同，修改压缩请求体时还必须同步处理编码头和请求体状态。

日志保留原始 requested model，使用 Provider 作用域的 `configured_model_route` 标记记录 source/effective model、effort、策略来源、Provider 和计价 CLI。目标模型用于成本查找；目标价格缺失时成本保持未知，不能回退到 source model。响应模型比较也必须以实际配置后的 effort 为依据。

外部实现有一项已决定调整的行为：规则已经匹配，但 body 解析、模型定位、序列化或 effort 写入失败时，`apply` 只返回 `applied=false`，发送链仍会继续使用未完整改写的请求。本仓库改为 Provider 级失败关闭：不向当前 Provider 发送，记录稳定、可审计的发送前路由失败，再继续同一请求的下一候选 Provider 并按其策略重新解析；全部候选失败才返回客户端错误。由于没有上游调用，该失败不改变健康、熔断、余额门控或 transport retry 状态。这是本任务对外部实现的有意增强。

外部契约还区分“有效规则已匹配但应用失败”和“持久化策略本身无法可信解析”：Provider override JSON 损坏时会转成显式 disabled policy，避免意外继承全局；全局策略经设置修复清洗，失效规则被删除，空 policy 被禁用。用户已确认沿用这一可用性优先行为：记录有界诊断并继续未改写请求；只有可信规则匹配后的应用失败才进入 Provider 级失败关闭。

## 3. 当前主线与会话分支交叉

### 3.1 可复用的当前结构

- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs` 已在每个 Provider attempt 前完成协议桥接和 Provider 级准备，适合在这里解析 global policy + provider override，但不能在这里发送或等待远端数据。
- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs` 已把 `RequestBeforeSend` 插件和 `GatewayRequestBody` 作为发送前边界；模型路由应在插件成功后改写同一 body state，再构造最终 URL，并保留当前 dispatch ownership/transport boundary。
- `src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs` 已统一从路径、query 和 JSON body 推导原始模型，并识别 Codex compact；可复用抽取结果，但不能直接把它当作目标提交的完整请求分类器。
- `src-tauri/src/infra/request_logs.rs` 的 `effective_cost_basis` 是统一成本入口，适合把 configured route marker 放在 managed/CX2CC 之前做 Provider 作用域优先选择。
- 当前 `src-tauri/src/domain/providers/types.rs` 的 `ModelMapping` 是协议桥接/Claude 模型映射，不等价于新的跨协议最终模型路由，二者必须保留独立所有权。

### 3.2 会话分支已占用的契约

会话功能把账户用量作为共同发送前 gate 的第一层：新鲜且可信的 zero-balance/expired 生成普通 `skipped` attempt；查询失败、过期快照和配置不完整 fail-open；恢复 epoch 只给现有 failback planner 提供资格，不改变 Provider 排序或 Session 绑定。`8757d32c` 又把账户投影解析结果分为非零 recovery epoch 或 `blocked_provider_ids`：后者只抑制有效绑定 fallback Session 中已知 Blocked 的高优先级 failback 目标、观察和 reservation；普通、未绑定及 forced 候选仍保留，并继续通过共同 gate 留下审计记录。全 Blocked 的压缩前缀不得创建 reservation，恢复后才重新启用目标。

模型路由不能绕开或重复触发账户 gate。有效规则应用失败发生在 gate 通过之后、transport commit 之前，必须放弃当前 Provider 的 pending dispatch ownership 并继续下一候选，但不能把 Provider 写入 `blocked_provider_ids`、推进 recovery epoch、创建健康/熔断结果或进入 transport retry。这样才能避免重新引入已由 `8757d32c` 修复的重复余额回切问题。

已完成提交的主要交叉文件：

| 层 | 会话已改动 | 模型路由融合要求 |
| --- | --- | --- |
| Provider 契约 | `domain/providers/types.rs`、`queries.rs`、`share.rs` | 在现有 account-usage 字段旁新增 policy override；所有 SQL、指定字段语义、复制/分享/导入导出一起更新 |
| Provider 准备 | `failover_loop/prepare/provider_iterator.rs` | 同一 `PreparedProvider` 同时携带 account-usage target 和 configured route；路由解析不得改变 gate/候选顺序 |
| Gate/故障转移 | `prepare/provider_checks.rs`、`provider_selection*`、`provider_resolution.rs`、`probe_planner.rs`、`routes.rs` | 余额 skip 仍是第一拒绝所有者；模型路由失败不得进入 `blocked_provider_ids`/reservation/recovery，并覆盖 skip 后 fallback、恢复回切和 forced/managed 路径 |
| 配置与绑定 | `infra/config_migrate/*`、`generated/bindings.ts` | route policy 是非敏感配置，可随当前 bundle/share 规则保留；绑定需一次性重新生成，不能覆盖会话新增字段 |
| Provider UI | `ProviderAccountUsageSection.tsx`、`ProviderEditorDialog.tsx`、`useProviderEditorForm.ts` | 在现有账户用量编辑状态中加入路由 override，防止保存/重置 custom test 时丢失路由策略 |
| 其他 | `app/provider_account_usage_runtime.rs`、Session/failback | 只读取 route policy，不把模型路由加入余额 runtime 或 recovery epoch |

账户用量产品提交没有修改 `attempt_executor.rs`、`infra/settings/defaults.rs`、`infra/request_logs.rs`；这些仍是模型路由的新增主落点，但实现必须以 `8757d32c` 的 Provider 解析、planner、dispatch ownership 和 session 生命周期为准。

### 3.3 Codex“转译”将作为完整功能删除

`src/pages/providers/ProviderEditorDialog.tsx` 中 Codex 的“转译”标签页渲染 `CodexBridgeSection`，当前同时拥有两层能力：

1. **协议桥接**：引用另一个普通 Provider 的 Base URL/凭据，把 Codex Responses 请求和响应转换为 OpenAI Responses 或 Chat Completions。后端由 `bridge_type`、`bridge_preparation.rs`、protocol bridge registry、非流式/流式 response conversion 等链路实现。
2. **模型映射**：`model_mapping.default_model` 和 `model_mapping.exact` 在协议桥接阶段改写模型名。新的可配置模型路由在 bridge/plugin 后拥有最终 wire model，可以替代精确映射；删除 default mapping 后，未列出的模型将保持原名，因为新路由没有 wildcard/default 规则。

删除整个“转译”会让已有 `codex_to_openai_chat`、`codex_to_openai_responses` 和 legacy `codex_to_anthropic_messages` Provider 失去运行时支持，并涉及现有记录的清理迁移；模型路由本身不能替代请求与流式响应协议转换。初步扫描至少有 15 个文件直接引用这些 Codex bridge type，另有 Provider CRUD、分享/复制、配置导入导出和通用 bridge 字段间接依赖。

用户已经明确决定删除完整 Codex 协议转译，而不只删除模型映射。因此后续实现必须移除“转译”标签页、Codex bridge 类型、来源 Provider/端点选择、模型映射、请求与响应转换、流式转换和对应配置分支；普通 Codex Provider继续保留，Claude CX2CC 继续保留。该删除是可独立验收的第二交付物，实施计划应与模型路由分阶段组织，避免把 Codex bridge 专属字段误删到仍由 CX2CC 使用的通用 bridge 基础设施。

现有转译 Provider 不能可靠地自动转成普通 Codex Provider：它们借用来源 Provider 的 Base URL 和凭据，且可能依赖协议转换，复制来源凭据既改变安全边界也不能保证协议可用。仅移除 UI/运行时则会留下不可编辑、不可运行的幽灵配置。用户已确认由幂等数据库迁移删除这三类 Codex bridge Provider，利用现有外键清理其活动路由/扩展引用，同时保留历史请求日志；迁移不得复制来源凭据，也不得影响普通 Codex Provider、来源 Provider 或 Claude CX2CC。

完整配置导入需要单独处理旧备份。当前 `prepare_config_import` 在任何数据库清理前执行纯预检，随后 `clear_existing_config_data` 会在事务内清空 Provider 等配置并整包重建；失败会回滚。`ConfigImportResult` 只有各对象导入数量，没有 skipped/warnings 契约。用户已确认在预检阶段发现三类已删除 bridge 时整包拒绝，给出稳定的“不再支持 Codex 转译”错误并保持当前配置不变；不得跳过后继续，也不得返回会掩盖配置丢失的成功结果。

兼容边界需要区分活动产品契约与历史格式壳：

- `model_mapping_json` 物理列可暂时保留以避免 SQLite 重建，但删除迁移应把存活 Provider 的值归一为 `{}`；活动 Provider DTO、编辑器、运行时、绑定和新分享格式不再暴露或解释它。
- Provider 分享 v1/v2 使用 `deny_unknown_fields` 严格解析，不能原地静默增加路由字段。模型路由应新增严格 v3；v1/v2 继续兼容读取，丢弃 legacy `model_mapping`，遇到已删除 Codex bridge 则返回稳定错误。
- 完整配置包沿用当前 v4 能力门槛机制，无需只为路由再升主版本；v4 可携带全局/Provider policy，v1-v3 中即使手工注入同名字段也必须在预检清洗掉。删除子任务可保留 bundle 的惰性 `model_mapping_json` 形状并固定为 `{}`，但不得恢复业务语义。

数据库外键核对表明 Provider pool/default route/sort mode、Provider model、extension 和 credential 引用可随 Provider 删除清理，`providers.source_provider_id` 会置 NULL；`request_logs` 不以 Provider 外键持有历史，`attempts_json` 已保存 Provider id/name 快照，因此可以保留日志。`codex_managed_profiles.model_uuid` 对 `provider_models` 是 `ON DELETE RESTRICT`，而受支持的创建路径只接受直接 Codex 模型；迁移仍应在删除前验证这一不变量。若发现损坏数据让 managed profile 引用了待删 bridge model，事务以稳定恢复错误整体失败，不自动删除本机 profile 或文件。

## 4. 直接移植风险

原始提交不是当前主线的增量补丁。相对当前主线的三方试应用已经出现 28 个冲突和 6 个当前不存在的 Observer/TUI 文件；目标代码还引用当前仓库没有的 `gateway::observation`、usage ledger/view 和 Observer snapshot。账户用量会话再加入后，Provider 查询、分享和 UI 冲突面会进一步扩大。因此推荐“提取契约并重写适配”，不推荐 cherry-pick、整提交 merge 或先移植 Observer/TUI。

目标提交的后续修复也提供了两个必须保留的教训：原生构建需要额外修正 settings 导出/Observer snapshot；`v43_to_v44` 迁移必须使用幂等列检查。当前 `8757d32c` 的 settings schema 仍为 56，SQLite 最新版本仍为 43，不能照抄目标版本号和 usage-events view 刷新逻辑；本计划暂分配删除任务 SQLite 43->44、路由任务 SQLite 44->45 和 settings 56->57，并在每个子任务启动前重新确认。

## 5. 推荐 MVP 与依赖顺序

### 前置条件

1. 账户用量产品代码及回切抑制修复已进入当前 `main` 的 `8757d32c`；实现前重新记录实际 HEAD、schema 和 dirty paths，并保留主工作树中不属于本任务的全部改动。
2. 重新运行 `trellis-before-dev`，读取会话新增/更新的 backend 与 cross-layer 规范。
3. 对 `types.rs`、`queries.rs`、`share.rs`、`provider_resolution.rs`、`probe_planner.rs`、`ProviderEditorDialog.tsx` 和 `bindings.ts` 做一次最新差异复查。

### 建议阶段

1. **删除 Codex 转译**：通过 SQLite 43->44 幂等迁移删除既有 Codex bridge Provider、清理活动引用并保留历史日志，再移除 UI、bridge 类型、配置分支和运行时转换；严格保留普通 Codex Provider、来源 Provider、Claude CX2CC 及其共用基础设施。
2. **策略契约**：settings schema 56->57；实现 policy/rule sanitizer、三态 override 和严格请求分类器。
3. **Provider 持久化**：从删除子任务的 SQLite 44 基线通过 44->45 加入 nullable policy JSON、specified patch 语义、复制、严格 Provider 分享 v3、完整配置 v4 能力门槛和已确认的坏数据处理行为。
4. **网关发送链**：在 Provider 准备阶段解析路由，在插件之后基于克隆原子改写最终 path/query/body/effort，成功后才构造 URL；保留原始模型、压缩体处理、managed `aio/` 排除和现有故障转移所有权。
5. **观测与成本**：增加 Provider 作用域 marker、response model mapping basis、effective cost basis 和成本回填；未知 target price 不回退 source。
6. **桌面 UI/绑定**：全局 GeneralTab、Provider override 编辑器、请求日志徽标和生成绑定；已确定 Observer/TUI 延后为独立任务。
7. **联合验证**：在账户门控基线之上补真实 gateway E2E，而不是只依赖路由模块单测。

## 6. 最小联合测试矩阵

- 策略：disabled/inherit/replace/suppress、精确大小写、重复/超长/非法规则、旧配置缺字段。
- 协议：Claude、Codex Responses、Codex compact、Grok chat/responses、Gemini generateContent；模型字段位于 body/path/query 的各类形态。
- 顺序：内建映射与插件先执行，configured route 最后生效；插件改模型后仍以原始请求模型匹配；managed `aio/` 和辅助/非 POST 请求不路由。
- Provider：不同 Provider 使用不同 policy；余额 blocked skip 后继续 fallback；恢复 epoch、`blocked_provider_ids`、reservation 和 route rewrite 不改变各自所有权；forced/provider probe/模型发现路径不旁路分类器。
- 请求体：gzip/zstd/brotli 解码后改写，正确移除 content-encoding；覆盖“当前 Provider 无法完整改写时不发送、继续下一 Provider、全部候选失败才返回稳定错误”。
- 计价/日志：原始 requested model 不变，最终 target 价格优先；target 无价格时成本未知；marker 必须绑定最终 Provider，不能被下一次 attempt 覆盖。
- 持久化/UI：删除子任务 SQLite 43->44、路由子任务 SQLite 44->45、settings 56->57、Provider 分享 v1/v2 兼容和 v3 round-trip、duplicate/bundle v4 策略、表单状态重置与生成绑定一致性。
- Codex 转译删除：三类 Codex bridge 旧记录迁移、活动路由引用清理、历史日志保留、旧完整配置原子拒绝、UI 无转译入口、运行时不再注册 Codex bridge；普通 Codex 与 Claude CX2CC 回归通过。

目标提交只有路由单测、成本/迁移/UI 局部测试，缺少上述当前热路径与账户门控组合的端到端覆盖，这是实现前必须补齐的主要测试缺口。

## 7. 复查与暂停条件

进入实现前必须重新执行：

```text
git -C D:/OrcaProjects/aio-coding-hub-fork/account-usage-routing-build status --short --branch
git -C D:/OrcaProjects/aio-coding-hub-fork/account-usage-routing-build log -5 --oneline
git rev-parse HEAD
git diff --name-only c2e6b065..HEAD
git status --short
```

若实际 HEAD、schema 版本或请求链路所有权发生变化，先更新本调研的冲突矩阵并重新评审；主工作树的所有未提交修改必须保留并协调。当前调研阶段不在两个工作树中修改产品源码。

## 8. 结论

功能可融合，但应视为基于当前 `8757d32c` 账户用量与回切抑制基线的跨层移植，而非外部提交合并。核心路由契约可复用，网关热路径、持久化、UI 和测试需要按现有架构重写；同时，完整删除 Codex Provider 转译已成为独立验收项，必须与仍保留的 Claude CX2CC 清晰隔离。本次 MVP 已明确排除 Observer/TUI、通知和发布变更。风险可控的前提是按子任务顺序实施并在每次启动实现前复查活动工作树与 schema。
