# Technical Design

## Boundaries

本任务由五个互相独立的修复组成。父任务只拥有共同约束、跨层质量门禁和最终合并；每个子任务拥有自己的行为与测试。所有业务代码在独立分支 `FingerCaster/first-group-reliability` 中修改。

## R1 Route Draft Initialization

在 Provider 视图的每 CLI UI 状态中记录 `routeDraftInitialized`。排序模式列表与激活模式列表完成加载后，仅当该标记尚未设置时选择当前有效模式或默认顺序。任何显式 setter 调用同时标记已初始化，保证用户在异步查询期间的选择不会被覆盖。

## R2 Manual Upstream Review

workflow 使用只读 contents 权限和 PR 写权限，统一把同步结果推到专用同步分支并创建/更新 PR。目标分支不由 workflow 直接 push 或 merge。独立策略脚本静态解析 workflow，拒绝危险命令、写权限和非失败闭合的合并状态处理；CI 与脚本自测共同约束该合同。

## R3 Startup Retry

Rust `DbInitState` 只缓存成功的数据库句柄；互斥锁继续串行化并发初始化，失败返回但不写缓存。前端启动状态采用“注册监听 -> 获取快照”的组合流程，使用订阅 token/世代号忽略卸载后的事件和较旧快照。重试复用相同的安全快照更新机制。

## R4 Diagnostic Redaction

新增纯函数诊断脱敏模块，集中处理敏感键、自由文本 token、URL 用户信息/查询/hash、异常值和资源预算。所有前端诊断边界先调用该模块；Rust 接收前端错误时再次脱敏，形成不信任调用方的纵深防御。失败或无法分类时输出固定占位符，不回退到原始值。

## R5 Notification Reliability

每次请求活动更新都递增 CLI 对应世代号，定时回调携带调度时世代。回调先验证世代和前端状态，再读取后端活动请求快照；同 CLI 仍活跃或快照失败都终止通知。通知静默期和现有通知设置逻辑不变。

## Compatibility And Rollback

- 不新增持久化字段，不需要迁移。
- 公共调用签名尽量保持不变；新增 helper 只在内部组合。
- 每个子任务有独立测试与逻辑提交，出现回归时可按提交回退。
- 最终只允许 fast-forward 或正常无冲突合并；主 worktree 未提交内容不得被暂存、覆盖或包含在本任务提交中。
