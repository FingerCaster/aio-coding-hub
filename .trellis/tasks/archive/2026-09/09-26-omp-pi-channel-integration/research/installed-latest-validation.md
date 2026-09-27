# 最新 Pi / OMP 本机安装验收

日期：2026-09-26；环境：Windows x64、Node 24.14.1。用户明确授权安装或升级最新 Pi / OMP 并继续测试。本轮只改测试 runner 与验收文档，未改变前轮已验收的产品代码。

## 安装与版本来源

通过 npm registry 的 `dist-tags.latest`、包版本及官方 OMP GitHub `releases/latest` 核实当日最新稳定版本。

| CLI | 安装前 | 安装后 | 实际安装方式 |
| --- | --- | --- | --- |
| Pi | 0.81.0 | 0.87.1 | `npm install -g @earendil-works/pi-coding-agent@0.87.1 --ignore-scripts --no-audit --no-fund` |
| OMP | 未安装 | 18.3.2 | 官方 release `v18.3.2` 的 `omp-windows-x64.exe`，安装到 `%LOCALAPPDATA%\omp\omp.exe` |

OMP release 发布时间为 `2026-09-26T00:00:25Z`。下载文件为 235686912 字节，SHA-256 与 GitHub asset digest 一致：

```text
5d99fe5c11ec3ff1792e427c0e61eeb5a6c84670cadb65d023dd2f5eb8705f40
```

Pi 实际全局包入口 `dist/bundle/cli.js` 的 SHA-256：

```text
e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774
```

已运行真实入口版本命令，分别输出 `0.87.1` 和 `omp/18.3.2`。OMP 安装目录已追加到用户 PATH，未安装全局 Bun，也未改写用户原生供应商、默认模型、认证文件或 shell 选择。已打开的父进程可能仍持有旧 PATH，需要重启终端或 AIO。

## 测试结果

测试显式指定全局 Pi 包入口及安装后的 OMP 独立可执行文件。每次进程的证据记录 `runtime.source=installed`、命令路径和网络护栏模式；网关入口另记录实际版本探测、可执行文件/入口哈希和生产生成节点。未将隔离解压包结果计作本机安装版验收。

| 验证 | 结果 | 覆盖 |
| --- | --- | --- |
| 安装版 CLI 直连本地模拟服务 | 8/8 | 两 CLI × 四协议，URL、认证、请求模型、SSE、完成事件 |
| 生产生成节点 → 安装版 CLI → 真实 AIO → 本地模拟服务 | 8/8 | 四协议、跨协议零发送、500 后同协议切换、请求日志身份和用量 |
| 增强协议场景 | 80/80 | 工具、思考、图片、HTTP 400、SSE 错误、中流错误、429、RPC 取消、模型覆盖及 agent 重试 |
| OMP Anthropic 认证对照 | 4/4 | 显式 apiKey 与省略 auth、两种假密钥形态；工具名、URL、请求头差异 |
| 原生配置和目标路径 | 23/23 | Pi JSONC、OMP YAML 优先级、坏文件、旧 JSON、目录/profile/环境/参数优先级 |
| 网关 CLI runner 自测 | 24/24 | 16 条成功及生产节点形状输入、4 项非法输入拒绝、2 项原生错误识别、Node/Bun 两种预加载护栏自测 |

所有测试进程均正常结束，无失败项。原生 CLI 的错误场景中，Pi 仍可能 exit 0，OMP exit 1；验收同时检查事件和 `stopReason`，没有把预期错误或错误退出码当作测试失败。

## 隔离边界

测试使用独立 HOME、agent 目录、工作目录与临时目录，清除继承的真实凭证和代理环境，关闭扩展和上下文自动发现，模型请求目标为本机模拟服务或本机 AIO，使用无效测试密钥。没有使用真实供应商密钥做付费服务验收。

全局 Pi 包运行于 Node，已加载 JavaScript 网络拦截。OMP 官方编译程序的 `--preload` 和 `BUN_OPTIONS` 探测均未执行预加载模块，因此它直接运行，并在证据中明确标为 `networkGuard=unavailable-standalone`；本轮不声称其具有 JavaScript 网络拦截或系统防火墙隔离。runner 自测中的 Bun 护栏项验证的是独立 Bun 运行时，不是 OMP 编译程序。

OMP 自身启动会在隔离目录创建数据库，配置测试按原生行为检查这些写入。AIO 的只读探测仍然不启动 CLI；无包元数据的独立程序版本可以显示未知。

## 复现

在本 worktree 的 PowerShell 中，设置显式测试入口；不会让测试读取用户 HOME：

```powershell
$globalModules = (npm root -g).Trim()
$env:PI_OMP_TEST_PI_ENTRY = Join-Path $globalModules '@earendil-works/pi-coding-agent/dist/bundle/cli.js'
$env:PI_OMP_TEST_OMP_BINARY = Join-Path $env:LOCALAPPDATA 'omp/omp.exe'
node scripts/pi-omp-wire-capture.mjs
node scripts/pi-omp-wire-extended.mjs
node scripts/pi-omp-config-probe.mjs
node scripts/pi-omp-runner-selftest.mjs
node scripts/pi-omp-wire-extended.mjs --omp-only --anthropic-only --scenarios=auth-generic,auth-apikey,auth-omitted-generic,auth-omitted-apikey
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'src-tauri/target'
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_INCREMENTAL = '1'
pnpm tauri:test --lib native_real_cli_eight_protocol_streams_through_actual_gateway -- --ignored --nocapture
```

本轮真实 AIO 测试直接运行前轮已编译的同一 Rust 测试二进制，避免无产品代码变更时重复全量构建。环境变量由 Rust runner 传给 Node 控制进程；随后 CLI 进程的环境仍由隔离函数重新构造。未设置两个覆盖变量时，测试仍默认使用固定隔离包。版本探测拒绝与当前固定契约不一致的版本，需要先重新核对协议。

## 本机证据

以下路径均相对 `.trellis/.runtime/research/omp-pi/`，未加入 Git：

- `installed-validation.json`：安装版结果聚合、版本、哈希与全部证据路径。
- `omp-installed.json`、`pi-global-upgrade.log`：安装记录。
- `installed-probe/results.json`：实际入口及 OMP 预加载兼容探测。
- `installed-basic.log`、`installed-real-gateway.log`、`installed-aio-gateway-results.json`。
- `installed-extended.log`、`installed-auth.log`、`installed-config.log`、`installed-runner.log`。
- `runs/2026-09-26T10-04-56-209Z/summary.json`：8 条基本链路。
- `runs/extended-2026-09-26T10-05-19-412Z/summary.json`：80 项增强协议。
- `runs/extended-2026-09-26T10-06-50-254Z/summary.json`：4 项认证分支。
- `runs/config-2026-09-26T10-05-19-387Z/summary.json`：23 项配置。
- `selftest/mui84ivu/summary.json`：24 项 runner 验证。

Windows 安装版验收没有扩大到 macOS/Linux、真实供应商服务或交互式 TUI 操作。前轮的全量 Rust、前端、Clippy、bindings 和构建结果见 [实施验收](implementation-validation.md)。
