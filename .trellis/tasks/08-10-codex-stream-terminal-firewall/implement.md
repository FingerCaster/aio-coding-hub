# 实施清单

## A. 合同与设置模型（先于流运行时）

1. [ ] 在 Rust 设置 wire 层定义 `passthrough_keywords` 和一版
   `legacy_retry_keywords` 兼容读取；保留旧 `non_retry_keywords` / `retry_keywords` 的
   双读，不让新 UI 继续生成旧字段；将 `stream_internal_errors.enabled` 定义为完整动作
   总开关，显式 `false` 必须旁路 capacity 与其他终态的拦截/retry/failover/projection。
2. [ ] 将 settings schema 升级到 59，更新迁移、规范化、长度/控制字符校验、全局默认值和 Provider override
   读写；确认完整 override 仍替换全局策略，不发生规则拼接。
3. [ ] 实现 Provider share v4，保留 v1-v3 读取；补 v4 往返、旧客户端明确拒绝和未知字段测试。
4. [ ] 重新生成 `src/generated/bindings.ts`，同步 `src/services/gateway/upstreamRetryPolicy.ts`
   的 clone/default/validate/patch 类型。

## B. 结构化终态分类

5. [ ] 在 `src-tauri/src/domain/usage.rs` 集中实现终态字段提取、类别枚举、优先级和
   evidence 投影；删除分类对公开 retry/non-retry 关键词的直接依赖。
6. [ ] 覆盖容量别名、`service_unavailable_error`/`server_error`、quota/auth/invalid/policy
   和 unknown 的结构化 fixture；验证硬性非重试优先于旧兼容词，容量优先于透传词。
7. [ ] 将旧 `legacy_retry_keywords` 限制为 unknown + pre-commit 的兼容覆盖，并在 evidence
   中记录兼容 disposition，不改变共享预算计算。

## C. Failover 前置路径

8. [ ] 修改 `inspect_buffered_event_stream_prefix` 返回显式的终态动作，不再把 unknown/
   non-retry 当作 `StartStreaming` 原样发送。
9. [ ] 让瞬态分类复用 `record_buffered_provider_failure` 的 retry budget/backoff/circuit；
   让 quota/auth/invalid/policy/unknown 走标准、脱敏的终态 response；透传例外只允许当前
   Provider 的唯一尝试结束。
10. [ ] 保留 gzip 解码、guard、prefix cap、probe、OAuth quota、session reuse 和
    continuation 的既有时序；所有实际 upstream evidence 在路由决策完成后才投影给客户端。

## D. 提交后 firewall

11. [ ] 新增有界 `terminal_firewall` stream helper，复用 `proxy/sse.rs` 的完整帧解析，支持
    跨 chunk、同 chunk 多帧、LF/CRLF、partial tail、EOF 和 downstream abort。
12. [ ] 在 `UsageSseTeeStream`/`spawn_usage_sse_relay_body` 中保持 tracker 先读原始 chunk，
    再把 firewall 的可见 bytes 送入 relay；默认丢弃提交后的终态错误并结束，不伪造终态帧。
13. [ ] 让 `response.completed` 只发送一次，迟到终态错误不能覆盖已完成响应；透传例外记录
    `passthrough_exception`，其余记录 `dropped_after_commit`。
14. [ ] 将 firewall disposition 和原始 evidence 接入 finalize/request-log；检查 debug/event/
    attempt/client projection 没有上游原文、容量码或凭据泄漏。

## E. 前端配置与文案

15. [ ] 将 `RetryPolicyFields` 的两个关键词文本框替换为一个透传例外输入，更新开关、说明、
    Provider override 语义和旧配置迁移提示；不改变 HTTP/transport retry rows。
16. [ ] 更新前端测试、settings fixtures、generated binding 检查和最长文本/窄宽度场景。

## F. 回归与质量门

17. [ ] 更新未知终态路由测试，加入截图复现帧，断言客户端不含原文而日志 evidence 保留。
18. [ ] 增加 pre-commit retry/failover、hard non-retry、passthrough exception、post-commit
    split/multi-frame/CRLF/drop、bridge canonical frame、malformed/cap，以及
    `enabled=false` 下 capacity/unknown 原流透传且零 stream-internal retry 的测试。
19. [ ] 运行聚焦 Rust tests、frontend unit tests，再运行完整 Rust suite、`pnpm typecheck`、
    `pnpm lint`、`pnpm tauri:fmt`、`pnpm tauri:check`、`pnpm check:generated-bindings`、
    `git diff --check` 和相关 Trellis quality check。
20. [ ] 更新 upstream error handling contract，明确 `enabled` 总开关替换“关闭仍硬拦截
    capacity”的旧规则；记录旧字段迁移计数/兼容窗口结束版本，完成一次基于 PRD/design
    的代码审查后再归档。

## 依赖与回滚点

- B 依赖 A 的运行时字段和 precedence；C 依赖 B 的分类动作；D 可与 C 并行编写但必须在
  同一 contract fixture 上集成；E 依赖 A 的生成绑定；F 等 A-E 合并后执行。
- 最小回滚点是关闭 firewall；此时 capacity 与其他终态一起恢复当前原流行为，但内部
  tracker/evidence 仍保留并标记 `disabled_passthrough`。
- 由于设置 schema、流行为和日志契约必须原子一致，本任务暂不拆成可独立归档的子任务。

## 启动前检查

- [x] 用户确认 pre-commit unknown/硬性非重试的客户端终态采用现有标准
  `502/GW_FAKE_200` envelope，不伪造 SSE 终态帧。
- [x] 用户确认 `enabled` 缺失/新安装默认 `true`，已有显式 `false` 在迁移后保持关闭。
- [x] 用户确认 settings schema 59 + Provider share v4 的一版兼容策略；旧客户端必须拒绝 v4。
- [x] 本次不新增伪造 SSE 兜底 payload；提交前使用现有 502/GW_FAKE_200，提交后丢帧结束。
- [x] 两个上下文 manifest 已填入真实 spec/research 条目并通过 `task.py validate`。
