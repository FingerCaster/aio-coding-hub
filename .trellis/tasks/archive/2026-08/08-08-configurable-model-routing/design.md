# 可配置模型路由融合联合设计

## 父任务职责

父任务只维护共同需求、子任务顺序、跨层契约和最终联合验收，不直接修改产品代码。两个可独立验证的交付物为：

1. `08-08-remove-codex-provider-translation`：删除 Codex 转译并迁移旧数据。
2. `08-08-configurable-model-routing-core`：基于清理后的 Provider 契约实现最终 wire model/effort 路由。

第二个子任务严格依赖第一个子任务完成。父子关系本身不提供依赖调度，因此启动和复查门槛同时写在三个任务的 `prd.md`/`implement.md` 中。

## 基线与并行会话

- 本次规划 HEAD 为 `main@534f878d`；它相对最后产品行为基线 `8757d32c` 只增加 Trellis journal。`8757d32c` 包括账户用量产品提交以及重复余额回切抑制修复。
- 会话 `019fda50-f0a6-7650-adb2-6b5d2457ebb5` 的工作树仍干净停在 `c2e6b065`，仅用于追溯，不作为后续实现 HEAD。
- 启动每个子任务时重新记录 HEAD、schema 版本和全部 dirty paths。任何新提交占用迁移版本、改变 Provider 解析或发送链所有权时，先回到 Phase 1 更新制品。
- 本规划不依赖、覆盖或提交工作区中其他会话的修改。

## 联合数据流

```text
client request
  -> bounded body decode + immutable original-model inference
  -> provider resolution / account route projection / failback planning
  -> common account, circuit and auth gates
  -> direct Provider or retained Claude CX2CC/plugin preparation
     (Codex translation paths have been removed by child 1)
  -> sanitizer -> RequestBeforeSend plugin
  -> atomic configured model/effort route by this Provider's policy
  -> final URL/body -> transport commit -> upstream
  -> response handling -> attempt/request log -> final-target cost
```

账户 gate 是第一个发送前拒绝所有者；模型路由是最后一个 wire request 改写所有者。模型路由失败发生在 gate 之后、transport commit 之前，不得回写 `blocked_provider_ids`、recovery epoch、健康/熔断、Session/failback 或 transport retry。

## 跨子任务所有权

| 交叉面 | 删除子任务 | 路由子任务 |
| --- | --- | --- |
| Provider types/queries | 移除活动 Codex bridge/ModelMapping，保留 CX2CC/插件通用字段 | 在清理后的 DTO 增加 nullable route policy override |
| SQLite | 43->44 删除 legacy bridge、清空惰性 mapping | 44->45 增加 policy JSON |
| Settings | 不变 | 56->57 增加全局 policy |
| Provider 分享 | v1/v2 兼容读取，bridge 拒绝、mapping 丢弃 | 新增严格 v3 并往返三态 override |
| 完整配置 | 旧 bridge 在 destructive clear 前整包拒绝 | v4 能力门槛往返全局/Provider policy |
| Gateway | 删除 Codex 请求/响应/流式转译，保留 direct/CX2CC/plugin | 插件后原子改写，失败切换 Provider |
| UI/绑定 | 删除 Codex“转译”标签和活动类型 | 增加全局规则、Provider 三态和日志展示 |
| 日志/成本 | 保留历史日志与 CX2CC 语义 | final Provider marker、target 定价、unknown 不回退 |

子任务 2 不得以复用旧模型映射为由恢复 `ModelMapping` 或任何 Codex bridge；子任务 1 也不得提前加入 route policy，避免迁移与生成绑定的所有权混杂。

## 版本与兼容阶梯

```text
SQLite 43
  -> child 1: 44 (remove legacy Codex bridge records)
  -> child 2: 45 (nullable Provider route policy)

Settings 56
  -> child 2: 57 (disabled global route policy default)

Provider Share v1/v2
  -> compatibility readers only
  -> child 2 exports strict v3

Full Config Bundle v4
  -> child 1 rejects legacy bridge before clear
  -> child 2 carries route fields behind v4 capability threshold
```

版本号是规划快照，不是预留锁。任何实现启动都要重新检查；发现占用时以新基线顺延并同步全部迁移、baseline、测试和制品。

## 联合失败矩阵

| 情况 | 行为 |
| --- | --- |
| 数据库含 legacy Codex bridge | child 1 原子删除，保留历史日志和来源/普通/CX2CC Provider |
| managed profile 异常引用待删 bridge model | child 1 整笔迁移失败，不自动删 profile/文件 |
| 旧完整配置含 legacy bridge | destructive clear 前整包拒绝，当前配置不变 |
| 全局策略损坏 | disabled global，原请求不改写继续 |
| Provider override 损坏 | 显式 disabled，抑制全局，原请求继续 |
| 有效规则匹配但 wire 应用失败 | 当前 Provider 零上游，健康中性，继续下一候选 |
| 后续 Provider 成功 | 返回同一客户端请求的正常响应，不创建新请求 |
| 所有 eligible Provider 均路由失败 | 稳定 502 路由错误及安全 attempt 审计 |

## 发布与回滚边界

- 先独立完成、检查、提交和归档删除子任务，再基于该提交重新评审路由子任务。
- 删除迁移成功后不能靠代码降级恢复旧 bridge，只能从迁移前备份恢复；迁移事务失败则无部分删除。
- 路由字段是加性的且默认 disabled，可通过关闭策略回退行为；Provider 分享 v3 由旧应用拒绝。
- 联合发布前必须同时验证普通 Codex、Claude CX2CC、插件 bridge、账户 gate/failback 和 configured route，不能只运行两个子任务的局部单测。

## 最终联合验收

- 产品中没有 Codex Provider 转译入口或活动运行时，旧数据/备份处理符合已确认策略。
- 普通 Codex 与 Claude CX2CC 保持可用，并能按最终路由契约配置 model/effort。
- 路由匹配、插件顺序、压缩请求、Provider failover、账户 Blocked/recovery 和 Session reservation 的组合测试通过。
- 原始模型、final Provider marker、target pricing 和 unknown cost 在日志/桌面展示中一致。
- SQLite/settings/share/bundle/生成绑定版本矩阵一致，工作区其他改动完整保留。
