# 实施计划：熔断试探解除与供应商回切策略

## 进入门槛

- [ ] 用户审阅并批准 `prd.md`、`design.md`、`implement.md`。
- [ ] `implement.jsonl` 与 `check.jsonl` 均通过 Trellis context 校验。
- [ ] 运行 `task.py start 08-03-circuit-breaker-probe-failback` 后才修改业务代码。
- [ ] 实施前重新记录工作区既有改动；不得纳入或还原与本任务无关的删除和未跟踪文件。

## 1. 熔断状态机、单飞 Lease 与持久化

- [ ] 先用可控时钟补齐状态机测试，固定 `OPEN` 不再因 `open_until` 到期而并发放行、首次/失败后短间隔、cooldown=0 串行放行、provider 级 single-flight、stale token 和 pre/post-dispatch abandon 语义。
- [ ] 将 circuit API 分为普通 gate、`try_acquire_probe`、`mark_probe_dispatched`、`complete_probe_success`、`complete_probe_failure`，由 provider-scoped generation/owner 校验所有更新。
- [ ] 一个 lease 覆盖完整同 provider retry chain；中间 retry failure 保留 generation，只有离开 provider 或完整成功才完成 lease。
- [ ] 完整成功一次即 `CLOSED` 并设置 300 秒 recovery guard；guard 内 counted failure 立即 `OPEN`，neutral/non-counted failure 不触发，guard 到期懒清理并恢复普通阈值。
- [ ] 新增 v41 -> v42 SQLite 加法迁移，持久化 probe reference/deadline、自然兜底、recovery guard 与 revision；更新 baseline、ensure、版本常量、load/upsert/inert 判定和删除路径。
- [ ] buffered writer 的 upsert 使用 revision 拒绝旧快照覆盖新状态；旧 `HALF_OPEN` 在加载时 fail closed 为 `OPEN`，重启不恢复 in-flight owner。
- [ ] 覆盖迁移幂等、旧行默认、重启 deadline、旧 snapshot 迟到、provider 删除级联与 v42 旧二进制不可直接回退的发布说明。

## 2. Session Trigger、压缩分类与最新路由规划

- [ ] 扩展 `SessionBinding`，保存 route fingerprint、completed/consumed compaction generation、Codex compaction fingerprint 和有界 trigger reservation；沿用现有 TTL/5000 条淘汰生命周期。
- [ ] 建立 request-owner reservation API：规划/gate 阶段只保留机会，cooldown、in-flight、准备失败和未发包 drop 均释放；真实 transport send 边界才提交 generation/fingerprint。
- [ ] 为提交后 circuit token 变 stale 的故障注入路径实现受版本保护的 reservation 补偿，证明零网络调用不会丢失自然/路由触发。
- [ ] Claude 复用严格 `/compact` 分类并只在完整成功终态递增 generation；Codex 严格识别 compact method/path 与结构化 `input[].type=compaction`，对 item 使用有界 canonical hash 去重。
- [ ] compact-producing 请求保持当前稳定 provider 且自身不得 probe；Grok/Gemini 不增加启发式识别。
- [ ] 新增 `ProbePlanner` 纯逻辑：每轮读取当前 CLI active sort mode 与有序 enabled providers，计算 route fingerprint，并只选择稳定 provider 之前最靠前的一个 `OPEN` 候选。
- [ ] 自然策略覆盖新 session、route change、成功压缩后的下一轮、provider 全局最大等待和最长 open 等 trigger；失败 dispatch 消费旧压缩代次，gate skip 不消费。
- [ ] 积极策略在当前 session 的每轮正常生成请求检查；高优先级 `CLOSED` 直接回切，`OPEN` 只生成 intent。
- [ ] 显式排除 token count、模型发现、warmup/ping、compact-producing、repair/auxiliary、strict/health-neutral、forced provider、单候选和非正常生成请求。

## 3. 公共 Gate、真实 Dispatch 与响应终态

- [ ] 在 provider selection 中只调整 request-scoped preference/intent，保留完整候选；公共 `run_gates` 继续拥有 circuit/cooldown/limit 的唯一最终放行权。
- [ ] gate 对 intent provider 原子领取 lease；其余 `OPEN` provider 仍记录 `circuit_open`，竞争失败记录 `probe_cooldown` / `probe_in_flight`，且不调用上游、不消耗 Ready-provider budget。
- [ ] 将“session reservation 提交 + 可选 probe dispatch + transport send”封装为唯一 dispatch coordinator，保证调用前所有 auth/model/plugin/request-build 准备已经完成。
- [ ] 正常回切请求固定一个 `probe_candidate_provider_id` 和一个 probe slot；全路由均 `OPEN` 时改用有序 recovery plan，串行 probe 多个 `OPEN` provider，首个成功停止，cooldown/in-flight 零调用跳过，且任一时刻只持有一个 lease。
- [ ] 把 `ProbeLeaseGuard` 传入同 provider retry context、abort guard、非流式 finalizer 与 `StreamFinalizeCtx`；取消、timeout、drop 和 retry/failover 每条 return path 都必须完成或释放。
- [ ] 非流式仅在完整有界 body、协议转换、response fixer 与 fake-200 分类成功后关闭 circuit并绑定真实 provider。
- [ ] 流式仅在可信 terminal completion/正常 EOF 且无 terminal error 后恢复并绑定；2xx、headers、首包、部分可见内容一律不是 probe success。
- [ ] 流式在 response commit 前失败可沿用 P2 failover；commit 后失败只结束当前流、重新保护 P1、保留 P2 binding，不缓存全流也不拼接另一 provider。
- [ ] 审计 client abort 和现有“已输出内容可视为成功”分支，确保它们不能为 probe 提前关闭 circuit。

## 4. 设置、运行时同步与数据库/IPC 类型

- [ ] Settings schema 从 53 升一版，增加 `provider_failback_strategy`（默认 `natural`）与 `natural_probe_max_wait_seconds`（默认 300，校验 1..=86400）。
- [ ] 扩展 `AppSettings`、defaults、repair/migration、persistence validation、`SettingsUpdate`、owned-field token、IPC DTO 与测试 fixture；未知 strategy 修复为自然。
- [ ] settings durable commit 后热更新 circuit/planner runtime config；强制 runtime failure 时只回滚本任务拥有的两个字段，CAS loser 保留并重新同步并发 winner。
- [ ] 更新 gateway 启动/重载路径，确保冷启动、运行中修改策略、缩短/延长 deadlines 都使用同一配置归一化函数。
- [ ] 重新生成 TypeScript bindings，并验证无无关生成差异。

## 5. 设置界面与 Probe 可观察性

- [ ] 在现有“熔断与重试”卡片加入自然/积极二选一控件与自然最大等待秒数输入；沿用现有控件、间距和错误提示，不新增独立页面。
- [ ] 将 `circuit_breaker_open_duration_minutes` 标签改为“最长熔断等待（可提前试探）”，清楚区分策略 trigger、自然最大等待与 30 秒最短 probe 间隔。
- [ ] 完成 CLI Manager data model、settings patch mapper、pending/rollback 状态、MSW/default fixtures 与组件测试；策略切换后下一轮请求读取新值，不重写 session。
- [ ] 扩展后端 `FailoverAttempt` 的结构化 probe 字段和 `selection_method=circuit_probe`，统一限制长度/枚举，不从 `reason` 文本反向解析。
- [ ] 更新 attempts JSON decoder、实时事件 projection、ProviderChainView 与请求详情，让 started/success/failed/cooldown/in-flight/trigger 可区分，并保留 provider hop、transition、attempt 计数语义。
- [ ] 使用合成标记审计日志、事件、请求详情和聊天响应，证明不新增正文、响应、凭据或内部 probe 标记泄漏。
- [ ] 在常用桌面宽度与窄窗口下检查“熔断与重试”和请求详情：文本不溢出、控件不跳位、结构化状态可扫描。

## 6. 测试矩阵

- [ ] **Circuit 单元**：首次 30 秒、失败后 30 秒、cooldown=0、100 并发只一 lease、lease expiry/drop、stale completion、一次成功、guard counted/neutral/expiry、guard reopen 后普通迟到成功不得关闭、配置热更新。
- [ ] **持久化**：v41 -> v42、旧 `HALF_OPEN`、空字段修复、revision 乱序、inert closed、重启 open/deadline/guard、删除级联。
- [ ] **自然模式**：compact 本轮 P2、下一轮 P1、gate skip 保留、真实失败消费、新 compact 再触发、300 秒全局兜底、A 恢复而 B 保持 P2、无信号 CLI。
- [ ] **积极模式**：同 session 每轮检查、cooldown 时 P2、lease winner P1、loser P2、其他 session 在 P1 恢复后的下一轮直接切回。
- [ ] **路由**：管理员上移/新增/禁用 provider、最新 sort mode、健康直接切换、Open 需 lease、失败后确认 route opportunity；有 Closed 备用时多个 Open 只 probe 最靠前一个，全 Open 时按顺序串行恢复。
- [ ] **请求分类**：正常 stream/nonstream 可 probe；count/models/warmup/compact/repair/strict/neutral/forced/single-candidate 全部零 probe。
- [ ] **重试与预算**：同 provider retry 共用 lease、任一次完整成功恢复、正常路径全部失败转 Closed P2、全 Open 路径失败推进下一候选、skipped 不占 Ready cap、没有 probe 专用 retry、普通非 probe attempt budget 不回归。
- [ ] **流式**：headers/首包不成功、可信 completion 成功、commit 前失败转 P2、可见输出后失败不拼接、client abort/fake-200/idle timeout 保持 Open 且不绑定 P1。
- [ ] **设置/界面/日志**：旧设置迁移、字段所有权竞态、运行时回滚、bindings、策略控件、窄屏布局、attempts JSON/live event/detail 的新增字段和敏感信息负例。

## 预期文件范围

- `src-tauri/src/shared/circuit_breaker.rs`、`src-tauri/src/shared/circuit_breaker/{types,tests}.rs`
- `src-tauri/src/infra/provider_circuit_breakers.rs`、`src-tauri/src/infra/db/migrations/{mod,ensure,baseline_v25,tests,v41_to_v42}.rs`
- `src-tauri/src/gateway/session_manager.rs`、`src-tauri/src/gateway/session_manager/tests.rs`
- `src-tauri/src/gateway/proxy/handler/{provider_selection,runtime_settings}.rs` 及 `provider_selection/`、`failover_loop/`、`middleware/model_inference.rs` 相关文件
- `src-tauri/src/gateway/{events.rs,streams/}`、非流式/流式 response finalizer 与 abort guard
- `src-tauri/src/infra/settings/`、`src-tauri/src/app/{settings_service,startup_settings,gateway_service/circuit}.rs`、`src-tauri/src/commands/settings.rs`
- `src/generated/bindings.ts`、`src/services/{settings,gateway}/`、`src/pages/cli-manager/useCliManagerPageDataModel.ts`
- `src/components/cli-manager/tabs/GeneralTab.tsx`、`src/components/ProviderChainView.tsx`、请求详情相关组件及对应测试/fixtures

实现阶段可按现有模块所有权收窄或增加直接依赖文件，但不得复制 route fingerprint、probe eligibility、attempt JSON 解码或 settings validation 规则。

## 分阶段评审门

1. 状态机与 v42 完成后，先证明并发 single-flight、stale token、重启保护和旧 HalfOpen 归一化，再接 gateway。
2. Planner/Session 完成后，用纯逻辑表覆盖两种策略、最新路由和所有排除项，再允许修改 provider ordering。
3. Gate/dispatch 完成后，用 transport 调用计数证明 cooldown/in-flight/pre-send failure 为零调用，真实失败恰好消费一次 trigger。
4. 流式完成后，逐一审计 response commit 前后分支；任何首包即恢复或可见内容后拼接都阻止进入下一阶段。
5. 设置/UI 完成后，先过 owned-field 并发回滚和生成绑定，再跑全量 gateway/前端回归。

## 回滚点

- 状态机尚未接 gateway 前可整体回滚内存 API；旧行为仍在生产路径。
- v42 迁移接入前必须单独确认 migration/baseline/load 测试；一旦真实数据库升级，旧 v41 二进制不能直接打开，发布回退需恢复迁移前数据库备份或执行受测 downgrade。
- Planner 仅产生 intent，若策略矩阵不通过可回滚其接线，不放宽公共 gate。
- 流式终态若无法证明可信 completion，保持 fail closed：probe 失败并继续使用原 session binding，不降级为首包成功。
- UI/日志字段均为后端契约成立后的最后接线；不得通过隐藏 UI 来掩盖不完整的运行时实现。

## 验证命令

```powershell
cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_circuit_breakers --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml session_manager --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_selection --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml migrations --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml settings_service --lib --locked
pnpm exec vitest run src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx src/components/__tests__/ProviderChainView.test.tsx src/components/home/__tests__/RequestLogDetailDialog.test.tsx src/services/gateway/__tests__/attemptsJson.test.ts src/services/gateway/__tests__/gatewayEvents.contract.test.ts src/services/settings/__tests__/settings.test.ts
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm test:unit
pnpm tauri:test
git diff --check
python .trellis/scripts/task.py validate 08-03-circuit-breaker-probe-failback
```

PowerShell 中逐条运行命令，保留首个失败的完整输出；修复后重跑对应 focused suite，最终再运行完整 `pnpm test:unit` 与 `pnpm tauri:test`。

## 完成条件

- [ ] AC1-AC16 均有可定位的自动化测试或明确的窄窗口视觉验收记录。
- [ ] 公共 gate、Ready-provider budget、strict/health-neutral、强制路由和普通非 probe 请求行为无回归。
- [ ] 代码检查确认所有 probe lease、trigger reservation 与 stream token 在成功、失败、取消、超时和 drop 路径闭合。
- [ ] `trellis-check` 完成全范围检查并修复发现，随后按流程评估是否更新 gateway 规范。
