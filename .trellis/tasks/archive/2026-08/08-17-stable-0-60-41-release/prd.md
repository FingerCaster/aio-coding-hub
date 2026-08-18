# 发布 aio-coding-hub 0.60.41 正式版

## Goal

将已经过用户实际测试且无已知问题的 `aio-coding-hub-v0.60.41-beta.10`
对应功能，通过 `FingerCaster/aio-coding-hub` 的标准稳定发布流程发布为
`aio-coding-hub-v0.60.41`，并验证版本、不可变源码、签名资产、稳定更新渠道、
Beta 指针和 Homebrew 状态一致。

## Background

- 当前 GitHub latest 稳定版是 `aio-coding-hub-v0.60.40`；
  `.release-please-manifest.json`、`package.json`、`src-tauri/Cargo.toml`、
  `src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 的稳定源码版本均为 `0.60.40`。
- `aio-coding-hub-v0.60.41-beta.10` 已公开发布为 prerelease，绑定功能提交
  `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`，14 项资产、签名、四平台 manifest
  和 release-channel CAS 已验证；用户确认实际测试无问题。
- 最新 `origin/main` 为 `fb1a21c7c281b6fed2927cb55e529787eb3b8727`，包含上述功能提交及其
  Trellis 归档。正式发布源必须是 release PR 合并后的新 `origin/main` 提交，而不是
  直接复用 Beta tag、Beta Release 或 Beta 资产。
- `aio-coding-hub-v0.60.41` 的 tag 和 GitHub Release 当前都不存在；没有标题包含
  `0.60.41` 的 release PR。
- 现存 release-please 生成分支仍停留在 `0.60.40`，比 `origin/main` 落后 54 个提交；
  首次无输入 dispatch 应刷新该分支并创建/更新新的 release PR，不得直接合并旧 head。
- `release-please-config.json` 对 pre-major feature 使用 patch bump，因此自然候选应为
  `0.60.41`。先观察首轮生成结果；只有生成版本错误时才停止并另行评估空的
  `Release-As: 0.60.41` 覆盖提交。
- 稳定发布由 `.github/workflows/release.yml` 的两次无输入 dispatch 完成：第一次只生成或
  刷新 release PR，第二次在该 PR 合并后创建 tag/草稿 Release、跨平台构建、候选校验、
  发布、release-channel CAS 和 Homebrew 处理。
- 所有仓库操作只针对 `origin` / `FingerCaster/aio-coding-hub`，不检查或操作 `upstream`。

## Requirements

- **R1 版本目标**：唯一目标是稳定版 `0.60.41`，tag 必须为
  `aio-coding-hub-v0.60.41`；不得发布 `0.60.42`、`0.61.0`、新的 Beta 或复用旧版本。
- **R2 独立环境**：发布协调只在独立 worktree
  `D:/OrcaProjects/aio-coding-hub-fork/stable-0-60-41-release` 中进行，保留其他 worktree
  的 tracked/untracked 改动和活跃任务不变。
- **R3 标准入口**：使用两次无输入
  `gh workflow run release.yml -R FingerCaster/aio-coding-hub --ref main`；不得用手工
  `release_tag` 路径绕过 release-please，也不得把 Beta Release 直接改成稳定 Release。
- **R4 发布前合同**：在首次 dispatch 前运行 release source、stable/Beta contract、
  version overlay、promotion、release-channel、signing scope、support matrix、Homebrew 和
  CI scope 自测；任何失败均阻止 dispatch。
- **R5 release PR 内容**：release PR 最终 head 只能修改标准六文件：
  `.release-please-manifest.json`、`CHANGELOG.md`、`package.json`、
  `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`。
  六文件版本必须一致为 `0.60.41`，changelog 顶部 compare range 必须从
  `aio-coding-hub-v0.60.40` 到 `aio-coding-hub-v0.60.41`。
- **R6 精确 head 门禁**：等待 Cargo.lock 同步提交稳定后，重新读取 PR 的最终
  `headRefOid`；`ci-gate`、frontend、Rust、generated bindings、support contracts 和
  Windows build 必须在该精确 head 上成功，预期按范围跳过的 job 必须可解释，旧 head
  的成功结果不得复用。
- **R7 合并与来源**：通过标准 merge 合并 release PR；第二次 dispatch 的 workflow head、
  `origin/main`、稳定 tag、GitHub Release target、候选 manifest source 和所有构建 checkout
  必须解析为同一个 40 位提交 SHA。
- **R8 正式资产**：稳定发布必须重新构建并公开精确 14 项正式资产，所有资产非空、状态为
  uploaded 且带 SHA-256 digest；不得复制、重命名或覆盖 Beta 10 的现有资产。
- **R9 更新渠道**：正式 Release 必须为 `draft=false`、`prerelease=false` 且成为 GitHub
  latest；`latest.json` 必须报告 `0.60.41`，严格包含四个受支持平台的正式 URL 和非空签名。
  `release-channels` 应通过现有高水位/CAS 逻辑将 Beta 参与者指向稳定 `0.60.41`，或在
  合同允许的高水位不前进分支中给出明确证据。
- **R10 Homebrew**：稳定发布必须生成 Homebrew Cask；有 token 时同步 tap，无 token 时仅可
  通过工作流的显式 skip 分支成功，不得把缺失 token 当作隐式成功。
- **R11 失败关闭**：错误版本、额外/缺失文件、pending/failed/旧 head 检查、tag/source
  漂移、非空或身份不符 draft、资产矩阵/digest 不一致、CAS 竞争失败均必须停止后续阶段；
  不 force-push、不覆盖资产、不手工修补已发布 Release。
- **R12 审计与收尾**：记录两次 workflow run、release PR、最终 SHA、tag、Release、资产、
  manifest、渠道和 Homebrew 证据；完成 Trellis 检查、归档、journal 和 Orca 状态更新。

## Acceptance Criteria

- [ ] 首轮无输入 dispatch 成功结束，只生成/刷新目标 release PR，不构建或发布资产。
- [ ] release PR 最终 head 仅含标准六文件，全部选择 `0.60.41`，changelog range 正确。
- [ ] release PR 的所有必需检查在同一个最终 head SHA 上成功，并已标准 merge 到 `main`。
- [ ] 第二轮无输入 dispatch 在 release PR merge SHA 上成功完成完整发布 job 图。
- [ ] `aio-coding-hub-v0.60.41` tag、`origin/main`、Release target 和 workflow source SHA 一致。
- [ ] GitHub Release 已公开、非 prerelease、为 latest，且精确包含 14 项带 digest 的正式资产。
- [ ] `latest.json` 为 `0.60.41`，四平台 URL 与公开资产一致且签名非空。
- [ ] release-channel CAS 结果可审计，Beta 参与者能选择稳定 `0.60.41`；Homebrew job 合同满足。
- [ ] Beta 10 Release、资产和 tag 保持不变；其他 worktree 与活跃任务未被改写。
- [ ] Trellis 任务通过全量校验并归档，Orca worktree 最终标记为 `completed`。

## Out Of Scope

- 不修改业务源码、发布 workflow 或 helper scripts；若现有发布合同自身失败，停止并另开任务。
- 不访问、同步或修改 `upstream`。
- 不删除、重写或重新分类 `aio-coding-hub-v0.60.41-beta.10`。
- 不绕过 required checks，不手工上传或覆盖 Release 资产。
