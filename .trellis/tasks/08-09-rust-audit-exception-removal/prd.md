# Rust 依赖审计豁免移除

## Goal

升级触发 `RUSTSEC-2026-0194`、`RUSTSEC-2026-0195` 的依赖链并恢复无忽略项的 fail-closed Rust 依赖审计。

## Evidence

- `.github/workflows/ci.yml` 当前运行 `cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195`。
- 当前锁文件包含 `quick-xml 0.39.2`、`plist 1.9.0`、`wayland-scanner 0.31.10`。
- 候选参考：`b0698f57`，升级目标为 `quick-xml 0.41.x`、`plist 1.10.x`、`wayland-scanner 0.31.11`；不得替换整份候选锁文件。

## Requirements

- `R1`：仅更新解除公告所需的 Rust 直接/传递依赖和最小锁文件集合。
- `R2`：CI、pre-push 或其他 audit 入口统一删除两个 ignore；未知 audit 失败继续阻断。
- `R3`：保留 `origin/main@336e01be` 已有 JS 依赖安全修复，不降级或重写其 pnpm lock 结果。
- `R4`：验证 Windows host 及能执行的目标相关编译；无法本地覆盖的平台明确交给 Actions。

## Acceptance Criteria

- [ ] `cargo audit` 无 ignore 且不再报告两个目标公告。
- [ ] Cargo.lock 只包含解析所需变化，没有候选仓库无关依赖漂移。
- [ ] Rust format、check/Clippy/tests 和 CI 静态合同通过。
- [ ] pnpm workspace/lockfile 未因本任务改变。
