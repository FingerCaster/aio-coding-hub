# 执行计划：发布 `0.60.41` 正式版

## 前置审阅门

- [x] 用户审阅并确认 PRD、技术设计和本执行清单。
- [x] `implement.jsonl` 与 `check.jsonl` 各包含真实 spec/research 条目。
- [x] `task.py validate` 已通过。
- [x] 用户确认后运行 `task.py start`，将任务切换为 `in_progress`。

## 有序步骤

### 1. 固定发布基线

- [x] fetch `origin/main` 与 tags，记录本地/远端 SHA、stable latest、Beta 10、release-please
      分支、`release-channels` head 及 `0.60.41` tag/Release 空闲状态。
- [x] 确认独立 worktree 除本任务目录外无用户或其他任务改动；设置 GitHub 默认仓库并在所有
      GitHub 操作中继续显式使用 `-R FingerCaster/aio-coding-hub`。
- [x] 核对 manifest 与五个源码版本位置仍为 `0.60.40`，历史 `Release-As` 不会强制另一个版本。

### 2. 运行发布前合同

- [x] `pnpm install --frozen-lockfile`。
- [x] 运行 `node scripts/release-source.selftest.mjs`。
- [x] 运行 `node scripts/release-contract.selftest.mjs` 与
      `node scripts/release-version-overlay.selftest.mjs`。
- [x] 运行 `node scripts/release-promotion.selftest.mjs`、
      `node scripts/release-channel.selftest.mjs`。
- [x] 运行 signing scope self-test/contract、`pnpm check:support-matrix`、
      `pnpm check:homebrew-cask`、`pnpm check:ci-change-scope` 和 `git diff --check`。
- [x] 失败时停止；不以 Beta 10 已通过测试为由跳过稳定发布合同。

### 3. 生成 `0.60.41` release PR

- [x] 记录 dispatch 前时间与 `origin/main` SHA，无输入触发 `release.yml`。
- [x] 唯一定位首轮 run，等待终态；确认 `release-please` 成功、`release_created=false`，
      build/publish job 按合同跳过。
- [x] 定位唯一 open release PR；若版本不是 `0.60.41`，停止并记录，不手改生成分支。
- [x] 等待 Cargo.lock 同步后锁定最终 `headRefOid`，验证精确六文件、五处版本值与 changelog range。
- [x] 等待并核对最终 head 的 CI、frontend、Rust、generated bindings、support contracts、
      Windows build；任何 pending/failed/旧 SHA check 都阻止合并。
- [x] 使用标准 merge 合并 release PR，fetch 并验证 `origin/main` merge SHA 与六文件内容。

### 4. 构建并发布正式资产

- [x] 记录第二次 dispatch 前时间与 release PR merge SHA，再次无输入触发 `release.yml`。
- [x] 唯一定位第二轮 run，确认 `headSha` 等于 merge SHA，记录 run ID/URL。
- [x] 等待 `release-please`、四平台 build、candidate assemble、promotion、publish、
      release-channel 和 Homebrew job 全部到终态。
- [x] 若 run 失败，读取具体 job/draft/candidate 状态后停止；不盲目重跑或覆盖资产。

### 5. 独立发布核验

- [x] 证明 `origin/main`、`aio-coding-hub-v0.60.41` tag、Release target、workflow head 和
      candidate source 是同一 40 位 SHA。
- [x] 验证 Release `draft=false`、`prerelease=false`、成为 latest，名称/版本正确。
- [x] 验证精确 14 项资产均 uploaded、非空且具有 SHA-256 digest；保存 Windows MSI 直接链接、
      大小和 SHA-256。
- [x] 下载并解析 `latest.json`，验证 `0.60.41`、严格四平台集合、正式 asset URL 和非空签名。
- [x] 验证 `release-channels` CAS/high-water 选择稳定 `0.60.41`，Beta 10 对象保持不变。
- [x] 验证 Homebrew Cask 生成，记录 tap sync 成功或无 token 的显式 skip。

### 6. 检查与收尾

- [x] 运行 `trellis-check`，将 run/PR/SHA/资产/channel/Homebrew 证据写入任务 research/artifacts。
- [x] 评估是否有新的通用发布约束需要 `trellis-update-spec`；没有新合同则不制造 spec churn。
- [ ] 归档任务并记录 journal；归档提交通过独立 PR 合入 `origin/main`，不改变发布源 SHA。
- [ ] 将 Orca worktree comment 更新为最终版本、SHA 和 run，并标记 `completed`。

## 验证与回滚点

| 阶段 | 成功条件 | 停止/回滚点 |
| --- | --- | --- |
| 基线 | `0.60.41` tag/Release 空闲，远端 SHA 固定 | 不触发 workflow |
| 首轮 run | 仅生成正确 release PR | 不合并错误 PR |
| PR final head | 六文件/版本/changelog/CI 全部精确一致 | 保留 PR，等待或报告失败 |
| 第二轮 run | 不可变 SHA 上完整 job 图成功 | 保留 draft/candidate，禁止覆盖 |
| 发布后 | latest、14 资产、manifest、channel、Homebrew 一致 | 不改 tag/资产，单独处理渠道问题 |
