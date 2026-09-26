# 实施验证记录

日期：2026-09-26。首期实施及下列自动化检查已完成；最终 Rust / bindings 检查于 17:20（Asia/Shanghai）结束，退出码为 0。代码保留在当前分支供审阅，未自动提交、推送或发布。

## 已完成的中央接线

- Pi/OMP 独立客户端及原生管理能力；既有 whole-client proxy、MCP、会话和安装更新能力不复用。
- SQLite v47 增量迁移与新安装/ensure 路径：原生档案、显式模型能力、生成入口所有权。
- 独立目标选择设置，默认兼容、每客户端唯一和输入边界；不加入普通设置编辑器的字段所有权。
- 原生九项 IPC、网关七项 IPC，以及不启动 CLI 的安装路径/包版本探测。

## 最终检查结果

| 检查 | 当前结果 |
| --- | --- |
| 固定 Pi/OMP × 四协议直连本地模拟服务 | 8/8 基础、80/80 增强、4/4 auth、23/23 配置边界和 24/24 runner；见 runtime-wire-contracts.md |
| 真实 Pi/OMP → AIO → 本地模拟上游 | 8/8 通过，使用生产 catalog/generate_entries 节点，含跨协议跳过与同协议失败切换；11.98 秒 |
| `cargo check --locked --lib` | 通过；最终全 targets Clippy 同样通过 |
| `pnpm tauri:gen-types`、`pnpm check:generated-bindings` | 通过；最终再生无漂移 |
| `pnpm build` | 通过；Vite 有已有大 chunk 与 Browserslist 更新提示 |
| `pnpm typecheck`、`pnpm lint`、`pnpm test:unit` | 最终通过：322 个文件、2977 项测试，包括 Pi/OMP 错误规则保留回归 |
| 原生配置后端聚焦测试 | 39/39 通过，含目标切换竞争、OMP 生命周期、Windows 私有 ACL 和锁定写失败 |
| 目录、发布和 Provider 聚焦测试 | 22/22 native_gateway 与 132/132 Provider 通过 |
| 中央 Rust 聚焦测试 | 298/298 通过：数据库迁移、设置、配置包、CLI 身份和只读探测 |
| `pnpm check:spec-links` | 通过 |
| `pnpm check:support-matrix` | 通过（发布平台矩阵；不代表新增功能跨平台实机验收） |
| `pnpm check:gateway-error-codes` | 42 个代码一致 |
| `pnpm check:no-instant-now-sub`、plugin API/docs、Homebrew selftest | 通过 |
| `pnpm tauri:test` | 27 个 suite、3290 项通过、0 失败；其中 library 3164 项通过，默认忽略 5 项见下文 |
| `pnpm tauri:clippy` | 全 targets、locked、`-D warnings` 通过 |
| `pnpm format:check`、`pnpm tauri:fmt`、`git diff --check` | 通过 |

默认忽略的 5 项是既有 Codex 实机 picker、2 项插件性能 smoke、bindings 导出测试及本次真实 CLI 测试。后两者分别通过正式生成绑定命令和显式 ignored 八链路测试另行验证；本次没有运行无关的 Codex 实机 picker 和插件性能 smoke。

首轮失败均已修复并复验：单连接测试夹具未释放连接、目录夹具缺 Default 路由成员、Anthropic JSON 与 SSE 夹具包装混用。生产权限或候选校验未为测试放宽。最终审查另外修复排队写入的目标切换竞态、发送前资格失效的中性预算释放，以及前端编辑设置时对 Pi/OMP 错误规则的保留。

## A01–A14 验收对应

| 验收项 | 实现及验证依据 |
| --- | --- |
| A01 | CLI 能力注册、独立原生/网关页面；NativeGatewayFlows、NativeTargetPicker 和旧 CLI UI 回归 |
| A02 | native_cli targets/document 测试：默认/自定义/profile、YAML 优先级、旧 JSON 只读及歧义拒绝 |
| A03 | native_cli service 生命周期、外部刷新、归档再应用与删除测试，39 项聚焦验证 |
| A04 | native_gateway import 分组、显式凭证、停用快照、原子导入和失败回滚测试 |
| A05 | unknown-field、文件修订/节点 digest、坏格式、外部修改和目标排队切换测试 |
| A06 | 固定真实 CLI 经生产生成节点到 AIO 的 8/8；W0 80 项增强实验及生产 router 的 32 个工具/图像/思考请求夹具 |
| A07 | 每条真实 CLI 链路含同协议失败切换；生产 router 验证跨协议零发送、零 attempt、零健康惩罚 |
| A08 | generation 白名单、无真实凭证/旁路字段、幂等发布及精确所有权撤回测试 |
| A09 | native_gateway service 的碰撞、外改、监听端口变化、DB 故障、补偿冲突与中断恢复测试 |
| A10 | native_cli 文件内容/mtime 保护测试覆盖认证、数据库和默认设置；表达式拒绝执行、导入预览无凭证 |
| A11 | router 日志核对 Pi/OMP 身份、协议、模型和 attempts；JSON/SSE、重复 finalize、SQL 与费用对账通过 |
| A12 | 四协议 JSON/SSE、提交前错误切换、提交后不重放、缺终止事件和八组合取消测试通过 |
| A13 | Rust 全库与前端全量回归，包括旧四 CLI、Grok 双协议、Codex 受管模型、SQLite v47 与设置迁移 |
| A14 | Pi/OMP 分享及旧 portable bundle 明确拒绝测试；生成节点不改原生默认或现有会话 |

A06 的证据分层记录：真实 CLI 的网关测试是流式；非流式成功及失败由生产 router 测试证明，不声称真实 CLI 发出了非流式请求。日志中的协议与原始用量审计数据持久化在 gateway_protocol marker；前端对未知 marker 保持兼容，不影响其他标记。

## 复现与证据

Windows 下统一设置 `CARGO_TARGET_DIR` 为本 worktree 的 `src-tauri/target`、`CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=1` 后运行上表命令。真实网关测试另运行：

```text
pnpm tauri:test --lib native_real_cli_eight_protocol_streams_through_actual_gateway -- --ignored --nocapture
```

先按 [真实 CLI 契约报告](runtime-wire-contracts.md) 准备固定隔离 runtime。测试只访问本地模拟服务，不使用真实供应商凭证或付费请求。

本机忽略的运行证据位于 `.trellis/.runtime/research/omp-pi/`：

- `rust-all-final.log`、`rust-clippy-final.log`、`bindings-final.log`。
- `frontend-unit-final.log`、`frontend-build-final.log`、`format-final.log`。
- `rust-real-cli-gateway.log` 与 `gateway-test/aio-gateway-results.json`（8/8）。

持久实施说明还包括 [原生后端](native-implementation-api.md)、[目录与发布](w4-w5-implementation.md)、[多协议网关](w3-gateway-report.md)，用户入口见 `docs/pi-omp.md`。

用户随后授权安装最新 Pi / OMP。本机 Pi 已从 0.81.0 升至 0.87.1，OMP 官方 Windows 独立版 18.3.2 已安装；实际安装入口完成八条 AIO 链路、80 项增强协议、4 项认证分支、23 项配置与 24 项 runner 验证。安装路径、版本与哈希、隔离边界和复现步骤见 [最新安装版验收](installed-latest-validation.md)。

## 已核实的运行差异

Pi Anthropic 使用 `x-api-key`。OMP 缺省认证与显式 `auth: apiKey` 的行为不同：早期缺省分支曾观测到 Bearer、`?beta=true` 和 OAuth 风格工具名；生成入口显式设置 `auth: apiKey` 后使用普通 `/v1/messages`，不能将早期分支结论套用到生产入口。最终 headers 与工具名以 W0 的 auth 分支对照及真实生成节点验收为准。

OMP 自身会在隔离运行中创建本地数据库；这与 AIO 管理操作的写入边界分别核验，不能将 CLI 自身写入误记为 AIO 变更。

## 已知交付边界

- 实机证据仅覆盖 Windows、Pi 0.87.1 与 OMP 18.3.2。macOS/Linux 路径和权限没有本轮实机验收；不据此扩大平台/版本支持声明。
- Pi/OMP 原生 OAuth、特殊云认证、WebSocket、扩展协议和 CLI 自动安装更新不在本期范围。
- 旧分享和 portable bundle 不能完整承载新语义，明确拒绝；不会静默丢弃协议、模型声明或节点所有权。
- 首次实施验收的 `pnpm build` 是前端生产构建；后续已按用户要求生成 Windows x64 本地测试安装包和免安装包，见 [本地打包验收](package-validation.md)。没有发布到远程。
- `.trellis/config.yaml` 的既有 dispatch_mode 改动来源未确认，原样保留，未擅自还原。`tauri.conf.json` 仅作三处数组排版修复，无配置语义改变。
- Trellis 任务保留待提交/评审状态；没有自动触发归档或会自动提交的 session 操作。
