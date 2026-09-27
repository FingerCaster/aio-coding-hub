# CLI 更新 MSI 测试包

构建时间：2026-09-27T01:10:01.5020727+08:00。按用户要求仅交付 Windows x64 MSI。

- 文件：D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-cli-update-20260927-011000/AIO-Coding-Hub-0.60.43-pi-omp-win64.msi
- 大小：18530304 字节。
- SHA256：55964068c8f299d81b785f7ed2b21818a5c6a4341c82f6cf05545a16a933fead
- 版本：0.60.43（本地未签名测试版，未发布到 GitHub）。
- 来源分支：FingerCaster/omp-pi-channel-integration，基准提交：3372d11f5a20a212bd551e0b0b7067ab2b639c79。
- 构建源码指纹：19dd5ade85d8ed6b68b6e711d30a9e7323b4bae61d8723f52a4450210ec62993。

## 构建与验证

- 冻结工作区的 265 个变更/新增文件，执行 pnpm tauri:build -- --bundles msi，Release 优化编译 9m39s；构建成功后源码指纹验证通过。
- MSI ProductName/ProductVersion、EXE AMD64/PE32+ 和文件版本匹配。
- 程序含 native_cli_check_latest_version / native_cli_update 命令。
- WiX dark 解包后，MSI 内 EXE 与本次 release EXE 的 SHA256 完全相同；4 个资源文件与源资源逐个哈希比对通过。
- dark 报告 UI 反编译引用警告；实际 MSI ControlEvent 到 Control 的引用检查为 0 个缺失，与上一测试包相同，文件解包和哈希验证不受影响。
- 未运行 MSI 安装，不改动用户现有 AIO 安装。由用户安装验收。

## 本包内容

包含 OMP 独立版版本识别修复，以及 Pi/OMP 自动检查、主动确认后下载安装或升级。检查不会自动安装/升级。OMP 的 npm 安装模式尚未接入；保留独立版及已识别 Bun 安装路径。

构建后只更新任务交付记录；安装包对应冻结源码，完整快照、文件哈希和校验结果保存于同目录 source-snapshot.json、SHA256SUMS.txt、package-verification.json。
