# 技术设计

## 1. 根因与边界

无限重试 SSE 路径必须在下游提交前收集完整 transformed final wire，再调用共享严格 validator。
当前实现同时使用两类时间约束：

1. 每次读取的 `upstream_stream_idle_timeout`，用于发现上游停止推进；
2. 包裹整个 collector 的固定 500 ms `tokio::time::timeout`。

第二类约束把完整生成时间误当成 TTFB。真实 Codex 在首字节后继续生成数秒或更久，因此即使
持续输出并最终合法完成，也会在 500 ms 被丢弃。修复只删除第二类约束，不改变 collector 所在
阶段、响应转换顺序或成功提交门。

## 2. Collector 数据流

```text
response headers / first event
        |
decode -> bridge -> response fixer -> response plugin
        |
collect final wire (20 MiB cap)
        |  each read is guarded by configured stream idle timeout
        v
strict validate_complete_codex_sse
        |
        +-- valid   -> commit one buffered success
        +-- invalid -> discard and continue Provider/round retry
```

`collect_bounded_final_wire` 恢复为单层循环。每次 `next_event_stream_chunk` 可由 idle timeout 包裹；
收到 chunk 后检查剩余 20 MiB 容量并追加，EOF 时返回完整 bytes。函数本身不再拥有总墙钟
deadline。客户端取消或网关关闭仍通过上层 future drop/abort 生命周期打断收集。

## 3. Supported path 合同

删除 `CodexResponsesPathContract.min_enabled_ttfb_secs`，把共享常量简化为
`CODEX_RESPONSES_PATHS: &[&str]`。eligibility 和 event-stream path 判断继续使用同一列表，避免
路径漂移；完整响应收集不再消费 TTFB 元数据。

## 4. 测试设计

- 新增有限慢流 fixture：按固定周期输出若干 SSE 片段，然后 EOF。
- 暂停时间推进到大于旧 500 ms 边界，断言 collector 返回完整字节，且共享严格 validator 接受
  唯一合法 `response.completed` 与其后的 `[DONE]`。
- 保留无限慢流 fixture 验证 idle timeout：首个 chunk 到达前的 gap 超过 idle budget 时，按准确
  budget 返回 `IdleTimeout`。
- 现有 eligibility/path、20 MiB、终态 validator、first-event timeout 和取消/关闭测试作为回归。

## 5. 兼容性、风险与回滚

- 默认关闭和普通请求路径完全不变，无设置或数据迁移。
- 开启测试模式后，如果上游永远持续输出且每个 gap 都小于 idle timeout，单次 attempt 可长期
  运行。这是原始 R15 明确允许的测试模式语义；20 MiB 上限、客户端取消和网关关闭仍提供资源与
  生命周期边界。
- 若未来产品需要完整响应总 timeout，应新增独立、语义正确且可配置的 response deadline，不得
  再复用 TTFB floor 或硬编码 500 ms。
- 回滚只需恢复本任务代码与 spec 差异；无持久化状态需要回滚。
