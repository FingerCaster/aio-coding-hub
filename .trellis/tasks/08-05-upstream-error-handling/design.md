# 技术设计

## 边界与依赖

```text
统一设置入口
  ├─ 重试规则 -> UpstreamRetryPolicy -> 每次失败的 failover 决策
  └─ 最终响应改写 -> UpstreamErrorResponseRule[] -> 终态 HTTP 响应构造

Codex 原生 Responses SSE
  -> 提交前 guard/classifier
  -> UpstreamRetryPolicy 的 stream_internal_errors
  -> 既有 RetrySameProvider/SwitchProvider/Abort
  -> attempt evidence / final projection
```

共享入口仅复用 UI 组件与视觉语义，不合并两个 schema。子任务可以独立关闭或回滚，空改写规则与关闭流内策略时均保持现有行为。

## 集成顺序

1. 先实现最终响应改写的独立规则模型、终态候选与 UI 模式，形成第一个原子提交。
2. 再扩展重试策略与 Codex event-stream guard，复用已存在的上游错误处理入口，形成第二个原子提交。
3. 父任务进行生成绑定、共享文案/布局、日志投影、完整测试与浏览器验收；只对发现的跨子任务缺口做最小整合提交。

## 共同合同

- `upstream_retry_policy` 与 `upstream_error_response_rules` 是不同字段；Provider override 只覆盖完整 retry policy，不覆盖响应改写规则。
- runtime snapshot 同时携带两个字段，但 failover 决策只读取 retry policy；响应改写只在已确定最终 HTTP 错误时读取规则。
- `FailoverAttempt.status` 永远是 upstream 状态。主请求日志 `status` 是客户端最终状态。流内 HTTP 200 错误记录结构化 evidence，不把 200 当作最终成功事实。
- special settings 使用类型化、有界条目。响应改写只记录规则/Provider 身份、前后状态与两个行为模式；guard cap 只记录放行诊断。
- 对设置读取采用兼容默认值和逐条容错；普通设置写入在共享锁内严格验证，所有权只覆盖显式字段。
- 任何规则/正文/构造/日志投影异常不得改变 Provider 选择、健康、circuit 或客户端原有响应。

## Fork 保护

- 不替换完整 `success_event_stream.rs`、`upstream_error.rs`、`routes.rs` 或 settings migration；以当前文件为基线逐块加入功能。
- 保留 probe ownership 的 complete success/failure、session binding token、ordered failback target、health-neutral、provider limit/cooldown gate 与 route observation。
- Codex 请求体编码在 Provider 选择前完成；响应 guard 不读取或恢复原始压缩请求体。
- transport 和 stream-internal retry 都只在最终决策为 `RetrySameProvider` 时调用统一 backoff helper，避免双重等待。
- continuation/OAuth/thinking rectifier 的内部 retry 不消耗 configured transient counter；stream-internal retry 与 HTTP/transport 才共享该计数。

## UI 结构

- 在 `GeneralTab` 的既有 retry 区域放置紧凑分段控件；页面 section 不包入新的外层 Card。
- “重试规则”保留 `RetryPolicyFields`，补充保护窗与流内关键词编辑；“最终响应改写”渲染规则列表和编辑 Dialog。
- 状态码与关键词输入抽取为轻量共享编辑组件或 helper，只有在确实减少重复时复用；动作、验证和保存仍由各自 owner 管理。
- 按现有移动断点让规则摘要、操作按钮和 Dialog 表单换行；图标按钮使用 Lucide 并带 Tooltip/可访问名称。

## 验证与回滚

- 子任务分别先跑 focused Rust/TS 测试并提交；父任务跑全量检查。
- UI 使用本地 dev server 和 in-app browser 做桌面/移动截图与真实交互；失败时修正布局后重新截图。
- 可通过清空改写规则、关闭 `stream_internal_errors.enabled` 或设置 guard 0 降级；代码回滚时未知字段由既有兼容读取处理。

