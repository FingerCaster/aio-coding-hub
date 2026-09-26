# OMP 原生设置 MSI 交付

- 构建时间：2026-09-27T02:12:56.1868726+08:00（本机时区 Asia/Shanghai）。
- 版本/架构：0.60.43 / Windows x64，本地未签名测试包。
- MSI：D:\OrcaProjects\aio-coding-hub-fork\omp-pi-channel-integration\.local\test-builds\omp-cli-settings-20260927-021255\AIO-Coding-Hub-0.60.43-omp-settings-win64.msi
- 大小：18624512 字节。
- MSI SHA256：73b351875e460a0f01f03ecfc3e29ab8ccb6c7ba554079dc3a00f52628dfab48
- EXE SHA256：faac371577b6fc64a35f4603c4f7fcfaa8e794d7b135c6dba27b50cef0b1084f
- 源码分支：FingerCaster/omp-pi-channel-integration。
- 基础提交：3372d11f5a20a212bd551e0b0b7067ab2b639c79；包含当前未提交集成改动。
- 构建前源码指纹：6fe67ea61f5a81164f4322503b320102a3584e640b3d4713eacb017fe1ea3dce。

## 验证

- pnpm tauri:build -- --bundles msi 成功；Rust release 优化编译用时 10m09s。
- 构建后、封装前均验证源码快照一致。仅在产物完成后更新本交付记录及任务状态；未再改实现源码。
- 生产前端包含 omp_settings_read/save、omp_agent_read/save；EXE 同时包含这些设置命令和既有 CLI 版本检查/更新命令。
- MSI 的 AMD64/PE32+ 主程序与本次 release 产物哈希一致；模型目录许可证、插件清单、插件脚本、gitleaks 规则四项资源均逐一核对哈希。
- MSI 产品版本与名称正确，UpgradeCode 与前包一致，ProductCode 已更新；升级表可检测同版本安装。
- WiX dark 的既有反编译提示已用真实 MSI 表核对：215 个 Control、131 个 ControlEvent，缺失关联 0。
- 未执行 MSI 安装，安装后的 WebView 和用户实际配置仍待用户验收。
- 详细核验结果：同目录 package-verification.json、installer-verification.json、source-snapshot.json、SHA256SUMS.txt。

## 用户测试

安装前退出 AIO（含托盘实例）。安装后打开 CLI 管理 → OMP → 原生设置，测试默认模型与思考等级、模型角色、会话/子任务参数、Agent 覆盖和自定义 Agent Markdown 编辑。

保存仅修改选中 OMP 目录中的相应文件，包含原文备份和冲突检查。AIO 供应商导入不自动改默认模型；版本检查不自动升级。

旧包保留。本次没有 commit/push/release；任务维持 ready_for_user_acceptance，等待用户反馈。
