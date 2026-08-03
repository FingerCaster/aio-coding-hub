# 上下文压缩信号证据

## 设计结论

自然回切应使用“压缩成功后的下一轮请求”，而不是压缩请求本身：

```text
当前稳定 provider=P2
  -> 压缩请求继续走 P2，保留已有 prompt cache 优势
  -> 压缩请求成功完成
  -> session 递增 completed_compaction_generation
  -> 下一轮正常请求取得 P1 probe lease
     -> 成功：恢复 P1，并按自然策略切回
     -> 失败：回退 P2，原绑定保持 P2
```

只有成功终态可以设置 pending；超时、取消、非成功 HTTP、fake-200、流读取失败或未分类终态都不能把失败压缩误记为完成。

## Claude

- `src-tauri/src/gateway/proxy/handler/middleware/model_inference.rs:22-27` 固定 Claude Code `/compact` system prompt 前缀。
- 同文件 `:58-68` 在 provider resolution 前写入 `ctx.is_compact_request` 和请求诊断。
- 同文件 `:92-117` 只接受 `claude + POST /v1/messages + system[0].text` 前缀匹配，避免对普通对话文本误判。
- `src-tauri/src/gateway/proxy/request_context.rs:250-266` 已明确 `/compact` 会使上游 prompt cache 失效，并为该请求放宽首字节超时。
- 当前 `is_compact_request` 会进入 `RequestContext`，但成功终态没有写回 session 的 compaction generation/pending 状态。

推荐：压缩请求仍按现有 session binding 路由；流式和非流式成功终态共同调用一个 session API，按 session key 递增 `completed_compaction_generation`。

## Codex remote compaction

- [OpenAI API Reference](https://platform.openai.com/docs/api-reference/responses/compact) 定义 `v1/responses/compact`，返回 `type=compaction` item。
- 当前 gateway 可透传任意 Responses 路径，但 `ModelInferenceMiddleware` 的压缩分类只覆盖 Claude，尚未分类 `POST /v1/responses/compact`。
- 推荐把严格路径、方法和 `cli_key=codex` 分类为压缩请求；只有完成且可验证的成功响应才递增 session compaction generation。

## Codex 请求内 compaction item

- `src-tauri/src/gateway/proxy/protocol_bridge/inbound/openai_responses.rs:176-178` 会在规范化 Responses input 时识别 compaction item。
- 同文件 `:290-318` 把带 summary 的 compaction 转成 developer message，空 compaction 被跳过。
- `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:1400-1450` 覆盖原生 compaction item 的保留。

这表示网关可以在 provider resolution 之前直接检查原始 `input[]` 是否包含严格 `type=compaction`。该请求已经处于压缩后的上下文，可直接视为自然回切候选，不需要先等待一个额外轮次；但仍需保持 session identity 连续并拒绝仅在普通文本中出现的伪标记。

## Grok 与 Gemini

当前 gateway 没有经过验证的压缩请求或压缩完成标记。仅凭消息数量骤减、body 大小下降、token 使用变化或 system 文本变化推断压缩会产生误判，不建议作为切换 provider 的依据。

最终决定：Grok/Gemini 以及其他没有可靠信号的路径使用 provider 级 `natural_probe_max_wait_seconds` 兜底，默认 300 秒。兜底只让下一条合格真实请求争取全局 single-flight lease，不生成后台请求，也不为每个 session 单独计时。

## Session 状态建议

为避免一个布尔值被并发请求重复消费，session binding 应使用 generation：

- `completed_compaction_generation`
- `natural_probe_consumed_generation`
- 带 owner 的短期 trigger reservation

请求仅在 `completed > consumed` 时保留自然试探机会。cooldown、single-flight 竞争或发包前错误只释放 reservation，不推进 consumed；一旦进入真实 transport send 边界即消费，之后成功或失败都不复用同一代次。该 reservation 需与 circuit probe lease 协调，不能依赖普通 `bind_success` 隐式覆盖。
