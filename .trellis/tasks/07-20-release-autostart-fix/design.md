# 发布自启动修复版本 - Technical Design

## Scope Boundary

发布前只允许两组已确认的 Clippy 阻塞修复：修复
`response/upstream_error.rs` 中 `upstream_error_decision` 的参数数量，以及消除
GitHub Actions run `29697960436` 精确报告的 8 个 Linux 平台专属 lint。前者的输入值、
决策顺序、返回值以及所有调用时机保持不变；后者仅采用下文列出的机械变换。除这两组
阻塞外不修改产品行为，发布配置和 workflow 本身也不做修改。

## Clippy Fix

在 `upstream_error.rs` 内定义文件私有参数结构，归组一次决策所需的规则匹配状态、
有效 retry policy、已用配置型重试数、当前 attempt 索引，以及配置型与基线 attempt
上限。`upstream_error_decision` 改为接收该结构（可连同极少数真正独立的输入，前提是
参数总数满足 Clippy），生产调用点和单元测试以相同字段构造输入。

必须保留以下语义不变量：

- count-tokens 请求立即 `Abort`，不计为配置型重试。
- 只有已经匹配的 HTTP 规则可以消费配置型瞬时重试容量。
- 配置型决策优先于通用基线决策；配置型预算耗尽后再回落到基线决策。
- 未匹配规则不能消费为瞬时重试预留的额外 attempt。
- 基线 `RetrySameProvider` 达到基线上限后切换 Provider。
- Provider override、熔断失败计数和 attempt reason 均不受本次结构调整影响。

参数结构保持文件私有，不扩展跨模块 API，不复制 policy，不新增持久化或前端契约。

## Linux CI Clippy Fixes

GitHub Actions run `29697960436` 在 Linux `--all-targets` 下报告 8 个平台专属 lint。
按以下边界逐项机械修复：

- `image_gen.rs` 与 `providers/share.rs` 的 3 个 native dialog builder 先建立不可变
  binding；Windows/macOS 用同名 shadow binding 调用 `set_parent`。Linux 继续使用
  原 builder，Windows/macOS 继续绑定同一个 parent window。
- `history.rs` 两个 rustix directory iterator 与 `skill_fs.rs` 一个 iterator 改用
  `for entry in entries`，每个 `Result` 的错误映射、`.`/`..` 跳过和后续顺序不变。
- 删除 Unix `open_trusted_task_dir` wrapper；Unix 删除路径本来通过已验证 handle 的
  `try_clone` 获取 capability，只有 Windows 的 `acquire_delete_task_handle` 调用同名
  Windows 实现。
- `set_after_quarantine_validation_test_hook` 仅由 `#[cfg(windows)]` 回归测试调用，
  因而将 setter 收窄为 `#[cfg(all(test, windows))]`；生产 quarantine 验证和测试 runner
  不变。

不得以 lint allow 代替上述修复。Linux 验证使用 Docker Linux engine 中的 Rust
1.90 与 CI 相同系统依赖，执行 `cargo clippy --all-targets --locked -- -D warnings`。

## Release Flow

```text
local main + Clippy fix
  -> origin/main + successful main CI
  -> release workflow dispatch #1
  -> release-please PR + Cargo.lock sync + successful PR CI
  -> merge release PR (immutable release commit SHA)
  -> release workflow dispatch #2
  -> tag/draft release resolved before matrix build
  -> matrix jobs checkout immutable SHA
  -> stable assets + latest.json
  -> publish release + Homebrew job
```

GitHub CLI 命令必须显式指定 `-R FingerCaster/aio-coding-hub`。发布 tag、Release
target、workflow `checkout_ref` 与 release PR merge commit SHA 必须相互一致。

## Compatibility And Rollback

- 代码修复不改变序列化格式、数据库、配置、生成绑定或用户可见行为。
- Linux lint 修复不改变 filesystem capability、原子导入导出或 native dialog 授权。
- 推送前若门禁失败，只修复当前分支上的具体回归，不推送。
- release PR 合并前若 CI 或版本审查失败，保留 PR 并修正后重跑，不创建 Release。
- Release 构建失败时保留 draft 与日志，修复具体发布问题后重跑现有 workflow；不得以
  可变分支替代不可变 SHA 构建。
