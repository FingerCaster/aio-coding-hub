# 初次规划研究

日期：2026-09-29。基线：fb1883f7 / 0.60.44。

## 证据来源

live-sse-evidence.json 来自同会话对 AIO 进程的只读检查，仅保存事件类型、计数、时间及空 output 事实，不包含原始 SSE、正文、凭据或内存映像。回归使用合成数据。

上午请求 1790649231-1367 最终以 GW_REQUEST_ABORTED / 499 结束，耗时 1443555 ms，当时尚不能仅凭日志确认帧类型。下午请求 1790672462-1837 的连续快照确证保活在增加、有效输出仍为零。

## 技术约束

| 事实 | 锚点 | 设计影响 |
| --- | --- | --- |
| 任意首块解除首字节等待 | src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs:108 | 网络首字节与有效输出要分开判断 |
| 有效输出前 guard_remaining 为 None | 同文件 :378、:494 | 500 ms 窗口不能充当首次有效输出期限 |
| 每次读取重获 idle 预算 | 同文件 :2081 | 保活刷新空闲，新增截止不能被它重置 |
| 缓冲 1 MiB 会强制放行 | 同文件 :22、:416 | 必须处理只有大量元数据时的容量边界，避免绕过保护 |
| UI 明确写的是首字节与块间静默 | src/components/cli-manager/tabs/GeneralTab.tsx:466、:501 | 配置复用的兼容性需要用户选择 |
| 有效输出分类器被 usage、空响应和其他协议共享 | src-tauri/src/domain/usage.rs:1132、:1167、:1261、:1569 | 扩充分类器需核对其他策略行为 |
| 正常现场流出现 custom_tool_call_input.delta | 同会话只读流摘要；分类器当前显式 delta 分支在 usage.rs:1176 | 核对已支持工具事件，避免有效进展被判为超时 |
| compact 有 300 秒下限，0 保留禁用 | src-tauri/src/gateway/proxy/request_context.rs:312、:317 | 不得直接套普通请求固定值 |
| 内部重入将首字节计时委托给真实 Provider | .trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md:107 | 避免双重计时或外层提前取消 |
| 无限重试收集器禁止隐式总生成期限 | .trellis/spec/aio-coding-hub/cross-layer/upstream-error-handling-contract.md:140 | 首次输出与总生成期限必须分开 |

## 成功请求兼容性抽样

只读查询本地 request_logs：cli_key=codex、requested_model=gpt-6-astra、status=200、error_code IS NULL、created_at_ms 早于 1790672462518；id 下界 454000；按 id 倒序取 100 条，读取最后一个 success attempt 的 attempt_duration_ms。

100 条中 4 条的成功尝试前置处理耗时超过 60 秒，最长 121354 ms，无一超过 300 秒。代码在前置缓冲处理后写入该指标（success_event_stream.rs:2294），因此它表示下游提交前的尝试耗时，不能替代缺失的精确首次语义事件指标，也不能证明每条记录都只有无效保活。原始查询只读，持久化样本不包含请求正文或地址。

这提示直接复用 60 秒的兼容性风险。用户后来确认 D1=A：保留网络首字节预算，复用生效流式空闲预算限制首次有效输出等待，保活不刷新这个固定期限；正常输出后的字节 idle 仍由原契约负责。

## 后续技术收敛

- 计时起点、完成/错误与截止同时就绪时的优先级。
- 未知/不完整事件、gzip、分块及 1 MiB 容量释放边界。
- 复用现有超时错误分类与阶段 reason 的可行性，避免破坏用户 timeout 重试匹配。
- 首次期限退出后与 500 ms 窗口及既有字节 idle 的协作。
- 真实思考、拒绝、function/custom-tool 进展的兼容清单。

## A 方案确认后的兼容性核对

用户已确认 PRD D1=A。resolve_effective_stream_idle_timeout（success_event_stream.rs:286）的正值 Provider 覆盖优先；0 或 None 继承全局。现有测试 :2775 验证 Some(90) 覆盖 300 秒、Some(0) 继承 300 秒及全局禁用。Provider 编辑器 :157 的帮助文本也明确空值或 0 为继承。新期限应直接复用这个已解析结果，不新建平行的覆盖算法，也不把 Provider=0 改成禁用。

单次尝试期限不等于整条请求期限。既有预算包含基线、OAuth/continuation 修复及配置重试预留，并按 Provider 分别计算，不能被新计时器隐式裁掉。证据：.trellis/spec/aio-coding-hub/backend/gateway-attempt-budget-contract.md:52、:78。用户已确认 D2=单次，不增加总请求期限。

产品决定以 PRD D1/D2 为准。这些研究材料不是实现批准。

## 最终技术收敛

新单次固定截止从取得符合范围的 SSE 响应头、进入响应处理时开始。原首字节探针独立，先到期保护生效；有效输出后结束新计时，不变成总生成期限。

补充锚点：success_event_stream.rs:313 是非桥接 Codex 资格；:2340 之后经 FirstChunkStream 重放前缀并接入 usage relay。usage_tee.rs:497 原 idle 仅在 Pending 检查，:573 每个 chunk 重置，故新截止须覆盖持续 Ready 与提前交接；:1065 的 relay 还须保留关闭优先级并约束首输出前发送等待。

streams/types.rs:14 的终止来源是内部 enum，经 :135 的 terminal_details_json 写入已有 JSON。追加 FirstOutputTimeout 不需要公开 schema。前置沿用 GW_UPSTREAM_TIMEOUT，后置沿用 GW_STREAM_IDLE_TIMEOUT 并明确 origin，避免 finalize.rs:23 错用 first-byte 秒数。usage_tee.rs:712 已有一次 finalize 与 usage/log 管线，必须复用。

1 MiB 既有放行保留，未满足截止移交后段；只有未提交时允许重试。设计与验证矩阵见 design.md / implement.md。无阻塞产品问题，等待最终摘要评审后实现。
