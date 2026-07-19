# 规则化瞬时错误重试 - 技术设计

## Architecture And Boundaries

本变更作为一个原子跨层契约交付，不拆分子任务。Rust 设置类型是规则数据的唯一权威来源，生成 TypeScript 绑定；同一策略经全局设置或 Provider 整套覆盖进入网关。共享 React 组件维护规则，Rust 网关在既有错误路径中完成一次有界响应体读取、规则匹配、重试决策和安全诊断。

受影响边界：

1. 全局设置 JSON 与 schema 修复。
2. Provider SQLite 覆盖 JSON 与数据库迁移。
3. 单 Provider 分享/导入的严格版本化 JSON。
4. Rust -> generated TypeScript -> 前端校验/表单。
5. 网关 HTTP 错误体读取、匹配、尝试预算、熔断计数和请求尝试记录。

## Data Contract

Rust 权威模型：

```rust
pub struct UpstreamHttpRetryRule {
    pub enabled: bool,
    pub status_code: u16,
    pub body_contains: Vec<String>,
    pub description: String,
}

pub struct UpstreamRetryPolicy {
    pub enabled: bool,
    pub http_rules: Vec<UpstreamHttpRetryRule>,
    pub transport_errors: Vec<UpstreamTransportRetryKind>,
    pub max_retries: u32,
    pub backoff_ms: u32,
    pub counts_toward_circuit_breaker: bool,
}
```

默认 `http_rules` 是三条启用的 `502/503/504` 规则，`body_contains=[]`、`description=""`。传输错误及其他默认值不变。

建议边界与前后端共享常量：

- HTTP 规则最多 16 条，延续旧状态码上限。
- 每条规则最多 16 个匹配内容。
- 每个匹配内容最多 512 个 Unicode 字符。
- 描述最多 256 个 Unicode 字符，且不得包含控制字符。
- 状态码必须是 `400..=599`。

规范格式中，`body_contains=[]` 是唯一的“全部内容”表示。非空项 trim 后必须仍非空，按 Unicode 小写结果去重；正则和通配符字符没有特殊含义。所有规则都要通过结构与边界校验，包括停用规则。

## Validation And Repair

- 前端在全局保存和 Provider 提交前执行同一套校验。
- Rust 将“写入规范化”和“加载修复”分成两个入口。`settings_service` 与 Provider `retry_policy_override_to_json` 在提交前调用严格写入规范化；无效用户输入返回 `SEC_INVALID_INPUT`，不得先经过 repair sanitizer 静默改写后再保存。
- 加载旧数据或修复损坏数据时可规范化 trim、大小写无关去重及数量上限，但必须 fail closed：若一个原本非空的内容列表规范化后无有效项，不得变成 code-only 全匹配规则，应停用或丢弃该规则。
- 策略开启时，至少有一条已启用 HTTP 规则或一个传输错误；否则加载修复应关闭策略，用户保存应报错。HTTP 规则为空但传输错误非空合法。
- 描述写入尝试记录前再次转成有界单行文本，防止导入或损坏数据注入日志结构。

## Compatibility And Migration

### 全局设置

- `UpstreamRetryPolicy` 使用兼容反序列化 wire type：优先读取新 `http_rules`；新字段缺失且存在旧 `status_codes` 时逐项转换；二者都缺失时使用默认规则。
- 序列化只输出 `http_rules`，不再输出 `status_codes`。
- 设置 schema 从 52 升至 53，新增迁移步骤并依赖 canonical JSON 比较自动回写新格式。

### Provider SQLite 覆盖

- 数据库从 v38 升至 v39，事务内确保 `upstream_retry_policy_json` 列存在，并把有效旧 JSON 的 `status_codes` 转成 `http_rules` 后删除旧字段。
- 已经是新格式的行保持稳定；`NULL`/空值保持继承全局。
- 单行 malformed JSON 不应扩大匹配或阻断整个应用启动；迁移保留该行，现有读取路径继续 fail closed 为显式禁用覆盖并告警。
- 运行时反序列化仍兼容旧格式，作为未经过迁移数据库和测试夹具的防线；写入只产生新格式。

### Provider 分享/导入

- 严格 v1 保持原字段集合与 `status_codes`，只读兼容并转换为内部新策略。
- 新导出使用明确的 schema v2，重试覆盖包含 `http_rules`，所有受控对象继续 `deny_unknown_fields`。
- 解析器先读取最小 header，再显式分派 v1/v2；两版各自严格拒绝未知字段。预览与导入只接收转换后的 canonical v2/internal envelope。
- copy/save 仍共享同一确定性序列化器、8 MiB 上限和原有 secret/native-I/O 边界。
- 更新 Provider Share spec，明确 v1 兼容读取、v2 新导出和规则字段。

旧版本应用不能读取新保存的规则或 v2 分享，这是用户已接受的单向升级代价；不得通过双写制造两个权威来源。

## Runtime Data Flow

```text
有效全局/Provider策略
  -> Provider 准备阶段预留 max_retries 尝试预算
  -> 收到 HTTP 400-599
  -> 先匹配同状态的 code-only 规则
  -> 若无 code-only 且存在内容规则，将规则扫描并入现有 body-scan 条件
  -> 只读取一次、解压并限制为前 64 KiB
  -> 不区分大小写的普通子串匹配
  -> 配置型重试决策 / 通用基线决策
  -> 退避、熔断计数、尝试记录
```

匹配器返回 `rule_index`、安全描述引用和是否匹配，不返回/持久化命中内容。code-only 规则对同状态直接命中；只有内容规则时才依赖响应体。多个匹配规则执行相同动作，首个匹配仅用于诊断，不构成优先级。

内容读取/解压失败、空正文或非 UTF-8：

- code-only 规则不受影响。
- 内容规则 fail closed 为未命中；使用 `String::from_utf8_lossy` 的文本范围内仍可匹配合法片段。
- 未命中后继续既有通用重试/切换/中止决策，不制造新的错误类别。
- 已消耗响应体且最终需要透传时，沿用现有 bounded-body 重建/失败行为。

现有 `maybe_gunzip_response_body_bytes_with_limit` 在解压输出超过上限时会退回压缩字节，不能直接作为内容规则扫描器。实现应增加错误扫描专用的 bounded prefix 解码：编码输入保持独立上限，解压成功时最多返回解压后的前 64 KiB；截断或超限不得退回 gzip 二进制参与文本匹配。该前缀仍与现有错误分类/诊断共用一次网络 body read。

配置型匹配不改变以下契约：

- count-tokens 请求仍中止，不启用配置型重试。
- Codex model discovery 保持每 Provider 一次的 strict limit。
- OAuth refresh、`previous_response_id` 和 thinking rectifier 的专用内部修复预算/优先级不变。
- 未命中规则的 5xx、408/429 和其他既有错误仍可使用通用 Provider 基线尝试预算。
- 只有实际进入配置型重试的尝试才享受 `counts_toward_circuit_breaker=false`；耗尽后的最终/通用失败照常计数。

## Observability And Privacy

- 实际触发配置型 HTTP 重试时，`FailoverAttempt.reason` 追加 `retry_rule=<1-based index>` 和经约束的非空描述。
- 不记录 `body_contains`、命中的具体条目、匹配片段或新增的响应体预览。
- 401/403 可以在内存中参与用户显式配置的内容规则，但响应体仍不得进入控制台、事件、attempt reason、request log 或 error details。
- 非认证状态继续保留现有诊断预览行为；本功能不得扩大其字节上限或新增消费者。

## Frontend Experience

- 扩展共享 `RetryPolicyFields`，因此 CLI Manager 全局卡片与 Provider 覆盖自动获得一致规则编辑器。
- HTTP 区域改为紧凑分隔列表，不嵌套装饰性卡片。每行包含启用 Switch、状态码 Input、描述 Input、匹配内容 Textarea 和带 Tooltip 的 Trash2 图标按钮。
- `body_contains` 在 Textarea 中一行一个普通文本；空白行被忽略，整个区域为空序列化为 `[]`。逗号和分号按字面值保留。
- 使用带 Plus 图标的“新增规则”命令；不提供复制、拖拽或预设。
- 保存失败保留草稿并显示首个明确校验错误。Provider 覆盖继续整套替换，不合并全局规则。

## Operational And Rollback Considerations

- SQLite v39 迁移必须事务化、幂等并有旧 v38 夹具测试；设置 canonical 回写必须有旧 schema JSON 测试。
- Provider 分享 v1/v2 解析、序列化和导入必须保持严格、确定且无 secret 泄漏。
- 回滚到旧应用不受支持；发布前应依赖用户现有备份/配置导出。迁移失败必须回滚数据库事务并保留 v38。
- malformed Provider JSON 保持显式禁用而不是回退全局，避免扩大重试。

## Trade-offs

- 不使用正则：牺牲复杂表达力，换取可预测性能、简单校验和无 ReDoS 风险。
- Provider 覆盖不合并：添加一条专用规则时需要复制所需全局规则，但禁用与优先级明确。
- v2 分享而非扩展 v1：增加版本转换代码，但保持严格 schema 合同。
- 不改变通用重试：UI 中的规则不是所有重试的唯一来源，但避免破坏现有 failover 预算和路由行为。
