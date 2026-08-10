# Current main 落地证据

核查基线：`main` 当前头为 `444b92ac`。集成提交 `a7e7675c` 在当前
`main` 祖先链；含完整任务记录的分支提交 `f48840a1` 与 archive bookkeeping
`49650389` 均不在当前祖先链，本次只对照内容，不 cherry-pick。

- `src-tauri/src/gateway/proxy/request_body.rs` 实现有界 gzip/deflate/br/zstd（含别名、
  重复/堆叠编码）解析、Codex 明文规范化与失败回退边界。
- `src-tauri/src/gateway/proxy/handler/middleware/body_reader.rs` 在 Provider 选择前
  将未知、损坏或过深编码映射为结构化早期错误；`http_util.rs` 提供有界编解码辅助。
- `src-tauri/src/gateway/proxy/routes.rs`、`error_code.rs`、`status_override.rs` 以及
  `src/constants/gatewayErrorCodes.ts` 保持错误码和状态覆盖一致。
- `request_body.rs`、`body_reader.rs` 和相关 gateway route 测试覆盖成功解码、编码别名、
  超限、非法压缩与非目标请求透传。

该子任务的目标已由当前 `main` 的实现和测试满足；本记录不引入业务变更。
