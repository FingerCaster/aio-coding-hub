# 技术设计

## 集成策略

以 `origin/main@a8c525cdaadce77dd4b00363962e501bc5fae491` 为唯一基线，在 Orca 管理的干净集成 worktree 建立规划提交。实现子任务从该规划提交创建独立 worktree 和 feature branch，各自提交可审查结果；协调者按依赖和冲突顺序将提交整合回本分支。

## 子任务映射

| 子任务 | 需求 | 主要所有权 | 并发关系 |
| --- | --- | --- | --- |
| Provider 路由总开关与自环保护 | R1-R2 | Rust provider queries、gateway failover/send | 独立；高风险 |
| 设置与模型价格数据一致性 | R3-R4 | settings query/service、model price infra/command/UI | 独立 |
| 跨重启数据重置维护门 | R5 | app bootstrap、data management、maintenance | 独立；可能与插件启动检查冲突 |
| Rust 依赖审计豁免移除 | R6 | Cargo manifests/lock、CI audit command | 独立；应较早整合 |
| 长会话与前端边界可靠性 | R7/R9 | sessions query/page、共享 UI/services/layout | 独立 |
| 发布链并发与制品不可变 | R8 | release workflow、release helper/tests | 独立 |
| 插件运行时完整性与资源边界 | R10 | plugin service/repository/pipeline/host | 等待范围确认；与 reset 启动路径联合复核 |

## 并发与集成

- 第一波并发启动前六个无依赖子任务；插件子任务仅在用户确认后启动。
- 每个 worker 仅修改其 PRD 所列所有权路径，不修改父/其他子任务文件，不触碰主工作区。
- 每个 worker 必须提交代码和自己的 task artifacts，并通过 Orca `worker_done` 返回 commit SHA、测试与风险。
- 集成顺序建议：Rust audit → settings/pricing → sessions/UI → release → reset → provider routing → plugin。发生语义冲突时由协调者停止并给出具体文件/行为证据。

## 跨子任务不变量

- 不实现网关入站鉴权，不删除 Provider 专用路由。
- 不覆盖 fork 的账户用量、熔断恢复、模型路由、Codex reasoning guard 或 release immutable SHA 契约。
- 不整体同步候选仓库，不把候选缺陷修复和新产品线混入。
- 每个行为变化必须有失败优先回归，集成后运行跨层质量门。

## 回滚

子任务保持独立提交边界。集成验证失败时只回退对应子任务提交；若两个子任务形成必要依赖，应在集成记录中明确成组回滚。
