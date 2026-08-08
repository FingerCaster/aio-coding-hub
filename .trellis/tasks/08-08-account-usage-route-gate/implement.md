# 实施计划：余额门控与恢复回切

## 进入门槛

- [ ] 依赖任务 `08-08-custom-account-usage-script` 已完成、检查并提交。
- [ ] 用户审阅并批准本任务 `prd.md`、`design.md`、`implement.md`。
- [ ] 运行 `task.py start 08-08-account-usage-route-gate` 后才修改业务代码。
- [ ] 使用 `trellis-before-dev` 重读 account-usage、gateway failover、provider share、config bundle 和 attempt budget 规范。
- [ ] 记录并保留工作区已有改动。

## 1. 配置、sanitizer 与纯投影

- [ ] 先写 Rust/TypeScript 配置测试，固定 `routeGateEnabled` 缺失/非法默认 false，adapter disabled 不创建隐式 gate。
- [ ] 扩展本地 persistence sanitizer 和前端 merge/read config，保持该字段与 `timedRefreshEnabled` 独立。
- [ ] 拆分/参数化 provider-share 与 config-bundle portability policy，按 native/custom 矩阵覆盖 export、strict parse、preview、prepare import 和 transactional import。
- [ ] 保持 local duplicate 的 gate 选择；custom duplicate 的新 Provider 身份继续撤销脚本确认，但不悄悄改变 gate 偏好。
- [ ] 实现纯 `project_account_usage_route` 函数，覆盖 generation/config-token、基于单调完成时刻的严格 TTL、future display time、所有状态、正余额冲突、future expiry 和无金额 custom 状态。
- [ ] 投影只返回稳定枚举/时间元数据，不把金额、message、Origin 或脚本复制到 Gateway DTO。

## 2. Gateway 消费租约

- [ ] 扩展共享 runtime consumer kind，Gateway target 忽略 display timed switch 但复用保存的 60..300 秒 interval。
- [ ] 在 Gateway background tasks 增加启动即运行、5 秒续期、15 秒 TTL 的协调器；数据库查询只投影 target 所需非敏感字段。
- [ ] `replace_gateway_targets` 精确更新集合，移除 gate off/disabled/deleted/invalid target，并对 config token 变化执行 generation invalidation；mutation hook 即时失效，5 秒 reconcile 修复漏失事件。
- [ ] 证明 Desktop/manual/Gateway 同 Provider 继续合并，同一时间最多四个 Provider fetch且无无界等待任务，Gateway stop 后 lease 有界消失。
- [ ] Provider upsert/duplicate/delete/enable 使用语义化即时 invalidate；config import 只在成功 commit 后同步 reset account runtime、live route runtime、recent errors 并立即 reconcile，Gateway restart 也立即投影目标，不把正确性留给下一次心跳。
- [ ] 用暂停时间测试 `timedRefreshEnabled=false`、interval 60/300、blocked/available/error、无 UI 和无 Gateway 的 due 行为。

## 3. 共同 Gate 与审计

- [ ] 增加 `GatewayErrorCode::ProviderAccountUsageBlocked`，同步 TypeScript 常量与 error-code parity 检查。
- [ ] 在 decision-chain constants 增加 `account_usage_zero_balance` / `account_usage_expired`，不复用 `rate_limited`。
- [ ] `IterationCounters` 增加 `skipped_account_usage`，扩展 all-unavailable verbose message/structured log，但不改变客户端总错误码。
- [ ] 在 `provider_checks::run_gates` 最前调用同步 route projection；Blocked 立即 push 普通 skipped attempt 并返回。
- [ ] 证明余额拒绝发生在 circuit lease/OAuth/spend DB gate/credential/base URL/transport 之前，且零 Ready budget、零 retry、零 circuit/Session 变化。
- [ ] 不更新 `earliest_available_unix`。测试纯余额阻断无 Retry-After、混合 gate 只使用其他权威时间。
- [ ] finalizer 在 `skipped_account_usage > 0` 时禁止写 recent-error 503 cache；混合 gate 仍可返回其他权威 Retry-After，但余额恢复/failure/stale 后的下一请求必须重新选择。
- [ ] 保持 all-unavailable attempt/route 投影；不得把余额 skipped 当 `not_triggered` observation 排除。
- [ ] 不改 planner 去为持续阻断 Provider 每请求制造 Direct/observation；测试仅在候选实际计划时要求普通 `decision=skip` / `selection=filtered` attempt，且 provider/retry index、circuit/probe 字段为空。

## 4. 运行时恢复 Epoch

- [ ] 用可控时钟/原子计数测试建立 `last_confirmed`、Provider epoch 和 global epoch，再接真实刷新完成路径。
- [ ] fresh Blocked 清零 Provider epoch；同 generation fresh Blocked -> Available 恰好发布一次；初次/连续 Available、连续 Blocked、错误、过期、矛盾输出均不发布。
- [ ] 失败/过期期间 gate fail-open但保留 transition memory；后续 fresh Available 可确认一次恢复。
- [ ] config token、permission、gate off、delete/invalidate 和 stale completion 重置或拒绝 Provider 信号；旧 generation/token 零写入。
- [ ] failure/stale 时保留 transition memory/已发布 epoch 但 planner 暂时读取零；重新 fresh Available 后只恢复可见性，不重复发布。
- [ ] 实现 checked epoch overflow：放行结果但不发布回切标记，且全局值不回绕。
- [ ] 在 runtime inner write lock 内 checked 计算 next，先写 Provider snapshot/epoch/last-confirmed，再 Release-store global epoch；Session baseline Acquire-load，禁止先暴露 global 的 fetch-add/update。用 barrier 证明并发读者一致。

## 5. Session baseline 与现有 Planner

- [ ] 扩展 `SessionBinding`、creation、routing snapshot 和测试 fixture，增加 account-usage baseline；新 binding 以 Acquire 语义同时捕获两个 global epoch。
- [ ] 保持 sliding TTL、same-provider success、route confirmation、其他 Session convergence 和旧 response 不推进 baseline；clear/expiry 后新 incarnation 重新捕获。
- [ ] 扩展 `ProbePlannerInput` 的候选 recovery snapshot；纯逻辑测试 `CLOSED + newer account epoch -> Direct`，equal/older/zero/reblocked 不触发。
- [ ] account epoch 不让 `OPEN/HALF_OPEN` 变 Direct、不新增 probe trigger；现有 due/cooldown/single-flight 结果保持原样。
- [ ] arbitrary-length higher-priority prefix 中 circuit/account 两类恢复按最新 route 排序，not-due 候选不阻塞后续恢复候选。
- [ ] provider resolution 从 runtime 读取当前 `ConfirmedAvailable` 约束下的 Provider epoch 与 Session account baseline；缺失/stale/failure runtime 使用零值，不影响现有 planner。
- [ ] gate skip 保留 request intent reservation，下一 planned target 仍可发送；所有目标零 send 时沿用 drop release。

## 6. 真实请求终态与显式配置变化

- [ ] 非流式/流式仍只在真实模型请求完整成功后 token-aware bind；余额刷新完成不调用任何 Session bind API。
- [ ] 用障碍同步测试旧 P2 request/stream 晚完成，新的 P1 account-recovery direct success 先绑定，旧 token 不能反转。
- [ ] 恢复计划产生后、发送前再次 Blocked：记录 account-usage skipped、零 P1 调用、继续 P2/fallback，且不提交错误绑定。
- [ ] account recovery 时 circuit 已 Open：不旁路，使用现有 cooldown/probe 规划；余额刷新不关闭 circuit。
- [ ] gate 从 true 显式关闭或有效 adapter 被关闭时，扩展 provider runtime reset decision，清理该 CLI live Session；不发布余额恢复/circuit 信号。
- [ ] name/note 与 `timedRefreshEnabled` 等 display-only 变化不清 Session、快照或 transition memory；query/route 语义变化分别通过 generation/token 与 route reset 失效。

## 7. Provider UI 与可观察性

- [ ] 在现有账户用量配置区域增加“余额影响路由” switch，默认 off；无 adapter 时不启用 Gateway consumer。
- [ ] custom 等待重新确认时保留 gate 偏好但显示未生效状态；确认后无需第二次开启。
- [ ] 更新 form state、submit payload、reset key、duplicate/edit fixture 和 account config tests。
- [ ] 更新 request detail/Provider chain 的稳定 reason label（如现有 UI 有 reason-code 映射），不展示金额、刷新时间、脚本或 Origin。
- [ ] 在桌面和窄窗口验证 switch、长 Provider 名和 skipped 文案不重叠/溢出。

## 8. 测试矩阵

- [ ] **默认兼容**：旧配置、无 adapter、gate off、Provider disabled/OAuth/source Provider，零 Gateway refresh 且路由与当前主线一致。
- [ ] **四适配路径**：sub2api、NewAPI billing、NewAPI account、custom 对相同 normalized result 得到同一投影。
- [ ] **新鲜度**：interval 60/300 的 monotonic `2x - 1`、精确 `2x`、future display timestamp、display 60 分钟缓存和 config-token mismatch。
- [ ] **状态**：available、zero/negative/absent amounts、任一正余额、expired/future expiry、全部 failure/unsupported/矛盾状态。
- [ ] **gate**：零 upstream/Ready/retry/circuit、stable reason、all balance blocked、balance+circuit/cooldown/limits、Ready cap、Retry-After、含余额 skip 不写 recent-error cache。
- [ ] **刷新**：无 UI、timed off、manual coalescing/force tail、Gateway start/stop、target exact replacement、config generation/token、应用重启首轮 fail-open。
- [ ] **恢复**：Blocked -> Available、Blocked -> error -> Available、stale/error 暂停 epoch 可见性、reblock clears epoch、manual refresh、Release/Acquire 发布顺序、overflow。
- [ ] **Session**：多 Session 独立 baseline、arbitrary prefix、circuit Open 不旁路、old request/stream token、再次 gate、真实成功才绑定。
- [ ] **请求类型**：forced Provider、managed model、model discovery strict、normal fallback、health-neutral/availability 测试不扩展范围、nonstream/stream。
- [ ] **portable/security**：native share false、native backup preserve、custom 所有 portable 路径 disabled/false、local duplicate preserve、crafted import、日志无敏感值。

## 预期文件范围

- `src-tauri/src/domain/provider_account_usage.rs`
- `src-tauri/src/app/provider_account_usage_runtime.rs`
- `src-tauri/src/gateway/background_tasks.rs` 及新增账户用量协调模块
- `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/{provider_checks,provider_iterator}.rs`
- `src-tauri/src/gateway/proxy/handler/failover_loop/response/finalize.rs`
- `src-tauri/src/gateway/proxy/handler/{provider_selection,middleware/provider_resolution}.rs`
- `src-tauri/src/gateway/proxy/handler/provider_selection/probe_planner.rs`
- `src-tauri/src/gateway/session_manager.rs`、`src-tauri/src/gateway/session_manager/tests.rs`
- `src-tauri/src/gateway/{events.rs,routes.rs}` 与 failover loop tests
- `src-tauri/src/gateway/proxy/error_code.rs`、`src/constants/gatewayErrorCodes.ts`
- `src-tauri/src/app/provider_service.rs`
- `src-tauri/src/domain/providers/{share,queries,tests}.rs`
- `src-tauri/src/infra/config_migrate/{export,import,mod,tests}.rs`
- `src/services/providers/providerAccountUsageConfig.ts`、`src/pages/providers/ProviderAccountUsageSection.tsx`、`src/pages/providers/useProviderEditorForm.ts` 及测试
- 如 Rust IPC DTO 变化：`src/generated/bindings.ts`、`src/services/generatedIpc.ts` 及契约测试

## 分阶段评审门

1. 配置/portable 矩阵与纯投影表全部通过后，才启动后台 Gateway lease。
2. Gateway lease 在 timed off、关闭 gate、stop/restart 下有界后，才接共同 gate。
3. gate 零调用/零 budget/零 circuit 副作用、all-unavailable/Retry-After 和 recent-error bypass 通过后，才接 recovery epoch。
4. recovery publication、reblock invalidation和 epoch overflow 通过后，才改 Session/planner。
5. 多 Session、stream token 和 circuit Open 不旁路通过后，才开放 UI switch。
6. 完整 Rust/前端/portable/敏感信息检查通过后，才更新规范和提交。

## 验证命令

```powershell
cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage_runtime --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_selection --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml session_manager --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml failover_loop --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_share --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
pnpm exec vitest run src/pages/providers/__tests__/ProviderAccountUsageSection.test.tsx src/pages/providers/__tests__/ProviderEditorDialog.test.tsx src/services/providers/__tests__/providerAccountUsageConfig.test.ts src/components/__tests__/ProviderChainView.test.tsx src/components/home/__tests__/RequestLogDetailDialog.test.tsx
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm test:unit
pnpm tauri:test
git diff --check
python .trellis/scripts/task.py validate 08-08-account-usage-route-gate
```

## 回滚点

- route gate 默认 false；在 UI 接入前可用 fixture/直接配置测试，发现问题不影响旧 Provider。
- common gate 异常时先移除其接线并保留 runtime/配置，当前路由即恢复。
- recovery epoch 尚未接 planner 前可独立验证，不修改 Session。
- planner 接线失败时回滚 account-usage baseline/input，保留 skipped gate；不得用 circuit epoch 伪装余额恢复。
- 发布后可通过关闭所有 gate 回退行为，无数据库降级。

## 完成条件

- [ ] PRD 全部 AC 有自动化证据。
- [ ] 未配置/gate off 与当前主线行为一致。
- [ ] 可信阻断全路径零上游、零 Ready budget、零 circuit 变化且审计完整。
- [ ] 只有同 generation fresh Blocked -> Available 发布恢复，多 Session 通过现有单调绑定收敛。
- [ ] portable 矩阵、Retry-After/recent-error cache 和敏感信息约束全部通过。
- [ ] `trellis-check` 通过并更新四份相关规范后提交。
