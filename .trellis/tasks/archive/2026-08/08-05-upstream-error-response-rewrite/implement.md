# 执行计划

1. [x] 以当前 settings schema 为基线增加响应规则类型、常量、容错读取、严格写入和迁移测试。
2. [x] 新增独立 Rust matcher/envelope/audit 模块及完整单元测试。
3. [x] 在当前 failover context/upstream error/finalize 中逐块接入 terminal candidate，保持 body 单次读取和 fork 决策顺序。
4. [x] 添加 route tests：最终命中、中间失败后成功、多 Provider、transport、fake200、header 与日志状态。
5. [x] 增加 generated type 的前端 adapter、规则 validation/helper 和设置 fixture。
6. [x] 在统一入口实现“最终响应改写”列表与 Dialog；接入 Provider 列表和 settings save。
7. [x] 增加 Home/Realtime/Logs 命中 badge 和 fail-open parser 测试。
8. [x] 运行 focused Rust/TS 测试、generated bindings、fmt/typecheck/lint/build。
9. [x] 执行子任务全范围 Trellis check，修复发现后形成一个 `feat(gateway)` 原子提交 `88673001`。

## 回滚点

- settings 失败：仅回滚新字段和迁移，不改 retry policy。
- matcher/构造失败：保持候选为 `None` 或使用既有响应，禁止放宽 body cap。
- failover 回归：撤出 candidate integration，不改当前 probe/failback/transport backoff。
- UI 回归：保留 backend 字段，单独回滚统一面板内的响应规则视图。
