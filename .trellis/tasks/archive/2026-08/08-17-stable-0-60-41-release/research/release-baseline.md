# `0.60.41` 稳定发布基线

记录时间：2026-08-17（Asia/Shanghai）

仓库：`FingerCaster/aio-coding-hub`（仅 `origin`）

## 用户与历史证据

- 用户明确确认 Beta 已全部测试且无问题，要求转为新的正式版。
- `trellis mem search "aio-coding-hub-v0.60.41-beta.10" --global` 命中 Codex 会话
  `01a00e92-b257-7771-9839-32c841758830` 与
  `01a00b21-c6d0-7dd0-b14c-e33771103aa7`；历史记录确认 Beta 10 workflow
  `32024411558` 成功，14 项资产、四平台 manifest、签名、tag/source 和渠道 CAS 均已核验。
- Beta 10 source 为 `6718b174b0dcecd5fabdb5e968b7c2aa8af5a616`；其后仅有 Trellis 归档
  merge `fb1a21c7c281b6fed2927cb55e529787eb3b8727` 进入 `origin/main`。

## 版本与远端状态

- `.release-please-manifest.json` 为 `0.60.40`。
- release-please config 设置 `bump-patch-for-minor-pre-major: true`；`0.x` feature 的自然下一
  版本应为 `0.60.41`。
- GitHub latest 稳定版为 `aio-coding-hub-v0.60.40`。
- 最新 prerelease 为 `aio-coding-hub-v0.60.41-beta.10`，公开时间
  `2026-08-17T11:42:39Z`。
- `git ls-remote origin refs/tags/aio-coding-hub-v0.60.41` 无输出；GitHub Release 列表也没有
  该稳定 tag，标题搜索没有 `0.60.41` release PR。
- 标准生成分支
  `release-please--branches--main--components--aio-coding-hub` 为
  `4d84dbe999ad63327fe6211f4b3b4e6f54153052`，相对 `origin/main` 落后 54、领先 1；
  其版本仍为 `0.60.40`，不可直接合并。
- 历史 `Release-As` 只有 `0.60.40`、`0.60.6` 和 `0.60.0`。本次先走自然 bump；首轮 PR
  若不是 `0.60.41`，再停止评估新的空 override，不预先增加提交。

## 发布合同证据

- `.trellis/spec/aio-coding-hub/cross-layer/release-operations-contract.md` 要求稳定版两次无输入
  dispatch、release PR 最终 head 六文件一致、tag 解析为不可变 SHA、精确候选 promotion 和
  发布后双重验证。
- `.github/workflows/release.yml` 的无 tag stable 路径调用 release-please；只有
  `release_created=true` 才进入 build。第二轮会执行四平台 build、assemble、promotion、
  publish、release-channel 和稳定版 Homebrew job。
- `publish-release-channel` 对稳定版本比较 promotion high-water；`0.60.41` 高于
  `0.60.41-beta.10`，应以 CAS 发布稳定 `latest.json` 到 `release-channels`。
- 稳定 Release 发布时设置 `prerelease=false` 并 `make_latest=true`；Homebrew 始终生成 Cask，
  token 缺失时只能走显式 skip。
- 先前 `0.60.40` 的归档证据表明同一两阶段流程以 release PR merge SHA 构建，成功发布
  精确 14 项资产并验证 `latest.json` 四平台签名，可作为本次执行模板。

## 结论

目标版本与执行路径无未决产品问题。推荐从最新 `origin/main` 首先无输入 dispatch，严格审计
生成的 `0.60.41` release PR；不复用 Beta 资产，不预先创建 `Release-As` override。
