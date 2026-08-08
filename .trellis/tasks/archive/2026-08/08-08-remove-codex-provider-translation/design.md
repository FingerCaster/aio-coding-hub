# 删除 Codex Provider 转译技术设计

## 设计目标

从活动产品契约中完整删除 Codex Provider 的三类协议转译，同时通过一次原子迁移清理旧记录。删除后，普通 Codex Responses/compact、Claude CX2CC、通用插件协议桥接、账户用量 gate 和历史请求日志都保持原有行为。

本子任务是父任务的第一个实现交付物。最后产品行为基线为 `8757d32c`，本次规划 HEAD `534f878d` 在其上只增加 Trellis journal；启动实现时必须重新确认 HEAD、SQLite/settings 版本及 dirty paths，不能从仍停在 `c2e6b065` 的原会话工作树回退代码。

## 所有权边界

### 必须删除

- 桌面端 Codex Provider 的“转译”标签页、`CodexBridgeSection`、端点/来源 Provider/模型映射表单状态和提交分支。
- `codex_to_openai_chat`、`codex_to_openai_responses`、legacy `codex_to_anthropic_messages` 的活动领域常量、校验、准备、探测、协议注册、请求转换、响应转换及流式转换。
- 活动 Provider DTO、upsert、summary、生成绑定和新交换格式中的 Codex 专属 `ModelMapping` 语义。
- 仅为 Codex-to-OpenAI-Responses 存在的响应缓存和相关 attempt/stream 字段。

### 必须保留

- 普通 Codex Provider 对 Responses、Responses compact 和既有认证/压缩请求的支持。
- Claude Provider 的 `cx2cc` 转换、其 inbound Anthropic/outbound OpenAI Responses 转换和通用流式包装。
- 插件协议贡献注册表及仍由 CX2CC/插件拥有的通用 `source_provider_id`、`bridge_type` 基础字段。
- 账户用量查询、共同发送前 gate、`blocked_provider_ids`、recovery epoch、Session/failback 和 transport ownership 语义。
- `request_logs` 及 `attempts_json` 中已经落盘的 Provider id/name 快照。

## SQLite 43 到 44 迁移

迁移与新安装 baseline 同步升级；启动时若 44 已被占用则先重新分配版本，不能覆盖已有迁移。

### 事务流程

```text
BEGIN
  -> 检查 schema/列存在性，允许重复执行
  -> 预检 managed profile 是否异常引用待删除 bridge 的 provider_model
       -> 命中：以稳定恢复错误 ROLLBACK，不删 profile 或本机文件
  -> 删除 bridge_type 属于三类 legacy Codex bridge 的 Provider
       -> 既有 FK 清理 pool/default route/sort/model/extension/credential
       -> source_provider_id 引用按 ON DELETE SET NULL
       -> 对无 FK 保证的活动引用执行显式、受限清理
  -> 将所有存活 Provider 的 model_mapping_json 归一为 '{}'
  -> 写入 schema version 44
COMMIT
```

受支持的 managed profile 创建路径只接受直接 Codex 模型，因此预检正常情况下为空。若历史损坏数据违反该不变量，自动删除 profile 会越过本任务的数据所有权，故整笔迁移失败并给出不含凭据、路径或请求内容的稳定错误。

`request_logs` 不通过 Provider 外键持有记录，迁移不得对其执行删除或重写。Provider 删除也不得读取、复制或重新绑定来源 Provider 的 URL、凭据和模型。

## 活动领域与运行时删除

### Provider 契约

- `domain/providers/types.rs` 等活动领域只接受仍支持的内建 `cx2cc`；三类 legacy 字符串只允许存在于迁移/导入兼容分类器的私有作用域，不能再次导出为活动常量。
- 从 Provider upsert/summary/query projection 移除 `model_mapping`。数据库物理 `model_mapping_json` 列暂留以避免重建表，但写入固定为空对象，业务代码不得读取其语义。
- 复制 Provider 时只复制仍支持的 direct/CX2CC/插件字段，不能复活 legacy bridge 或模型映射。

### 协议桥接

- 从 protocol bridge registry 删除三个 Codex factory 和 `ProviderModelMapper`。
- 删除只服务非 CX2CC 内建 bridge 的 `bridge_preparation.rs` 路径；若通用插件仍调用其中某个抽象，则只下沉保留真实共享部分，不能保留 Codex 分支。
- 删除 Codex bridge 专属 inbound/outbound 模块：OpenAI Responses inbound、OpenAI Chat outbound、Anthropic Messages outbound。保留 CX2CC 使用的 Anthropic inbound 和 OpenAI Responses outbound。
- 从非流式和流式响应路径移除 Codex bridge 分支与专属 response cache；保留 CX2CC 的通用响应转换包装。
- 可用性探测、模型目录和用量路径不再识别三类 legacy bridge。

### 桌面端

- `ProviderEditorDialog` 仅为 Claude 显示 CX2CC 配置；Codex 不再显示“转译”模式。
- 删除 Codex bridge 的 form state、submit model、effects/actions/view data、卡片标签和测试夹具。
- 重新生成 Rust/TypeScript 绑定，不能手工编辑生成结果；保留账户用量字段及其他既有工作区改动。

## 导入、分享与兼容壳

### 完整配置包

`prepare_config_import` 是任何 `clear_existing_config_data` 之前的纯预检边界。预检扫描 Provider 时，发现任一 legacy Codex bridge 立即以稳定错误拒绝整包；不得清空当前配置、静默跳过或返回部分成功。事务导入的其他失败回滚行为保持不变。

完整配置 v1-v4 为兼容读取可继续保留 `model_mapping_json` 形状，但规范化结果固定为 `{}`，活动导出也只写空对象。该字段只是旧格式壳，不构成产品功能。

### 单 Provider 分享

现有 v1/v2 结构使用严格未知字段拒绝，继续按原版本解析：

- legacy bridge type：稳定拒绝，不能导入为 direct/CX2CC。
- legacy `model_mapping`：读取后丢弃并规范化为空，不进入活动 DTO。
- 普通 Provider/CX2CC：保持既有禁用导入、预览和敏感字段策略。

后续模型路由子任务再引入严格 Provider 分享 v3；本子任务不得提前混入路由字段。

## 错误契约与诊断

- 旧完整配置或单 Provider 分享包含 legacy bridge：返回固定、可本地化的“不再支持 Codex 转译”分类，不输出 URL、凭据、正文或完整 Provider JSON。
- 数据库存在异常 managed profile 引用：迁移整体失败，错误指出需要先修复数据引用，不删除任何受影响记录。
- 已删除运行时类型若绕过导入进入活动 upsert：验证阶段拒绝，不能落库后再降级运行。

## 兼容性与回滚

- SQLite 删除是有意的数据迁移，应用层回滚不能自动恢复旧 bridge。发布前应依赖现有数据库备份/迁移事务作为恢复点。
- 迁移失败必须原子回滚；成功后降级到仍依赖 Codex bridge 的旧版本不受支持。
- 物理 `model_mapping_json` 列暂留，降低 SQLite 表重建和降级读表风险，但其中值已清空且没有活动所有者。
- 此子任务不提升 settings schema，不引入模型路由 policy，也不修改账户门控状态。

## 验收测试矩阵

- 迁移：三类 bridge 删除、普通/CX2CC/source 保留、所有活动引用清理、历史日志保留、重复执行一致。
- 异常数据：managed profile 引用 bridge model 时全事务失败且无部分删除。
- 配置：旧完整包预检原子拒绝，当前配置不变；分享 v1/v2 legacy bridge 拒绝、mapping 丢弃。
- 领域：CRUD、复制、探测、模型目录、用量和生成绑定不再暴露或创建 Codex bridge。
- 网关：三个 bridge factory/请求/响应/流式路径不可达；普通 Codex Responses/compact、Claude CX2CC 和插件 bridge 回归通过。
- UI：Codex 无“转译”标签和残留状态；Claude CX2CC 与账户用量编辑不回归。
