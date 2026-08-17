# Research: origin Beta release and Windows MSI packaging path

- Query: 核对 `FingerCaster/aio-coding-hub` 当前 Beta promotion high-water、最高 Beta tag/Release、`release.yml` 精确输入、Windows MSI 打包/签名路径、14 项官方资产、发布前门槛和最终独立验证命令。
- Scope: mixed (本地仓库只读检查 + `origin` GitHub API/CLI 只读检查；未查看或操作 `upstream`)
- Date: 2026-08-17 (最后一次远端复核约 02:00 +08:00)

## Findings

### 1. 当前远端和版本状态

| 项目 | 只读核验结果 |
| --- | --- |
| 仓库 | `FingerCaster/aio-coding-hub` |
| `origin/main` | `d02728cea0990c0fc019c9c8c6bfa67796a0295b` |
| 稳定源码版本 | `0.60.40` (`package.json`、`src-tauri/tauri.conf.json`、`.release-please-manifest.json` 的远端 `main` 值一致；本地 `Cargo.toml`/`Cargo.lock` 也为 `0.60.40`) |
| GitHub latest 稳定版 | `aio-coding-hub-v0.60.40`, source `99272e4b2beffc52f483efef2e3985d9867d8051` |
| `release-channels` ref | `60caaf330fbc93eb75179075493bb95165dad5df` |
| 当前 Beta 指针 | `aio-coding-hub-v0.60.41-beta.8` |
| promotion high-water | `0.60.41-beta.8` |
| 当前 Beta source | `c56f589e74115a10cc82392f0cc325a87d5a7158` |
| 当前 Beta Release | ID `370978989`, `draft=false`, `prerelease=true`, published `2026-08-15T06:57:03Z` |
| 当前 Beta workflow | run `31869855209`, attempt `1`, conclusion `success` |
| 当前 manifest digest | `a3da77f5ad39930c38eea5744d01aec1f3ba2554af26584018d3f5973f44630f` |
| 下一候选 | `aio-coding-hub-v0.60.41-beta.9` |
| `.9` 占用检查 | tag API 和 Release-by-tag API 均返回 HTTP 404；截至最后复核仍空闲 |

`release-channels` 根目录只含 `latest-beta.json` 和
`beta-channel-state.json`。对 `latest-beta.json` 原始 bytes 独立计算得到
SHA-256 `a3da77...630f`、长度 2628 bytes，与 channel state 和 Beta 8
Release 的 `latest.json` asset digest 完全一致。

本地所读发布实现与当前 `origin/main` 相同。以下文件的本地 Git blob SHA
与 GitHub Contents API 返回的 `main` blob SHA 逐项相等：

- `.github/workflows/release.yml`: `1ce29d83174f8f01be88b922947fae5644428362`
- `scripts/release-contract.mjs`: `25883528712d70947b76378b277ead7371ec4d64`
- `scripts/release-version-overlay.mjs`: `5b293e5c3f03040d7ff6fe71094f584cce1d15d4`
- `scripts/release-promotion.mjs`: `fdb40407d778ab81a3bcfcfb411f311413428b0a`
- `scripts/release-channel.mjs`: `1b6a8b745efe90689e6198194eb5956be728a982`
- `scripts/support-matrix.mjs`: `ce9714abded7bd4e005d656decffcf0234931b56`

任务登记的 feature branch 是 `codex-gpt56-372k-context`，但同名 origin
branch 当前不存在，且按该 head 查询不到 PR。`origin/main` 也仍是规划时的
`d027...`。因此当前状态尚未到发布门槛；不能从未合入的 feature branch
直接发布。

### 2. `release.yml` 的精确 Beta 入口

唯一正常入口是手工 dispatch `.github/workflows/release.yml`：

```powershell
$Repo = "FingerCaster/aio-coding-hub"
$Tag = "aio-coding-hub-v0.60.41-beta.9"
$Sha = "<合入 origin/main 后的 40 位小写 commit SHA>"

gh workflow run release.yml --repo $Repo --ref main `
  -f release_channel=beta `
  -f release_tag=$Tag `
  -f target_commitish=$Sha `
  -f repair_beta_pointer=false
```

四个 workflow inputs 及约束见 `.github/workflows/release.yml:4-26`：

- `release_channel=beta` 必填语义；默认虽为 `stable`，Beta 必须显式传入。
- `release_tag` 必须为 canonical
  `aio-coding-hub-vMAJOR.MINOR.PATCH-beta.N`，不能有前导零，`N >= 1`
  (`scripts/release-contract.mjs:10-15,82-99`)。
- `target_commitish` 对 Beta 必须提供，且必须为 40 位小写 SHA。
- `repair_beta_pointer=false` 是正常发布；`true` 只允许已公开 Beta 的指针恢复。

workflow 在任何构建前做以下 fail-closed 检查：

1. 运行 support matrix、source、release contract、version overlay、promotion、
   channel pointer、signing secret scope 自测
   (`.github/workflows/release.yml:54-78`)。
2. 校验 Beta tag/source 格式 (`.github/workflows/release.yml:87-115`)。
3. 只从 `origin/main` 取目标；目标 SHA 必须可解析且是 `origin/main` ancestor
   (`.github/workflows/release.yml:148-176`)。
4. tag 已存在时必须精确指向该 SHA；不存在时通过 GitHub Git Refs API 创建，
   并重新 fetch 验证 (`.github/workflows/release.yml:178-204`)。
5. Release 已存在时，正常路径只接受 identity 完全相同的空 draft；否则创建
   `draft=true`, `prerelease=true`, `make_latest=false` 的空 draft，然后再次通过
   contract 检查 (`.github/workflows/release.yml:206-293`)。
6. 下游只 checkout 已验证的 40 位 `checkout_ref`，不按 tag checkout
   (`.github/workflows/release.yml:392-415,424-440`)。

### 3. Beta 版本 overlay 和 MSI 路径

源码保持稳定版本 `0.60.40`。每个 release matrix runner checkout 同一不可变
source SHA 后，Beta overlay 只修改以下四个构建工作区文件：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`

文件集合由 `scripts/release-version-overlay.mjs:18-23` 固定；应用逻辑要求
clean checkout、四个原始版本一致且为 stable，写入 tag version 后只允许这四个
文件改变，并为每个文件记录 SHA-256 attestation
(`scripts/release-version-overlay.mjs:354-414`)。

对候选 `.9`：

- 应用版本为 `0.60.41-beta.9`。
- Tauri WiX numeric version 为 `0.60.41.9`；四段均经过 WiX 范围检查
  (`scripts/release-version-overlay.mjs:107-136`)。

Windows 官方矩阵精确为：

- runner: `windows-latest`
- Rust target: `x86_64-pc-windows-msvc`
- bundle: `msi`
- updater platform: `windows-x86_64`
- release label: `win64`
- canonical assets: `aio-coding-hub-win64.msi` 与
  `aio-coding-hub-win64.msi.sig`

来源是 `scripts/support-matrix.mjs:32-58`。release build 使用
`tauri-apps/tauri-action`，参数为
`--target x86_64-pc-windows-msvc --bundles msi`
(`.github/workflows/release.yml:533-541`)。原始 MSI/.sig 从 action 的
`artifactPaths` 中选取，再重命名为 canonical release asset
(`scripts/support-matrix.mjs:724-770`)。Windows portable zip 由同一 build
目录中的 exe 单独组装为 `aio-coding-hub-win64-portable.zip`
(`.github/workflows/release.yml:587-627`)。

发布前本地 MSI 验收命令是：

```powershell
pnpm install --frozen-lockfile
pnpm tauri:build:win:x64

$Msi = Get-ChildItem `
  "$PWD/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi" |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1

if (-not $Msi) { throw "Windows x64 MSI was not produced" }
$Msi | Select-Object FullName, Length, LastWriteTime
Get-FileHash -Algorithm SHA256 -LiteralPath $Msi.FullName
```

`package.json:52-57` 把该命令路由到 `scripts/tauri-build.mjs`；脚本对 Windows
x64 默认补 `--bundles msi` (`scripts/tauri-build.mjs:26-32,95-110`)。本地、
非 CI 且未提供 updater private key 时，脚本只关闭 updater artifacts，仍构建
MSI (`scripts/tauri-build.mjs:126-138`)。因此：

- 本地 MSI 是功能/安装验收门槛，并记录绝对路径、字节数和 SHA-256。
- 正式 Release 不上传这份本地 MSI；GitHub runner 会从合入 SHA 重新构建并生成
  signed updater artifact。
- 当前 worktree 的上述 MSI 目录为空/不存在，本次研究未执行构建。

### 4. 签名、secrets 和权限

GitHub 仓库当前配置了三个相关 secret name（只能确认存在及更新时间，不能只读
证明值仍有效）：

- `RELEASE_PLEASE_TOKEN` (updated `2026-06-28T07:13:03Z`)
- `TAURI_SIGNING_PRIVATE_KEY` (updated `2026-06-28T07:21:08Z`)
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (updated `2026-06-28T07:21:09Z`)

workflow 每个平台 build 都会去除 key whitespace、写入权限 0600 的 runner temp
文件，用 `tauri signer sign` 对 probe 做一次真实 key/password 校验，之后才构建，
并在紧邻下一 step 删除 key (`.github/workflows/release.yml:492-546`)。签名 secret
被限制在 validation/build step；contract 见
`scripts/check-release-signing-secret-scope.mjs:80-188`。

这是 Tauri updater/minisign 签名，证明 `.msi.sig` 和 `latest.json` signature；
仓库没有 `signtool`、Windows code-signing certificate、thumbprint 或 Azure signing
配置，所以不能把它表述成 MSI Authenticode/Windows Publisher 签名。

仓库 Actions 默认 token 权限是 `read`；workflow 使用 `permissions: {}` 后按 job
显式授予最小权限。`RELEASE_PLEASE_TOKEN` 负责 tag、draft Release、asset upload、
publication 和 channel pointer API。仓库没有 `HOMEBREW_TAP_TOKEN`，但 Beta 的
Homebrew job 被 channel 条件明确跳过
(`.github/workflows/release.yml:1374-1379`)，因此不构成 Beta blocker。

### 5. CI/merge 门槛

GitHub API 当前返回 `main` “Branch not protected”，branch rules 为空，ruleset
列表也为空。因此 GitHub 没有真正配置 server-enforced required checks。这是发布
操作风险，不是可以跳过检查的授权。

项目 contract 要求人工把最终 PR head 当作硬门槛：

- `ci-gate` 必须成功；它 always-run 并汇总 change-scope、PR title、docs、support、
  desktop matrix、frontend、rust (`.github/workflows/ci.yml:311-347`)。
- 对本功能改动，change scope 应选择 full CI；support contract 会运行所有 release
  pipeline self-tests (`.github/workflows/ci.yml:98-138`)。
- frontend job 包含 lint、generated bindings、SDK/tests、E2E、coverage 和 build
  (`.github/workflows/ci.yml:162-233`)。
- rust job包含 fmt、Cargo.lock、Clippy、tests 和 cargo audit
  (`.github/workflows/ci.yml:235-309`)。
- `dev-build.yml` 在 feature branch push 上只构建 Windows portable `--no-bundle`，
  不是 MSI 验收的替代品 (`.github/workflows/dev-build.yml:3-6,44-79`)。

推荐在合并前验证 PR 最终 head：

```powershell
$Repo = "FingerCaster/aio-coding-hub"
$Pr = <PR_NUMBER>
$PrHead = gh pr view $Pr --repo $Repo --json headRefOid --jq '.headRefOid'
gh pr checks $Pr --repo $Repo --watch --fail-fast
gh pr checks $Pr --repo $Repo
```

合并后记录 immutable merge SHA，并额外等待该 SHA 的 `main` push CI 成功：

```powershell
$Sha = gh pr view $Pr --repo $Repo --json mergeCommit --jq '.mergeCommit.oid'
if ($Sha -notmatch '^[0-9a-f]{40}$') { throw "Invalid merge SHA: $Sha" }

$Main = gh api "repos/$Repo/git/ref/heads/main" --jq '.object.sha'
if ($Main -ne $Sha) { throw "origin/main moved or merge SHA is not current: $Main" }

$CiRun = gh run list --repo $Repo --workflow ci.yml --commit $Sha --limit 1 `
  --json databaseId --jq '.[0].databaseId'
gh run watch $CiRun --repo $Repo --exit-status
```

### 6. 发布前最后 preflight

在 dispatch 前重新查询，不复用本文件的 `.8` 快照：

```powershell
$Repo = "FingerCaster/aio-coding-hub"
$Tag = "aio-coding-hub-v0.60.41-beta.9"
$Sha = "<verified merge SHA>"

$State = gh api -H "Accept: application/vnd.github.raw+json" `
  "repos/$Repo/contents/beta-channel-state.json?ref=release-channels" |
  ConvertFrom-Json
$State | Select-Object selected_tag, promotion_high_water_version, source_sha

node scripts/release-contract.mjs describe `
  --channel beta --tag $Tag --source-sha $Sha

gh api "repos/$Repo/compare/$Sha...main" `
  --jq '{status,ahead_by,behind_by}'

# 两条命令都必须得到 HTTP 404；任何成功响应都表示候选已被占用。
gh api "repos/$Repo/git/ref/tags/$Tag"
gh api "repos/$Repo/releases/tags/$Tag"
```

要求：

1. 新版本严格高于 `promotion_high_water_version`。
2. tag 和 Release 都不存在。
3. SHA 是 40 位小写并仍可从 `origin/main` 到达；最佳状态是当前 `main` 与 SHA
   完全相等。
4. 先记录 `gh api repos/$Repo/releases/latest --jq .tag_name`，用于证明 Beta 后
   stable latest 未变化。

`release.yml` 的 concurrency key 按 release tag 分组，而 channel high-water 检查
发生在 publication 后的 pointer job。不同 tag 的两个 release 可能并行。因此
“紧邻 dispatch 重读 high-water + 选择未占用且严格更高的 tag”是必要操作门槛，
不能只依赖最终 CAS。

### 7. 14 项官方 Release assets

`scripts/release-promotion.mjs:27-43` 固定以下精确集合，大小写和额外文件都不允许：

1. `aio-coding-hub-linux-amd64.AppImage`
2. `aio-coding-hub-linux-amd64.AppImage.sig`
3. `aio-coding-hub-linux-amd64-wayland.AppImage`
4. `aio-coding-hub-linux-amd64.deb`
5. `aio-coding-hub-macos-arm.tar.gz`
6. `aio-coding-hub-macos-arm.tar.gz.sig`
7. `aio-coding-hub-macos-arm.zip`
8. `aio-coding-hub-macos-intel.tar.gz`
9. `aio-coding-hub-macos-intel.tar.gz.sig`
10. `aio-coding-hub-macos-intel.zip`
11. `aio-coding-hub-win64-portable.zip`
12. `aio-coding-hub-win64.msi`
13. `aio-coding-hub-win64.msi.sig`
14. `latest.json`

Beta 8 远端 Release 现场确有且仅有这 14 项，每项都有 GitHub `sha256:` digest。
其四个平台 build、candidate assembly、promotion、publication、channel pointer 均
成功，Homebrew job skipped。这是下一次 Beta 的有效基线，但不替代对新 run 的
独立复核。

### 8. workflow 内部 promotion/publication 顺序

1. 四个平台分别构建并上传短期 candidate + version attestation
   (`.github/workflows/release.yml:424-670`)。
2. assembly 验证四个平台 overlay attestation 完全一致，生成 strict four-platform
   `latest.json`，创建带 source/tag/version/overlay/run identity 和每项 size/SHA-256
   的 immutable candidate manifest (`.github/workflows/release.yml:672-800`)。
3. promotion 再次验证 candidate，重新验证 tag SHA，只接受 empty draft；一次性
   上传全部 14 项，`overwrite_files=false`，随后按 size/digest 复核
   (`.github/workflows/release.yml:802-993`)。
4. publication 再次验证 tag/main reachability、candidate identity 和 Release
   assets，然后设置 `draft=false`, `prerelease=true`, `make_latest=false`
   (`.github/workflows/release.yml:995-1124`)。
5. channel job 再次验证 candidate 和 public Release，通过 Git Data API CAS 写
   `release-channels`，`force=false`，并等待 ref confirmation
   (`.github/workflows/release.yml:1223-1372`；
   `scripts/release-channel.mjs:576-625,690-710`)。

正常 run 命令完成后只读监控：

```powershell
$RunId = gh run list --repo $Repo --workflow release.yml `
  --event workflow_dispatch --commit $Sha --limit 1 `
  --json databaseId --jq '.[0].databaseId'

gh run watch $RunId --repo $Repo --exit-status
gh run view $RunId --repo $Repo --json status,conclusion,headSha,url,jobs `
  --jq '{status,conclusion,headSha,url,jobs:[.jobs[]|{name,conclusion}]}'
```

所有 build、assemble、promote、publish、publish-release-channel 必须 success；
`repair-beta-release-channel` 和 `publish-homebrew-cask` 必须 skipped。

### 9. 最终独立验证命令

以下命令在 run terminal success 后执行；它不依赖 workflow 日志里的自述结论：

```powershell
$Repo = "FingerCaster/aio-coding-hub"
$Tag = "aio-coding-hub-v0.60.41-beta.9"
$Version = "0.60.41-beta.9"
$Sha = "<verified merge SHA>"
$RunId = <RELEASE_RUN_ID>
$StableLatestBefore = "aio-coding-hub-v0.60.40" # 用 dispatch 前实测值替换

$Expected = @(
  "aio-coding-hub-linux-amd64.AppImage",
  "aio-coding-hub-linux-amd64.AppImage.sig",
  "aio-coding-hub-linux-amd64-wayland.AppImage",
  "aio-coding-hub-linux-amd64.deb",
  "aio-coding-hub-macos-arm.tar.gz",
  "aio-coding-hub-macos-arm.tar.gz.sig",
  "aio-coding-hub-macos-arm.zip",
  "aio-coding-hub-macos-intel.tar.gz",
  "aio-coding-hub-macos-intel.tar.gz.sig",
  "aio-coding-hub-macos-intel.zip",
  "aio-coding-hub-win64-portable.zip",
  "aio-coding-hub-win64.msi",
  "aio-coding-hub-win64.msi.sig",
  "latest.json"
) | Sort-Object

$TagSha = gh api "repos/$Repo/git/ref/tags/$Tag" --jq '.object.sha'
if ($TagSha -ne $Sha) { throw "tag/source mismatch: $TagSha != $Sha" }

$Release = gh api "repos/$Repo/releases/tags/$Tag" | ConvertFrom-Json
if ($Release.tag_name -ne $Tag -or $Release.target_commitish -ne $Sha) {
  throw "Release identity mismatch"
}
if ($Release.draft -ne $false -or $Release.prerelease -ne $true) {
  throw "Release flags mismatch"
}
$ActualNames = @($Release.assets.name | Sort-Object)
if (Compare-Object $Expected $ActualNames) { throw "14-asset matrix mismatch" }
foreach ($Asset in $Release.assets) {
  if ($Asset.digest -notmatch '^sha256:[0-9a-f]{64}$') {
    throw "Missing digest: $($Asset.name)"
  }
}

$Temp = Join-Path ([IO.Path]::GetTempPath()) ("aio-beta-verify-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Temp | Out-Null
gh release download $Tag --repo $Repo --dir $Temp

foreach ($Asset in $Release.assets) {
  $Path = Join-Path $Temp $Asset.name
  $Hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
  if ($Hash -ne $Asset.digest.Substring(7)) {
    throw "Downloaded digest mismatch: $($Asset.name)"
  }
}

$Content = gh api "repos/$Repo/contents/latest-beta.json?ref=release-channels" `
  --jq '.content'
$ChannelBytes = [Convert]::FromBase64String(($Content -replace '\s', ''))
$ChannelPath = Join-Path $Temp 'latest-beta.json'
[IO.File]::WriteAllBytes($ChannelPath, $ChannelBytes)

$ReleaseLatestHash = (Get-FileHash -Algorithm SHA256 `
  -LiteralPath (Join-Path $Temp 'latest.json')).Hash.ToLowerInvariant()
$ChannelHash = (Get-FileHash -Algorithm SHA256 `
  -LiteralPath $ChannelPath).Hash.ToLowerInvariant()
if ($ReleaseLatestHash -ne $ChannelHash) { throw "channel/latest.json bytes differ" }

$Manifest = [Text.Encoding]::UTF8.GetString($ChannelBytes) | ConvertFrom-Json
if ($Manifest.version -ne $Version) { throw "manifest version mismatch" }
$PlatformNames = @($Manifest.platforms.PSObject.Properties.Name | Sort-Object)
$ExpectedPlatforms = @(
  'darwin-aarch64', 'darwin-x86_64', 'linux-x86_64', 'windows-x86_64'
) | Sort-Object
if (Compare-Object $ExpectedPlatforms $PlatformNames) {
  throw "manifest platform matrix mismatch"
}
foreach ($Property in $Manifest.platforms.PSObject.Properties) {
  if ([string]::IsNullOrWhiteSpace($Property.Value.signature) -or
      [string]::IsNullOrWhiteSpace($Property.Value.url)) {
    throw "empty updater entry: $($Property.Name)"
  }
}

$State = gh api -H "Accept: application/vnd.github.raw+json" `
  "repos/$Repo/contents/beta-channel-state.json?ref=release-channels" |
  ConvertFrom-Json
if ($State.selected_tag -ne $Tag -or
    $State.selected_version -ne $Version -or
    $State.source_sha -ne $Sha -or
    $State.manifest_sha256 -ne $ChannelHash -or
    $State.promotion_high_water_version -ne $Version -or
    $State.workflow_run_id -ne $RunId) {
  throw "release channel state mismatch"
}

$ChannelFiles = @(gh api "repos/$Repo/contents?ref=release-channels" --jq '.[].name' |
  Sort-Object)
if (Compare-Object @('beta-channel-state.json','latest-beta.json') $ChannelFiles) {
  throw "release-channels contains unexpected files"
}

$StableLatestAfter = gh api "repos/$Repo/releases/latest" --jq '.tag_name'
if ($StableLatestAfter -ne $StableLatestBefore) {
  throw "Beta changed GitHub latest: $StableLatestAfter"
}

gh run view $RunId --repo $Repo --json status,conclusion,headSha,url,jobs `
  --jq '{status,conclusion,headSha,url,jobs:[.jobs[]|{name,conclusion}]}'

Remove-Item -LiteralPath $Temp -Recurse -Force
```

还应把四个 `.sig` asset 的 UTF-8 文本（移除 CR/LF）与 manifest 对应
`signature` 精确比较；`scripts/release-channel.mjs` 的 normal pointer job 已执行该
检查，但独立复核应再次做。Beta 8 的当前 manifest 四个平台均有非空签名和
canonical Release URL。

### 10. 失败后的唯一安全动作

- 正常 release 失败后不要盲目重跑。先确定 tag、draft/public Release、asset
  是否已产生；workflow 拒绝覆盖已有 assets。
- 若 Release 仍是 empty draft 且 identity 未漂移，正常路径可以复用；其他中间
  状态需要逐项调查，不移动 tag、不覆盖 assets。
- 若 14 项已验证且 Release 已公开成功，只有 `release-channels` pointer job 失败，
  才使用相同 tag/source 的显式恢复：

```powershell
gh workflow run release.yml --repo $Repo --ref main `
  -f release_channel=beta `
  -f release_tag=$Tag `
  -f target_commitish=$Sha `
  -f repair_beta_pointer=true
```

该路径只接受 exact public prerelease，跳过 build/promotion/publication/Homebrew，
重新验证 public asset/manifest/signature 后做同一个 idempotent CAS pointer 操作
(`.github/workflows/release.yml:206-240,1126-1222`)。

`.github/workflows/beta-channel.yml` 是选择已验证 Release 或 pause 的通用指针工具，
输入为 `action`, `release_tag`, `expected_ref_sha`, `withdrawn_tag`
(`.github/workflows/beta-channel.yml:3-24`)；它不构建或发布 Release，不应被当作
首次 Beta 发布入口。

## Files Found

- `.github/workflows/release.yml` - Beta tag/draft 创建、immutable source build、
  candidate promotion、publication、pointer 和 Homebrew 条件。
- `.github/workflows/beta-channel.yml` - 已验证 Release 的显式 promote/pause CAS
  指针工作流。
- `.github/workflows/ci.yml` - full CI 与 always-run `ci-gate` 汇总逻辑。
- `.github/workflows/dev-build.yml` - feature branch Windows portable build；不是 MSI。
- `scripts/release-contract.mjs` - canonical tag/version/SHA/Release state parser。
- `scripts/release-version-overlay.mjs` - Beta 四文件 overlay、WiX numeric version、
  cross-platform attestation。
- `scripts/release-promotion.mjs` - 精确 14 项资产和 immutable candidate digest
  contract。
- `scripts/release-channel.mjs` - public Release/manifest/signature 验证、high-water、
  idempotency 和 `force=false` CAS。
- `scripts/support-matrix.mjs` - 官方四平台 build matrix、Windows MSI canonical name、
  `latest.json` 生成。
- `scripts/check-release-signing-secret-scope.mjs` - updater signing secret 的 step scope
  和 cleanup contract。
- `scripts/tauri-build.mjs` - 本地 target/bundle 默认和无 key 时 updater artifact
  overlay。
- `src-tauri/tauri.conf.json` - updater public key、stable endpoint、WiX 配置和
  `createUpdaterArtifacts=true`。
- `package.json` / `pnpm-lock.yaml` - `pnpm tauri:build:win:x64`，pnpm `10.34.3`，
  locked Tauri CLI `2.9.6`。
- `.trellis/spec/aio-coding-hub/cross-layer/release-operations-contract.md` - origin-only、
  immutable SHA、promotion/publication 验收。
- `.trellis/spec/aio-coding-hub/cross-layer/beta-release-update-channel-contract.md` -
  manual Beta、14 assets、four-platform manifest 和 CAS pointer 契约。

## Code Patterns

- Manual-only release: `.github/workflows/release.yml:3-26`；support contract 还禁止
  `push:` trigger (`scripts/support-matrix.mjs:589-594`)。
- Immutable source: `.github/workflows/release.yml:148-204,392-415,437-440`。
- Beta overlay: `scripts/release-version-overlay.mjs:354-414`。
- Windows MSI matrix: `scripts/support-matrix.mjs:32-58`。
- Step-scoped signing probe: `.github/workflows/release.yml:492-546`。
- Exact assets/no overwrite: `scripts/release-promotion.mjs:27-43` 和
  `.github/workflows/release.yml:923-936`。
- Public Beta flags: `.github/workflows/release.yml:1098-1123`。
- Strict promotion high-water: `scripts/release-channel.mjs:496-507`。
- Exact-promotion idempotency: `scripts/release-channel.mjs:523-535,609-621`。
- Non-force CAS: `scripts/release-channel.mjs:690-710`。

## External References

- Current public Beta:
  `https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v0.60.41-beta.8`
- Current successful release run:
  `https://github.com/FingerCaster/aio-coding-hub/actions/runs/31869855209`
- Current Beta manifest:
  `https://raw.githubusercontent.com/FingerCaster/aio-coding-hub/release-channels/latest-beta.json`
- Current Beta state:
  `https://raw.githubusercontent.com/FingerCaster/aio-coding-hub/release-channels/beta-channel-state.json`
- Remote facts were read through explicit `gh ... --repo FingerCaster/aio-coding-hub`
  or `repos/FingerCaster/aio-coding-hub/...` endpoints. No implicit repo resolution and
  no upstream endpoint was used.

## Related Specs

- `.trellis/spec/aio-coding-hub/cross-layer/release-operations-contract.md:60-91`
- `.trellis/spec/aio-coding-hub/cross-layer/release-operations-contract.md:122-136`
- `.trellis/spec/aio-coding-hub/cross-layer/beta-release-update-channel-contract.md:52-66`
- `.trellis/spec/aio-coding-hub/cross-layer/beta-release-update-channel-contract.md:128-170`
- `.trellis/spec/aio-coding-hub/cross-layer/beta-release-update-channel-contract.md:226-240`
- `.trellis/tasks/08-16-codex-gpt56-372k-context/prd.md` R14-R16
- `.trellis/tasks/08-16-codex-gpt56-372k-context/design.md` “Beta Release”
- `.trellis/tasks/08-16-codex-gpt56-372k-context/implement.md` sections 5-6

## Caveats / Not Found

- 没有 GitHub branch protection/ruleset，因此“required checks”目前是项目 contract，
  不是 GitHub server enforcement。操作者必须自己核对 final PR head 和 merge SHA。
- 只能确认三个 release/signing secret name 已配置；只有实际 workflow signer probe
  能确认 key/password 可用。
- 未找到 Windows Authenticode signing 配置；`.msi.sig` 是 Tauri updater signature。
- 未找到 feature branch 或 PR；当前不能发布。
- 当前 worktree 未产生 MSI，本研究没有执行 build、test、commit、push、PR、tag、
  Release、workflow dispatch 或 channel pointer 操作。
- `.9` 空闲和 `.8` high-water 是 2026-08-17 最后核验时的瞬时事实；dispatch 前
  必须再次读取。
- 本研究没有查看、fetch 或操作 `upstream`。
