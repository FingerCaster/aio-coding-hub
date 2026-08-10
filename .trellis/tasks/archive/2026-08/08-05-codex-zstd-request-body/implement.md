# Codex 压缩请求体兼容执行计划

- [ ] 在独立 worktree 中记录当前本仓库基线，并对照 `KNaiFen/aio-coding-hub@5b13683b` 相关文件。
- [ ] 移植 zstd/brotli 依赖和统一有界 decoder；覆盖 gzip/x-gzip、zlib/raw deflate、br、zstd/zst。
- [ ] 移植目标 Codex 端点 matcher、重复/堆叠编码解析、8 层上限和反向逐层解码。
- [ ] 在 BodyReader 前置规范化并删除三个实体编码头；接通 400/413 early error。
- [ ] 增加 `GW_INVALID_REQUEST_CONTENT_ENCODING` 的 Rust/TypeScript 合同与诊断映射。
- [ ] 移植并适配 route 测试：reasoning/session 解析、identity 上游、插件修改、损坏/超限、零上游尝试和非目标透传。
- [ ] 运行目标 Rust 测试、`cargo fmt --check`、`cargo check`、Clippy 及相关生成绑定/前端检查。
- [ ] 执行 Trellis quality check，修复发现并提交子任务。

## Validation Commands

```powershell
cargo test --manifest-path src-tauri/Cargo.toml request_body
cargo test --manifest-path src-tauri/Cargo.toml body_reader
cargo test --manifest-path src-tauri/Cargo.toml codex_request_classifier
cargo test --manifest-path src-tauri/Cargo.toml codex_session_id
cargo test --manifest-path src-tauri/Cargo.toml gateway_inspects_and_normalizes_zstd_codex_request
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Review Gates

- 每个解码层均有硬上限，无部分 JSON 继续处理。
- 目标 Codex 请求始终 identity 上游；非目标请求传输语义不变。
- 未知、损坏、过深和超限均在零上游尝试时 fail-closed。
- 请求正文和认证信息不进入新增日志。
