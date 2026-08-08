# 实施计划：自定义 JavaScript 账户用量查询

## 进入门槛

- [ ] 用户审阅并批准本任务 `prd.md`、`design.md`、`implement.md`。
- [ ] 运行 `task.py start 08-08-custom-account-usage-script` 后才修改业务代码。
- [ ] 使用 `trellis-before-dev` 读取 backend/cross-layer 规范和前端现有模式。
- [ ] 记录工作区既有改动，不纳入或还原无关文件。

## 1. 领域配置与 portable sanitizer

- [ ] 扩展 `ProviderAccountUsageAdapterKind`、配置类型和前端 config type，加入 `custom` 与有界字段。
- [ ] 先写 UTF-8 脚本大小、Origin 规范化/去重/数量/长度、timeout、非法 adapter 和未知字段测试。
- [ ] 实现权限指纹、Base Origin 绑定、proof 剔除、existing-permission 复用和配置变更撤销。
- [ ] 区分本地 persistence 与 portable sanitizer；custom portable 结果必须 adapter disabled 且完全无源码/Origin/授权材料。
- [ ] 更新 `replace_extension_values`，在 Provider 事务内读取 existing values/Base URL，验证输入并只接受后端 proof。
- [ ] 覆盖 `values=None` 时对历史本地字段的规范化，避免旧 proof 或失效 enabled 状态永久残留。

## 2. 原生确认与 Provider 保存/草稿测试

- [ ] 增加 confirmation service，使用 non-waiting 单飞槽和原生 warning dialog，显示完整有效 Origin 与 SHA-256 指纹。
- [ ] Provider upsert 入口先剔除 Renderer proof，再判断已确认授权或执行原生确认；取消/关闭/并发均零数据库写入。
- [ ] 后端确认成功后注入瞬时 proof，由 domain sanitizer 在事务内消费；proof 不出现在返回 summary。
- [ ] 增加不含密钥与含密钥的 account-usage fetch context，使用 Provider UUID、Base URL/auth/source/config 快照验证确认竞态。
- [ ] 实现 `provider_account_usage_test_custom_script`：确认前不加载 API Key，确认后只允许 API Key 单独轮换，其他身份变化拒绝。
- [ ] Provider duplicate 保留本机草稿与启用意图但不继承授权；源 custom 已启用时用 `Duplicate` 原生确认，取消零写入，确认后复检源 UUID/config/Base Origin 再为新身份注入 proof。源 custom 未启用时直接复制并保持禁用；现有 NewAPI 私有 credentials 复制行为不回归。

## 3. 一次性 QuickJS worker

- [ ] 新建 `provider_account_usage_script.rs`，先覆盖 worker handshake、bounded line、missing function、syntax/runtime error 和 child 回收测试。
- [ ] 在主二进制最早入口增加 `--account-usage-script-worker` 分支，保持 extension-host worker 与正常 Tauri 启动行为。
- [ ] worker 清空环境并只保留当前平台启动必需 allowlist，不继承 HOME/AppData/通用 LD/DYLD；关闭 stderr 回显，设置 `kill_on_drop` 和平台隐藏窗口标志。
- [ ] 配置 QuickJS 8 MiB memory、256 KiB stack、100 ms interrupt；只启用所需 intrinsic 并移除所有 host/network globals。
- [ ] 父进程为启动、request/parse 调用、输入/输出协议及 terminate/reap 设置独立硬上限；所有 timeout/错误 kill + bounded wait。
- [ ] 用实际 child-process 测试死循环、不会轮询 interrupt 的 native built-in、过大 stdout、异常退出和连续运行无残留。

## 4. 请求计划、HTTP 与结果归一化

- [ ] 实现 opaque API-key/Base-URL placeholders 和有界逐段物化；只替换所有精确、完整 token occurrence，变换后的 token 不替换，脚本永远看不到明文。
- [ ] 校验 GET/POST、HTTPS、无 userinfo、精确 Origin allowlist、header 数量/名称/值、Cookie/Set-Cookie/Host/代理/逐跳 forbidden headers 和 body 上限。
- [ ] 使用 no-redirect、配置 timeout 和稳定 user-agent 的 client；覆盖 3xx、401/403、非成功状态、网络错误和响应 body 64 KiB。
- [ ] JSON 解析后递归清理精确 API Key/Base URL，再把 `{status,data}` 传给独立 parse worker。
- [ ] 实现 DTO 字段类型/有限数字/i64 expiry/文本上限；失败状态清空所有部分值并使用本地消息。
- [ ] custom workflow non-waiting semaphore 固定四个 permits，超限证明零脚本/零 HTTP 执行。

## 5. 共享账户用量运行时

- [ ] 选择性移植 process-owned runtime，先用可控时钟测试 schedule、lease、硬 TTL 和 generation。
- [ ] 建立 Provider entry、15 秒 Desktop lease、1 秒 scheduler tick、同 Provider watch completion、每 Provider 合并 due/force 位和四 Provider 全局 fetch limiter；只有取得 permit 才 spawn，不产生无界 semaphore 等待任务。
- [ ] 把现有 command 主体拆为 `fetch_account_usage_uncached`，所有内置/custom fetch 都由 runtime 调度；in-flight 期间的多个 manual force 合并为至多一次尾随刷新，强刷调用者等待尾随完成。
- [ ] 建立 generation 校验后的只读 snapshot 投影，保证 route-ready try-read 不包含 secret 且不会接受旧 completion；本任务不接任何 route consumer。
- [ ] 使用 checked-increment 进程 generation 和规范化非敏感 config token，不依赖秒级 `updated_at`；mutation helper 比较查询语义/凭据身份，只在 Base URL/auth/source/API Key、adapter/mode/interval/custom 授权等变化时 invalidate，delete/disable 与配置导入成功提交/reset 清理，name/note/display timed-only 变化保留有效结果。
- [ ] 证明成功结果 60 分钟硬过期、失败按 timed interval 重试、`timedRefreshEnabled=false` 仅保留首次/手动/硬过期行为。

## 6. Desktop、IPC 与生成绑定

- [ ] 把 IPC 拆为 snapshot read、remote refresh、Desktop lease acquire/heartbeat/release；草稿测试命令加入 registry。
- [ ] React Query 保持当前单一前端镜像 owner、exact-key cancellation 和逆序完成保护；不复制旧提交的 5 秒远端轮询，不借 `meta` 传 force 命令。
- [ ] Provider Editor 增加 custom adapter、脚本 textarea、Origin 列表、timeout stepper、测试按钮和启用 switch；沿用现有 disclosure 与紧凑布局。
- [ ] UI 在脚本/Origin/Base Origin 变化时立即撤销本地 enabled 展示和陈旧测试结果；最终权威仍是后端 sanitizer。
- [ ] 从 Rust 重新生成 bindings，更新 generated IPC adapter 与契约测试，不手工维护重复 DTO。

## 7. 分享、完整备份与导入

- [ ] 单 Provider export/normalize/preview/import 全部调用 portable sanitizer；序列化 bytes 和 preview 不含 custom 字段。
- [ ] config bundle export、prepare import 和 transactional import 对 custom 配置双向 portable sanitize；crafted v3/v4 字段也被删除。
- [ ] 保持 NewAPI account credentials 的既有差异：单 Provider 分享不读取，完整敏感备份仍按当前 contract 处理。
- [ ] 使用合成脚本、保留测试域名和合成 secret 做序列化全文负例；不得把真实主机/金额写入 fixture。

## 8. 验证矩阵

- [ ] **配置/授权**：UTF-8 边界、16 Origin、canonical duplicate、HTTP/path/query/userinfo、timeout、proof forging、Base Origin、API Key rotation、Provider UUID/auth/source 变化。
- [ ] **worker**：缺函数、异常、死循环、native built-in、heap/stack、输入输出超限、启动失败、kill/reap、四工作流并发。
- [ ] **HTTP**：method、Origin、redirect、Cookie/Set-Cookie/headers、placeholder exact occurrence、request/response cap、invalid JSON、401/403、timeout、精确 redaction。
- [ ] **归一化**：全部状态、所有 DTO 字段、NaN/infinity、expiry integer、失败状态夹带金额、超长文本和未知字段。
- [ ] **runtime**：initial/manual/timed、same-provider coalescing、force tail、four-provider cap、零无界等待任务、lease expiry、60 分钟 TTL、failed retry、future time、old generation/config token、import ID reuse。
- [ ] **跨层**：Desktop 初始/定时/手动共用后端所有权、所有 Provider mutation 走语义化 invalidate、share/config bundle/duplicate、bindings、无 routing/circuit/session 副作用。

## 预期文件范围

- `src-tauri/src/domain/provider_account_usage.rs`
- `src-tauri/src/domain/provider_account_usage_script.rs`
- `src-tauri/src/app/provider_account_usage_confirmation.rs`
- `src-tauri/src/app/provider_account_usage_runtime.rs`
- `src-tauri/src/app/{mod,plugin_registry,provider_service}.rs`
- `src-tauri/src/commands/providers/account_usage.rs`、`src-tauri/src/commands/registry.rs`
- `src-tauri/src/domain/providers/{types,queries,share,tests}.rs`
- `src-tauri/src/infra/config_migrate/{export,import,mod,tests}.rs`
- `src-tauri/src/{lib,main}.rs`
- `src/query/providers.ts`、`src/services/providers/`、`src/pages/providers/`、`src/components/providers/`
- `src/generated/bindings.ts`、`src/services/generatedIpc.ts` 及对应测试

## 分阶段评审门

1. 配置/授权通过 forged proof、确认取消和竞态测试后，才接真实密钥。
2. worker 在死循环/native built-in/异常退出下均有界回收后，才允许 HTTP。
3. HTTP 与归一化完成敏感信息负例后，才接共享 scheduler。
4. runtime generation/config-token/coalescing/TTL 通过后，才接 Desktop lease 与快照镜像。
5. portable 双向净化和完整回归通过后，才更新规范并提交。

## 验证命令

```powershell
cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_account_usage_script --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml provider_share --lib --locked
cargo test --manifest-path src-tauri/Cargo.toml config_migrate --lib --locked
pnpm exec vitest run src/query/__tests__/providers.test.tsx src/pages/providers/__tests__/ProviderAccountUsageSection.test.tsx src/pages/providers/__tests__/ProviderEditorDialog.test.tsx src/services/providers/__tests__/providerAccountUsageConfig.test.ts src/services/providers/__tests__/providers.service.test.ts src/services/__tests__/generatedIpc.test.ts
pnpm check:generated-bindings
pnpm typecheck
pnpm lint
pnpm tauri:fmt
pnpm tauri:check
pnpm tauri:clippy
pnpm test:unit
pnpm tauri:test
git diff --check
python .trellis/scripts/task.py validate 08-08-custom-account-usage-script
```

## 回滚点

- UI 尚未保存 custom config 前，可回滚 UI 接线而不影响数据库格式。
- worker/HTTP 未接 runtime 前，可整体回滚 custom adapter；内置适配器路径保持原样。
- runtime 接线异常时可恢复原 IPC query owner，但不得让脚本回到 Renderer 或主进程。
- portable sanitizer 发现泄密时必须停止交付并修复已生成载荷路径，不能仅关闭 adapter。

## 完成条件

- [ ] PRD 全部 AC 有自动化证据。
- [ ] 任何取消、越权、超限、异常、旧 generation 路径均不读取/泄露不必要凭据且在有界时间结束。
- [ ] Desktop 初始/定时/手动刷新共享后端所有权，现有内置适配器无回归。
- [ ] 路由、circuit、Session 和 Provider 启用/顺序保持纯展示行为。
- [ ] `trellis-check` 通过并更新账户用量规范后提交。
