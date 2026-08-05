# Codex 流内错误拦截与重试

## Goal

在原生 Codex Responses HTTP 200 SSE 的下游响应提交前识别可重试的终止错误，复用现有 Provider retry/failover/backoff/circuit 策略，避免容量错误直接落到客户端；同时用限长、脱敏、结构化 evidence 完善请求日志。

## Confirmed Facts

- 仅处理未桥接的 /v1/responses、/responses、/v1/codex/responses 原生 Codex Responses SSE；Claude/Gemini/Grok/通用 SSE 不在首版范围。
- 当前 success_event_stream.rs 同时承载 SSE prefix、relay、probe terminal commit、fake200 与 Codex continuation 语义；参考文件不能整体覆盖。
- 当前 transport retry backoff 已把等待放在最终 RetrySameProvider 决策之后；stream-internal retry 必须复用该位置并避免二次等待。

## Requirements

1. 识别 SSE event 或 data.type 为 error、response.error、response.failed、response.incomplete 的帧；只从已知 error envelope 字段提取 error.message/type/code、response.error.message/type/code、顶层 message/code。
2. metadata 帧不启动 guard。首次真实文本、拒绝、推理摘要、工具参数、具体 output/tool item 才启动保护窗。默认 500ms，可配置 0..=5000ms；每请求缓冲上限 1 MiB。
3. 默认启用；正向关键词默认 selected model is at capacity，禁止关键词默认包含 invalid_request、content_policy、policy、safety、high-risk cyber、not allowed、violat。列表可编辑，大小写不敏感字面子串，正向词优先。
4. guard 内命中可重试错误时丢弃当前缓冲，调用当前 Provider 的现有 UpstreamRetryPolicy。共享 max_retries/backoff_ms/configured retry budget/counts_toward_circuit_breaker；额度耗尽后切 Provider，跨 Provider 不额外退避。
5. 所有 Provider 失败返回标准 502/GW_FAKE_200，不得把原始 HTTP 200 capacity SSE 下发给 Codex。
6. guard 到期、1 MiB cap、下游已提交后不可靠重试：保持原始 SSE 行为并记录诊断；禁止拼接两次输出。cap 放行不计 Provider failure。
7. 在可见 retry 规则中提供 HTTP 400 + capacity 默认规则。迁移全局策略和 Provider 完整覆盖时尊重等价规则、全 400 规则和显式禁用意图，避免重复。
8. 每次错误记录 event/type/code/message/classification/matched_keyword/disposition/truncated；message 最多 2048 Unicode 字符，常见 Bearer/API key/access token 等必须清理。不得持久化原始 SSE、普通输出或完整上游响应。
9. 重试成功仍在 Provider chain 保留早期 evidence；最终失败的 error_details_json 投影末次 evidence。前端只能消费结构化 attempts/evidence，并提供复制已脱敏 message 的按钮。
10. 设置、Provider override、分享/导入/备份、迁移与 generated TypeScript bindings 必须保持字段完整。
11. 保持现有非流 fake200、Codex continuation、count-tokens、health-neutral、strict helper budget、Provider override、分享/导入/备份行为。

## Acceptance Criteria

- [ ] 同 chunk/跨 chunk、大小写变化、event/type 组合的四类错误均能分类。
- [ ] 正反词同时命中时正向词获胜；禁止词、unknown、disabled、guard 到期或 cap 不重试且原始帧可见。
- [ ] guard 0/500/5000ms、EOF/completion 提前提交与 1 MiB 放行使用 paused time 覆盖。
- [ ] 流内 retry 与 HTTP/transport 共用 Provider configured retry counter/backoff/circuit，且不双重等待。
- [ ] 多 Provider 失败返回 502/GW_FAKE_200 且原始 capacity SSE 不下发；成功结果不拼接早期输出。
- [ ] 全局策略、Provider 完整覆盖、分享/导入/备份和旧设置迁移保持字段完整、幂等且尊重显式禁用。
- [ ] attempts/evidence/error details 均限长脱敏；synthetic credentials 不泄露至日志、前端或导出。
- [ ] Provider chain、最终错误详情、复制按钮和日志列表通过组件/解析测试。
- [ ] focused/full Rust/TS 质量门通过并形成第二个原子提交。

## Out Of Scope

- 不支持 Claude、Gemini、Grok 或通用 SSE 的流内恢复。
- 不实现下游提交后回滚、生成拼接或透明重试。
- 不改变最终 HTTP 4xx/5xx 响应改写规则的匹配和构造语义。
