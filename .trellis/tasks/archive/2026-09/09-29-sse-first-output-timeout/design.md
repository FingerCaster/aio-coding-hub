# 技术设计：单次尝试的首次有效输出期限

状态：用户于 2026-09-29 批准实施；D1=A、D2=单次，独立 worktree 实现。

## 1. 架构与资格

限定普通、非桥接 Codex Responses SSE，排除 provider_health_mode.bypasses_circuit() 的无限重试完整收集路径。资格复用 success_event_stream.rs:313。外层 CX2CC、其他协议不新增此期限；内部重入如落到普通 Codex 的真实 Provider，由该真实尝试独立执行。

主要文件：

- success_event_stream.rs：有效 idle 配置、截止创建、前置等待、失败记账和交接。
- 新增 gateway/streams/first_output.rs，由 streams.rs 导出内部类型：有界首输出识别和绝对截止状态，仅服务此生命周期。
- streams/usage_tee.rs：承接提前放行后的未满足截止，负责后段读取/发送等待与终结。
- streams/types.rs、finalize.rs：内部终止来源及现有失败/探测收尾。
- GeneralTab.tsx、ProviderEditorDialog.tsx：新增语义说明，字段和数值不变。
- 对应测试及现有 gateway attempt / upstream error 契约。

domain/usage.rs 的计费和空响应分类默认不改。新识别器组合调用 has_codex_meaningful_output，在此流生命周期自己的 helper 中补现场 custom-tool，避免重写共享业务分类。

## 2. 计时语义

| 计时 | 起点 / 重置 | 结束 | 配置 |
| --- | --- | --- | --- |
| 原首字节 | 保持 AttemptTiming / 首块 probe 现状 | 原首字节条件 | 原 first-byte |
| 新首次有效输出 | 已取得符合范围的 SSE 响应头，进入响应处理时建立一次 | 截止前识别有效输出或协议终止 | 同一 effective idle 结果 |
| 原字节 idle 与 500 ms guard | 各自原有规则 | 原规则 | 原 idle / guard |

使用 tokio 单调时间与绝对 deadline，支持 paused-time 测试。新期限不从上一尝试或请求最初时刻继承，也不在首次/后续心跳到达时重建。首字节与首输出同时适用时先到期者生效。

现场示例：先受原 60 秒首字节保护；收到 SSE 响应头后，本次尝试最多等 300 秒有效输出。这不是整个请求合计 300 秒。重试重新建立新尝试预算。

配置沿用正值 Provider > 全局，Provider 0/None 继承，全局 0 且无正值覆盖禁用。EffectiveStreamIdleTimeout 中现为 cfg(test) 的 source/seconds 如需运行诊断，可提升为内部运行时信息并实际使用，不另复制覆盖算法。不新增 settings schema、SQLite migration 或生成绑定字段。

## 3. 状态与可验证边界

内部 FirstOutputWait 维护 started_at、绝对 deadline、budget/source 和 Waiting/Satisfied/Expired 状态。禁用或不符合资格为 None。只有 Waiting 可进入终态；任何字节活动都不重建 deadline。

前置阶段复用 prefix 已解析的完整 JSON 帧，避免重复解析整个累计缓冲。只保留跨 chunk 所需的有界片段，采用现有 1 MiB 级解析边界；满足首输出后释放额外解析状态。

在等待前、读取返回后、处理持续就绪数据的循环中检查期限，不能只在 Stream::Pending 时检查。网关观测 now>=deadline 且此前没有有效输出，判超时；此前已满足首输出则不因之后的 guard 等待而追溯失败。维持现有客户端取消与网关停止优先级。

## 4. 有效输出与协议终止

沿用既有 helper 的非空正文、拒绝、思考摘要、function 参数和具体输出项，另明确支持现场正常流：

- response.custom_tool_call_input.delta：非空字符串 delta。
- response.custom_tool_call_input.done：非空字符串 input。
- 实际 custom_tool_call 输出项：非空 input，或可识别的非空 name 与 call_id。

这些 shape 用合成测试固定，不对任意 delta、JSON 或未知类型放宽。注释保活、空 output 的 created/in_progress、纯 usage 更新、空 delta、未知/未完成/无法识别的帧都不满足新条件。

保留终止错误分类的原有处理。response.completed、error、response.failed/incomplete 交给既有完成、空响应、防火墙或重试路径，不当作任意续命信号。新识别不改 usage、完成状态与计费。

同一首输出识别结果用于本资格范围 prefix guard 的启动，使 custom-tool 进展可进入既有保护窗口后放行，而不是只解除 timer 却仍等 completed。共享 usage tracker 保持原行为，由回归验证其结果。

## 5. 前置期限与恢复

保留首块 probe 的原首字节规则，并以新期限剩余预算约束它；不能先等完一个更长的 probe 才开始新计时。first-byte=0 时，收到响应头后仍执行已启用的首输出期限。

prefix 循环选择原 idle、原 guard、新首输出截止中最早的适用唤醒；使用明确的等待原因 enum，避免原 guard_timeout 布尔值混淆第三种原因。guard 到期仍表示可提交，新截止到期才表示首输出失败。

未提交的新超时沿用 record_system_failure_and_decide 与 stream_transport_decision(Timeout)：

- 保留 GW_UPSTREAM_TIMEOUT 和现有客户端超时映射，不增加公共错误码。
- outcome / reason 明确 stream_first_output_timeout，timeout_secs 来自 effective idle，不能误用 first-byte 秒数。
- 配置重试计数、backoff、counts_toward_circuit_breaker 及切换归属原策略。
- 丢弃失败尝试未提交前缀并释放上游，不让其内容混入下一尝试。

## 6. 缓冲提前放行后的连续性

保留 1 MiB cap 与解析兼容放行。它们是内存/兼容机制，不等于有效输出；不将 cap 改成新 Provider 故障，也不删未知帧。

若放行时仍 Waiting，将同一个 deadline/budget 传入 Codex relay/tee，不能重新开始 300 秒。缓冲仍通过 FirstChunkStream 只交付一次。后段从重放前缀开始有界识别，防止把原不完整尾帧重复拼接；replay 不赋予新预算。

以可选内部参数或内部专用入口接入，其他路径为 None。读取、持续 Ready 与通道发送背压都竞争同一截止，不能因下游不读取而不再轮询 timer。有效输出一旦满足，移除该约束，后续 byte-idle 保持原行为。

提交后到期：释放上游，进行一次最终收尾。沿用 GW_STREAM_IDLE_TIMEOUT 家族，增加内部 StreamTerminalOrigin::FirstOutputTimeout（first_output_timeout）区分原因；不能把已发 HTTP 200 改成 524。现有规则若生成网关错误尾帧，继续使用 relay-owned-tail 和有界结束，不能让防火墙误吞自己的尾帧，也不能为等待尾帧发送而无限持有上游。

该阶段不进入 Provider 重试。更新 finalize.rs 所有 exhaustive match、探测结果和 terminal_details_json，保留已有重复 finalize 防护。新增 origin 是内部枚举值，写入现有 JSON，不引入 API enum 或数据库列。

## 7. 诊断与 UI

- 前置：attempt outcome/reason/timeout_secs 明确首输出等待及实际预算，保持上游状态事实。
- 后置：terminal origin=first_output_timeout，记录有效秒数及 before/after-commit 阶段；后续 EOF 不能覆盖失败为成功。预算证据走已有 special_settings/日志 JSON 表面。
- 仅记录 trace、Provider/attempt 标识、阶段、预算和耗时；不写原始帧、正文、credential 或内存。
- ttfb_ms/visible_ttfb_ms 含义不变，不伪造缺失的语义首 token 时间。
- 全局说明：原块间静默仍受限；普通 Codex 首次有效输出前还使用同一值作为固定等待上限，保活不延长，按单次上游尝试计算。
- Provider 说明：覆盖同样作用于上述首次等待；空值/0 继续继承全局。首字节输入不改。

## 8. 兼容矩阵

| 场景 | 行为 |
| --- | --- |
| 普通 native Codex，effective idle>0 | 启用新期限 |
| 全局 300 / Provider 90 | 使用 90 |
| Provider 0/空值 | 继承全局 |
| 全局 0，无正值覆盖 | 禁用新期限 |
| 全局 0，Provider 正值 | 使用 Provider 值 |
| first-byte=0，idle 启用 | 原首字节仍禁用，取得 SSE 响应头后启用新期限 |
| 桥接、CX2CC 外层、其他客户端 | 不新增此期限 |
| 内部重入真实普通 Codex Provider | 在真实尝试层应用，外层不重复 |
| compact | 原特殊首字节预算和归属不变 |
| 无限重试 test collector | 原收集器与总生成契约不变 |
| 错误拦截 master switch 关闭 | 原终止透传不变，超时按自身设置执行 |
| 截止前已有有效输出 | 新期限永久解除，允许长生成 |
| 无输出但 cap/兼容放行 | 截止移交，提交后只终止当前流 |

## 9. 验证矩阵

| 场景 | 证明 | PRD |
| --- | --- | --- |
| 状态 + 每 15 秒 13 字节保活 | 固定截止不续命 | AC1 |
| 覆盖、继承、禁用 | 同一 effective resolver | AC1a |
| 无/迟首块、LF/CRLF、分块、gzip | 原网络与新语义期限无漏保 | AC2 |
| 截止前/恰到/之后、Ready 热循环 | 边界确定，无 timer 饿死 | AC2 |
| 正文、拒绝、摘要、function/custom-tool | 兼容真实进展，允许长流 | AC3 |
| 首输出早于截止，guard 晚于截止 | 不追溯超时 | AC3 |
| 超时后 retry/switch 再成功 | 原计数/退避，单一响应 | AC4/AC8 |
| 全失败、取消、网关停止 | 分类与一次收尾 | AC4/AC5 |
| 1 MiB 元数据、兼容放行、慢下游 | 截止不重置，无提交后重试 | AC9 |
| 完成/错误、防火墙、尾帧、usage | 协议/计费契约保持 | AC10 |
| 非目标路径与模式 | 范围隔离 | AC6 |

内部状态用 tokio paused-time；router + 本地 mock upstream 用显式屏障和外层墙钟限时，避免保活测试自身挂住。无需真实 Provider、付费模型或进程读取。

## 10. 风险与回滚

静默思考可能在预算内不给可识别进展，届时按策略超时，用户可调大现有设置。未知事件不宽松解除保护，未来扩展需 shape 证据与回归；超大/不完整帧维持有界识别限制。

没有持久化迁移，旧参数保持。实现交付不自动安装或重启；旧版本可恢复旧执行语义。源码回滚只回滚本任务相关 diff，不 reset/清理其他未提交工作。
