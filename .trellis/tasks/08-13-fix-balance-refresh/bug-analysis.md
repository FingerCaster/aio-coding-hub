# Bug Analysis: 余额恢复后仍需先测试 Provider

## 1. Root Cause Category

- **Category**: B - Cross-Layer Contract
- **Specific Cause**: 手动刷新已经绕过 TanStack Query 和 Rust runtime 的本地快照，
  但内置 sub2api、NewAPI billing/account 与 custom adapter 的最终 HTTP 请求没有
  显式绕过代理/CDN 缓存。一次新的 IPC 因此仍可能得到旧的零余额响应。次要类别
  是 D（真实请求头合同缺少回归）和 E（隐式假定“新请求”等于“权威新响应”）。

## 2. Why The Previous Fix Did Not Cover This Case

1. 归档任务修复了本地完成身份：普通后台完成不能满足手动 force waiter，在途请求
   完成后必须派发唯一尾随强刷。该修复仍然有效，现有异步测试全部通过。
2. 之前的测试验证了新 IPC、runtime generation 和 query cache 的提交顺序，但没有
   检查实际发往上游的 HTTP 缓存指令；测试中的可控 fetcher 天然每次返回新值。
3. Provider 可用性测试是独立 POST 探针，不更新本地账户用量缓存。它可能改变上游
   请求时序或缓存状态，因此在用户观察中掩盖了真正缺口。

## 3. Prevention Mechanisms

| Priority | Mechanism | Specific Action | Status |
| --- | --- | --- | --- |
| P0 | Architecture | 为 RequestBuilder 与已构造 Request 提供一个共享账户用量缓存绕过 helper | DONE |
| P0 | Change propagation | sub2api、NewAPI billing/account 和 custom adapter 的最终请求全部调用该 helper | DONE |
| P0 | Test coverage | 单测断言两种 helper，并在真实 NewAPI 请求捕获中逐请求断言两个缓存头 | DONE |
| P1 | Documentation | 更新账户用量可执行契约和 cross-layer forced-refresh 检查项，并同步模板 | DONE |

## 4. Systematic Expansion

- **Similar Issues**: 任何标记为 force/reload/revalidate 的动作都可能只绕过最近一层
  缓存；UI、进程快照、single-flight、HTTP 中间层和上游服务必须分别审计。
- **Design Improvement**: 缓存指令由共享请求边界强制覆盖，适配器和 custom script
  不能各自遗漏或把刷新与另一个探针绑定。
- **Process Improvement**: 强制刷新回归除验证调用次数和最终状态外，还要捕获最终
  网络请求，断言其缓存语义；相似症状不得默认沿用上一次根因。

## 5. Knowledge Capture

- [x] 更新 Provider account-usage 可执行契约。
- [x] 更新 cross-layer thinking guide。
- [x] 同步 `src/templates/markdown/spec` 的相关规范与指南。
- [x] 增加共享 helper 与真实 NewAPI 请求头回归。
- [x] 明确 Provider 可用性测试不是账户刷新或恢复的前置条件。
