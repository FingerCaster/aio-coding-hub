# 发布自启动修复版本 - Implementation Plan

## 1. 准备独立 Worktree

- 将已批准的规划产物和任务状态落盘。
- 从本地 `main` 创建 `release-0-60-29-preflight` Orca worktree，记录 branch 与路径。
- 在干净 worktree 中再次确认 `pnpm tauri:clippy` 的唯一发布阻塞。

## 2. 修复 Clippy 回归

- 在 `upstream_error.rs` 增加文件私有参数结构。
- 更新两个生产调用点和全部 `upstream_error_decision` 单元测试调用点。
- 不改判断顺序、重试计数、attempt 上限或返回结果，不添加 lint allow。

## 3. 实现验证

依次运行：

```powershell
pnpm tauri:fmt
Set-Location src-tauri
cargo test --locked --lib upstream_error_decision -- --test-threads=1
Set-Location ..
pnpm tauri:clippy
pnpm tauri:check
pnpm check:prepush
```

若聚焦测试过滤范围不能覆盖全部相关测试，补充运行对应 failover 模块或完整 Rust
library suite。最后由独立 check agent 审查行为不变量、diff 和门禁结果，并修复其确认的
问题后重跑受影响检查。

## 3A. 修复 Linux CI 专属 Clippy 阻塞

- 根据 Actions run `29697960436` 精确修复 3 个 dialog `unused_mut`、3 个
  `while_let_on_iterator` 和 2 个平台/test dead-code。
- 读取 Image Gen、Provider Share 和 config-migration code-spec，逐项证明修改不改变
  信任边界与数据行为。
- 在 Windows 重跑 fmt/check/clippy/相关测试；在 Docker Linux Rust 1.90 环境安装与
  CI 相同的 Tauri 系统依赖并运行：

```bash
cd src-tauri
cargo clippy --all-targets --locked -- -D warnings
```

- Linux 验证已在 `rust:1.90-bookworm` 中完成，使用 `rustc 1.90.0`，安装 CI 的首选
  Tauri 依赖组合（Ayatana AppIndicator、WebKit2GTK 4.1 与 libsoup 3）。仓库源码以只读
  方式挂载，仅为构建所需的 `src-tauri/gen/schemas` 提供独立可写挂载；上述 Clippy
  命令退出码为 0。
- 重新运行 `pnpm check:prepush`，由独立 check agent 做第二轮 full-scope 审查。

## 4. 提交与集成

- 动态从当前 shell 解析 `node` 和 `pnpm` 目录并注入 hook PATH。
- 仅提交任务文件和 Clippy 修复，确认 worktree 干净。
- 将修复分支 fast-forward 合并到本地 `main`，不触碰主工作区用户改动。
- 执行 `git push origin main`；若 CI 暴露本地平台未覆盖的 lint，只修复日志明确列出的
  发布阻塞并重新验证。等待 `main` CI 全部成功。

## 5. 创建并合并 Release PR

- 首次 dispatch `release.yml`（`ref=main`，无额外输入）。
- 定位 `release-please--*` PR，等待 Cargo.lock workflow 与 PR CI 完成。
- 审查 PR 只包含 `0.60.29` 版本文件、Changelog 与自动同步的 Cargo.lock。
- 合并 PR，并用 `git pull --ff-only origin main` 更新本地 `main`。

## 6. 构建并发布 `0.60.29`

- 第二次 dispatch `release.yml`（`ref=main`，无额外输入）。
- 记录 release PR merge commit SHA，确认 workflow 输出的 checkout ref 为该 SHA。
- 持续监控所有 matrix build、`assemble-latest-json`、`publish` 和
  `publish-homebrew-cask`，直到 run 完成。
- 验证 tag `aio-coding-hub-v0.60.29`、非 draft Release、tag target SHA、
  `latest.json` 及支持矩阵资产清单。

## 7. 收尾

- 完成最终 Trellis check/spec 判断，归档任务并记录 journal。
- 审查自动 bookkeeping commit，仅推送任务归档和日志到 `origin/main`。
- 最终报告版本、tag/commit、Release URL、Actions 结果与资产验证结果。
