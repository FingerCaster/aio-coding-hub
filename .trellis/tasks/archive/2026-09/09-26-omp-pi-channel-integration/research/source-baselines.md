# 调研来源与验证边界

调研日期：2026-09-26。此任务以源码证据形成设计，不把源码阅读写成已经完成的真实 CLI 验收。

## 固定基线

| 对象 | 本地目录 | 固定版本 / 提交 | 用途 |
| --- | --- | --- | --- |
| AIO | D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration | 3372d11f5a20a212bd551e0b0b7067ab2b639c79 | 本任务修改基线 |
| 用户指定的 CCS | D:/UGit/cc-switch-main | e0f70019b2758f5b6b9a04dd60e4689481a0c0ac | Pi 产品行为、文件同步和 UI 参考 |
| Pi 官方源码 | D:/UGit/pi-source-reference | v0.87.1 / f07218c4d4bbc12bef056a7058c3dd49dfe41abe | 当前发布版契约；参考克隆已 detached 到此提交 |
| OMP 官方源码 | D:/UGit/oh-my-pi-source-reference | v18.3.2 / 7853b4e499936f9dcc13c9b64adb55f6b342aabf | 当前发布版契约 |

- Pi 官方地址：https://github.com/earendil-works/pi 。npm 当前包为 @earendil-works/pi-coding-agent，查询到 0.87.1 于 2026-09-22 发布；旧命名空间 @mariozechner/pi-coding-agent 的 latest 为 0.73.1，不能用旧包的 latest 代替当前项目版本。
- OMP 官方地址：https://github.com/can1357/oh-my-pi 。npm 包 @oh-my-pi/pi-coding-agent，查询到 18.3.2 于 2026-09-26 发布。
- npm 元数据来源：https://registry.npmjs.org/@earendil-works%2fpi-coding-agent 与 https://registry.npmjs.org/@oh-my-pi%2fpi-coding-agent 。发布包也已下载作交叉核对，保存在 gitignored 的 .trellis/.runtime/research/omp-pi/。
- Pi 初次克隆 main 为 d6af72e1857cfb10b41d8ff8e69f0d72b4cf6d31，和已发布版本存在源码差异；后续契约以固定发布提交为准，不能以 package.json 版本仍相同推断 main 等同已发布包。
- 本机 PATH 中的 Pi 包为 0.81.0；它不能充当 0.87.1 的验收证据。当前 shell 未找到 omp 命令，不据此断言整机未安装 OMP。

## 证据分级

1. 已确认：固定提交中的代码、schema、默认路径、路由入口；CCS 和 AIO 源码结构；用户已确认的产品选择。
2. 设计建议：数据表、IPC 名称、AIO 入口名称、协议 URL 分段、UI 布局；这些均是拟实现契约。
3. 实施前必须实测：两种真实 CLI 的四种协议请求路径、认证头、SSE 结束帧、工具回合、图片/思考、取消、重试与 Google 版本前缀。
4. 本轮未做：安装/升级用户的 CLI、读写用户真实 auth 文件、登录或调用收费模型、产品业务代码开发。

## 用户确认

- 首期同时支持原生供应商管理和 AIO 网关接管，包含路由、重试与统计。
- 使用独立 AIO 入口，与原生供应商并存；不自动改默认供应商、默认模型或当前会话。
- 可以 clone Pi / OMP 到本地查证源码。
- 旧 pi-omp-cli-management 方案已经弃用；本任务不恢复旧分支或移植旧补丁。

## 阅读方式

其他调研中的路径均相对于本表对应仓库。引用形式为 文件:行号，并附符号名；实施者应使用固定提交定位，后续版本变更时重新核验。外部源码不会作为运行时依赖打包进 AIO。
