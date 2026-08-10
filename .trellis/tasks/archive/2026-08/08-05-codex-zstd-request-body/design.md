# Codex 压缩请求体兼容设计

## Reference Baseline

以 `KNaiFen/aio-coding-hub@5b13683b` 的当前压缩请求合同为语义基线，主要来源为：

- `13a3c6f`：zstd codec helper 与 `GatewayRequestBody::Zstd`。
- `909b7a0`：Codex 目标端点的多编码明文规范化、early error、错误码和端到端测试。

实现时按当前本仓库代码边界移植，不整仓合并、不直接覆盖 fork-specific gateway 行为。

## Boundary

受影响范围：

- `src-tauri/Cargo.toml` / `Cargo.lock`：直接依赖 zstd 与 brotli。
- `gateway/proxy/http_util.rs`：统一有界 decoder，支持 gzip、deflate、brotli、zstd；保留 gzip/zstd encode helper 供非目标请求状态机使用。
- `gateway/proxy/request_body.rs`：编码解析与 `normalize_codex_request_body`，并补充单层 zstd 透明状态。
- `gateway/proxy/handler/middleware/body_reader.rs`：在构建 `GatewayRequestBody` 前执行目标 Codex 规范化并映射 early error。
- Rust/前端错误码合同与 route 集成测试。

## Data Flow

```text
wire body (bounded read)
  -> target matcher: cli=codex + POST + JSON endpoint suffix
  -> parse every Content-Encoding header/token
  -> validate supported encodings and <= 8 effective layers
  -> decode in reverse declaration order
       each intermediate output <= max_request_body_bytes
  -> remove Content-Encoding / Content-Length / Transfer-Encoding
  -> GatewayRequestBody(identity)
  -> plugins / introspection / model / reasoning / session / fingerprint
  -> Provider transforms and retries
  -> identity JSON upstream
```

非目标请求不经过规范化，继续进入现有 `GatewayRequestBody`。单层 zstd 可像 gzip 一样供插件查看 decoded body，未修改时原始字节直通，修改后按 zstd 重压缩。

## Target Matching

- `cli_key == "codex"`。
- 方法为 POST。
- 去掉查询串和尾斜杠后，路径段后缀为：`responses`、`responses/compact` 或 `chat/completions`。
- 非目标路径不读取或修改编码头。

## Encoding Contract

- aliases：`gzip | x-gzip`、`zstd | zst`。
- `deflate`：zlib decoder 失败后再尝试 raw deflate。
- `br`：brotli decoder。
- 所有头按出现顺序拼接，逗号拆分；解码按反向顺序。
- `identity` 忽略；有效层最多 8 层。
- 在实际解码前先验证完整编码链，避免已部分解码才发现未知 token。

## Failure Contract

- 未知 token、非文本头、损坏流、超过 8 层：`InvalidRequestContentEncoding`，HTTP 400，公开码 `GW_INVALID_REQUEST_CONTENT_ENCODING`。
- 任一中间层或最终层超过请求体上限：复用 BodyTooLarge，HTTP 413。
- 两者均在插件、Provider 选择和尝试前短路；日志仅记录安全分类消息。

## Compatibility

- 目标 Codex 请求统一发送 identity JSON，避免中转站无法解析压缩正文。
- 非目标请求保持当前透明代理或变更后重编码行为。
- 不改 reasoning/session 模块；修复来自它们重新获得 decoded JSON。
- 不改响应体处理和 remote compaction。

## Port Strategy

两个参考提交跨越本仓库后续 fork-specific 变更，不能假设原样 cherry-pick 无冲突。按文件和行为选择性移植，并逐项对照参考 route 测试；Cargo.lock 由本仓库依赖图重新生成。

## Rollback

移除 BodyReader 规范化、错误码和两个直接依赖即可回退；无持久化格式或数据库迁移。
