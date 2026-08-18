# `0.60.41` 稳定发布执行证据

执行日期：2026-08-18（Asia/Shanghai）

仓库：`FingerCaster/aio-coding-hub`（仅 `origin`）

## 发布前基线

- 基线复核时间：`2026-08-18T01:01:00.7002248Z`。
- 独立 worktree：`D:/OrcaProjects/aio-coding-hub-fork/stable-0-60-41-release`；
  当前分支 `FingerCaster/stable-0-60-41-release`。
- 本地 `HEAD` 与 `origin/main` 均为
  `fb1a21c7c281b6fed2927cb55e529787eb3b8727`，ahead/behind 为 `0/0`。
- 工作树除本任务目录外无 tracked/untracked 改动；GitHub CLI 默认仓库已设置为
  `FingerCaster/aio-coding-hub`，后续仓库操作仍显式使用 `-R FingerCaster/aio-coding-hub`。
- `aio-coding-hub-v0.60.41` 的远端 tag 查询无结果，GitHub Release 查询返回
  `release not found`。
- GitHub latest 是公开稳定版 `aio-coding-hub-v0.60.40`，Release ID `367539335`，
  source 为 `99272e4b2beffc52f483efef2e3985d9867d8051`。
- 旧 release-please 分支 head 为
  `4d84dbe999ad63327fe6211f4b3b4e6f54153052`，相对 `origin/main` 落后 54、领先 1；
  open PR `#31` 与生成分支 manifest 仍为 `0.60.40`，不得直接合并。
- `release-channels` head 为 `58f248ff0fd7a5798907615c2133d96de53a99d0`；
  `latest-beta.json` blob 为 `b8fa224797e9b0a93342218f660dd51097fa9d42`，
  `beta-channel-state.json` blob 为 `b8fb8bfbfa893184e6fa3a6eafd9e5902b1417c1`。
- Beta 10 Release ID `371702027`，公开 prerelease，source 为
  `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`。其 14 项资产的
  `name/size/state/digest` 排序快照 SHA-256 为
  `2acf36f1a2859b32b665054d96ed7254d4e792e4255f2ea0f5363385ce99a71c`。
- `.release-please-manifest.json`、`package.json`、`src-tauri/Cargo.toml`、
  `src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 均为 `0.60.40`。
  历史 `Release-As` 仅为 `0.60.40`、`0.60.6`、`0.60.0`；自然 patch bump 目标仍为
  `0.60.41`。

## 发布前合同

`pnpm install --frozen-lockfile` 成功，随后以下命令全部退出 `0`：

- `node scripts/release-source.selftest.mjs`
- `node scripts/release-contract.selftest.mjs`
- `node scripts/release-version-overlay.selftest.mjs`
- `node scripts/release-promotion.selftest.mjs`
- `node scripts/release-channel.selftest.mjs`
- `node scripts/check-release-signing-secret-scope.selftest.mjs`
- `node scripts/check-release-signing-secret-scope.mjs`
- `pnpm check:support-matrix`
- `pnpm check:homebrew-cask`
- `pnpm check:ci-change-scope`
- `git diff --check`

关键输出确认 stable/Beta source ancestry、release contract、deterministic four-file overlay、
promotion、CAS/channel fixtures、签名 scope、支持矩阵、Homebrew Cask 及 CI workflow contract
全部通过。发布前不存在允许跳过的失败项。

## 首轮 release PR

- 首轮无输入 dispatch 前时间为 `2026-08-18T01:02:45Z`，source 为
  `fb1a21c7c281b6fed2927cb55e529787eb3b8727`。run
  [`32086854358`](https://github.com/FingerCaster/aio-coding-hub/actions/runs/32086854358)
  成功；`release-please` 成功，build、assemble、promotion、publish、channel、Homebrew 和
  repair jobs 全部按合同 skipped，未创建正式资产。
- release PR [`#31`](https://github.com/FingerCaster/aio-coding-hub/pull/31) 正确刷新为
  `0.60.41`。release-please 提交为
  `ddeae5ff3db21e12ae6ce1d4b90b2197a3c5c34a`，Cargo.lock 同步提交后最终 head 为
  `3c3852252a64ce8c92aa5267fc1a402b52c58d49`。
- 最终 head 相对 base 精确修改 `.release-please-manifest.json`、`CHANGELOG.md`、
  `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和
  `src-tauri/tauri.conf.json` 六个文件。五处版本均为 `0.60.41`，changelog 顶部 compare
  range 为 `aio-coding-hub-v0.60.40...aio-coding-hub-v0.60.41`。
- Cargo.lock 同步 run 为
  [`32086928017`](https://github.com/FingerCaster/aio-coding-hub/actions/runs/32086928017)，
  Windows build run 为
  [`32086925339`](https://github.com/FingerCaster/aio-coding-hub/actions/runs/32086925339)，
  CI run 为
  [`32086928035`](https://github.com/FingerCaster/aio-coding-hub/actions/runs/32086928035)；
  三者均绑定最终 head `3c3852252a64ce8c92aa5267fc1a402b52c58d49`。
- CI attempt 1 的 frontend runner 在项目代码执行前的
  `Install system deps (Tauri/Linux)` 停留 54 分钟后由用户授权定向重试；run 被取消后，仅
  rerun frontend 及其下游 `ci-gate`。attempt 2 仍绑定同一 final head，frontend 与
  `ci-gate` 均成功。Rust、Windows build、Cargo.lock sync、support contract、三平台
  desktop contract、change-scope 和 pr-title 均在同一 head 成功；docs-contract 按合同
  skipped。不存在代码测试失败或旧 SHA 检查替代最终检查的情况。
- PR 使用标准 merge 合并；merge commit 为
  `72b749341e2db5e26cd6c839cd4f24072213ac25`，其父提交为原 main 与精确 PR final head。
  fetch 后 `origin/main` 精确指向该 merge commit。

## 正式发布

- 第二次无输入 dispatch 前时间为 `2026-08-18T02:25:03.7389867Z`，source 为 release PR
  merge commit `72b749341e2db5e26cd6c839cd4f24072213ac25`。run
  [`32091757145`](https://github.com/FingerCaster/aio-coding-hub/actions/runs/32091757145)
  的 `headSha` 与该 SHA 精确一致。
- release-please 创建 tag `aio-coding-hub-v0.60.41` 与空稳定 draft Release
  `372053135`；初始状态为 `draft=true`、`prerelease=false`、0 assets，tag 与 target 均为
  上述不可变 SHA。
- release-please、Windows、macOS ARM、macOS Intel、Linux、candidate assemble、
  promotion、publish、release-channel 和 Homebrew jobs 全部成功。repair-beta job 按合同
  skipped。run 于 `2026-08-18T02:53:59Z` 以 `success` 完成。

## 发布后核验

- `origin/main`、tag、Release target、workflow head 和 candidate source 均为
  `72b749341e2db5e26cd6c839cd4f24072213ac25`。
- Release [`aio-coding-hub-v0.60.41`](https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v0.60.41)
  为 `draft=false`、`prerelease=false`，并由 GitHub `/releases/latest` 返回；Release ID 为
  `372053135`，公开时间为 `2026-08-18T02:53:36Z`。
- 精确 14 项资产均为 `uploaded`、size 大于零且具有 `sha256:` digest：

| Asset | Bytes | SHA-256 |
| --- | ---: | --- |
| `aio-coding-hub-linux-amd64-wayland.AppImage` | 93522424 | `534aba541055a1c3411fc2032564adea9db9021be1b35037f1fcf712e1d15b4c` |
| `aio-coding-hub-linux-amd64.AppImage` | 93522424 | `916f52fbe5b935f4dd8d2d69359505c6b5c9fe0d9f7271311d6b0249412320c5` |
| `aio-coding-hub-linux-amd64.AppImage.sig` | 432 | `5e85af3a9202dca4b5e710b560d90aa5959b394fa48da157fe59a2424b547f90` |
| `aio-coding-hub-linux-amd64.deb` | 17088596 | `8cf2af721e7836f27a2fa00f9a7fb9166a516b601ba1c8125e76dcb0a42bd42a` |
| `aio-coding-hub-macos-arm.tar.gz` | 17286154 | `1a96c51e7477d9c82e6ef84140ce2f90e2077934d89894ff7d59e8c92fd1767a` |
| `aio-coding-hub-macos-arm.tar.gz.sig` | 416 | `35711566d3371832ab53d6fdd3b6ac0c8aec4758d1b7558fc5d882bb073b7cf0` |
| `aio-coding-hub-macos-arm.zip` | 16768462 | `d515f53f652ab88537ca16c98f2a49f3a4fbace73eac095c6f0f864ab4ced190` |
| `aio-coding-hub-macos-intel.tar.gz` | 17919237 | `9300d1a1d008e86babc0ae1ba0e3134bca9014803f44a3a3e2dff10c76f584f3` |
| `aio-coding-hub-macos-intel.tar.gz.sig` | 416 | `f144268993772c301e76944271f1a28c15d742a1ccabcd79c287faea83f80889` |
| `aio-coding-hub-macos-intel.zip` | 17589104 | `cd845032ab1d03bffe3ff06f00831944cc4fc42be64515e743e2e056cd2511e3` |
| `aio-coding-hub-win64-portable.zip` | 18034486 | `3e2c0e111a461f3f54d26c698b85938e66ee0547a74e50895a478fbe46e46289` |
| `aio-coding-hub-win64.msi` | 17797120 | `3bbc95c38a6b15ac6f4cf06d8272698df7f79aa2dcd4defec8cfdaefa41c4fc2` |
| `aio-coding-hub-win64.msi.sig` | 428 | `3e9e8a2fdd42f67ab117def1b994e76577e89dbf81d1fdf2230c3447e1581c2e` |
| `latest.json` | 7964 | `65b095851cc201e5865237efa140ab859e9296b875b868014b83e0fbe97a8bca` |

- 按 `name/size/state/digest` 排序后的正式资产快照 SHA-256 为
  `6d16e4da2156ed17cac02c8704884ba423968c83d36fc85c1d38d43dca5f9d73`。
- Windows MSI 直接链接为
  `https://github.com/FingerCaster/aio-coding-hub/releases/download/aio-coding-hub-v0.60.41/aio-coding-hub-win64.msi`；
  独立下载得到相同的 17,797,120 bytes 与 SHA-256。
- 独立下载的 `latest.json` 为 `0.60.41`，严格包含 `windows-x86_64`、
  `darwin-x86_64`、`darwin-aarch64`、`linux-x86_64` 四个平台。所有 URL 精确指向本次
  stable assets；四个平台 signature 与对应 `.sig` 资产 UTF-8 内容逐字一致。
- `release-channels` 从 `58f248ff0fd7a5798907615c2133d96de53a99d0` 以非强制 CAS 推进到
  `a1577a8856b4ab2ffabb072534c99d8293cf1a2e`，唯一父提交为旧 head，树中仍只有
  `latest-beta.json` 与 `beta-channel-state.json`。频道 manifest 与 Release
  `latest.json` 同为 7,964 bytes、SHA-256
  `65b095851cc201e5865237efa140ab859e9296b875b868014b83e0fbe97a8bca`。
- channel state 为 schema 2、`promote`、`selected_channel=stable`、selected/high-water
  `0.60.41`、source 与 run identity 正确，`previous_selected_tag` 为 Beta 10。Beta 10
  Release 仍为 ID `371702027`、source `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`、公开
  prerelease 且保留 14 项资产；发布后资产快照 SHA-256 仍为
  `2acf36f1a2859b32b665054d96ed7254d4e792e4255f2ea0f5363385ce99a71c`。
- Homebrew Cask 生成步骤成功；`HOMEBREW_TAP_TOKEN` 未配置，显式 skip 步骤成功且 tap sync
  步骤按合同 skipped。

## 最终质量检查

发布执行和任务证据完成后，下列本地检查全部退出 `0`：

- release source、stable/Beta contract、version overlay、promotion、release-channel、
  signing scope、support matrix、Homebrew Cask 与 CI scope 全套合同；
- `pnpm lint`；
- `pnpm typecheck`；
- `pnpm check:generated-bindings`；
- `pnpm tauri:fmt`；
- `python ./.trellis/scripts/task.py validate .trellis/tasks/08-17-stable-0-60-41-release`；
- `node scripts/check-spec-links.mjs`；
- `git diff --check`。

本次未修改业务源码、workflow、helper 或现有 spec。执行过程没有发现超出现有 release
operations、Beta channel 与 CI scope 合同的新通用约束，因此无需制造 spec churn。任务归档、
journal 与 Orca comment/status 由主会话在本执行证据通过最终校验后处理。
