# 技术设计

## 设置写入

TanStack mutation scope 只负责跨组件串行；持久化 runner 负责在执行时读取最新设置并应用 changed-key patch。缓存只接收后端确认结果，不再作为完整写入源。保留现有字段所有权、校验和外部副作用回滚契约。

## 模型价格别名

编辑命令调用严格 `read`，UI Query error 成为显式不可编辑状态。用于成本估算的后台读取继续 fail-open，从而把编辑完整性与可用性降级分开。

## 兼容性

- 不改变设置 DTO 的字段语义。
- schema 版本以 Rust 类型为权威，前端常量和 fixture 同步。
- 不顺带重构设置页已有局部队列。

## 验证

使用确定性 barrier 制造并发顺序；使用临时目录/测试 failpoint 验证 alias 读取失败零写入。
