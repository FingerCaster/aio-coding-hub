# Codex 压缩请求体兼容

## Goal

参考 `KNaiFen/aio-coding-hub` 最新实现，将 Codex 压缩 JSON 请求在网关入口有界解码并规范化为明文，使 reasoning.effort、prompt_cache_key、Session、指纹、插件和请求日志都读取真实 JSON，同时兼容不支持压缩请求体的上游中转站。

## Reference Evidence

- 参考仓库：`https://github.com/KNaiFen/aio-coding-hub`，核对基线 `5b13683bd2a44699cd8c99e7aeffc317bcc19674`。
- `13a3c6ffe183a0e19ced2e63b425b78815504c3f` 首先加入 zstd 有界检查、透明转发和变更后重压缩。
- `909b7a0d4c1c6f4903bb4e39b374112dd39a9947` 将目标 Codex 请求升级为统一明文规范化，并覆盖其他编码、堆叠编码、early error 和端到端测试。
- 第一组可靠性修复同样以该 fork 为参考来源并按本仓库现状选择性移植；本任务延续同一策略，不使用官方 upstream 代替参考 fork。

## Requirements

- R1：仅规范化 Codex POST JSON 端点：`responses`、`responses/compact` 和 `chat/completions`；支持 `/v1/`、嵌套前缀、查询串和尾斜杠。
- R2：支持 `gzip`、`x-gzip`、`deflate`、`br`、`zstd`、`zst`、重复 `Content-Encoding` 头及按 HTTP 语义反向解码的堆叠编码；`identity` 不计入编码层。
- R3：`deflate` 先尝试 zlib，再尝试 raw deflate；有效编码最多 8 层，每层解码结果均受现有请求体上限约束。
- R4：成功规范化后删除 `Content-Encoding`、`Content-Length` 和 `Transfer-Encoding`，插件、模型识别、reasoning、Session、隐私过滤、日志、Provider 选择与重试统一使用明文 JSON；发往上游时不得恢复或重新压缩目标 Codex 请求。
- R5：未知编码、损坏压缩流、非法头或超过编码层数时在 Provider 选择前返回结构化 HTTP 400；任一解码层超限时返回现有结构化 HTTP 413。
- R6：编码失败不得触发上游请求、重试、熔断或 Provider 失败计数；公开错误和日志不得包含正文、凭据或底层解码器细节。
- R7：增加并贯通 `GW_INVALID_REQUEST_CONTENT_ENCODING` 后端错误码、状态覆盖、前端常量、短标签和诊断说明。
- R8：非目标请求保留现有压缩透传/重编码语义；不修改响应解压、`Accept-Encoding`、remote compaction、认证、数据库或设置 UI。

## Acceptance Criteria

- [ ] 所有支持编码、别名、重复头和堆叠编码均可在三个目标端点解码，最终上游正文可直接解析 JSON 且无三个实体编码头。
- [ ] zstd Codex 请求中的 reasoning.effort、prompt_cache_key、Session 和请求指纹被真实解析，不再因压缩回退为 unknown/fingerprint-only。
- [ ] 插件或隐私过滤修改正文后仍以明文发送；请求日志和 Provider/重试链路使用同一明文状态。
- [ ] 未知/损坏/过深编码返回 `GW_INVALID_REQUEST_CONTENT_ENCODING` 与 400；任一层超限返回正文过大错误与 413；两者上游尝试数均为零。
- [ ] 非 POST、非目标路径和非 Codex 请求保持既有压缩传输合同。
- [ ] 前后端错误码合同、目标 Rust 测试、完整 Rust 检查和相关前端检查通过。

## Out Of Scope

- 响应体 zstd/br/deflate 解压或新的 `Accept-Encoding` 协商。
- 新增用户设置、Provider 配置或压缩开关。
- 修改 Codex remote compaction 或语义续写逻辑。
