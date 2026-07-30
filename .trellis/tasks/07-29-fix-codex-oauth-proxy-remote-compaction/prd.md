# 修复 Codex OAuth 代理与 remote compaction 配置行为

## Goal

让 Codex OAuth 兼容代理模式和 `remote_compaction` 在 AIO 托管路由启停、配置同步与状态检测中保持快速、可解释且一致，避免合法的 `OpenAI` provider 被误判为漂移，也避免与用户已有 provider 发生重复或破坏性覆盖。

## Background

- 用户反馈切换“OAuth 兼容代理模式”时界面会长时间卡住且没有错误提示；切换菜单后回来仍可再次点击，表现得像设置没有生效。
- 用户反馈开启 `remote_compaction` 后，AIO 会按功能要求把 provider 从 `aio` 改为 `OpenAI`，但侧栏 Codex 路由仍显示“修复”。
- 用户补充：开启 `remote_compaction` 前没有检查是否已经存在 `OpenAI` provider；已有同名 provider 时，当前导入/重命名路径会报错。
- 用户询问路由关闭时 Codex 配置是否仍可编辑以及是否会被覆盖；该任务需要把配置所有权边界写清并用回归测试固定。

## Confirmed Facts

- `src-tauri/src/infra/codex_config/patching.rs:814` 明确要求：`remote_compaction = true` 时目标 provider 为 `OpenAI`，关闭时恢复为 `aio`；`:392` 的现有重命名没有目标冲突预检。
- `src-tauri/src/infra/cli_proxy/codex.rs:725` 的代理配置重写仍无条件投影为 `aio`，而 `:794` 的状态检查接受 `aio` 和 `OpenAI`，检测与写入规则不一致。
- `src-tauri/src/app/settings_service.rs:493` 把 OAuth 兼容模式变化视为需要同步已启用 CLI 代理的设置变化；`:1133` 把失败折叠成布尔值，`:1449`/`:1690` 仍可返回设置保存成功。
- `src/pages/cli-manager/useCliManagerPageDataModel.ts:501` 在设置保存后还会同步等待 `:440` 的完整 `refreshCodex()`；`src-tauri/src/infra/codex_model_catalog/protocol.rs:15`/`:22` 的相关子进程 timeout 为 20 秒。
- `src/query/cliManager.ts:200` 起的 Codex 配置 mutations 不失效 CLI proxy status；`src/ui/Sidebar.tsx:297` 直接使用该缓存决定是否显示“修复”。
- `src-tauri/src/infra/codex_config/mod.rs:289`/`:314` 在 proxy 开启时会把提交内容同步进 backup，但当前输入来自活动投影，存在污染可恢复基线的风险。
- `aio/<profile_name_key>` 受管模型路由只有在 Codex CLI 代理开启时才能经过 AIO 网关；普通 Codex 配置编辑不要求开启该路由。
- 路由关闭时，结构化配置写入当前 `config.toml`；路由开启时，AIO 维护启用前基线并投影代理拥有字段，需防止同步/恢复覆盖用户拥有字段。

## Requirements

- R1：OAuth 兼容代理模式的开关必须在合理时间内完成，不得因为无关的完整 Codex 模型目录刷新而长时间阻塞；进行中、成功和失败状态必须可观察，失败不能伪装成未点击。
- R2：OAuth 兼容模式变化若需要重写已启用的 Codex 路由，设置持久化、当前路由投影和最终 UI 状态必须一致；不得留下“设置已保存但路由仍是旧模式”的部分成功。
- R3：`remote_compaction = true` 时，所有配置投影、状态检测、修复、重绑和恢复路径必须共同认可有效 provider `OpenAI`；关闭时共同使用 `aio`。
- R3a：只有显式修改 `remote_compaction` 或生成 AIO 活动代理投影时才能收敛 provider 身份；普通 Codex 配置补丁不得改写根 provider、provider 地址或 provider table。
- R4：开启 `remote_compaction` 前必须检测已存在的 `[model_providers.OpenAI]` 及其嵌套表，不能生成重复 TOML 表，也不能静默覆盖或删除用户已有 provider 数据。
- R5：已有 `OpenAI` provider 与 AIO 当前 `aio` provider 同时存在时，必须使用一条确定、无数据丢失的冲突处理规则；错误必须在写入前返回并保持原配置逐字节不变，除非规划阶段明确采用可证明安全的合并方案。
- R5a：用户确认采用以下冲突规则：若现有 `OpenAI` provider 与当前 AIO 本地代理投影语义等价，则自动识别、复用并去重；若地址或用户自有字段冲突，则在写入前拒绝并提示用户先重命名/处理该 provider，禁止无条件覆盖。
- R6：路由关闭时，AIO 普通 Codex 配置修改应持久化且不被后台代理同步覆盖；随后开启路由应先捕获最新直连配置作为基线。路由开启期间，AIO 设置页修改的用户字段必须同步进可恢复基线，而代理拥有字段只存在于活动投影中。
- R7：路由关闭/恢复后不得残留 AIO 代理地址、AIO 注入认证字段、AIO 生成的模型目录绑定或仅为活动投影创建的 provider 表；用户原有字段和 provider 表必须保留。
- R8：`remote_compaction` 开关涉及 Codex provider sync 的既有安全约束保持不变，包括 Codex App 运行检查、原子写入、失败回滚和备份。

## Acceptance Criteria

- [ ] AC1：切换 OAuth 兼容代理模式不会等待无关的 `model/list`/完整模型目录刷新；测试能证明慢或失败的模型目录读取不会让开关永久 pending。
- [ ] AC2：OAuth 兼容模式开关在成功、后端失败和路由同步失败三种情况下均结束 pending；成功后缓存与持久化值一致，失败后恢复原值并显示可操作错误。
- [ ] AC3：活动 Codex 路由在 `remote_compaction = true`、`model_provider = "OpenAI"` 且 provider 表/网关地址正确时，状态为已应用且不显示“修复”。
- [ ] AC4：对同一配置执行修复、网关重绑、OAuth 模式切换或已启用代理同步后，`OpenAI` provider 身份和 `remote_compaction = true` 仍保持一致。
- [ ] AC5：关闭 `remote_compaction` 后，活动投影与 provider sync 一致回到 `aio`，不留下重复的 `OpenAI`/`aio` 托管表。
- [ ] AC5a：不含 `features_remote_compaction` 的结构化补丁保持根 provider 和所有 provider table 语义不变；路由关闭时的身份重命名保留原 provider 地址。
- [ ] AC6：配置已经含有用户自有 `[model_providers.OpenAI]` 时，开启 `remote_compaction` 不产生重复表；若不能无损采用，操作在任何文件/数据库变更前失败并给出明确冲突错误，原配置逐字节不变。
- [ ] AC6a：配置中的 `OpenAI` 已是与当前 AIO loopback 地址及托管字段等价的投影时，重复开启或从残留的 `aio + OpenAI` 状态收敛均幂等成功，只保留一个有效 `OpenAI` provider。
- [ ] AC7：覆盖 provider 表的无引号、单引号、双引号形式及嵌套表冲突，确保检测不依赖脆弱的字符串包含判断。
- [ ] AC8：代理关闭时修改普通 Codex 字段后再启用、停用路由，修改仍存在；代理开启时通过 AIO 修改普通字段后停用路由，修改也仍存在，而代理拥有字段被正确移除或恢复。
- [ ] AC9：无受管 Profile、有受管 Profile、OAuth 兼容模式开/关、`remote_compaction` 开/关的组合均有聚焦回归覆盖，且现有非 Codex CLI 代理行为不回归。

## Out of Scope

- 改变 `remote_compaction` 本身对 ChatGPT 身份验证的要求。
- 让 `aio/<profile_name_key>` 在 Codex CLI 代理关闭时绕过 AIO 网关运行。
- 扩展新的 Codex provider 类型或引入每个上游供应商一个 `model_provider`。
- 对用户任意外部编辑做无条件自动合并；无法证明所有权时继续失败关闭。

## Notes

- 本任务是跨前端状态、Tauri settings、CLI proxy、Codex provider sync 与受管模型目录的复杂任务；进入实现前必须补齐 `design.md`、`implement.md`、`implement.jsonl` 和 `check.jsonl`。
