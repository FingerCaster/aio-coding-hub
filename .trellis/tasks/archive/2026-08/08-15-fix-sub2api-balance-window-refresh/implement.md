# 实施计划

## Ordered Checklist

- [x] 在 domain 增加内部 `ProviderAccountUsageFetchIntent` 和严格的 Sub2API 订阅窗口
  维护谓词，先补表驱动单元测试。
- [x] 将 `fetch_account_usage_uncached` 改为接收 intent；Runtime 的普通调度传
  `Background`，force epoch 调度及手动兼容回退传 `Manual`。
- [x] 抽取一次 Sub2API GET 的现有错误映射，新增同源 `{}` POST 预检和仅 400 后的
  第二次 cache-busting GET，不修改 NewAPI/Custom 分支。
- [x] 新增本地 HTTP server 回归，验证请求序列、空 JSON、无 `model`、认证/缓存头、
  strict no-trigger 和非 400 fail-closed。
- [x] 扩展 runtime 测试记录 fetch intent，并复跑 force epoch、尾随强刷、generation、
  route projection 与 adapter 隔离测试。
- [x] 按重复缺陷复盘结果更新账户用量契约与跨层思考指南，并同步模板副本（如存在）。
- [x] 完成 focused/full 质量门禁，检查 diff 中无凭据、真实主机、响应正文和账户数据。

## Validation Commands

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked provider_account_usage
cargo test --manifest-path src-tauri/Cargo.toml --locked account_usage::tests
cargo test --manifest-path src-tauri/Cargo.toml --locked provider_account_usage_runtime::tests
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
pnpm tauri:fmt
pnpm typecheck
pnpm lint
pnpm check:generated-bindings
git diff --check
```

根据脚本实际名称调整格式检查命令，但不得跳过 Rust format、Clippy、完整 Rust 测试和
生成绑定验证。

## Review Gates

- [ ] 首次 GET 的 raw body 是严格触发判断的唯一来源，显示 DTO 状态不能反向猜测模式。
- [ ] POST body 构造处可直接证明没有 `model`，且 URL 从同一已验证 usage URL 派生。
- [ ] 每个手动 force epoch 最多一次预检；后台任务不可能携带 Manual intent。
- [ ] 非 400 不会二次 GET，第二次 GET 失败不会伪造正额度。
- [ ] 没有新增 IPC、持久化字段、Provider 测试调用或 route/circuit mutation。

## Risk And Rollback Points

- intent 传递编译面小但连接 command/runtime；完成该步骤后先跑 runtime focused tests。
- HTTP refactor 必须保持现有 status/body-limit/redaction/cache headers；完成后先跑 command
  和 domain focused tests，再进行其他编辑。
- spec 更新独立于运行时代码，可在代码回滚时同步回滚新增合同段落。
