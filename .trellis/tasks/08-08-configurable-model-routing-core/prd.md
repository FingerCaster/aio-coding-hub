# 融合可配置模型路由核心

## Goal

基于外部提交 `e2a2996265a92baf9d363e8ee8e6370a817f2d62` 的产品契约，在本仓库当前网关架构中实现可配置的最终模型与 reasoning effort 路由，并与账户用量 gate、故障转移、日志和成本保持一致。

## Background

- 本任务是父任务 `08-08-configurable-model-routing` 的第二个独立交付物。
- 必须等待 `08-08-remove-codex-provider-translation` 验收完成，并以已经包含账户用量产品提交及后续重复余额回切抑制修复的 `8757d32c` 为最低实现基线。
- 外部提交只作为契约和测试参考；因当前架构、schema 与 Observer/TUI 差异，禁止直接 cherry-pick。

## Requirements

### M1. 全局策略和 Provider 三态覆盖

提供全局 enabled + 精确规则列表；Provider 未设置 override 时继承全局，enabled override 完整替换全局，disabled override 对该 Provider 关闭路由。旧配置缺字段时保持禁用/继承的安全默认值。

### M2. 精确路由契约

规则只按客户端原始 source model 精确、区分大小写匹配一次，每条可修改 target model、reasoning effort 或两者；校验重复 source、空结果、长度、控制字符及有界 effort 文本，不能把前一条规则的 target 继续级联匹配。无匹配规则时原样透传，managed `aio/` 与非真实推理请求不参与路由。

### M3. 最终 wire request 改写

覆盖当前支持的 Claude Messages/CX2CC、普通 Codex Responses/compact、Grok Chat/Responses 和 Gemini generateContent/streamGenerateContent。在协议准备、内建清理和 `RequestBeforeSend` 插件之后原子改写最终 path/query/body；只有 target 与 effort 的全部要求都能在最终 wire request 中验证时才提交改写。压缩请求体必须保持编码和 body state 一致，最终 URL 必须在成功改写之后构造。

### M4. Provider 级失败关闭与故障转移

规则匹配但当前 Provider 无法完整改写时，不发送上游请求，记录稳定的发送前路由失败，并继续同一客户端请求的下一候选 Provider。该失败不得改变健康、熔断、账户余额 gate、恢复 epoch 或 transport retry；各候选必须重新解析自己的策略。全部候选失败后才返回明确错误，不自动创建下一次客户端请求。

### M5. 日志与成本语义

保留客户端原始 requested model，记录最终 Provider 作用域的 source/effective model、effort 和策略来源。成本按最终 target model 查找；目标价格缺失时保持未知，不回退 source model，marker 不得跨 attempt 泄漏。

### M6. 桌面端配置与观测

在现有全局设置、Provider 编辑器、请求日志和成本界面提供完整配置/展示，并同步 Provider 分享、复制、完整配置导入导出及生成绑定。Provider UI 明确显示“继承全局 / 使用专属规则 / 禁用路由”。严格 Provider 分享格式升级到 v3；v1/v2 仅兼容读取且默认无 override。完整配置继续使用 v4，通过模型路由功能最低版本门槛承载新增策略。

### M7. 依赖顺序与实现基线

实现前必须确认 Codex 转译删除子任务已经完成，并从其 SQLite 44 基线使用 44->45 增加 Provider policy override；全局 policy 使用 settings 56->57。仍需重新记录实际 HEAD、schema 版本和 dirty paths，不得覆盖账户用量字段或其他会话改动，不得绕开共同发送前 gate，也不得让路由失败进入 `blocked_provider_ids`、recovery epoch 或 transport commit。

### M8. 损坏策略防御性降级

全局策略无法解析或清洗后无有效规则时按 disabled 处理；Provider override 无法解析时返回显式 disabled policy，不能因解码成 `None` 而继承全局。记录有界诊断后继续未改写请求；该路径不视为“有效规则匹配后应用失败”，因此不触发 Provider failover。

## Acceptance Criteria

- [ ] 全局和 Provider 三态策略的校验、持久化、复制/分享/导入导出与 UI 测试通过。
- [ ] 各支持协议、path/query/body/effort、压缩请求和插件顺序均有自动化覆盖。
- [ ] 路由改写失败不发送当前 Provider，并按约定继续下一候选；健康、熔断、余额 gate、session/failback 与 transport retry 不受污染。
- [ ] 日志保留原始模型且展示最终路由，成本只按最终目标模型结算，未知价格不回退。
- [ ] managed `aio/`、辅助接口、非 POST 请求、无匹配规则与 disabled policy 均保持既有行为。
- [ ] 损坏全局策略和 Provider override 均禁用正确作用域、无敏感信息泄漏且不中断原请求；损坏 override 不继承全局。
- [ ] 普通 Codex 在删除 bridge 后仍支持配置路由；Observer/TUI、通知和发布链路未被引入。
- [ ] 联合网关 E2E、Rust 测试、前端测试、lint、type-check 和生成绑定一致性检查通过。

## Out Of Scope

- Observer/TUI 投影。
- 通知、release、CI 版本改动。
- 恢复已删除的 Codex Provider 协议转译。
- 通配符、正则或非精确 source model 匹配。
