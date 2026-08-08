# 可配置模型路由核心实施计划

## 进入门槛

- [ ] `08-08-remove-codex-provider-translation` 已完成质量检查、提交和归档，SQLite 44 成为实际基线。
- [ ] 用户审阅并批准本子任务的 `prd.md`、`design.md`、`implement.md` 后，只启动本子任务。
- [ ] 运行 `trellis-before-dev`，读取 backend、frontend、cross-layer、gateway、request log、Provider 分享和配置迁移规格。
- [ ] 记录实际 HEAD、SQLite/settings/bundle/share 版本及 `git status --short`；确认至少包含 `8757d32c`，45/57/share v3 未被占用。
- [ ] 复核 `provider_resolution.rs`、`probe_planner.rs`、`provider_iterator.rs`、`attempt_executor.rs`、`request_logs.rs` 和 Provider 编辑器的最新所有权及 dirty overlap。

## 1. 建立共享策略契约

- [ ] 在 settings/domain 公共类型中增加 policy/rule、128/256/64 限制和默认 disabled 策略。
- [ ] 实现唯一的后端严格写入规范化器和防御性读取清洗器；覆盖 trim、重复、空输出、控制字符和 disabled-empty 行为。
- [ ] 增加全局损坏 -> disabled、Provider 损坏 -> `Some(disabled)` 的有界诊断测试，禁止记录原 JSON。
- [ ] 提供 exact single-pass matcher 和显式 policy source，不接受通配符、默认或 target 级联。

## 2. 完成 settings、Provider 和交换格式持久化

- [ ] 增加 settings 56->57 迁移、默认值、CAS/update、完整配置 v4 字段和独立能力门槛测试。
- [ ] 增加 SQLite 44->45 nullable policy JSON 列、baseline 和幂等迁移测试。
- [ ] 更新全部 Provider query projection、summary/gateway DTO、upsert specified 语义及本机复制事务。
- [ ] 新增严格 Provider 分享 v3，v1/v2 转 canonical 时 override 为 NULL；覆盖 v3 三态 round-trip、未来字段/版本拒绝。
- [ ] 更新完整配置 v1-v4 矩阵：v4 往返全局/Provider policy，v1-v3 清除注入字段且不影响其他能力。

## 3. 实现协议分类与原子路由器

- [ ] 基于 immutable original model 和显式 request intent 实现 POST inference 分类，排除 managed/辅助/探测/发现/非 POST。
- [ ] 为 Claude Messages、Responses/compact、Chat 和 Gemini path/body 建立结构化 target/effort adapter。
- [ ] Gemini 数值 effort 写 `thinkingBudget`，文本 effort 写 `thinkingLevel`，并保证 sibling 互斥。
- [ ] 在克隆 path/query/body 上应用和验证所有请求输出；成功才提交，失败返回安全分类且不留部分改写。
- [ ] 覆盖 CX2CC 最终 Responses、插件先改写、target 等于当前值及压缩 body state。

## 4. 接入发送链与 Provider failover

- [ ] 在每个 attempt 开始清理旧 configured route marker，gate 和 Provider preparation 顺序保持不变。
- [ ] 将配置路由接在 `RequestBeforeSend` 成功之后、finalize/fingerprint/URL/transport commit 之前；URL 构造移动到路由成功之后。
- [ ] 增加 `ConfiguredModelRouteApplyFailed` prepared/send outcome、`configured_model_route_apply_failed` attempt outcome 和 `GW_CONFIGURED_MODEL_ROUTE_APPLY_FAILED`。
- [ ] 应用失败直接 break 当前 Provider retry，outer loop 继续下一 Provider；不调用 transport/upstream/circuit/health/retry 分类器。
- [ ] 调整 all-providers finalization：全部 eligible 候选均路由失败时返回明确 502；存在实际上游失败时保留既有终局优先级。
- [ ] 对 pending dispatch ownership、probe reservation、abort guard、Session binding、`blocked_provider_ids` 和 recovery epoch 增加无副作用断言。

## 5. 接入日志、成本和桌面观测

- [ ] 增加 Provider-scoped configured route marker 和 per-attempt 摘要，确保 marker 只匹配 final Provider。
- [ ] 保留 `requested_model`，记录 effective model/effort/policy source/pricing CLI 和应用标志。
- [ ] 在 `effective_cost_basis` 中加入 configured route 优先级；target 无价格返回 unknown，不回退 source；保留 CX2CC usage 语义。
- [ ] 更新请求日志 DTO、列表/详情和成本展示，覆盖失败 attempt 与后续成功 Provider 的隔离。
- [ ] 确认无敏感请求体、凭据或超长模型字符串进入日志/诊断。

## 6. 实现全局和 Provider UI

- [ ] 建立共享规则编辑器和前端 normalize/validate/clone helper，与 Rust 限制一致。
- [ ] 在全局设置接入 enabled policy，在 Provider 编辑器接入明确三态 segmented control 和专属规则草稿。
- [ ] 更新创建/编辑/重置/复制/分享预览/提交适配器，保证 `override_specified` 和账户用量状态互不覆盖。
- [ ] 更新请求日志 original/effective/effort/source 与 unknown cost 展示，检查窄屏和长模型文本不溢出。
- [ ] 从 Rust 重新生成 bindings，逐项审计 Provider、settings、request log 和错误码差异。

## 7. 联合质量检查

- [ ] 运行策略、迁移、Provider、分享、配置、协议 adapter、request log/cost focused tests。
- [ ] 运行 gateway route E2E：插件顺序、压缩、零上游应用失败、下一 Provider 成功、全部失败及账户回切组合。
- [ ] 运行全量前端/Rust 质量门槛和生成绑定检查。
- [ ] 执行 `trellis-check`，修复后更新相关规格，提交并归档子任务。
- [ ] 回到父任务执行两个子任务的联合验收，不在子任务中直接归档父任务。

## 验证命令

```powershell
cargo test --manifest-path src-tauri/Cargo.toml model_routing --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml migrations --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml providers --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml request_logs --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml failover --lib --locked
pnpm test:unit
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm tauri:test
git diff --check
```

实现时若 Rust 测试模块过滤名不同，先用 `cargo test -- --list` 定位后记录实际 focused 命令；不得用空过滤结果冒充通过。

## 审查门槛

- [ ] 有效匹配应用失败时当前 Provider 零上游发送，下一 Provider 仍完成自己的 gate 和策略解析。
- [ ] 损坏策略继续原始请求，与有效匹配应用失败的 outcome/控制流严格分离。
- [ ] 路由失败不改变健康、熔断、账户 Blocked/recovery、Session/failback、reservation 或 transport retry。
- [ ] final marker/hash 只属于最终 Provider，requested model 不变，target 未定价不回退。
- [ ] managed/辅助/探测/发现/非 POST 和 disabled/no-match 请求保持既有行为。
- [ ] Provider 分享 v3、完整配置 v4、SQLite 45、settings 57 与生成绑定的版本矩阵一致。

## 完成记录

- 实际基线为 `10ceb1cd`，包含账户用量基线 `8757d32c` 与已归档的 Codex 转译删除子任务。
- 已实现 settings 57、SQLite 45、Provider 分享 v3、完整配置 v4、Provider 三态覆盖、最终 wire 路由、失败切换、日志/成本语义与桌面端配置。
- 已补充精确大小写、单次不级联、CX2CC 最终 Responses、同值 target、Gemini effort、压缩请求与插件顺序、零上游失败及下一 Provider 切换等自动化证据。
- 全量通过：前端 304 个测试文件/2749 项测试、`typecheck`、`lint`、生成绑定检查、网关错误码检查、Rust fmt/check/clippy 与 `cargo test --locked`/`tauri:test`。
- `git diff --check` 通过；仅任务 JSON 存在 Git 的 CRLF 转 LF 提示，无空白错误。
- Orca 内嵌浏览器服务本轮未附着，无法执行截图式 UI QA；相关界面由组件测试、类型检查、lint 与响应式布局代码审查覆盖，未以截图完成冒充验证。
