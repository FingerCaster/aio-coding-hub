# 技术设计：typed timeout owner 与本机 reentry transport

## 现状与决策

最新 `origin/main` 已有 `InternalCodexReentry`（CX2CC null-source 准备时创建）、运行时
一次性 nonce registry、入站早期消费，以及 `direct_internal_reentry_client()`。发送层目前
在消费 intent 后以 `authorized_internal_reentry: bool` 只委托 `.send()` 的首字节 timer；
SSE handler 则仍使用 `_prepared` 与通用外层预算，形成两个独立判断。

本任务保留 `PreparedProvider.internal_codex_reentry` 及其共享 `OnceLock` 消费语义，不在
preparation 阶段提前关闭 timeout。最终 intent match 与普通 target validator 的
`SelfLoop` 结果同时成立后，`attempt_executor` 才创建私有的 attempt target：

```rust
enum AttemptTarget {
    ExternalProvider,
    InternalCodexReentry,
}

struct AttemptTiming {
    target: AttemptTarget,
}

AttemptTiming::response_header_timeout(configured);
AttemptTiming::sse_first_chunk_timeout(configured);
```

该 enum 与 `AttemptTiming.target` 都不向路由输入暴露。transport header timer 使用
`response_header_timeout`；同一个 `AttemptTiming` 经 `response_router` 借给 SSE handler，
由 `sse_first_chunk_timeout` 决定首 event deadline。普通/显式 source 没有 intent match，
因此保留 configured timeout。

## 请求时序

```text
Claude CX2CC null-source
  -> typed InternalCodexReentry
  -> plugin/body finalization
  -> fingerprint (without private nonce)
  -> final URL/self-loop validation
  -> one-time nonce + direct no-proxy/no-redirect client
  -> Codex ingress consumes/strips nonce
  -> inner Provider attempts own timeout/retry/failover
```

若最终 URL 不是 intent 精确绑定的当前 Gateway self-loop，或 nonce 不能 issue/consume，
请求健康中性拒绝；不得退化为 localhost 白名单。普通 target validator、configured model
route skip（CX2CC/可信 reentry）、Provider enabled reread 与 dispatch boundary 保持不变。

## Timeout ownership

- `AttemptTarget::ExternalProvider`：两个投影都返回 configured first-byte duration。
- `AttemptTarget::InternalCodexReentry`：两个投影都返回 `None`。
- inner Codex request 在 ingress 后重新构造正常 runtime settings，因此真实 Provider 仍
  获得 configured timeout、configured retry/backoff、524 映射；delegation 不关闭 stream
  idle 或 non-stream total timeout。
- cancellation 继续由现有 abort guard/drop 传播；不把 timeout 改写成 499。

## Transport / proxy boundary

保留现有 `build_direct_internal_reentry_client()` 的 `.no_proxy()` 与
`Policy::none()` 及其 proxy/redirect listener 回归。私有 nonce 只在 fingerprint 后写入
最终 headers，入站移除后不得出现在插件观察、attempt/log 或真实 Provider 请求。

## Response contract

不修改 `terminal_firewall`、Codex continuation repair 或 effort/local-retry 实现。以现有
Codex SSE route fixture 增加/保留断言：`response.completed` 只出现一次，合法
`output_text` 仍可见，迟到 `response.error` 不覆盖完成输出。该测试同时防止把 timeout owner
移植误改成整流/终态投影变更。

## 风险与回滚

改动集中在 attempt executor、response router、success event stream、聚焦测试与
attempt-budget/CX2CC contract。回滚只需移除私有 attempt target 及 SSE 投影调用，现有严格
self-loop 拒绝和普通 timer 会恢复；无持久化迁移。
