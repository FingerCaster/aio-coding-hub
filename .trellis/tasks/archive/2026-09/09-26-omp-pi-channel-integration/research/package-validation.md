# Windows 本地测试包验收

生成时间：2026-09-26 18:45:58（Asia/Shanghai）。用户要求“打包，我测试一下”。

- 版本：0.60.43，包含当前未提交的 Pi / OMP 接入改动。
- 平台：Windows x64；Release 优化构建；本地未签名测试包。
- 分支：FingerCaster/omp-pi-channel-integration。
- 基线提交：3372d11f5a20a212bd551e0b0b7067ab2b639c79。
- 198 个工作区变更文件的指纹：f04f2e2460b343ae24ef1e71dac05251a452a42f0f3528c08e5dff4f18d91b1f。打包前后核对一致；本记录在打包后补充。
- 构建命令：pnpm tauri:build -- --bundles msi。使用现有本地 overlay 关闭 updater artifacts，未改变正式发布配置。
- pnpm build 的 TypeScript/Vite 构建成功；Rust release 编译 11 分 04 秒，WiX candle/light 成功，构建进程 exit 0。

## 产物

目录：.local/test-builds/pi-omp-20260926-184558/

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| AIO-Coding-Hub-0.60.43-pi-omp-win64.msi | 18243584 | ac30830d4c1cd01dd43868a2aa69f501d41f051bb932a6282429190745bccc89 |
| AIO-Coding-Hub-0.60.43-pi-omp-win64-portable.zip | 18495162 | f3751cc0d59454d3c5274dfb61839a73d508e6cc4aeb5e86da9493aa59e6706e |

## 校验

- EXE 为 AMD64、PE32+，产品版本与当前源代码一致；SHA-256 为 934dbd2f9a02cbaacce0381c2c4c161e19694b26677b1fcc4b281a5a15ccc7e0。
- 只读检查 MSI Property/File 表：产品 AIO Coding Hub、版本 0.60.43、升级标识与项目一致；包含主程序及官方隐私插件的 manifest、extension.js 和 gitleaks.toml。
- ZIP 中逐文件检查长度与 SHA-256，并核对三个插件资源与源文件一致。包含完整 resources、Pi/OMP 使用说明、README 和构建来源信息。
- 再次读取最终 MSI/ZIP，哈希与生成记录一致。MSI Authenticode 状态为 NotSigned。
- 本地产物目录含 SHA256SUMS.txt、package-verification.json；构建日志在 .local/test-builds/pi-omp-release-build.log。

没有自动安装或启动应用，等待用户交互测试。先退出现有 AIO 后再运行测试版。免安装包仍使用同一用户数据目录，且包含数据库 v47 迁移；若需要回退旧版，应先备份用户数据。没有提交、推送或远程发布。
