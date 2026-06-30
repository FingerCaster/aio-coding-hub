# 插件安全与隔离

插件系统围绕最小 host surface 和运行时隔离设计。默认 vNext hook timeout: 150 ms。

核心规则：

- Extension Host 是唯一 community runtime。
- 不在 Rust 主进程或 Tauri WebView 执行第三方插件代码。
- Extension Host 只暴露 capability-gated APIs。
- Legacy WASM、process 和第三方 native 都是 unsupported pre-release legacy runtime。
- Hook 失败必须记录审计事件。
- 高风险 hook 可以使用 fail-closed 策略。
- 重复 runtime failure 可以让插件进入 `quarantined` 状态。

未签名离线包会受到限制。High 和 critical host-mediated labels 只用于风险展示、审计和未来 API 设计；社区 Extension Host manifest 不能通过 top-level `permissions` 申请它们。
