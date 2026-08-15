# 移植 CX2CC 嵌套首字节预算与 reentry 直连

## Goal

将旧分支提交 `27efa801`、`39f85ae5`、`d7921ee5` 的有效语义最小移植到最新
`origin/main`。当前 main 已包含 effort mapping、本地重试、Codex stream firewall 和
continuation repair；本任务不得用旧分支的大文件覆盖这些行为。

## Requirements

- null-source CX2CC 的 Claude -> 当前 AIO Codex Gateway hop 是调度包装层；其 response
  headers 与 SSE first chunk 均委托给内层真实 Codex Provider attempt 的首字节预算。
- 普通 Provider、显式 CX2CC source 和内层真实 Provider 保持现有首字节超时、重试退避、
  failover、circuit、compact minimum、stream-idle、non-stream total timeout 与 0-disabled
  语义。
- timeout owner 必须由现有 typed `InternalCodexReentry` 匹配与最终 `SelfLoop` 校验共同
  生成的私有 attempt target 派生，不能由 URL、`source_provider_id == None`、
  `cx2cc_active` 或响应阶段的 mutable state 猜测。
- 合法内部 target 继续经过最终 URL/self-loop 校验；一次性 nonce 在插件和 fingerprint
  之后注入、入站最早阶段消费并剥离。内部 transport 使用 no-proxy、no-redirect client，
  普通 Provider 仍遵循现有代理策略。
- 不泄漏私有 nonce 到插件、fingerprint、attempt/log 或外部 upstream；错误 target、
  replay、真实内层超时和真实客户端 abort 必须 fail closed/保持既有分类。
- 保留当前 Codex `response.completed` 可见输出契约：完成事件最多一次，合法 output
  文本可见，迟到终态错误不覆盖已提交完成事件。

## Acceptance Criteria

- [x] typed intent 同时控制 send/header 与 SSE first-chunk timeout，单测覆盖
      External/Nested、Some/None 和 owner 一致性。
- [x] 受控延迟 local hop 超过外层配置仍成功且测试有明确 wall-clock 上界；保留真实 Provider
      retry/timeout 回归，外层不伪造 524。
- [x] 内层真实 Provider header/first chunk 超时仍为既有 524；显式 source 与普通路径
      的 timeout 回归通过；真实 client abort 仍为 499。
- [x] loopback/reentry 在系统代理和显式代理环境下直连，代理零请求且私有 header 不穿越；
      普通 external Provider 仍走代理。
- [x] `response.completed` 输出文本/单次完成事件测试通过，现有 firewall/continuation
      repair 回归不变。
- [x] focused Rust、fmt、clippy、bindings/spec links 与 task validate 通过或记录
      明确环境缺口；提交只包含本任务文件与实现改动。

## Notes

- 外部发布父任务（当前 checkout 不可见）：`08-15-merge-balance-fix-beta-release`；本任务
  作为独立 task 执行，结果在交付说明中注明。
- 不触及 settings/schema、数据库迁移、前端或 generated bindings（除非验证发现合同必须
  同步）；不改 `continuation repair` 实现，只保留其专项回归门禁。
