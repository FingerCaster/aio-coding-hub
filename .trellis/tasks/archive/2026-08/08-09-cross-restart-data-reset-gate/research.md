# 研究摘要

- `commands/data_management.rs:61-83` 已持有 gateway lifecycle lock、停止网关、恢复 CLI proxy、调用 `prepare_db_reset`。
- `app/app_state.rs` 在 reset guard 下释放 cached DB；`infra/data_management.rs` 仍在当前进程顺序删除设置/SQLite/日志。
- 没有 durable reset marker，也没有在 logging/DB/background workers 之前运行的 maintenance-only bootstrap gate。
- 候选 `99de56bb` 增加 marker、下一进程优先 cleanup、maintenance retry/exit；应适配现有锁，不覆盖它们。
- 候选 `ab76a307` 的通用 recovery journal 及 Plugin/Skills/Provider Sync replay 明确排除。
