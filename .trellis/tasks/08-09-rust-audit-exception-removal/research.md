# 研究摘要

- `.github/workflows/ci.yml` 当前显式 ignore `RUSTSEC-2026-0194`、`RUSTSEC-2026-0195`。
- 基线锁文件包含 `quick-xml 0.39.2`、`plist 1.9.0`、`wayland-scanner 0.31.10`。
- 候选 `b0698f57` 将其更新为 `quick-xml 0.41.0`、`plist 1.10.0`、`wayland-scanner 0.31.11` 并恢复 plain `cargo audit`。
- `origin/main@336e01be` 已单独修复 JS 高危依赖；本子任务不得改写 pnpm workspace/lock。
- 需审查 Cargo 精确 update 的传递变化，不能复制候选完整 Cargo.lock。
