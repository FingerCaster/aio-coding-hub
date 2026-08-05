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

## 实现提交

- `88673001`：最终 upstream HTTP 错误响应改写。
- `d884f830`：Codex HTTP 200 SSE 流内错误恢复。
- `636fdd04`：统一“上游错误处理”入口、分段模式及保存隔离。
- `6c515332`：backend/cross-layer 长期合同与执行清单。

## 最终自动化证据

- `pnpm test:unit -- --reporter=dot`：303 个测试文件、2714 项测试通过。
- `pnpm build`、`pnpm typecheck`、`pnpm lint`、`pnpm check:generated-bindings`、
  `pnpm check:spec-links`：通过。
- `cargo fmt --all -- --check`、`cargo check --all-targets --locked`、
  `cargo clippy --all-targets --locked -- -D warnings`：通过。
- `cargo test --lib --locked`：2580 通过、0 失败、4 ignored。
- guard/backoff 边界使用 paused time；同 Provider 只等待一次，切换/终止不等待。
- Trellis 三个任务 context 校验通过；既有 failover 合同超过 32 KiB 注入上限，仅产生
  截断警告，不影响完整文件读取或验证。

## 有意差异

- 迁移从 fork 当前 SQLite 42/settings 55 顺延到 43/56，不移植参考仓库后续无关迁移。
- 保留 fork 的 circuit probe/failback、Provider route、Codex continuation、
  health-neutral 辅助请求、fake200 和压缩请求体正常化路径。
- 复用 `12e565c0` 的统一 transport backoff helper，不复制参考实现的旧等待位置。

## 待补证据

- 本地 Vite 服务已在 `http://127.0.0.1:5174/` 启动，但当前会话浏览器控制端返回
  `Browser is not available: iab` 且可用浏览器列表为空。按浏览器技能约束未改用无关
  自动化后端，因此桌面/移动截图、真实模式切换、弹窗和保存检查仍待浏览器实例可用后完成。
