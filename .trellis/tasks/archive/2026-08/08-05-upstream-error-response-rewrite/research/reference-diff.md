# 参考差异

- 参考行为源：`26c4e020c1b2565dedd0326540937b730596792b` 和参考仓库最终 `5b13683`。
- 参考新增独立 Rust 模块约 706 行、前端规则组件约 686 行，可作为纯规则/表单行为源。
- 共享文件 `context.rs`、`finalize.rs`、`thinking_signature_rectifier_400.rs`、`upstream_error.rs`、`routes.rs` 已被当前 fork 大幅扩展，禁止整体替换。
- 当前 HTTP retry 内容匹配已经限定解码前缀和隐私边界；response rewrite 必须复用该 body read，不创建第二个 reader。
- 参考规范要求高优先级正文规则不可评估时整体 fail open，而不是跳过到低优先级规则。
- 当前主日志与 attempts 已可分离状态，改写只需在最终 response/log projection 点更新 client-visible status。

