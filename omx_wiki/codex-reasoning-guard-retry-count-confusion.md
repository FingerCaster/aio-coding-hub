---
title: "Codex reasoning guard retry count confusion"
tags: ["codex-reasoning-guard", "continuation-repair", "retry", "request-log", "ui-copy"]
created: 2026-07-04T09:48:26.129Z
updated: 2026-07-05T11:13:07.453Z
sources: []
links: ["codex-reasoning-continuation-response-contract.md"]
category: debugging
confidence: medium
schemaVersion: 1
---

# Codex reasoning guard retry count confusion

后续修复记录：用户提供的问题机器 attempts_json 显示第 1 次外层请求触发 codex_reasoning_guard_retry，reason 包含 continuation_repair、hit=1、phase=immediate、action=retry_same_provider_no_circuit；第 2 次外层请求 retry_index=2 已经 status=200/outcome=success。诊断结论：只重试一次是因为第二次外层请求已成功，预算语义是最多重试而不是跑满 3 次。用户设置的 3 次更可能是 codex_reasoning_guard_continuation_max_rounds，即一次请求内部 continuation repair 的最大轮数，不等同于外层 guard retry 次数。attempts_json 只记录外层 attempt，不展示内部 continuation repair 每轮。截图/详情文案里的“最终状态：补救失败”容易误导，因为它描述的是 continuation repair 本身失败，而最终请求可能已经通过预算重试成功。后续修复方向：详情页文案区分“补救状态”和“最终请求结果”；在日志/详情中展示 sentRounds、maxRounds、continuationRepairStatus、guard retry budget 使用情况；如需要机器定位，补充 machine_id/hostname/source_instance 字段。

相关契约：[[codex-reasoning-continuation-response-contract]] 记录了 continuation repair 成功后只暴露一个连贯的用户可见 assistant response；中间轮可见文本不能直接合并给客户端，内部轮次只用于安全判定、诊断、回放和 provider usage accounting。
