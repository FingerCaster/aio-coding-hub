# Beta 发布流水线与频道指针设计

## Ownership

只修改 release workflow、release-promotion/support-matrix 相关脚本、版本 overlay、channel-state 脚本和它们的 selftest。不得修改 Rust updater 或 React 组件。后端消费的固定输入是 release-channels/latest-beta.json、beta-channel-state.json、Tauri static manifest 和 release URL。

## Flow

1. workflow_dispatch 接收 release_channel、release_tag、target_commitish；stable 默认路径保持现有输入和行为，Beta 缺少 tag/SHA 立即失败。
2. 把 target 解析为 40 位 SHA并验证 origin/main ancestor；已有 tag 必须 peel 到该 SHA，不存在时先创建 tag ref。重新 fetch 验证 tag 后才创建/读取身份匹配的空 draft Release；下游只 checkout SHA，禁止按 release tag 单独 checkout。
3. 每个平台运行版本 overlay、构建、签名和 attestation；assemble job 要求所有 attestation 相等。
4. 生成标准 latest.json 和 candidate manifest；promotion 校验 channel、prerelease 状态、Release identity、资产列表/大小/digest。
5. public job 只在全部复核成功后设置 Beta prerelease=true、make_latest=false；Homebrew job 条件为 stable。
6. channel job 在 Release 公开后读取 release asset latest.json，使用 Git Data API CAS 更新 pointer 和 state。

## Pointer contract

分支名为 release-channels。单次提交写 latest-beta.json 和 beta-channel-state.json。state 中保存 schema_version、action、selected_tag/version/channel、release_id、source_sha、manifest_sha256、previous_ref_sha、workflow_run_id/attempt、operator 和 updated_at。普通 promote 只允许严格更高 SemVer；pause 允许显式安全目标但必须重新校验公开 Release、完整矩阵和签名。

## Version overlay

Node 脚本用 JSON parser 更新 package/tauri 文件，对 Cargo.toml 只接受唯一的 package version 行并拒绝漂移；随后运行 cargo metadata 更新 lock，并用 cargo metadata --locked 验证。四个版本文件都以确定的 UTF-8、LF 和格式输出。脚本输出四文件 digest 和 source/tag/channel attestation。任何额外工作树 diff、非法 prerelease 或跨平台 digest 漂移都停止候选。

## Failure / recovery

所有 ref 更新 force=false；CAS 失败不自动重试。Release/asset 已公开但 pointer 更新失败时保留 Release，报告可恢复状态，不删除或覆盖资产。pause 只产生新 pointer commit，withdrawn Release 永久保持原身份。
