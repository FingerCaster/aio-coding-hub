# 实施计划：自定义余额查询与路由恢复集成

## 进入门槛

- [ ] 用户审阅父任务与两个子任务的 `prd.md`、`design.md`、`implement.md`。
- [ ] 仅启动下一项实际交付的子任务，不启动无直接代码工作的父任务。
- [ ] Child 1 完成、检查并提交后，才能启动依赖其运行时契约的 Child 2。
- [ ] 每个子任务开始前使用 `trellis-before-dev` 读取目标层规范；Codex inline 模式不创建 JSONL 上下文清单。
- [ ] 记录并保留工作区现有改动，不纳入或还原无关删除、`.orca/` 与其他任务目录。

## 1. Child 1：自定义 JavaScript 与共享运行时

- [ ] 启动 `.trellis/tasks/08-08-custom-account-usage-script`。
- [ ] 按其 `implement.md` 选择性移植脚本 worker、原生确认、配置净化、草稿测试、共享运行时和 Desktop 接线；不移植当前主线已删除的 TUI/Observer。
- [ ] 先通过脚本安全/资源边界和运行时 generation/config-token/in-flight 测试，再接 UI。
- [ ] 验证 Child 1 没有任何 route、circuit、Session、Provider 顺序或启用状态副作用。
- [ ] 更新 `provider-account-usage-query-contract.md`，完成 `trellis-check`、提交并结束该子任务。

## 2. Child 2：余额门控与恢复回切

- [ ] 确认 Child 1 的共享快照 API、进程内 generation、配置 token 和失效测试已成为当前基线。
- [ ] 启动 `.trellis/tasks/08-08-account-usage-route-gate`。
- [ ] 按其 `implement.md` 依次实现配置/可移植性、Gateway 租约、纯投影、共同 gate、恢复 epoch、Session planner 和 UI。
- [ ] 先证明所有 denied 路径零上游调用、零 Ready budget、零 circuit 变更，再接恢复回切。
- [ ] 通过多 Session、并发旧响应、stream/nonstream、forced/managed/model-discovery 和全门控组合测试。
- [ ] 验证含余额 skip 的混合 503 不写 recent-error cache，余额恢复/failure/stale 后下一请求重新进入 Provider 选择。
- [ ] 更新 account-usage、gateway-failover、provider-share、config-bundle 规范，完成 `trellis-check`、提交并结束该子任务。

## 3. 父任务集成验收

- [ ] 在两个子任务提交之上运行完整 Rust、前端、生成绑定和格式检查。
- [ ] 用一个合成 Provider 验证 `custom -> blocked -> fallback -> available -> direct failback -> session bind` 全链路。
- [ ] 用原生 sub2api/NewAPI fixture 验证同一归一化状态得到相同 gate 结果。
- [ ] 验证无适配器、gate 默认关闭、快照失败/过期和应用重启均保持 fail-open。
- [ ] 验证单 Provider 分享、完整配置备份和本机复制符合矩阵，输出中无自定义源码/Origin/授权材料。
- [ ] 审计 request attempts、普通日志、IPC 和测试输出，不含金额、凭据、脚本或上游响应。
- [ ] 完成父任务最终规范索引检查并归档父任务。

## 预期评审门

1. **Child 1 安全门**：脚本越权、超时、输出超限、确认取消和竞态测试未全部通过前，不接共享定时刷新。
2. **Child 1 所有权门**：同 Provider 合并、generation 失效和 60 分钟显示 TTL 未证明前，不允许 Child 2 读取快照。
3. **Child 2 gate 门**：门控仍有同步 I/O、静默预过滤、Ready 计数或 circuit 副作用时，不接恢复 planner。
4. **Child 2 恢复门**：只有 fresh Blocked -> Available 能发布 epoch，多 Session baseline 与旧响应单调提交未证明前，不开放 UI 开关。
5. **父任务发布门**：可移植性矩阵、全量测试和敏感信息审计未通过时，不归档父任务。

## 父任务验证命令

```powershell
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm test:unit
pnpm tauri:test
git diff --check
python .trellis/scripts/task.py validate 08-08-custom-account-usage-script
python .trellis/scripts/task.py validate 08-08-account-usage-route-gate
python .trellis/scripts/task.py validate 08-08-custom-account-usage-routing
```

PowerShell 中逐条运行；保留首个失败的完整输出，修复后重跑 focused suite，最后再跑完整套件。

## 回滚点

- Child 1 未接路由，可独立回滚其 UI/运行时接线；内置适配器必须继续通过原回归。
- Child 2 的配置默认关闭。出现运行时问题时先关闭全部 gate，即可恢复当前路由行为并保留查询能力。
- 不需要数据库降级；扩展 JSON sanitizer 必须能把未知/缺失字段修复为 gate 关闭。
- 任何自定义字段已经进入便携载荷时，不能仅隐藏 UI；必须修复导出与导入双向 sanitizer 并补负例测试。

## 完成条件

- [ ] 两个子任务的全部 AC 有自动化测试或明确的跨层验收证据。
- [ ] 默认关闭和未配置供应商行为与当前主线一致。
- [ ] 余额阻断、恢复、回切和再次阻断全链路不旁路共同 gate 或现有 Session 单调绑定。
- [ ] 分享/备份/复制策略与父设计矩阵完全一致。
- [ ] 父任务 `trellis-check` 通过后再归档。
