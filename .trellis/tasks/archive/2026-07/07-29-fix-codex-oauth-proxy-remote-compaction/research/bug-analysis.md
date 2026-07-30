# Bug Analysis: remote_compaction 历史同步内存与目录基线污染

## 1. Root Cause Category

- **Category B - Cross-Layer Contract**：配置开关只表达布尔功能值，IPC 没有表达“是否迁移历史”的一次性范围，导致每次 provider 身份变化都隐式扫描全部会话、SQLite 和 global state。
- **Category D - Test Coverage Gap**：旧测试覆盖了同步结果和 rollback，却没有约束 change set 的驻留字节，也没有验证 config-only 完全不访问历史。
- **Category D - Test Coverage Gap**：旧的进程运行测试通过省略 `sync_history` 得到默认 `false`，却仍断言必须返回 `CODEX_PROVIDER_SYNC_PROCESS_RUNNING`，把错误的 scope 认知固化成了测试预期。
- **Category E - Implicit Assumption**：目录代码假设 proxy backup 永远是启用前用户基线；旧版本或失败恢复可把 AIO generated catalog 写回 backup，随后又把该生成文件当作自身 base。

## 2. Why Fixes Failed

1. 仅给 UI 增加选择会遗漏 raw/自动调用，后端仍可能先扫描再跳过；必须把 `sync_history` 放在 command/service 边界并在收集前分支。
2. 仅把 rollout 改成流式写入后，Windows 聚焦测试返回 `os error 5`。读取器仍借用在闭包外，`MoveFileExW` 原子替换目标时句柄尚未释放；改成 move closure 持有并在 finalize 前销毁 reader 后通过。
3. 仅绕过 self-base 校验会继续保留污染 backup，路由停用时仍可能恢复错误绑定；必须把 backup 修复纳入 generated/live 同一事务。
4. 首版 `sync_history = false` 只跳过了历史目录/数据库收集，却把 Codex App 进程检测留在共同入口；结果“仅更新配置”仍被历史迁移专属预检阻塞。scope 分支必须同时包住迁移专属预检与迁移目标发现。

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | `sync_history` 是瞬时 IPC 选项；false 在历史枚举前短路 | DONE |
| P0 | Architecture | `SessionChange` 只保留路径；rollback 内容存入磁盘备份 | DONE |
| P0 | Test Coverage | 损坏 UTF-8 rollout 证明 config-only 零历史访问 | DONE |
| P0 | Test Coverage | 同一 running-process 条件下成对验证 config-only 成功与 explicit history 零写入失败 | DONE |
| P0 | Architecture | 仅 `sync_history = true` 执行锁前/锁后的 Codex App 进程检测 | DONE |
| P0 | Cross-platform Test | Windows 实际执行流式原子替换，确保 reader 在 finalize 前关闭 | DONE |
| P1 | Runtime | rollback 尝试所有磁盘条目并聚合失败为稳定错误 | DONE |
| P1 | Documentation | 更新 Codex config、managed catalog 与 cross-layer checklist | DONE |

## 4. Systematic Expansion

- **Similar Issues**：任何“可选批量迁移”若只在 UI 分流、仍在后端先构建全量 change set，都会产生相同的隐藏成本。
- **Similar Issues**：即使数据枚举已经受 scope 控制，若迁移专属进程/服务状态预检仍位于共同入口，opt-out 仍会出现与选择不符的错误。
- **Design Improvement**：事务计划只保存元数据和路径；大对象进入磁盘 staging。调用方必须显式选择历史范围。
- **Process Improvement**：涉及 Windows 原子替换时必须跑真实文件句柄路径，不能只用 mock rename 或 Unix 行为推断。
- **Knowledge Gap**：生成产物与恢复基线是不同所有权层；生成文件绝不能成为自己的 canonical base。

## 5. Knowledge Capture

- [x] 更新 `.trellis/spec/aio-coding-hub/cross-layer/codex-config-contract.md`
- [x] 更新 `.trellis/spec/aio-coding-hub/cross-layer/codex-managed-model-route-contract.md`
- [x] 更新 `.trellis/spec/guides/cross-layer-thinking-guide.md`
- [x] 同步对应 `src/templates/markdown/spec/` 模板
- [x] 新增 config-only、显式历史同步、path-only change set、流式 CRLF/幂等和目录基线精确识别测试
- [x] 新增 running-process 的 config-only 成功与 explicit-history 零写入失败成对测试
