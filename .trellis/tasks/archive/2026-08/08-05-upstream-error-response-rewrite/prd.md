# 上游错误响应改写

## Goal

允许用户按最终 upstream HTTP 4xx/5xx 的真实状态、有限正文关键词、CLI 与实际 Provider 范围，安全地改写客户端最终看到的状态码和错误消息；该功能必须在重试、故障切换、配额、冷却、熔断和健康计数完成后执行，并且发生任何异常时保持现有 AIO 响应。

## Confirmed Facts

- 当前错误路径已经有界读取/解压正文并执行 HTTP retry 匹配，不能二次消费网络 body。
- 当前请求日志可分别保存主状态、attempt chain 和有界 `special_settings_json`。
- 当前普通设置写入通过共享锁内字段所有权提交；settings schema 版本高于参考功能的 v57，迁移必须从当前版本顺延。
- 参考实现 `26c4e02` 提供完整行为，但共享 failover 文件早于当前 fork 的 probe/failback/transport backoff，必须逐块适配。

## Requirements

1. 规则支持稳定 ID、名称、说明、启停、`0..=9999` 优先级、Any/All、状态码、大小写不敏感字面关键词、CLI scope、Provider ID scope、状态 passthrough/override 和消息 passthrough/override。
2. 最多 32 条规则；每条最多 16 个 400..=599 状态码和 16 个关键词；名称最多 100 字符、说明 256、关键词 512、自定义消息 4096。至少配置一个条件组。
3. 状态组内部 OR，关键词组内部 OR。Any 要求任一已配置组匹配；All 要求每个已配置组都匹配。空 CLI/Provider scope 表示全部，两个非空 scope 必须同时命中。
4. 规则按优先级升序和稳定列表位置求值，只采用第一条命中。若高优先级规则需要正文但正文不可用、不完整或无法安全解码，停止并 fail open 到既有 AIO 响应，不继续低优先级规则。
5. 仅匹配最终 upstream HTTP 4xx/5xx。HTTP 200 流内错误和 transport error 不进入规则；任何中间失败候选必须被后续成功、transport failure 或不同最终失败替换/清除。
6. retry/failover/quota/cooldown/circuit/probe/failback 先用原始 upstream facts 完成。attempt status、decision、circuit accounting 与未启用规则时一致。
7. 消息 passthrough 从有界错误体的已知字段提取并重新构造协议兼容 JSON，绝不直接转发未知 bytes。支持 `error.message`、JSON 字符串 error、`detail`、顶层 `message`、字符串 `error` 和有限纯文本。
8. Claude envelope 为 `{type:"error",error:{type:"upstream_error",message}}`；Codex/Grok 为 `{error:{type:"upstream_error",code:"upstream_error",message}}`；Gemini 为 `{error:{code,status:"UNKNOWN",message}}`。
9. 改写响应清理 stale entity/hop-by-hop headers，安全保留合法 `Retry-After` 和 AIO `x-trace-id`。状态 override 仍限制 400..=599。
10. 设置持久化采用严格写入、逐条容错读取与幂等迁移；无效/未来格式条目单独丢弃，不能让整个设置读取失败。
11. 主请求日志记录 client-visible status；attempt 保留 upstream status。命中只追加有界审计：规则 ID/名称、Provider ID/名称、前后状态、状态/消息模式。不得记录正文、关键词、提取消息或自定义消息。
12. 在统一“上游错误处理”入口的“最终响应改写”模式提供规则列表、启停、新建、编辑、删除、保存和表单验证；首页/实时请求/日志显示 fail-open 徽标和 tooltip。

## Acceptance Criteria

- [ ] 空规则配置与当前行为字节/决策等价。
- [ ] priority、Any/All、两个 scope、启停及四种状态/消息组合有 Rust 与前端覆盖。
- [ ] 三类协议 envelope、状态边界、合法/非法 Retry-After、x-trace-id 与 header 清理通过测试。
- [ ] 中间失败后成功、中间规则命中后其他 Provider 失败、transport 终态和 HTTP 200 流内错误均不误改写。
- [ ] retry 次数、transport backoff、Provider 切换、probe/failback、circuit 和 health 状态不受规则影响。
- [ ] 正文不可用、未知编码、畸形 JSON、过大正文、无效规则和构造异常均 fail open。
- [ ] settings 严格写入/逐条容错读取/迁移、分享/导入/备份与 generated bindings 完整。
- [ ] Home/实时/Logs 徽标可用；损坏 `special_settings_json` 只隐藏徽标，不破坏渲染。
- [ ] focused Rust/TS 测试、格式、typecheck、lint 和 build 通过并形成原子提交。

## Out Of Scope

- 不参与任何重试或 Provider 健康决策。
- 不匹配 transport error、HTTP 2xx 或 SSE 内嵌错误。
- 不新增数据库表或“跳过监控”行为。
