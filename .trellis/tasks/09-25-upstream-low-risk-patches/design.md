# 设计：移植上游低风险修复

## 边界

本任务是选择性 cherry-pick，而不是把 `upstream/main` 的 30 个提交整体合并。目标提交已在临时 worktree 中逐个试应用并确认没有文本冲突；实现阶段仍需检查序列化、cfg 分支和当前 fork 语义是否保持一致。

## 变更面

1. `HomeRequestLogsPanel`：使用稳定的缓存写入 token 默认值，只有正数 token 才显示 TTL，避免缺失数据让整行消失。
2. `tauri.conf.json` 与 asset URL 测试：补齐 Windows asset protocol 的 HTTP origin，并验证 `convertFileSrc` 对两种协议的输出。
3. 通知音频：将 `rodio` 限定到非 macOS target；macOS 模块使用系统 `afplay` 播放写入临时目录的 MP3，设置超时并在失败或超时后终止、等待子进程和清理文件。

## 冲突与取舍

- 本次三个提交均可独立干净应用，因此不需要语义冲突改写。
- 上游其他 provider/model、网关、迁移、OAuth、依赖升级和发布提交与当前 fork 存在冲突或已被本地实现覆盖，保持在研究记录中，不在本任务内处理。
- macOS 代码无法由 Windows host 的普通 Rust 编译覆盖；必须运行 host 可执行的 Rust 测试，并明确记录未执行的 macOS target 验证。

## 验证策略

- 先验证目标文件和测试 diff，再运行相关 Vitest/Rust 测试。
- 运行 `pnpm typecheck`、`pnpm lint`、`pnpm tauri:fmt`、`pnpm check:generated-bindings` 中适用于当前环境的检查，以及 `git diff --check`。
- 对跨层 asset 配置和 target-gated Rust 代码分别检查配置、调用方和测试，不把 host 编译结果当作 macOS 覆盖证明。
