# 设置与模型价格数据一致性

## Goal

消除普通设置并发保存的整份快照覆盖风险，并确保模型价格别名读取失败时无法用空配置覆盖有效规则。

## Evidence

- `src/query/settings.ts:63-111` 的普通 set/patch mutation 没有共享 mutation scope；patch 在执行时从缓存展开整份设置。
- `src-tauri/src/commands/model_prices.rs:79-86` 的编辑读取使用 `read_fail_open`。
- 候选参考：`5c756edc`、`db92a480`；实现需适配当前设置页已有 runner queue 和模型价格 schema v2。

## Requirements

- `R1`：所有普通设置生产写入共享串行 scope；调用方 patch 只声明实际拥有的 changed keys。
- `R2`：串行执行时基于最新权威状态合并，不从过时 Query cache 构造可覆盖其他字段的快照。
- `R3`：模型价格别名编辑命令严格读取并向 UI 暴露错误；读取未成功前禁用新增、编辑和保存。
- `R4`：成本计算等只读路径可继续使用明确的 fail-open 策略，编辑路径不得复用它。
- `R5`：前后端 alias schema 版本、默认值和保存 payload 一致。

## Acceptance Criteria

- [ ] 两个不相交设置 patch 反向完成时均被保留；相同字段遵循明确串行顺序。
- [ ] 普通 set/patch 的缓存同步和 Codex proxy projection invalidation 不回归。
- [ ] alias 文件损坏、读权限失败和 schema 错误均显示阻断状态且零写入。
- [ ] 成功读取、升级旧 schema、保存和成本计算回归通过。
- [ ] 聚焦前端测试、Rust 测试、typecheck、lint、generated bindings 检查通过。
