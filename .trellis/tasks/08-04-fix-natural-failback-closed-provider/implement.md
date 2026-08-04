# 修复自然回切健康高优先级供应商 - 执行计划

1. 扩展 `CircuitBreaker` 的 `CLOSED` 失败/成功状态转换，维护自然回切 reference/deadline，并覆盖配置热更新与持久化重载。
2. 扩展 `ProbePlanner`，允许到期的 `CLOSED` pending 候选返回 `DirectClosed(NaturalMaxWait)`，保留无期限 `CLOSED` 会话黏性。
3. 补充 circuit 与 planner 单测：建立、重置、清除、到期前后、配置更新和无期限回归。
4. 视现有 route test harness 可复用程度补充一次 P1 `CLOSED` 到期直回 P1、失败回退 P2 的集成回归。
5. 更新 Gateway failover contract 与设置页说明，运行定向测试、完整 Rust library 测试、格式和相关前端测试。

## Validation

```text
cargo test --lib natural -- --nocapture                         # 13 passed
cargo test --lib legacy_closed_failure_reload_arms_deadline_from_latest_failure -- --nocapture
cargo test --lib not_triggered_observation_is_structured_and_unnumbered -- --nocapture
cargo test --lib                                                # 2464 passed, 4 ignored
pnpm test:unit -- src/components/cli-manager/tabs/__tests__/GeneralTab.test.tsx
                                                               # 19 passed
pnpm typecheck                                                  # passed
pnpm lint                                                       # passed
pnpm tauri:fmt                                                  # passed
pnpm check:generated-bindings                                   # passed, no drift
pnpm tauri:check                                                # passed
pnpm tauri:clippy                                               # passed with -D warnings
python ./.trellis/scripts/task.py validate --all                # 62 manifests passed
git diff --check                                                # passed
```

`pnpm check:precommit:full` 在第 1 项全仓库 `format-check` 停止，因为当前 `HEAD` 中未被本任务修改的 `src-tauri/tauri.conf.json` 存在既有 Prettier 漂移。本任务所有变更文件的定向 Prettier 检查通过；聚合门禁其余 12 项均已按相同底层命令单独运行并通过。

## Rollback Point

若 `CLOSED` pending deadline 与现有持久化/公共 gate 契约无法在不新增 schema 的情况下保持一致，停止实现并回到设计阶段，不通过 session 临时时间或绕过 gate 的方式补丁式回切。
