# 技术设计

## 设置模型

在 `AppSettings` 新增 `upstream_error_response_rules: Vec<UpstreamErrorResponseRule>`。通过自定义反序列化或 wire wrapper 逐条解析，读取时丢弃非法条目；`settings_set` 对完整列表做严格规范化并在共享设置锁内只更新该字段。

```rust
UpstreamErrorResponseRule {
  id, name, description, enabled, priority,
  match_mode, status_codes, keywords,
  cli_scope, provider_scope,
  status_behavior, message_behavior,
}
```

行为使用带标签枚举，避免把 passthrough 与 override 混成 nullable primitive。Provider scope 保存稳定数据库 ID；CLI scope 使用现有 `claude/codex/gemini/grok` key。

## 运行时观察

新增独立 `upstream_error_response_rules` 模块，拥有：

- 写入规范化与运行时安全过滤；
- 规则排序、scope/status/content matcher；
- 有界 message extraction；
- 三种协议 envelope 和 header 构造；
- special setting audit builder。

HTTP error body 仍由 `upstream_error.rs` 的单次有界读取路径拥有。匹配获得的只读 view 产生可选 terminal candidate；candidate 只包含规则索引/身份、Provider identity、原始状态和构造所需的有界响应信息。

## 终态提交

`FailoverContext` 保留至多一个 rewrite candidate：

- HTTP error attempt 读取正文后重新求值并替换候选；
- retry/switch 会在下一 attempt 开始前清除旧候选；
- transport error 清除候选；
- success 清除候选；
- 只有最终 `Abort` 的 upstream HTTP 4xx/5xx 可以把候选交给 `finalize`。

`finalize` 先让现有决策、attempt/circuit/route 记录完成，再尝试构造客户端响应。构造成功时更新主日志 status 并追加 audit；失败时原样返回既有 AIO response。

## 安全与兼容

- matcher 不 panic、不记录 body；body unavailable 对需要关键词的高优先级规则实行 fail-open 停止。
- envelope 只用 `serde_json` 构造。清理 content-length/encoding/type 和 hop-by-hop headers，再设置 JSON content type/length。
- 只复制经过语法验证的 `Retry-After` 和当前请求 trace id；不信任 upstream trace header。
- 当前 schema 版本基础上新增一个迁移版本；默认空列表，迁移幂等。
- Provider 删除不要求级联改写历史配置；不存在的 Provider ID scope 自然不匹配，UI 显示缺失项并允许用户清理。

## 前端

- `upstreamErrorResponseRules.ts` 负责生成默认草稿、clone、规范化、验证和显示摘要。
- `UpstreamErrorResponseRulesCard` 实现列表与 Dialog，但作为统一 section 内的内容面板，不再添加 Card 外壳。
- 复用现有 Button/Switch/Input/Textarea/Select/Checkbox/Dialog/Tooltip 与 Lucide `Plus/Pencil/Trash2/Save/X`。
- `requestLogSpecialSettings.ts` 是命中审计的唯一结构化 parser；Home、Realtime、Logs 复用同一 projection。

## 测试

- Rust unit：matcher、优先级、Any/All、scope、limits、message extraction、三类 envelope、headers、fail-open。
- Rust route：中间失败后成功、多 Provider 最终失败、direct abort、retry/circuit 不变、client status 与 attempt status 分离。
- settings：严格写入、逐条容错读取、迁移幂等、字段所有权并发保护。
- frontend：规则 helper、表单/Dialog、统一模式、设置 adapter、徽标 parser 与损坏 JSON。
