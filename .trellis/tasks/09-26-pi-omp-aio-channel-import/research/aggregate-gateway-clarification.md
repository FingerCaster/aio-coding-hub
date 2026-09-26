# 聚合网关入口说明修正

日期：2026-09-26。用户指出导入页容易被理解为接入单个上游。

## 核实结果

- 实际请求始终经 Pi/OMP 的 AIO 本地渠道绑定入口，绑定由目标、来源渠道和协议确定，不包含某个上游的 provider ID。
- catalog.rs 的 channel_plan 生成本地 /_aio/channel/ 地址；请求时 channel_candidate_eligible 重新核对来源当前启用池、认证、协议及发布模型能力。
- 导入选择仅传 sourceChannel、protocol 与 modelIds。模型资料维护关联上游，是 AIO 匹配实际能力的依据，不是原生客户端的独立直连配置。
- 当前渠道候选通过准入不代表可处理所有模型；请求仍受模型、额度、健康、工作模式与选源规则约束。

## 界面调整

- 标题改为“从 AIO 聚合网关接入”，展示“Pi / OMP → AIO 聚合网关 → 渠道可用上游”。
- 上游详情改为“AIO 调度池”；候选状态改为“准入通过”，补充实际请求会继续筛选的解释。
- 操作改为“维护模型资料”；模型选择区明确为通过该渠道使用的模型。
- 发布预览明确标识 AIO 网关地址，保留原有多选、能力编辑和单渠道入口逻辑。后端选源机制没有变动。

## 本轮验证

- NativeChannelFlows：13 passed。导入选择断言使用完整渠道对象，确保没有附加 provider ID 绑定。
- Rust native_channel：19 passed，1 ignored；随后单独运行 ignored 的真实 CLI 矩阵并通过。
- Pi 0.87.1 / OMP 18.3.2 × Claude Messages、Codex Responses、Grok Chat、Grok Responses、Gemini 标准 API：10/10。每条通路均验证第一上游失败、第二上游成功，attempts 保留同一 AIO 渠道绑定下的两个 provider ID。仅使用隔离目录与本地模拟上游，无真实账号或付费请求。
- TypeScript、定向 ESLint、Prettier、spec-links 通过。
- Edge 真实组件 + mock IPC：Pi 浅色 / OMP 深色、宽屏 / 720px 窄屏、两个候选仍只生成一个入口、调度池默认折叠、焦点与横向溢出检查通过。截图已目视核对。
- CLI 证据：.trellis/.runtime/research/omp-pi/channel-test/results.json。
- UI 证据及截图：.trellis/.runtime/research/omp-pi/channel-ui/aggregation/。

## 交付边界

用户要求重新打包后，已于 2026-09-26T23:58:13+08:00 生成 Windows x64 Release MSI/ZIP，版本仍为 0.60.43。新包包含本次聚合入口界面说明以及之前的批量模型和默认参数功能。未提交、推送或发布。

- 目录：D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-235813
- 源码指纹：24fa62261608174890bbecd1db67e6ac8ef7e9091e1fc51f8eb8e056f64f5d0d；构建前后 252 个文件一致。
- 前端生产构建通过，资源包含聚合网关、调度池、模型资料和网关地址文案；Rust Release 编译 6m13s，MSI 生成通过。
- EXE/MSI 更新时间晚于本轮源码快照；AMD64/PE32+、版本、MSI 资源、模型目录许可证、ZIP 全部 11 个文件及 SHA-256 校验通过。
- MSI：AIO-Coding-Hub-0.60.43-pi-omp-win64.msi；18456576 字节；SHA-256 7c3537396f8867ee1de7789f77292017b36309b7a475d14ed7a3618a518886cc。
- ZIP：AIO-Coding-Hub-0.60.43-pi-omp-win64-portable.zip；18719136 字节；SHA-256 905ab7ad1f45d94d1b4777a69dbb2399b28c81d840a6557d304346752bd42f7a。
- 完整清单：包目录下 package-verification.json、source-manifest.json、frontend-verification.json 和 SHA256SUMS.txt。
- 打包后仅更新当前任务的四份交付文档；源码未变证明保存于 .local/test-builds/aggregate-gateway-postbuild-proof.json。
