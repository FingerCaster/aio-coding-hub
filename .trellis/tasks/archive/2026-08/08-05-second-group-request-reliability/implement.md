# 第二组执行计划

- [ ] 用户审阅并批准本组 PRD/design/implement 范围。
- [ ] 从当前本地 main 创建独立 worktree 和集成分支，记录主工作树保护基线。
- [ ] 启动并完成 `08-05-transport-error-retry-backoff`，通过子任务质量门禁并提交。
- [ ] 启动并完成 `08-05-codex-zstd-request-body`，通过子任务质量门禁并提交。
- [ ] 在独立 worktree 跑完整受影响检查和 Trellis 最终审查。
- [ ] 确认主工作树既有 diff 未变化，以非破坏方式合并到本地 main。
- [ ] 在 main 合并态复核提交、测试结果和工作树保护证据后通知用户。

## Final Validation

```powershell
pnpm typecheck
pnpm lint
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

若完整测试受环境阻断，必须记录具体命令、错误和已通过的替代覆盖，不得把环境失败表述为通过。
