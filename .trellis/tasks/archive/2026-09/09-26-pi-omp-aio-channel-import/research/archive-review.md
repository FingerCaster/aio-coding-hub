# 归档复核（2026-09-27）

2026-09-27 归档复核：用户已明确授权提交、合并、发布，并要求归档已完成任务。功能已通过 PR #51 合并 origin/main，随稳定版 0.60.44 发布；此前等待验收/不提交发布的阶段性说明已被后续授权与正式发布结果取代。完整完成证据见 research/archive-review.md。

## 完成依据

- 功能 PR：https://github.com/FingerCaster/aio-coding-hub/pull/51；合并提交 3b7ecd4161012ea8e5e05f331a78009b8ef8e5fb。
- 版本 PR：https://github.com/FingerCaster/aio-coding-hub/pull/50。
- 发布工作流：https://github.com/FingerCaster/aio-coding-hub/actions/runs/36268634533。
- 正式发布日期：2026-09-27（北京时间；UTC 2026-09-26T20:37:58Z）。
- 本次仅核对已有完成记录并归档，未重新运行测试或发布；不扩大现有验证记录的平台、真实账号及协议支持声明。
- 归档使用 task.py archive --no-commit；保留所有历史方案、验证记录及阶段性交付信息。

# 0.60.44 稳定版发布完成

- 功能提交：90e330a678eb14a45d958518078d31b631bb09fe。
- 独立版本覆盖提交：f4d7ddf8fe129c8c2e1965b326e4f54ba56e32a7；空 index、空提交。
- 功能 PR #51、版本 PR #50 均通过最终 head 的完整 CI、Windows 构建后合并 main。
- 本地 pre-push 15 项检查全部通过：前端 3084 项，Rust 主测试 3218 项及全部集成测试，Clippy 和生成绑定通过。
- 六个版本文件均为 0.60.44，变更日志范围验证通过。
- 发布工作流 36268634533 全部必需作业成功。
- tag、Release target、origin/main 均指向 96272aff6fc4e4e3caa7c20b9754224ba2f40a26。
- 14 个发布资产与不可变候选包逐一核对名称、大小、SHA-256；已实际下载完整候选包并验证。
- latest.json 四平台 URL、签名及资产摘要一致；GitHub 最新稳定版指向 0.60.44。
- MSI ProductVersion = 0.60.44，UpgradeCode 保持原值，未安装或改动用户配置。
- 自动重复追加历史的 PR #52 已核对并关闭。
- Homebrew Cask 生成成功；未配置 HOMEBREW_TAP_TOKEN，tap 同步依流程显式跳过。
- 本地原有 .trellis/config.yaml 和 test-out.txt 保留，未纳入提交。

Release: https://github.com/FingerCaster/aio-coding-hub/releases/tag/aio-coding-hub-v0.60.44
MSI SHA-256: 8d168d72d8c9984dbdbe9b0a0ea50a1105604c1a96d51b8366db578daacecf22

详细机器校验：published-verification.json、msi-verification.json。
