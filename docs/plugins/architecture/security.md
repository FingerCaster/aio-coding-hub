# 插件安全与隔离

插件系统围绕最小 host surface 和运行时隔离设计。默认 vNext hook timeout: 5000 ms。

核心规则：

- Extension Host 是唯一 community runtime。
- 不在 Rust 主进程或 Tauri WebView 执行第三方插件代码。
- Extension Host 只暴露 capability-gated APIs。
- Gateway hook timeout 是 host-owned invocation budget，由 gateway pipeline 传入 runtime executor；Extension Host 使用该预算启动和执行 hook，executor 不另行固定或放大 timeout。
- Legacy WASM、process 和第三方 native 都是 unsupported pre-release legacy runtime。
- Hook 失败必须记录审计事件。
- 高风险 hook 可以使用 fail-closed 策略。
- 当前 `log.beforePersist` 默认 fail-open；hook 失败、超时或返回非法 payload 时保留原始日志继续入库。它不是强制合规日志脱敏边界，除非后续补宿主兜底脱敏、丢弃日志或专用策略。
- 重复 runtime failure 可以让插件进入 `quarantined` 状态。

未签名离线包会受到限制。High 和 critical host-mediated labels 只用于风险展示、审计和未来 API 设计；社区 Extension Host manifest 不能通过 top-level `permissions` 申请它们。
