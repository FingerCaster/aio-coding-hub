# Pi / OMP CLI 更新验证

日期：2026-09-27（本机 Asia/Shanghai）。用户批准自动检查、主动确认下载安装/升级，禁止自动升级。

## 已实现

- 新版本卡片复用 CLI 管理风格，进入页面和前台每小时查询；可手动重查。检查命令仅读取版本/校验元数据。
- 点击安装/升级并确认目标版本、目录和安装方式后才执行。取消、页面卸载、检查错误、定时查询不会调用安装命令；安装重试关闭，并发操作在前后端均互斥。
- 后端计划绑定 client、版本、目录和安装方式，30 分钟有效且单次使用；执行前复查，拒绝变化或重放。严格语义版本比较，不降级、不将未知版本当作最新。
- Pi 保持识别出的 npm prefix / Bun root；OMP 支持已识别 Bun 与官方独立程序。Pi 旧包已被官方标记停用，提示手动迁移，比较维护中包的最新版本。
- OMP 下载固定官方 Release 资产，校验 SHA256SUMS 和隔离 --version 后原子激活；失败保留旧程序。包安装使用独立工作目录、固定版本及参数数组，输出/时限受限，超时结束子进程树。
- 原生 CLI 配置、登录凭证和供应商未被更新器改写。Windows 补充用户 PATH；其他平台提示检查 PATH。

## 实际验证结果

- 前端定向测试：5 个文件、36 项通过（版本卡片 10、原生 CLI 页 12、既有 CliVersionBadge 4、既有 cliUpdate service 6、新 nativeCliUpdate service 4）。
- Rust cli_update：21 项通过，2 个联网测试在普通测试中按设计 ignored。
- Rust cli_manager：9 项通过，1 个本机版本查询按设计 ignored（前次修复已显式验证通过）。
- 单独显式运行 2 个联网隔离安装测试，全部通过，共 82.29 秒：OMP 18.3.2 完成官方下载、SHA256、激活和版本确认；Pi 0.87.1 完成隔离 npm prefix/cache 安装与版本确认。没有调用用户 PATH 修改逻辑，没有升级本机现有 CLI。
- 安装后核对本机 Pi 仍为 0.87.1；OMP SHA256 仍为 5d99fe5c11ec3ff1792e427c0e61eeb5a6c84670cadb65d023dd2f5eb8705f40，与测试前一致。
- TypeScript、定向 ESLint、Prettier、cargo fmt --check、Clippy all-targets --locked -D warnings、生成绑定漂移检查、spec-links、git diff --check 均通过。
- pnpm build（TypeScript + Vite 生产构建）通过。构建仅提示浏览器兼容数据库较旧和已有较大 chunk，不影响产物生成，未借机升级依赖。
- Clippy 首轮发现测试代码中一个无用 format!，已修正并重跑全部检查通过。

## 官方资料核查

- 通过官方 npm registry 核实当前稳定包：@earendil-works/pi-coding-agent 0.87.1，@oh-my-pi/pi-coding-agent 18.3.2。
- 旧 @mariozechner/pi-coding-agent 停留在 0.73.1，官方 deprecated 指向 @earendil-works/pi-coding-agent。
- GitHub 官方 OMP v18.3.2 Release 包含 SHA256SUMS.txt 与 Windows/Linux/macOS 各平台资产；公开 GitHub API 本次遇到限流，产品采用 npm 版本 + 固定 Release 校验清单，不依赖 GitHub API 配额。
- Bun 官方 bunfig 文档确认 install.globalDir / install.globalBinDir；执行安装时写入临时 config 固定原 root。Bun 路径与命令有单测，本轮未做 Bun/macOS/Linux 的实际安装验收。

## 交付状态

源码和文档已完成，等待用户验收。未提交、推送或创建发布；未重新生成 MSI/便携包。此前交付 pi-omp-20260926-235813 不包含 OMP 版本修复或本轮更新功能。

## 后续 MSI 交付

已按用户要求构建 pi-omp-cli-update-20260927-011000 MSI，源码指纹、内部 EXE 与资源哈希验证通过，等待用户安装测试；详见 research/cli-updates-msi.md（research 目录内文件可直接查看 cli-updates-msi.md）。
