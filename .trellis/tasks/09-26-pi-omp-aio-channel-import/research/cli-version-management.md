# CLI 版本识别与更新功能核查

日期：2026-09-27（本机 Asia/Shanghai）。用户反馈 OMP 管理页版本未知，Pi 正常，并询问两者更新功能是否实现。

## 根因与现状

- Pi 使用 npm 全局包，现有已知包元数据路径可读取 0.87.1。
- 本机 OMP 是官方独立 EXE，位于 LOCALAPPDATA/omp/omp.exe，没有相邻 npm package.json；原版本查询只有包元数据分支，因此返回空版本。
- 该 EXE 的 Windows ProductName 为 Bun，ProductVersion 为 1.4.2，不能将其误认为 OMP 版本。
- Pi/OMP 页面均已提供官方安装/更新命令复制、文档与发布页链接；没有查询最新版本、下载升级或执行安装的 AIO 内一键功能。前期 PRD 明确将自动安装更新列为范围外。

## 修复

- 保留已知 npm 包元数据优先级；只在 OMP 包版本不可用且入口是原生二进制时执行 --version 后备查询，不执行文本脚本入口。
- 复用现有受限子进程机制：清空继承环境，只保留 OS loader 必需变量；HOME、USERPROFILE、AppData、XDG、agent、cache、临时目录和 cwd 全部指向自动清理的临时目录，stdin 为空，Windows 不弹控制台。
- 查询最多五秒，stdout/stderr 各限 4 KiB；仅接受 omp/ 开头的单行版本，Bun 输出、截断输出、失败退出和无效版本均不当作 OMP 版本。
- 补充 Windows 官方 LOCALAPPDATA/omp 安装目录扫描，以兼容旧进程尚未继承新 PATH。
- 页面准确说明“未能读取版本”和“手动安装更新指引”；同步使用说明与跨层契约。
- 临时目录库原来仅是 macOS 正式依赖及通用测试依赖，首次 Windows 非测试检查发现缺失后已提升为通用正式依赖，沿用原有锁定版本。

## 验证

- Rust CLI manager：9 passed、1 ignored；随后显式运行真实独立 OMP 查询，1 passed，返回 18.3.2，耗时约 0.24 秒。
- 新覆盖包括官方版本格式、拒绝 Bun/异常输出、不执行文本脚本、隔离配置目录与环境；既有 Pi 已知包身份匹配测试保持通过。
- NativeCliTab：12 passed，含 Pi/OMP 具体版本展示、未知/错误状态、刷新、目录选择和手动更新边界。
- TypeScript、定向 ESLint、Prettier、Rust fmt、Clippy all-targets --locked -D warnings、spec-links 均通过。
- Windows 本机 OMP 验证通过；其他系统的独立二进制查询尚无本轮实机证据。没有升级已安装的 CLI，也没有真实推理请求。

## 交付

版本识别修复已在源码完成，尚未重新打包。最后交付的 pi-omp-20260926-235813 不包含这次修复；自动检查更新和一键升级仍未实现。未提交或推送。

> 后续：用户已批准自动检查和主动确认安装/升级，后续实现见 cli-updates-plan.md；本文件中的“未实现”描述仅记录此前版本识别修复阶段。
