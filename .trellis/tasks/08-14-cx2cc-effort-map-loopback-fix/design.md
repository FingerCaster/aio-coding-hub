# Technical Design

## 1. Settings Contract

在 `AppSettings` 增加有序 `cx2cc_reasoning_effort_mappings: Vec<Cx2ccReasoningEffortMapping>`，元素包含 `source` 与 `target`。有序数组适合 UI 增删改，同时避免 JSON 对象键归一化和顺序不稳定问题。

默认常量由 Rust 单一函数返回以下规则，前端恢复默认通过生成绑定暴露的默认值或共享前端常量保持契约测试一致：

```text
low -> low
medium -> medium
high -> high
xhigh -> xhigh
max -> max
ultra -> max
```

后端写入时统一 trim，并校验：最多 32 条、每个 source/target UTF-8 最多 64 字节、无控制字符、非空、source 精确唯一。读取旧 schema 时迁移到默认规则；损坏的当前配置不能静默写回，应走现有设置恢复/校验路径。

设置 schema 从 62 升到 63。`SettingsUpdate`、`SettingsPatch`、`SettingsView`、字段所有权 token、前端 view-to-input 映射和生成绑定同时更新。完整配置 bundle 已携带 AppSettings，因此只需保证新版 round-trip 与旧 bundle 缺字段迁移。

## 2. Reasoning Transformation

映射发生在 Anthropic inbound 已解析显式 effort、OpenAI Responses outbound 尚未生成 JSON 之前：

```text
Anthropic body
  -> parse thinking/effort presence
  -> map only explicit effort variants with CX2CC settings
  -> IRReasoningConfig
  -> Responses reasoning.effort
  -> final outbound effort logger
```

`Cx2ccSettings::map_reasoning_effort` 对 trim 后的精确 source 查找，命中返回 target，未命中返回原值。`Disabled` 分支不调用映射，确保用户无法把 disabled 改成其他强度；`Absent` 与 enabled/adaptive-without-effort 也不调用映射。

不在翻译后的 JSON 上盲改 `reasoning.effort`，因为那会失去 `disabled -> none` 的来源信息。旧 `model_reasoning_effort` 保留在结构中但运行时仍不读取。

## 3. UI

在 `Cx2ccTab` 增加一个独立的“思考强度转换”区块：

- 每行两个紧凑输入：来源强度、目标强度。
- 使用删除图标按钮移除行，带 tooltip/aria-label。
- “添加规则”命令新增空行；“恢复默认”一次提交默认集合。
- 编辑保存在本地草稿，行级 blur 或明确保存动作通过现有串行 settings patch owner 提交完整数组，避免并发局部写覆盖。
- 保存失败恢复服务端 canonical 值并显示 toast。
- 小屏幕换行，输入与按钮不重叠。

## 4. Authenticated Local Reentry

当前合法链路为：

```text
Claude -> CX2CC outer provider
       -> direct POST local /v1/responses + one-time nonce
       -> inner Codex provider selection/failover
       -> real upstream
```

外层是协议转换/委托层，不是真实上游重试 owner。对 `bridge_type=cx2cc && source_provider_id=None`：

- Provider 基础与有效最大尝试数固定为 1，禁用外层同 Provider 重试预算。
- 只有 typed intent 与最终 POST URL 完全匹配并通过自回环验证时，发送函数的外层 first-byte timeout 才设为 `None`。
- nonce 仍在 fingerprint 之后签发，仍由 ingress 移除并一次性消费；客户端断开通过现有 abort guard 取消委托请求。
- 内层请求不带可再次生成 nonce 的 typed intent，因此最多一跳。

该设计消除 120 秒双层计时与授权消费后的必败重试，同时不改变普通 Provider、自定义 CX2CC source Provider 或伪造 localhost 请求。

## 5. Observability

映射后的 Responses body 是 attempt `reasoning_effort` 提取器的输入，因此现有实时/历史日志自然显示 target 值，无需增加第二套日志字段。

回环修复保留内层 Codex attempt 证据。外层不再产生误导性的 timeout + self-loop 对；若内层最终失败，外层只记录一次委托结果。

## 6. Compatibility And Rollback

- 旧设置字段缺失时迁移为默认规则；旧固定 effort 不转成映射规则，避免恢复固定覆盖。
- 未命中原样透传，避免软件升级滞后时破坏未来 effort。
- 回滚代码后新版 settings 的未知字段由旧版 serde 行为处理；发布前用真实旧设置 fixture 验证不会破坏启动。
- 回环改动只作用于 typed current-gateway CX2CC intent；删除该分支即可回滚，不影响普通网络客户端。

## 7. Test Boundaries

- Settings：default/migration/normalize/duplicate/bounds/patch ownership/config round-trip。
- Bridge：default map、自定义 map、unknown passthrough、absent、disabled、enabled/adaptive、日志提取。
- Gateway：delegated attempt budget=1、authorized reentry timeout disabled、ordinary timeout retained、ordinary self-loop rejected、nonce replay rejected。
- Frontend：列表增删改、失败回滚、恢复默认、校验、窄屏稳定布局、settings field mapping。
