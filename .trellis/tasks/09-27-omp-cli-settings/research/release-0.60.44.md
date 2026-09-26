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

- [ ] 本地发布契约检查和完整 pre-push 检查通过。
- [ ] 完整功能 PR 最终 head CI 通过并合并 main。
- [ ] 空 index 创建独立 Release-As override；版本 PR 六文件一致、最终 CI 通过后合并。
- [ ] 二次 release dispatch 完成多平台构建、签名、发布。
- [ ] 独立核验 tag SHA、Release target/state、资产清单/摘要及 latest.json 四平台。

本记录在发布执行过程中继续补充；已有功能验证见同目录 implementation-validation.md 和 model-inheritance-preview.md。
