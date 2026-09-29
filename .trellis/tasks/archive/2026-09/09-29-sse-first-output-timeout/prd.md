# 修复 SSE 保活导致首次有效输出无限等待

状态：in_progress；用户已于 2026-09-29 批准在独立 worktree 实施，实现与回归通过，本地工作提交 df8e0a66，用户已批准归档。基线：fb1883f7 / 0.60.44。日期：2026-09-29。

## Goal

让常规 Codex Responses 请求在持续保活却没有有效输出时按明确期限结束当前尝试，并提供可解释的超时与恢复结果。正常已经产出的长请求可继续运行，多次尝试的总耗时不受本次新增限制。

## Background and Evidence

- F1：现场首字节 60 秒、流式空闲 300 秒、错误保护窗口 500 毫秒，无限重试测试关闭。
- F2：请求 1790672462-1837 于 2026-09-29 17:01:02（Asia/Shanghai）开始。同一流缓冲在 17:28:55、17:29:11、17:29:27 分别有 110、111、112 个 13 字节 keepalive，输出增量均为 0。response.created/in_progress 均为 in_progress 且 output=[]。[脱敏证据](research/live-sse-evidence.json)。
- F3：任意首块满足首字节探针；前置缓冲等有效输出，500 ms 窗口从有效输出出现才开始；逐块空闲预算每次读到数据后重置。因此状态帧和保活可长期维持等待。锚点：src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:108、:378、:494、:2081。
- F4：当前 UI 将设置解释为首字节等待和块间静默，A 方案需要准确补充首次输出语义。锚点：src/components/cli-manager/tabs/GeneralTab.tsx:466、:501。
- F5：有效输出分类器覆盖正文、拒绝、思考摘要、function 参数及部分输出项，且被多个 usage/协议策略共享。现场正常流还出现 custom-tool，不能未经核对就把旧分类器当作完整白名单。锚点：src-tauri/src/domain/usage.rs:1132、:1167、:1261、:1569。
- F6：故障前最近 100 条成功请求中，4 条超过 60 秒才结束前置缓冲，最长 121354 ms，均未超过 300 秒。这不是精确首输出时间，也不证明其此前只有保活，但提示复用 60 秒的兼容风险。[样本](research/compatibility-sample.json)；计时锚点：success_event_stream.rs:2294。

## Confirmed Product Decisions

- D1：用户选择 A，复用生效流式空闲预算作为首次有效输出上限；保留独立首字节保护，不新增设置。
- D2：用户选择“单次”，每次尝试独立计算，不加整条请求或全部重试的总时限，不削减恢复预算。
- 产品选择无未决项；最终规划已获批准。

## In Scope

常规、非桥接、非无限重试测试模式的 Codex Responses SSE：/v1/responses、/responses、/v1/codex/responses。包含首次等待、跨缓冲提前放行的计时连续性、现有超时/重试链路、设置说明、诊断及确定性回归。资格沿用 is_native_codex_responses_event_stream_path（success_event_stream.rs:313），不另建路径清单。

## Requirements

- R1：每次取得符合范围的 SSE 响应头、进入响应流处理时建立一次固定截止。200、空输出生命周期帧、保活、注释、空/未知事件及无法识别为有效输出的字节不能刷新或解除它。首字节保护保持独立，更早到期的适用保护生效。
- R1a：正值 Provider stream_idle_timeout_seconds 优先，否则继承全局 upstream_stream_idle_timeout_seconds；Provider 空值/0 为继承，全局 0 且无正值覆盖时禁用。直接复用解析结果，不新增字段、迁移或改存量值。锚点：success_event_stream.rs:286、:2775；src/pages/providers/ProviderEditorDialog.tsx:157。
- R2：已支持的非空正文、拒绝、思考摘要、函数调用及现场 custom-tool 输入进展可结束等待。占位状态不算输出；完成/错误仍交原终止及空响应规则。具体形状见设计，不以任意 JSON 或未知事件放宽。
- R3：未提交超时沿用可取消的失败处理、Provider 重试/切换、退避与熔断策略，不新增隐式次数。提交后只结束当前响应，不改已发 HTTP 状态、不拼接另一次输出。
- R4：诊断区分首次有效输出超时、字节空闲超时与客户端取消，记录实际预算和阶段。使用既有错误码与日志 JSON 表面，不误报首字节 60 秒、不只留 started/499，也不重复终结记录。
- R5：保留原首字节、字节 idle、500 ms guard、取消、网关停止、正常完成与终止错误职责；compact 特殊预算和内部重入的真实 Provider 计时归属不变。
- R6：截止前一旦观测到有效输出，首次期限永久结束，不升级成总生成或后续语义空闲时限；正常长请求可超过原预算。
- R6a：重试/切换后，新尝试使用自己的生效预算。整条请求可超过单次值，UI 和诊断须明确单次范围。
- R7：1 MiB 容量放行、解析兼容放行不等于有效输出。保留既有放行，将尚未满足的同一个绝对截止交给后段；读取、发送等待、保活及持续就绪数据不能延后它。提交后到期按 R3 结束，不新增恢复尝试。

## Acceptance Criteria

- AC1（R1）：空输出状态帧后每 15 秒一个 13 字节 keepalive，无有效输出，当前尝试在 300 秒或覆盖值边界结束。
- AC1a（R1a）：全局 300 / Provider 90 用 90；Provider 0/空值继承；全局 0 且无正值覆盖禁用；全局 0 / Provider 正值仍启用；保存读回不改数值。
- AC2（R1/R5）：无首块、纯状态、注释、分块、gzip、截止前/边界/截止后有效输出及持续 Ready 流均有确定结果，无 timer 饿死。观测时 now>=deadline 且此前无有效输出判超时，原取消优先级保留。
- AC3（R2/R6）：正文、拒绝、摘要、函数或 custom-tool 在期限内有真实进展后，后续正常流及 guard 可跨越原截止；不丢失或重复有效事件。
- AC4（R3/R4）：未提交超时走原重试/切换，次数与退避不变；原因/预算正确，成功恢复只交付一个连贯响应。
- AC5（R3/R5）：取消和网关停止可结束等待/退避；已提交不拼接其他尝试。
- AC6（R5）：禁用、存量设置、compact、内部重入、桥接、其他客户端及无限重试模式有兼容测试，不扩大资格范围。
- AC7（R4）：使用合成帧复现；测试和诊断不含真实正文、凭据、Provider 地址或内存映像。
- AC8（R3/R6a）：首次超时、第二次成功时有独立新预算；总耗时超过单次值不构成独立失败原因。
- AC9（R3/R7）：仅元数据达 1 MiB 或兼容提前放行后，原截止仍到期终止当前流，不重试、不重新计时；慢下游/持续 Ready 不使期限失效。
- AC10（R2/R4/R5）：usage、空响应、防火墙、错误尾帧、日志终结和 TTFB/计费无意外变化；现有诊断能识别新阶段原因。

## Out of Scope and Constraints

- 不改上游生成能力，不重建重试、防火墙或计费体系。
- 不加总请求时限、独立设置或新默认数值，不减少已配置尝试次数。
- 不增加有效输出之后的语义空闲计时，不统一升级所有客户端协议，不永久抓原始 SSE。
- 无限重试完整收集器保持独立契约，不加隐藏总生成期限（.trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:140）。
- 保留 .trellis/config.yaml 和其他任务改动；未经批准不实现。构建交付不隐含重启或替换当前安装实例。

## Risks and Deferred Items

静默思考超过生效预算仍会超时，这是有限等待策略的结果，可用现有设置调大。未知新事件不被宽松当作输出；本次覆盖既有及现场确认形状。整条请求可因多次尝试较长，这是 D2 保留的行为。

## Artifacts and Review State

PRD 已完成结构收敛；design.md 定义技术边界，implement.md 定义执行与验证，证据位于 research/。无阻塞产品问题；最终规划已批准，实现与回归验证已完成，Phase 3.4 已获确认，工作提交 df8e0a66。Inline 模式不派发子代理，不要求 JSONL 派发上下文。
