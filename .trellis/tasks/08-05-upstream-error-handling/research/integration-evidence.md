# 集成证据

## 固定基线

- 当前 `HEAD`: `12e565c0e7fbcb461f0ccb0fccaa5274846f8185`。
- 参考 `HEAD`: `5b13683bd2a44699cd8c99e7aeffc317bcc19674`。
- 共同祖先：`1a551cbee35960fbb954e475a13b2d8d55d709df`。
- 功能对象在本 clone 可读取，但参考提交与当前 fork 已显著分叉。

## 参考提交范围

- `26c4e02`：最终响应改写，42 个文件、约 3190 行新增。
- `957b649`：Codex 流内错误，74 个文件、约 3455 行新增。
- `358a999`：迁移/日志修正，4 个文件。
- `ca15f02`：PR #31 最终合并状态。

## 必须语义移植的证据

- 参考 `success_event_stream.rs` 补丁会删除当前文件已有的 probe terminal commit 辅助逻辑，不能整体替换。
- 当前 `routes.rs` 含大量 ordered failback、probe、session convergence 和 circuit 回归测试，参考功能提交基线不具备相同结构。
- 当前 `12e565c0` 已统一 transport backoff 的最终决策时机；stream-internal retry 必须接入同一 helper，不能复制参考提交中的旧等待位置。
- 当前 `a7e7675c` 在 Provider 选择前把目标 Codex 请求体正常化为 identity bytes；响应 guard 不应接触这一请求边界。

## 参考规范结论

- 最终响应规则只匹配最终 upstream HTTP 4xx/5xx，使用原始 status/body/CLI/Provider facts。
- 高优先级规则正文无法安全评估时停止并 fail open，不跳过到低优先级规则。
- stream-internal retry 与 HTTP/transport 共享 `max_retries`、backoff 和 circuit accounting。
- guard cap 是放行诊断，不计 Provider failure；下游提交后禁止重试拼接。

