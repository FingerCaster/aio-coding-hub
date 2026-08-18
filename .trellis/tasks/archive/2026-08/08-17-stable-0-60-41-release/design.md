# 技术设计：`0.60.41` 稳定发布

## 边界与所有权

本任务是发布协调任务，不修改业务源码。权威状态分布在四处：

1. `origin/main` 与 release-please 生成分支；
2. release PR 的最终 head、六个版本文件和 GitHub checks；
3. `release.yml` 生成的 tag、草稿/公开 Release、候选 manifest 与正式资产；
4. GitHub latest、`release-channels` 指针和 Homebrew tap。

本地独立 worktree 只保存 Trellis 计划和审计证据。所有 GitHub CLI 操作显式指定
`-R FingerCaster/aio-coding-hub`，Git 只读写 `origin`；不访问 `upstream`。

## 已确认状态

| 对象 | 规划时状态 | 目标状态 |
| --- | --- | --- |
| `origin/main` | `fb1a21c7c281b6fed2927cb55e529787eb3b8727` | release PR merge SHA |
| stable manifest | `0.60.40` | `0.60.41` |
| GitHub latest | `aio-coding-hub-v0.60.40` | `aio-coding-hub-v0.60.41` |
| Beta high-water | `aio-coding-hub-v0.60.41-beta.10` | 保留，并由稳定 `0.60.41` 推进指针 |
| stable tag/Release | 不存在 | 同一不可变 merge SHA 上的公开正式 Release |
| release-please 分支 | `4d84dbe9...`，落后 `main` 54 个提交 | 首轮 dispatch 生成的新 final head |

## 发布状态机

### 1. 基线与合同闸门

重新 fetch `origin/main` 和 tags，证明 `0.60.41` tag/Release 仍不存在、stable latest 仍为
`0.60.40`、工作树除本任务记录外无其他变化。安装锁定依赖并运行发布合同自测。任一检查失败
则保持远端不变并停止。

### 2. 首轮无输入 dispatch

执行：

```powershell
gh workflow run release.yml -R FingerCaster/aio-coding-hub --ref main
```

按 dispatch 时间、event、branch 和 head SHA 唯一定位 run。该 run 应由 release-please 刷新
生成分支并创建/更新 release PR，`release_created=false`，所有 build/publish job 因条件跳过。

不预先创建 `Release-As` 提交。若生成版本不是 `0.60.41`，不合并、不手工改生成分支；保存
PR/run/历史 trailer 证据后停止，另行评估空的 `Release-As: 0.60.41` 提交。

### 3. release PR 最终 head 闸门

release-please 初始提交后，`release-pr-sync-cargo-lock` 可能追加 Cargo.lock 同步提交。等待 head
稳定，再读取最终 `headRefOid`，验证：

- base 是 `main`，head 是标准 release-please 分支；
- diff 集合精确等于六个标准文件；
- manifest、package、Cargo.toml、Cargo.lock、tauri.conf 全部为 `0.60.41`；
- changelog 首段为 `0.60.41`，compare range 为 `v0.60.40...v0.60.41`；
- 所有 required checks 都属于最终 head 并成功。

满足后使用标准 merge commit 合并，并 fetch/复核新的 `origin/main`。不 squash 生成分支，保持
与现有 release-please 历史一致。

### 4. 第二轮无输入 dispatch

从 release PR merge SHA 再次无输入 dispatch。工作流必须先解析/创建稳定 tag 并输出 40 位
`checkout_ref`，所有平台构建都 checkout 该 SHA。按 job DAG 等待：

```text
release-please
  -> build (Windows, macOS Intel/ARM, Linux)
  -> assemble-release-candidate
  -> promote-release
  -> publish
  -> publish-release-channel
  -> publish-homebrew-cask
```

`repair-beta-release-channel` 仅在工作流合同需要恢复时出现；不得用它掩盖普通发布失败。

### 5. 发布后双重核验

工作流成功后独立读取 Git refs、Release API、asset API、`latest.json`、GitHub latest endpoint、
`release-channels` 分支和 Homebrew job。验证 tag/source/target/run head 完全一致，14 项资产名称
集合严格相等、大小非零、digest 存在，四平台 URL/签名有效。稳定版本 `0.60.41` 高于
`0.60.41-beta.10`，因此 release-channel 高水位应 CAS 推进到稳定 tag。

## Beta 转正式的语义

“Beta 转正式”表示相同功能线进入稳定版本号，不表示复用 Beta 二进制。正式构建必须使用
release PR merge 后的源码版本 `0.60.41`，重新生成二进制版本元数据、签名、候选 attestations
和 `latest.json`。Beta 10 的 tag、Release、资产与 digest 保持不可变，作为可追溯候选记录。

## 失败与恢复

| 阶段 | 失败条件 | 处理 |
| --- | --- | --- |
| 首轮 dispatch 前 | 合同测试、tag/Release 空闲或远端基线失败 | 不 dispatch，保留本地任务证据 |
| release PR | 版本错误、非六文件、changelog 错误、旧 head/pending/failed check | 不合并；记录证据并停止 |
| 合并前 | `origin/main` 漂移 | fetch 后从新基线重新审计，不 force-push |
| 第二轮 dispatch | tag/source/Release draft 身份不一致 | 阻止 build/promotion，不手工修补 |
| promotion/publish | 资产缺失、额外、digest 或 run identity 不符 | 保留 draft，禁止覆盖或部分发布 |
| 发布后 | latest/channel/Homebrew 不一致 | 不改 tag/资产；保留审计数据并单独修复渠道 |

## Trellis 收尾

正式发布成功后，发布源 SHA 已固定。任务归档和 journal 通过独立归档 PR 合入，不改变已经
发布的 tag/source。Orca comment 记录版本、Release SHA、run ID 和验证结论后标记 completed。
