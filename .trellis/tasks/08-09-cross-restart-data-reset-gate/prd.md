# 跨重启数据重置维护门

## Goal

让“清空应用数据”在进程崩溃、后台任务滞留或文件删除中断后仍可于下一次启动安全收口，不产生半重置数据库或设置。

## Evidence

- 当前 `app_data_reset` 已持有 gateway lifecycle lock、停止网关、恢复 CLI proxy，并通过 `prepare_db_reset` 持有 DB reset guard。
- 当前删除仍在同一进程按文件顺序执行，没有跨进程 durable marker。
- 候选参考：`99de56bb`；完整 recovery journal `ab76a307` 不属于本任务。

## Requirements

- `R1`：执行破坏性删除前以原子、持久方式建立 maintenance marker，并在成功完成后清除。
- `R2`：应用启动最早阶段检测 marker，在日志、数据库、网关和后台任务启动前完成重试或进入明确维护失败状态。
- `R3`：保留当前 Risky IPC confirm、gateway lifecycle lock、CLI proxy restore 和 DB reset guard。
- `R4`：marker 写入、清除和损坏状态必须 fail closed；路径受 app data root 所有权约束。
- `R5`：重置成功后不得在旧进程继续以半初始化状态运行；重启/退出行为与候选实现适配当前 Tauri 生命周期。

## Acceptance Criteria

- [ ] 删除任一步骤失败或模拟崩溃后，下一次启动在创建 DB/日志/后台 worker 前重试维护。
- [ ] marker 损坏、不可读、不可写时应用不进入普通运行态。
- [ ] 成功重置清除 marker，重启后生成全新合法设置与数据库。
- [ ] 当前网关停止、CLI proxy restore 和 DB pool 释放测试继续通过。
- [ ] 不引入通用 filesystem recovery journal 或扩大删除根目录。
