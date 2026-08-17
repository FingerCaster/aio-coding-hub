# Research: Codex 模型上下文规则前端编辑器与查询契约

- Query: 为 Codex 自定义模型上下文规则确定前端整集合草稿编辑器、canonical settings 确认与失败回读、模型建议/基础值查询、GPT-5.6 372K 预设、生成绑定/MSW/fixture 影响面和测试矩阵。
- Scope: mixed（仓库源码、任务/Spec、OpenAI 官方 Codex 文档及本机 Codex CLI 0.147.0 前序实测）
- Date: 2026-08-17

## Findings

### 1. 结论摘要

1. 用“摘要区 + 管理规则 Dialog”替换现有单个 372K Switch。Dialog 持有完整规则集合草稿，添加、编辑、启停、删除和 GPT-5.6 预设都只改草稿；只有“应用更改”发送一次整集合 SET，取消发送零请求。
2. canonical 规则只读暴露在 `SettingsView.codex_model_context_rules`，写入只走 `settings_codex_model_context_rules_set(rules) -> SettingsView`。普通 `SettingsUpdate`、`SettingsPatch`、设置导入导出都不得获得该字段。
3. settings GET 是编辑授权边界：`isError` 即使仍有 stale cache 也必须让编辑器只读并提供重试。模型候选/基础值只是辅助信息；其加载、degraded 或失败不得阻止手工模型 ID 和规则提交。
4. 不建议给现有 `CodexModelCapability` 直接补所谓基础值。当前 catalog command 会先 `sync_current_locked`，再从 active App Server `model/list` 读取派生目录；这既可能写文件，也可能把规则后的值误称为 base。应采用后端研究已提出的独立、只读 `cli_manager_codex_model_context_candidates_get`，从 original base source 投影候选和 base window。
5. 保存成功的定义不是 Promise resolve，而是返回的 canonical 规则与规范化提交逐字段完全相等。reject、null、空响应或确认不一致都不能 toast 成功；必须强制 settings GET，成功后丢弃失败草稿并恢复 canonical，GET 再失败则进入只读保护。
6. `CodexTab.tsx` 已超过 2,600 行，应拆出 `CodexModelContextRulesSection.tsx`；规则规范化、校验、canonical key、比较、基础值展示与预设放在无 React 依赖的纯模块，避免把新状态机继续堆进 Tab。
7. Codex home 禁用条件从旧布尔改为 `canonicalRules.some(rule => rule.enabled)`。仅有 disabled rules 时允许换 home；规则 SET pending、structured/raw config pending 期间仍互相阻塞，保持现有单写入面。
8. GPT-5.6 预设只在前端定义三个精确 ID、`372000`、`enabled=true`。已有规范化后同 ID 的任意 enabled/disabled 规则都跳过，不覆盖、不制造重复、不给后端保留 family 特例，也不自动提交。

### 2. 推荐的生成 IPC 与读模型

规则类型由 Rust/Specta 生成，前端不要另写结构镜像：

```typescript
type CodexModelContextRule = {
  model_id: string;
  context_window: number;
  enabled: boolean;
};

commands.settingsCodexModelContextRulesSet(
  rules: CodexModelContextRule[]
): Promise<Result<SettingsView, string>>;
```

专属 service 建议为 `src/services/settings/settingsCodexModelContextRules.ts`，只负责运行时输入检查、调用 generated command 和统一 IPC error semantics。纯规则逻辑建议为 `src/services/settings/codexModelContextRules.ts`，供 service、页面 handler 和组件测试共用；核心 API 可保持很小：

```typescript
normalizeCodexModelContextRules(draft): CodexModelContextRule[]
codexModelContextRulesKey(rules): string
codexModelContextRulesEqual(left, right): boolean
addGpt56Preset(draft): { draft; added: number; skipped: string[] }
```

`normalize...` 必须剥离仅供 React 使用的行 ID，trim model ID、校验所有行、按 `model_id` 的 UTF-8 字节顺序排序后再提交。canonical 比较按三个字段比较规范化数组，不比较对象引用，也不对未规范化输入直接 `JSON.stringify`。

候选/基础值建议新增独立 DTO，而不是污染现有推理能力 DTO：

```typescript
type CodexModelContextCandidate = {
  model_id: string;
  display_name: string;
  hidden: boolean;
  base_context_window: number | null;
  base_max_context_window: number | null;
};

type CodexModelContextCandidatesState = {
  status: CodexModelCatalogStatus;
  issue: CodexModelCatalogIssue | null;
  snapshot: CodexModelCatalogSnapshot;
  models: CodexModelContextCandidate[];
};
```

`hidden` 很重要：R12 要求搜索建议只列当前可见模型，但 canonical 规则和 GPT-5.6 预设仍需对 hidden base entry 显示基础值。UI 用全量 candidate map 做精确比较，只对 datalist/filter suggestion 排除 `hidden`。未知 `visibility` 应由后端保守映射为 hidden，而不是由前端猜测。

candidate query key 继续隔离 `configPath + executablePath + cliVersion`，另建 `codexModelContextCandidatesAll()` 家族。不要用 `keepPreviousData` 把旧 home 的基础值展示到新 home；candidate payload snapshot 与当前 Codex config/info 不一致时也不可用于比较。查询可沿用 catalog 的 `retry:false`、显式 Retry，但失败只降级辅助 UI。

### 3. Query 与页面 handler

`useSettingsCodexModelContextRulesSetMutation` 应沿用旧 372K mutation 的 cache 行为：

- `mutationFn(rules)` 调专属 service，一次发送完整规范化集合。
- `onSuccess(updated)` 仅在非 null 时把完整 `SettingsView` 写入 `settingsKeys.get()`。
- `onSettled` invalidate settings、Codex config、raw TOML、CLI proxy status；reset active catalog query 让自停错误可重试。
- 若采用独立 candidate GET，也 reset/invalidate candidate family，但候选刷新不得延迟规则 SET 的成功确认。
- 不在 mutation hook 中做乐观 rules cache 更新。

页面 data model 的 `persistCodexModelContextRules` 应串行防重，并与 structured config、raw TOML、Codex home 写入互斥。推荐让 handler 返回明确结果，而不是继续用只表达开关成功/失败的 boolean：

```typescript
type RulesSaveResult =
  | { status: "confirmed"; settings: AppSettings }
  | { status: "reverted"; settings: AppSettings }
  | { status: "blocked" };
```

处理顺序：

1. settings read protection 或任一互斥 writer pending 时不调用 mutation。
2. 规范化草稿并 `mutateAsync`。
3. response 非空且 response rules 与提交相等时返回 `confirmed`，此时才 toast 成功。
4. response null/不一致或 reject 时，记录原始错误但不成功提示，立即 `await settingsQuery.refetch()`。
5. refetch 成功且无 `isError` 时返回 `reverted`，Dialog 用该 canonical settings 重建草稿；提示保存失败且已恢复已确认状态。
6. refetch 失败时返回 `blocked`；保留 stale canonical 仅供显示，所有编辑/保存禁用并显示 settings 只读错误。

普通 `onSettled` invalidate 不能替代第 4 步的显式、可等待回读；否则 Dialog 会在 cache refetch 尚未完成时继续保留伪草稿。若 data model 不采用 discriminated result，至少要分别暴露 `persist` 与 `rereadSettings`，让 Dialog 能等待回读并显式 reset。

### 4. UI 结构与交互

摘要区放在当前 372K section 位置，展示“启用 X / 共 Y 条”、仅新启动 Codex 生效提示、基础能力不因目录声明而增加的简短风险文案，以及“管理规则”命令按钮。不要把三条预设做成常驻三个开关。

Dialog 每行提供：

- `Switch`：独立 `enabled`。
- 模型 ID `Input + datalist`：可搜索当前可见候选，同时允许手工精确输入。
- token 文本输入：`inputMode="numeric"`，不用 `type=number` 接受浏览器指数/符号语法。
- 基础/目标对比与仅在提高时出现的非阻塞 warning。
- Lucide `Trash2` icon button 删除，并带明确 aria-label/tooltip。

Dialog toolbar 提供“新增规则”和“添加 GPT-5.6 372K 预设”；footer 提供“取消”和“应用更改”。规则没有 drag handle、priority 或排序 UI。React 行使用不持久化的稳定 ID，不能用 array index 作为 key；可借鉴定价别名编辑器的递增 row ID。

保存期间必须禁用每行输入/Switch/删除、添加/预设、取消/应用按钮，并拦截 Escape、遮罩点击和 Dialog close。重复点击只产生一个 IPC。Cancel 把 draft 重置为当前 canonical 后关闭，不发 mutation。

canonical rules 在 Dialog 打开期间发生变化时，以 `codexModelContextRulesKey` 检测内容变化，不依赖数组对象引用。若不是本次 save confirmation，应丢弃 stale draft并提示“规则已更新”；不能把旧 draft 覆盖新 canonical winner。

### 5. 前端验证与预设

规则集合验证与后端一致，但后端仍是最终权威：

| 字段 | 前端规则 |
| --- | --- |
| 集合 | 最多 128 条；第 128 条可保存，第 129 条拒绝 |
| `model_id` | `trim()` 后非空；`TextEncoder` UTF-8 为 1..256 bytes；不含 `\p{Cc}` 控制字符 |
| reserved | 区分大小写拒绝 literal `aio/` 前缀，不扩大为 `AIO/` |
| duplicate | 在 enabled + disabled 全集合中，对 trim 后 ID 做区分大小写精确去重 |
| token lexical | trim 后必须匹配 `^[0-9]+$`，拒绝符号、指数、小数、空值 |
| token numeric | `Number.isSafeInteger`，并复用 `MODEL_CONTEXT_WINDOW_MIN_TOKENS..MAX`（1,024..10,000,000） |
| disabled row | 仍执行 ID、重复、reserved 和 token 全部静态验证 |

不要在规则模块复制 token 常量。直接导入 `src/services/providers/providerModels.ts:12-13`，并在 cross-layer test 中断言它们等于现有 `CX2CC_CONTEXT_WINDOW_MIN/MAX` 及 Rust 常量，避免第三套边界悄悄漂移。

预设常量只在前端纯模块中出现：

```typescript
[
  { model_id: "gpt-5.6-sol", context_window: 372000, enabled: true },
  { model_id: "gpt-5.6-terra", context_window: 372000, enabled: true },
  { model_id: "gpt-5.6-luna", context_window: 372000, enabled: true },
]
```

加入预设前先规范化现有行 ID；任一精确 ID 已存在就跳过该项，不修改其 token 或 enabled 状态。反馈应给出 added/skipped 数量或具体 ID。加入后仍需普通校验和“应用更改”；接近 128 条时只加入有容量的项会制造半个预设，建议整体预检容量，不足则零新增并明确提示。

### 6. 基础值展示语义

candidate 用 `model_id` 精确、区分大小写匹配；不按 display name、前缀或 family 匹配。

- 两个 base 字段相同：`基础 272,000 -> 目标 372,000`。
- 两个 base 字段不同：分别显示 `context 272,000 / max 300,000 -> 目标 372,000`，不能折叠成一个基础值。
- 只有一个已知：显示已知字段，并将另一个标为不可用。
- 两者未知、candidate GET 失败或 snapshot 不匹配：显示“基础值不可用”，仍允许提交。
- enabled rule 的目标只要高于任一已知 base 字段，就显示非阻塞风险提示；disabled rule 不显示激活风险。
- canonical rule 在 candidate 中缺失时仍完整保留。标记“当前基础目录不可用”；disabled rule 可编辑/删除，enabled rule 也必须允许用户禁用或删除以修复 catalog upgrade 后的失配。

风险文案只说明“目录声明不会增加模型/provider 的真实能力”，不得宣称设置扩大了实际能力，也不得推导或改写 `effective_context_window_percent`、auto compact threshold。

### 7. 状态矩阵

| Settings | Candidates | UI/提交行为 |
| --- | --- | --- |
| loading / null | 任意 | 摘要 loading；不开放编辑/保存 |
| error，含 stale data | 任意 | stale 只读显示；QueryErrorCard + Retry；零写请求 |
| success | loading | canonical 可编辑；建议 loading；手工 ID 可用 |
| success | error/degraded/unavailable | canonical 可编辑；建议/基础比较降级；Retry；仍可提交 |
| success | ready | 可见候选建议 + 全量精确 base map |
| saving | 任意 | Dialog、home、structured/raw config 相关写入口全部禁用 |
| SET confirmed exact | 任意 | cache/canonical 重建、成功 toast、关闭 Dialog |
| SET reject/null/mismatch + reread success | 任意 | 失败 toast、丢弃草稿、恢复 reread canonical；不成功 toast |
| SET reject/null/mismatch + reread failure | 任意 | stale 只读、Dialog 保持可见错误或关闭后摘要只读；Retry 后才能再编辑 |

### 8. 生成绑定与 Settings ownership 断言

`src/generated/__tests__/bindings.contract.test.ts` 应增加一个专属测试，同时断言新面存在和旧面消失：

```typescript
expect(extractTypeBody(bindingsSource, "SettingsView"))
  .toContain("codex_model_context_rules: CodexModelContextRule[]");
expect(extractTypeBody(bindingsSource, "SettingsUpdate"))
  .not.toContain("codexModelContextRules");
expect(extractTypeBody(bindingsSource, "SettingsPatch"))
  .not.toContain("codexModelContextRules");

const command = extractGeneratedCommand(bindingsSource, "settingsCodexModelContextRulesSet");
expect(command).toMatch(/rules: CodexModelContextRule\[\]/);
expect(command).toContain(
  'TAURI_INVOKE("settings_codex_model_context_rules_set", { rules })'
);
expect(bindingsSource).not.toContain("settings_codex_gpt56_372k_context_set");
```

`src/services/settings/settings.ts` 要同步替换四个 compile-time ownership 点：`AppSettingsPatch` omit、ordinary input key `AssertNever`、dedicated view key、`SettingsViewKeysHandledOutsideCreateInput`。现有 `__AssertNoUnhandledSettingsViewKeys` / `__AssertNoStaleHandledSettingsViewKeys` 会在漏配时让 typecheck 失败，应保留而不是用宽泛 cast 绕过。

### 9. MSW 与 fixture 影响面

- `src/test/fixtures/settings.ts:9-88`：required `AppSettings` factory 把旧 boolean 改为 `codex_model_context_rules: []`；所有使用 factory 的页面/组件测试自动获得新字段。
- `src/test/msw/state.ts:27-105`：backend-like `DEFAULT_SETTINGS` 同样改为空规则数组，schema 由后端实现决定更新。
- `src/test/msw/handlers.ts:130-135`：删除旧 372K endpoint，新增 `settings_codex_model_context_rules_set`，读取 `{ rules }` 并把完整 rules 写入 canonical MSW state 后返回完整 SettingsView。
- MSW handler 最好使用同一测试 normalizer 或至少返回排序后的 canonical 数组，才能覆盖“response canonical 与提交确认”；不要只原样 echo 而掩盖前端比较错误。

推荐独立 candidate API 后，现有 `CodexModelCapability` 不增加 required 字段，因此这些非空 fixture 不需要机械补字段：

- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx:112-142`
- `src/components/cli-manager/tabs/__tests__/codexModelCapabilities.test.ts:25-42`
- `src/components/cli-manager/tabs/__tests__/useCodexModelMigration.test.ts:66-95`

应为新 candidate service/query/section 测试建立自己的 candidate factory，显式覆盖 equal、different、one-null、both-null、hidden 和 missing。

若实现者仍选择给 `CodexModelCapability` 增加 required nullable `base_*` 字段，上述三个非空 fixture factory 都必须补字段；`src-tauri/src/infra/codex_model_catalog/protocol.rs:276-284` 的唯一 Rust constructor 也必须补字段。`src/query/__tests__/cliManager.test.tsx:233-244` 和 `src/services/cli/__tests__/cliManager.service.test.ts:115-128` 只有 `models: []`，不会因模型项 required 字段直接失败。该方案仍不能解决 active/derived 值被误当 base 及 GET 有写入副作用的问题，故不推荐。

### 10. 前端测试矩阵

`codexModelContextRules.test.ts`（纯函数）：

- trim/sort/canonical equality；输入排列不同但 key 相同。
- 0/128/129 条；重复覆盖 enabled/disabled、trim 后重复、case-sensitive 非重复。
- UTF-8 256/257 bytes、多字节边界、控制字符、空 ID、literal `aio/`。
- token 1024/10,000,000 成功；边界外、空、符号、指数、小数、非安全整数失败。
- GPT-5.6 三项精确值；部分/全部已存在时跳过不覆盖；容量不足零新增；不自动调用写入。
- equal/different/null/missing base 展示和“提高任一已知字段”warning 判定。

`CodexModelContextRulesSection.test.tsx`（组件）：

- 从 canonical 打开 Dialog；添加、编辑、启停、删除均只改草稿。
- datalist 只含可见去重候选，手工 ID 可输入；hidden/missing canonical 仍显示。
- Cancel 重置且零 mutation；Apply 只发一次规范化整集合。
- pending 时所有控件、Escape、outside click/close、重复 submit 被阻止。
- validation 聚焦对应行并阻止 mutation；disabled row 也验证。
- candidate loading/error/degraded 只降级建议；settings error + stale data 完全只读并可 Retry。
- canonical exact success 才关闭/toast；null/mismatch/reject 强制 reread并重置；reread failure 转只读。
- 新 canonical revision 到达时不提交旧草稿。
- 新会话生效提示与能力风险提示可访问。

现有测试改造：

- `src/services/settings/__tests__/settingsCodexGpt56372kContext.service.test.ts` 改为 rules service 测试：generated args、null/error、运行时非数组/非法 rule 拒绝。
- `src/query/__tests__/settings.test.tsx:604-660` 改为整集合 cache/invalidations/reset 测试，并加 response null 不覆盖 cache。
- `src/query/__tests__/cliManager.test.tsx:654-883` 为 candidate snapshot key、inactive failed refresh、in-flight 去重补同级覆盖；若 candidate query更简单，至少覆盖 no retry、旧 snapshot 不串用。
- `src/services/cli/__tests__/cliManager.service.test.ts:264-350` 增加 candidate generated command 映射。
- `src/pages/__tests__/CliManagerPage.test.tsx:1432-1539` 把 boolean handler 测试改为整集合 exact confirmation、reject/null/mismatch 后 settings refetch、refetch failure、互斥 writer 和 home guard。
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx:626-713` 删除旧 switch 断言，改为 section wiring、enabled-rule home block、disabled-only home allowed、rules pending 锁定。
- `src/generated/__tests__/bindings.contract.test.ts` 增加第 8 节 ownership contract。
- `src/constants/__tests__/crossLayerContracts.test.ts:221-228` 同时锁定 Provider、CX2CC 与 Rust token 边界。
- `src/test/msw` 增加真实 invoke -> handler -> canonical SettingsView 的整集合 smoke，避免只在 mock mutation 层通过。

## Files Found

- `.trellis/tasks/08-17-codex-custom-model-context-rules/prd.md`：R8、R12-R23 定义编辑、整集合原子提交、建议降级、禁用态、验证、基础比较和预设。
- `.trellis/tasks/08-17-codex-custom-model-context-rules/research/backend-rules-contract.md`：后端 schema 65、专属 SET、只读 candidate API、base source、错误码和事务建议；前端接口应与其对齐。
- `src/components/cli-manager/tabs/CodexTab.tsx`：现有 372K Switch、home guard、模型 datalist 和 catalog suggestion 过滤。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts`：settings read protection、旧专属 mutation、确认 toast、Codex writer 互斥与 Tab props 聚合。
- `src/components/settings/ModelPriceAliasesDialog.tsx`：最接近的稳定 row ID、本地整集合草稿、严格读阻塞和保存期间 controlled Dialog 模式。
- `src/components/settings/__tests__/ModelPriceAliasesDialog.test.tsx`：stale read error 阻塞、Retry、pending close 和增删改保存测试先例。
- `src/services/settings/settings.ts`：generated settings wrapper、普通 owner 排除和 `AssertNever` 完备性守卫。
- `src/services/settings/settingsCodexGpt56372kContext.ts`：待替换的旧专属 IPC adapter。
- `src/query/settings.ts`：旧专属 mutation 的 canonical cache 与 settings/config/TOML/catalog/proxy 刷新模式。
- `src/query/cliManager.ts` / `src/query/keys.ts`：catalog snapshot key、no retry、自停错误和显式 refresh。
- `src/services/cli/cliManager.ts`：generated Codex catalog DTO 的薄 adapter/export。
- `src-tauri/src/commands/cli_manager.rs`：当前 catalog GET 先 reconcile 再读 App Server，不能作为只读 base query。
- `src-tauri/src/infra/codex_model_catalog/mod.rs` / `protocol.rs`：现有 `CodexModelCapability` 与 `model/list` parser，没有 context 字段。
- `src-tauri/src/infra/codex_model_catalog/managed.rs`：original user/bundled base source、完整 JSON validator、exact slug 和两个 context 字段的权威读取位置。
- `src/generated/bindings.ts`：生成命令、SettingsView 和 Codex catalog DTO 当前形状；不可手改为真源。
- `src/generated/__tests__/bindings.contract.test.ts`：generated ownership/command contract 的现有测试入口。
- `src/services/providers/providerModels.ts`：前端 Provider context 边界 `1_024..=10_000_000`。
- `src/constants/__tests__/crossLayerContracts.test.ts`：现有 CX2CC -> Rust context 常量漂移测试。
- `src/test/fixtures/settings.ts` / `src/test/msw/state.ts` / `src/test/msw/handlers.ts`：required settings fixture、MSW canonical state 和旧专属 endpoint。
- `src/components/cli-manager/tabs/__tests__/CodexTab.test.tsx`、`codexModelCapabilities.test.ts`、`useCodexModelMigration.test.ts`：所有现有非空 `CodexModelCapability` fixture factory。

## Code Patterns

- canonical-only旧 Switch 不做乐观 checked 更新：`src/components/cli-manager/tabs/CodexTab.tsx:910-947`。
- 旧 handler 检查后端返回 bit 才提示成功，但失败未强制回读：`src/pages/cli-manager/useCliManagerPageDataModel.ts:540-567`。
- settings read error 即使有 stale data 也阻止写入：`src/query/settings.ts:20-48`；对应测试 `src/query/__tests__/settings.test.tsx:80-101`。
- mutation success 写完整 SettingsView，settled 刷新相关状态并 reset 自停 catalog：`src/query/settings.ts:161-178`。
- 普通 Settings payload 的专属字段排除和完备性 `AssertNever`：`src/services/settings/settings.ts:83-124,194-225`。
- 当前模型建议 trim、过滤 hidden 并按 model 去重：`src/components/cli-manager/tabs/CodexTab.tsx:2052-2060`；Input+datalist 允许建议和手工值：`:1250-1274`。
- alias editor 的稳定行 ID/整集合序列化：`src/components/settings/ModelPriceAliasesDialog.tsx:37-79`；strict read block 与 source draft：`:465-540`；controlled save：`:563-635`。
- alias stale error 即使有 data 仍隐藏编辑器并禁用保存：`src/components/settings/__tests__/ModelPriceAliasesDialog.test.tsx:123-181`。
- catalog query 按 config/executable/version 隔离、禁 retry、自停错误：`src/query/keys.ts:303-324`，`src/query/cliManager.ts:38-59,89-114`；反向完成/失败覆盖：`src/query/__tests__/cliManager.test.tsx:654-883`。
- 当前 App Server parser 请求 `model/list`，DTO 只有 ID/display/hidden/reasoning：`src-tauri/src/infra/codex_model_catalog/protocol.rs:148-175,203-284`。
- 当前命令先 `sync_current_locked` 后读取 active model list：`src-tauri/src/commands/cli_manager.rs:34-43`。
- authoritative base source 是用户绝对 catalog 或 `codex debug models --bundled`：`src-tauri/src/infra/codex_model_catalog/managed.rs:431-513,1331-1364`；完整 base parser/slug 唯一校验：`:1501-1579`；两个 window 字段读取：`:1684-1724`。
- Provider context 前端边界和 safe integer 校验：`src/services/providers/providerModels.ts:11-13,219-231`；现有跨层常量测试：`src/constants/__tests__/crossLayerContracts.test.ts:221-228`。
- required settings fixtures：`src/test/fixtures/settings.ts:9-88`、`src/test/msw/state.ts:27-105`；旧 MSW endpoint：`src/test/msw/handlers.ts:130-135`。
- generated contract 已有 dedicated update-channel exclusion/command断言可直接仿照：`src/generated/__tests__/bindings.contract.test.ts:126-139`。

## External References

- OpenAI Codex App Server 文档：<https://developers.openai.com/codex/app-server/>。`model/list` 示例公开 ID、display name、hidden、默认/推理能力和输入模态等，但未公开基础 `context_window` / `max_context_window`；不能把缺失字段推断成 272K。
- OpenAI Codex developer commands：<https://developers.openai.com/codex/developer-commands?surface=cli#cli-codex-debug-models>。`codex debug models --bundled` 是读取 bundled raw catalog 的官方入口。
- 本任务前序在本机 Codex CLI `0.147.0` 对 bundled catalog 实测：`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna` 的 `context_window` 与 `max_context_window` 均为十进制 `272000`。这只支持设计判断，不应成为 CI 对本机安装的依赖。
- 归档研究 `.trellis/tasks/archive/2026-08/08-16-codex-gpt56-372k-context/research/runtime-dataflow.md` 固定了 OpenAI Codex `rust-v0.147.0` / commit `be6e8eac029b183056b7e4402879f15d2c85f61b` 的目录启动快照证据。

## Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md:507-703`：待泛化的专属 372K policy、base source、canonical confirmation、home guard、query invalidation和前端测试合同。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md:201-245`：完整 catalog、Profile capability 与 generated binding 的跨层 ownership。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md:76-137`：专属 field owner、普通 payload 排除、stale read 只读和 strict editor GET。
- `.trellis/spec/aio-coding-hub/cross-layer/settings-ownership-rollback-contract.md:138-209`：错误矩阵和编辑器 Retry/保存阻塞测试要求。
- `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md:41-63`：generated boundary、unknown 状态保留和 UI 只从显式动作写入。
- `.trellis/tasks/08-17-codex-custom-model-context-rules/prd.md:17-39`：当前任务的规则、边界、候选、草稿、基础比较和预设权威要求。

## Caveats / Not Found

- 后端研究建议独立 candidate API；这是修复“active/derived 不是 base”和“建议 GET 不应有写副作用”的必要边界。若设计阶段决定不新增该 API，UI 只能诚实显示“基础值不可用”，不能从现有 `model/list` 或 generated catalog 伪造 base。
- 后端 candidate 草案原先未含 `hidden`。为了同时满足“只建议可见模型”和“hidden canonical/preset 仍能比较 base”，本研究建议加 required `hidden: bool`；需在 design 中与后端研究统一。
- Rust `str::trim` 与 JavaScript `String.trim` 基于各自 Unicode 版本，极端新 Unicode whitespace 可能不完全一致；前端验证只提供即时反馈，canonical response 和后端 normalizer 始终权威。
- base 的两个 window 字段可能不同或缺失；UI 不能选一个冒充两者，也不能把 `effective_context_window_percent` 计算后的有效值称为 base。
- candidate failure不能阻止修复已 canonical 的 missing enabled rule，否则 Codex upgrade target disappearance 后用户会被锁死；settings read failure才是编辑授权阻断条件。
- 本次仅做研究，未修改代码、生成 bindings 或运行测试。外部文档与 CLI 证据沿用本任务前序只读调查；CI仍应使用绝对 user catalog fixture。
