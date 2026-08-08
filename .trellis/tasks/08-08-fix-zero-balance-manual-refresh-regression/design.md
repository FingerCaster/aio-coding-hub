# 零余额手动刷新回归修复设计

## 目标

把手动刷新从“等待若干任意完成通知”收紧为“等待本次合并强刷批次完成”。fresh 零余额、
后台请求在途、关闭定时刷新和多 UI 消费者都必须遵守同一语义。

## Runtime 状态模型

- 为显式强刷引入单调递增的 force epoch。
- `pending_force_epoch` 表示尚未开始、可供同时到达的手动调用者共享的强刷批次。
- `in_flight_force_epoch` 标识当前网络请求是否属于显式强刷；普通自动请求没有 force epoch。
- `completed_force_epoch` 只在对应强刷结果通过 generation/config token 校验并提交后推进。
- 手动调用保存目标 generation/config token 和所需 epoch；每次被唤醒先验证 target 身份，
  再判断该 epoch 是否完成。普通后台完成和生命周期通知不能使它提前返回旧快照。
- 若任意请求在途，第一次显式 force 排一个 pending epoch；同一窗口后续调用共享它，保证
  当前请求之后至多一次尾随强刷。若 pending 强刷已经开始，之后的新 force 才进入下一批。

## 调度

- scheduler 增加进程内唤醒信号。请求完成后如仍有 pending force，释放并发 permit 后立即唤醒，
  不等待固定 tick。
- pending/in-flight force 本身视为活跃消费者，避免 UI heartbeat、Gateway lease 或定时刷新开关
  决定显式 refresh 能否收尾。
- 网络并发上限、Provider generation 提交保护和 route snapshot 发布顺序保持不变。

## 前端与 IPC

- 保留 snapshot/refresh 两个命令及现有 exact-key cancellation；不新增 Provider 测试依赖。
- 手动 refresh 仍由同一个 TanStack Query key 提交权威结果，旧自动 Promise 的逆序完成继续被取消
  语义隔离。
- 不改变生成 bindings 的公开参数，修复位于所有适配器共享的 runtime。

## 测试

- 增加可控 fetcher 的 runtime 异步测试，先复现 fresh zero 恢复和旧请求在途场景。
- 断言调用数、请求先后、合并批次、调用者返回结果、最终 snapshot 与 route projection。
- 保留/扩展前端 Query deferred 测试，证明 exact-key 写入及无 availability/circuit/Provider 副作用。
- 运行账户用量 focused tests、Rust/前端全量质量门。

## 回滚

无 schema、配置或 IPC 迁移。产品变更可通过回滚 runtime 状态机提交恢复；前端查询键与持久化数据
均不需要清理。
