# 技术设计

## 1. 目标边界

本任务把“终态错误安全处理”设计成一个独立的客户端可见性边界，而不是把所有终态
错误都转换成重试。处理对象是最终交给 Codex 客户端的 Responses SSE，包括未桥接的
原生 Codex 和经过供应商桥接后仍输出 Codex Responses SSE 的请求。Claude/Gemini
自身协议的流终态不在本次默认启用范围内，除非已有桥接层把它归一化为 Codex Responses
事件。

三个职责必须分开：

1. **结构化识别**：从完整 SSE 终态帧提取 event/type/code/message/status，得到稳定类别。
2. **路由决策**：仅在下游尚未提交时决定同 Provider 重试、切换 Provider 或终止；使用
   现有共享预算、backoff、circuit 和 failover。
3. **客户端投影**：无论是否重试，决定终态帧能否到达客户端；默认脱敏或丢弃，只有显式
   透传例外允许原帧到达客户端。

## 2. 数据流

```text
上游 HTTP 200/流 chunk
        |
        v
有界 SSE 帧解析器（跨 chunk、CRLF、同 chunk 多帧）
        |------------------------------+
        v                              v
原始 evidence/tracker                 结构化 terminal classifier
（限长、脱敏、仅内部）                  |
        |                              +--> 提交前：retry/failover/标准终态
        |                              |
        +--> 客户端 firewall ----------+--> 提交后：丢弃并结束 / 显式透传
                    |
                    v
              Codex 客户端
```

`SseUsageTracker` 必须继续消费原始上游帧；客户端 firewall 只能消费同一帧的投影结果，
不能用改写后的 bytes 覆盖 tracker 或请求日志中的 evidence。

## 3. 结构化分类契约

在 `src-tauri/src/domain/usage.rs` 现有提取逻辑上增加稳定类别（实现时可用枚举并以
snake_case 记录）：

- `transient_capacity`：容量、过载、`slow_down`、`server_is_overloaded`、明确的
  `service_unavailable_error` 等瞬态 Provider 故障。
- `transient_provider`：结构化字段表明临时服务故障，但不是容量别名。
- `quota`：`insufficient_quota`、硬额度/计费限制等；不在同 Provider 盲重试，可按现有
  Provider 语义切换或终止。
- `auth`：认证、权限、凭据失效；不重试当前 Provider。
- `invalid_request`：参数、模型或协议请求错误；不跨 Provider 扩散。
- `policy`：内容安全/策略拒绝；默认不重试，只有显式透传例外才允许原帧。
- `unknown`：无法安全归一化的终态；不重试、不跨 Provider，客户端使用稳定投影。

分类优先级固定为：

1. 结构化硬性非重试类别（`auth`、`invalid_request`、`quota`、`policy`）。
2. 结构化瞬态/容量类别。
3. 显式 `passthrough_keywords` 例外（仅改变客户端投影，不改变路由决策；在 firewall
   开启时容量和敏感硬阻断优先）。
4. 短期 `legacy_retry_keywords` 兼容覆盖，但只能提升 `unknown` 的提交前重试资格，不能
   覆盖硬性非重试或容量安全阻断。
5. `unknown` 默认终止并隐藏。

关键词匹配仍是大小写不敏感的字面匹配，但新公开配置不再提供重试关键词；结构化类别
不需要供应商每换一句文案就修改设置。

## 4. 提交前行为

扩展 `inspect_buffered_event_stream_prefix` 的决策类型，避免用 `StartStreaming` 同时
表达成功、未知错误和透传例外：

- 瞬态/容量：返回现有 `ProviderFailure`，调用
  `record_buffered_provider_failure`，复用共享 retry budget/backoff/circuit。
- 硬性非重试或未知：返回新的标准终态分支，使用现有网关错误构造器和
  `GW_FAKE_200`/502 语义，客户端只看到中性、协议兼容的错误；不复制上游 message/code/type。
- 显式透传：允许当前完整终态帧作为唯一尝试的客户端 body；不重试、不切换，不拼接其他
  Provider 输出。透传前仍做大小、UTF-8、SSE 完整帧和敏感字段检查。

具体 Codex SSE 兜底事件字段必须先由当前客户端 fixture 或官方协议验证。未验证前不在网关
凭经验伪造 `response.failed`/`response.error` 帧；优先使用已有标准 HTTP 终态 envelope。

## 5. 提交后客户端 firewall

在 `UsageSseTeeStream` 的原始 tracker 与 relay 发送之间增加一个只针对 Codex final-wire
Responses 的有界过滤器（建议新文件
`src-tauri/src/gateway/streams/terminal_firewall.rs`）：

- 累积到完整 SSE 帧才解析；支持跨 chunk、一个 chunk 多帧、LF/CRLF、尾部未完成帧和
  下游提前断开。
- 普通非终态帧按原 bytes 发送，保持顺序和 chunk 内容；不合并不同 Provider 的尝试。
- 识别到终态帧后：
  - 默认丢弃该帧，标记 `dropped_after_commit`，停止向下游发送后续终态错误并结束 relay；
  - 命中有效透传例外时保留该完整帧，标记 `passthrough_exception`，随后按原有 EOF 结束；
  - 无法解析或超过帧缓冲上限时 fail closed，不能把未分类原文直接发送给客户端。
- `response.completed` 仍只发送一次；完成后收到的迟到错误也不能覆盖已发送完成事件。
- filter 的 retained bytes 使用独立上限，不能把整个响应缓存起来；原始 tracker 继续受现有
  1 MiB pending / 20 MiB aggregate 边界约束。

relay 结束时把 firewall disposition 传给 finalize/request-log；客户端 body 与内部 evidence
各自取源，避免“日志只看到改写结果”或“客户端拿到日志原文”。

## 6. 第三方/桥接供应商

HTTP 状态为 200 不代表终态成功。桥接供应商可能在 SSE `event`、`data.type`、嵌套
`error.type/code/message` 或 status 字段中返回错误。默认 classifier 只依赖可验证的结构化
字段和有限的通用状态，不依赖某一家完整文案：

- 能归一化为瞬态/容量的帧进入共享预提交 retry/failover。
- 能归一化为策略、认证、配额或请求错误的帧不盲重试。
- 其他帧按 unknown 处理，客户端隐藏，内部保留 evidence。

桥接层若已经把错误转换成 Codex Responses 终态，直接复用同一个 firewall；若仍是非
Codex 协议，保留现有桥接终态处理，后续新增适配器时只实现分类接口，不新增公共关键词字段。

## 7. 设置和迁移

### 7.1 运行时模型

`UpstreamStreamInternalErrorPolicy` 的公开投影改为：

```text
enabled
passthrough_keywords[]
```

`enabled` 是该流终态处理层的总开关，而不是单独的“是否重试”开关：

- `true`：执行结构化分类、提交前 retry/failover、客户端安全投影和透传例外。
- `false`：完整旁路新增动作层，capacity 与其他终态都不拦截、不重试、不改写；tracker
  仍可被动记录 evidence，并标记 `disabled_passthrough`。

缺失字段按默认 `true` 补齐；已有显式 `false` 不由迁移覆盖，避免把用户的关闭选择误判为旧
配置缺失。

后端在一个兼容窗口内另存 `legacy_retry_keywords[]`（只读高级兼容状态），不再向新 UI
暴露。`non_retry_keywords` 只作为旧 wire alias 读取并合并到
`passthrough_keywords`，归一化、去重、限长后再保存。

硬性安全类别永远不能被透传例外覆盖；Provider 完整 override 仍按现有“替换全局策略”
语义解析，因此 Cyber 供应商可以在自己的 override 中配置透传词而不影响其他 Provider。

### 7.2 持久化/分享

- 全局 settings schema 升级到 59：旧 `non_retry_keywords` 转为新字段，旧
  `retry_keywords` 转为隐藏兼容字段，并记录可观测的迁移标记。
- Provider override JSON 读路径双读旧/新字段，写路径只写新字段和必要的
  `legacy_retry_keywords`；非法或冲突字段 fail closed，不继承成更宽的全局透传。
- Provider share 升级为 v4，保留 v1-v3 读取；旧客户端遇到 v4 必须明确拒绝，不能静默丢失
  透传语义。导出/导入测试必须验证旧分享、当前分享和新字段往返。
- 生成的 Rust/TypeScript bindings、前端 clone/validate/default、Provider fixtures 同步更新。

## 8. 客户端展示与诊断

- 客户端尝试摘要、顶层 message 和错误 body 统一经过安全投影；未知/非重试/容量 evidence
  不得含上游原文、代码或容量词。
- 请求日志和 gateway event 继续保留限长、脱敏的 event/type/code/message/classification/
  disposition；不写完整 SSE、凭据、透传规则原文或 Bearer 值。
- 新 disposition 至少区分 `retry_same_provider`、`switch_provider`、
  `sanitized_before_commit`、`dropped_after_commit`、`passthrough_exception` 和
  `legacy_retry_override`，方便从截图中的 `forwarded_after_commit` 识别修复是否生效。

## 9. 兼容、发布和回滚

- `enabled` 提供明确的产品级回滚：关闭后恢复当前终态原流行为，包括 capacity，不再另设
  capacity 硬拦截分支。内部被动诊断保留并记录 `disabled_passthrough`。
- 新安装与升级迁移使用已确认的默认策略：缺失字段补 `true`，显式 `false` 保持关闭；实现
  不得把这两种状态混成同一状态。
- 第一阶段不重写 HTTP 终态规则，不改变非流请求、Codex continuation、probe、普通成功流
  和现有 attempt budget 公式。
- 发现客户端协议不接受标准 502/GW_FAKE_200 时，可通过关闭 firewall 恢复当前原流行为；
  内部 evidence 仍保留，便于确认回滚影响。

## 10. 重要取舍

- 结构化分类可能暂时把某些本可重试的新供应商错误归为 unknown，代价是少一次重试；换取
  不会把策略、凭据或敏感文本误判为瞬态并扩散到其他 Provider。
- post-commit 丢帧可能让客户端只显示通用 disconnect，而不是供应商具体原因；这是已确认的
  流完整性与信息泄露取舍。
- 保留一版隐藏兼容字段增加少量 schema 复杂度，但能避免已有自定义重试设置在升级时静默
  消失；新配置模型仍只有一个透传入口。
