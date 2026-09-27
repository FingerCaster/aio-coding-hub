# 0.60.44 发布执行记录

用户于 2026-09-27 明确授权完整 Pi/OMP 集成提交、推送、建立 PR、合并 main 和发布新版本。

## 基线与范围

- origin: FingerCaster/aio-coding-hub；不访问 upstream。
- 基线 main/HEAD: 3372d11f5a20a212bd551e0b0b7067ab2b639c79。
- 当前稳定版和 manifest: 0.60.43；拟发布下一 patch 0.60.44。
- 功能范围：Pi/OMP 原生管理、AIO 聚合渠道多模型导入、多协议网关及按模型调度、CLI 版本检测与手动升级、OMP 原生设置与 Agent 管理、继承模型预览。
- 保留本地 Trellis inline 执行配置和临时 test-out.txt，不纳入发布。
- 旧 release-please PR #50 只更改 CHANGELOG，版本仍为已发布的 0.60.43；必须刷新后重新审查，禁止直接合并。

## 执行与验收

- [x] 本地发布契约检查和完整 pre-push 检查通过。
- [x] 完整功能 PR 最终 head CI 通过并合并 main。
- [x] 空 index 创建独立 Release-As override；版本 PR 六文件一致、最终 CI 通过后合并。
- [x] 二次 release dispatch 完成多平台构建、签名、发布。
- [x] 独立核验 tag SHA、Release target/state、资产清单/摘要及 latest.json 四平台。

以上发布步骤已完成；以下为最终发布结果，前述基线与范围保留为发布前记录。功能验证见同目录 implementation-validation.md 和 model-inheritance-preview.md。


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
