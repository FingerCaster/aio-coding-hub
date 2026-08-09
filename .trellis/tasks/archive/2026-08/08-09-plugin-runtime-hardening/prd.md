# 插件运行时完整性与资源边界

## Goal

在不恢复用户正在删除的 SDK/脚手架包文件前提下，补齐已确认保留的插件运行时安装完整性、持久化原子性、Hook 合同、失败策略和 Extension Host 资源边界。

## Evidence

- `plugin_install_from_file` 仅接收路径，preview checksum 未绑定到确认安装。
- `plugin_versions` 使用 `INSERT OR IGNORE`，同 ID/version 可隐藏不同内容。
- Hook body budget 等于网关最大请求体，明显大于 QuickJS 适合的固定预算；runtime wire 与 SDK camelCase 合同不一致。
- fail-open 插件的非法 header patch 当前仍返回错误；timeout 未覆盖 gate/queue/cold start/cleanup；idle recycle 缺少主动调度。
- 候选参考：`e94c83bd`、`cab1229a`、`4ee5faa8`、`e6cf04d3`、`d26524f2`、`735cec12`、`94da784b`、`4800bc87`、`871b84dc`。

## Requirements

- `R1`：preview 返回内容摘要，confirm/install 必须对同一文件内容重新校验；路径相同但内容变化时拒绝。
- `R2`：已记录 plugin ID/version 的 manifest、安装目录和内容身份不可静默改变。
- `R3`：Hook 可见 body/stream/log/message 使用固定有界预算；canonical wire 为 camelCase，并为已安装旧插件提供明确兼容策略。
- `R4`：非法 header patch 遵循插件 fail-open/fail-closed policy，且应用 patch 具备事务性。
- `R5`：一个 absolute deadline 覆盖 gate、队列、cold/warm activation、RPC 和 cleanup；超时实例按身份摘除并终止。
- `R6`：Extension Host 具有主动 idle sweeper，配置写入不覆盖 Storage，并发写有原子/CAS 语义。
- `R7`：fail-closed redaction/log policy 在入口、未知 policy、invalid output 和 circuit-open 路径一致生效。
- `R8`：本任务不恢复 `packages/plugin-sdk` 或 `packages/create-aio-plugin` 的用户删除；若生产 runtime 依赖这些包才能通过合同，则向协调者升级而非擅自恢复。

## Acceptance Criteria

- [ ] 文件在 preview 后被替换时安装零落盘；相同内容正常安装。
- [ ] 同 ID/version 不同内容被拒绝，旧合法版本读取/回滚保持兼容。
- [ ] 大 body、字段 casing、header policy、queue/cold/warm timeout 和 idle recycle 有直接测试。
- [ ] 并发配置/Storage 更新无丢失，fail-closed 日志在所有提前退出路径一致。
- [ ] Rust plugin tests、前端插件测试（适用时）、bindings、typecheck、lint、Clippy 和 diff check 通过。
- [ ] SDK/脚手架删除状态保持协调者指定结果。
