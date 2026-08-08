# 可配置模型路由融合联合执行计划

## 评审与启动规则

- [ ] 用户审阅父任务及两个子任务的 `prd.md`、`design.md`、`implement.md`。
- [ ] 本轮调研结束前不运行 `task.py start`，不修改产品源码。
- [ ] 获得实现批准后只启动 `08-08-remove-codex-provider-translation`；父任务不作为产品代码实现目标。
- [ ] 每次启动子任务都运行 `trellis-before-dev`，重新记录 HEAD/schema/dirty paths 并复核与 `8757d32c` 后续改动的重叠。

## 1. 执行删除子任务

- [ ] 按子任务计划完成 SQLite 43->44、活动 Provider/网关/UI 删除和旧配置原子拒绝。
- [ ] 运行局部与全量质量门槛，重点验证普通 Codex、Claude CX2CC、插件 bridge、账户 gate 和历史日志。
- [ ] 执行 `trellis-check`、规格更新、提交和归档。
- [ ] 记录完成 SHA、实际 SQLite/settings/share/bundle 版本及残留兼容壳；父任务更新该快照。

## 2. 重审并执行路由子任务

- [ ] 以删除子任务完成 SHA 为新基线，重新扫描 Provider types/queries/share、config migration、gateway hot path、logs/cost、UI 和 bindings。
- [ ] 若迁移版本或发送链所有权改变，先更新路由 `prd.md`/`design.md`/`implement.md` 并交用户复审。
- [ ] 获得启动批准后执行 SQLite/settings/share/bundle、原子路由器、failover、日志成本和桌面 UI 计划。
- [ ] 执行 `trellis-check`、规格更新、提交和归档，记录完成 SHA 与实际版本。

## 3. 父任务联合验收

- [ ] 确认两个子任务均完成且没有未处理的范围漂移、失败测试或规格缺口。
- [ ] 在两个完成提交的组合状态运行 gateway E2E 和全量前后端质量门槛。
- [ ] 搜索 legacy Codex bridge：只允许迁移、旧格式兼容分类器和负向测试残留。
- [ ] 搜索 configured route：所有最终发送必须位于插件之后、URL/fingerprint/transport commit 之前。
- [ ] 审计其他会话 dirty paths，确认没有被覆盖、格式化或纳入本任务提交。
- [ ] 按 Trellis 支持的父任务完成流程记录联合结果并归档父任务；父任务不新增产品代码提交。

## 联合验证命令

```powershell
rg -n "codex_to_openai_chat|codex_to_openai_responses|codex_to_anthropic_messages|CodexBridgeSection" src src-tauri
rg -n "configured_model_route|model_routing_policy|GW_CONFIGURED_MODEL_ROUTE_APPLY_FAILED" src src-tauri
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

搜索结果必须人工按“活动引用 / 兼容读取 / 负向测试”分类，不能把非零结果直接当作失败，也不能以空测试过滤器冒充验证。

## 联合审查门槛

- [ ] 数据迁移顺序为实际 43->44->45 或经重审后的连续版本，没有并行迁移覆盖。
- [ ] legacy bridge 删除和 route policy 增加在分享、完整配置、复制和 bindings 中没有交叉回归。
- [ ] 配置路由失败不重新引入 `8757d32c` 已修复的重复余额 failback/预留问题。
- [ ] 不存在上一 Provider marker 泄漏到最终 Provider、source model 误计价或 target 未定价回退。
- [ ] Observer/TUI、通知、release/CI 及任何自动新客户端请求仍在范围外。
