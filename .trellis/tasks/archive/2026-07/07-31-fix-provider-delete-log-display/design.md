# 技术设计

## 范围与原则

本任务修复两个独立但共享 provider 身份语义的前端问题，并用 Rust 契约测试证明既有持久化级联。实现不改变数据库 schema、Tauri 命令、Specta DTO、生成绑定、路由选择或日志写入格式。

## 删除数据流

```text
删除确认 -> useProviderDeleteMutation -> provider_delete IPC
  -> Rust provider 事务 -> SQLite FK 级联 -> 网关运行时清理
  -> 前端取消旧查询 -> 同步过滤缓存 -> 失效并重取后端真值
```

删除成功后的前端提交边界由 `useProviderDeleteMutation` 单独拥有：

1. 规范化 `cliKey` 后，取消 provider list、Default route 和该 CLI 的 sort-mode provider 查询族，防止删除前的 IPC Promise 回写旧快照。
2. 仅按 `providerId` 同步过滤三类已存在缓存；`null` 或不存在的缓存保持原样，不创建虚假数据。
3. 失效相同查询族，从已完成级联的 SQLite 重新获取权威状态。
4. 保留现有账号用量、模型 generation/cancel/remove 清理；service 返回 `false` 或抛错时不修改任何缓存。

`src/query/sortModes.ts` 导出 CLI 级 provider-key 前缀，并让 exact key 基于此前缀构造，避免 `providers.ts` 复制私有 key 布局。页面的 `ModeProvidersState` 继续通过查询结果 reset key 更新，不增加第二套删除逻辑。

Rust 保留外键级联作为唯一持久化机制。新增领域测试在同一数据库中布置 provider pool、Default route 和多个 sort mode 引用，删除目标后断言所有目标行消失、相似 provider 保留，并执行外键一致性检查。

## 决策链数据流

```text
FailoverAttempt 历史快照 -> request_logs.attempts_json
  -> RequestLogDetail / RequestAttemptLog -> 前端合并
  -> ProviderChainView 统一身份格式化 -> 摘要与 attempt 卡片
```

`ProviderChainView` 增加局部纯格式化函数，输出：

- 有效名称和正整数 ID：`名称 (#ID)`；
- 空白、`Unknown`（忽略大小写）或 `未知`：`未知供应商 (#ID)`；
- ID 非正安全整数：`名称 (ID 不可用)` 或 `未知供应商 (ID 不可用)`。

同一格式同时用于起始/最终摘要、attempt 折叠标题和展开后的供应商行。`base_url` 继续作为二级端点信息，不能代替稳定 ID，也不用于去重。provider 被删除后仍使用日志内的请求时名称快照，不回查当前 provider 表，也不猜测“已删除”状态。

## 兼容性与边界

- 旧日志名称缺失或使用中英文 unknown 哨兵时稳定回退。
- raw JSON 损坏但兼容 attempt 接口可用时，仍展示后者携带的名称与 ID。
- 同名、同 URL、同类型的 provider 只按不同 ID 区分。
- URL 为空不影响身份展示；无可靠 ID 时不渲染 `#undefined`、`#null` 或 `#0`。
- 不增加 provider 当前存在性查询，避免 ID 复用或同域名配置造成历史误映射。

## 验证与回滚

先运行两组 focused Vitest 和 Rust 删除测试，再执行生成绑定、完整 precommit/prepush 门禁、CI 补充项和 Windows x64 MSI 构建。若缓存修复回归，可独立回滚 query helper/mutation；若展示修复回归，可独立回滚格式化与组件渲染；两者不涉及数据迁移。
