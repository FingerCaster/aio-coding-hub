# 零余额手动刷新链路核查

## 已核实路径

1. 前端账户用量自动读取使用 `provider_account_usage_snapshot`，手动按钮使用独立的
   `provider_account_usage_refresh`；手动调用会先取消同 Provider 的 exact query key，再由
   TanStack Query 写回结果。
2. Tauri refresh 命令重新加载当前 Provider 账户用量 target，并调用进程级
   `ProviderAccountUsageRuntimeState::refresh`。Provider 可用性测试不读写账户用量缓存，
   也不负责触发该刷新。
3. runtime 对 fresh `zero_balance` 快照不会在手动路径短路：当前实现会设置
   `pending_force`；若已有请求在途，则设置 `tail_force` 并用完成计数等待两个完成事件。
4. Sub2API、NewAPI billing、NewAPI account 与 custom adapter 最终都由
   `fetch_account_usage_uncached` 执行，刷新调度与缓存提交由同一 runtime 所有。

## 已发现的覆盖与协议缺口

- 现有 runtime 测试只直接修改 `pending_force` / `tail_force` 并检查计数，没有运行异步
  调度、远端结果提交和 refresh 调用者返回链路，因此无法证明“零余额 -> 正余额”一次点击
  可恢复。
- `completion_generation` 同时表示配置同步、失效、普通后台请求和强制请求完成；手动调用
  等待的是推算出的计数，而不是自己要求的强制刷新批次。非强刷完成事件可以推进该计数，
  缺少“返回结果确实来自调用之后的权威强刷”这一可验证身份。
- 在途请求结束后，`tail_force` 只转换为 `pending_force`；真正启动尾随请求依赖 1 秒 scheduler
  轮询和短租约，没有完成事件驱动的立即唤醒。现有测试没有覆盖此窗口。
- 前端 deferred 测试覆盖旧自动 Promise 不得覆盖手动结果，但后端没有相应的真实 runtime
  时序测试，因而账户用量所有权迁移后形成了回归盲区。

## 可证伪结论

修复需要让每次合并后的显式强刷拥有独立 epoch：普通后台完成、配置通知或旧请求完成都不能
满足该 epoch；在途旧请求完成后必须立即调度至多一个尾随强刷，调用者只在该强刷提交后返回。
若加入真实异步 runtime 测试后，现实现已经能稳定满足这些条件，则该结论被证伪，应继续沿
前端调度或具体协议适配器定位，不得保留无依据的 runtime 改造。

## 回归矩阵

| 场景 | 预期 |
| --- | --- |
| fresh zero、runtime idle、单击刷新 | 新远端请求返回正余额 |
| 旧自动请求在途、单击刷新 | 旧完成不返回；紧接一次尾随强刷并返回其结果 |
| 多个调用者在同一 pending 窗口 | 合并到同一强刷 epoch，只有一次远端请求 |
| 强刷已经在途时又有显式刷新 | 当前请求后至多再排一个尾随强刷 |
| target generation/config token 改变 | 旧结果不得提交，等待者收到不可用结果 |
| timed refresh 关闭或没有 Gateway lease | 显式强刷仍能独立完成 |
| Provider 可用性测试未执行 | 刷新结果不受影响 |
