# 验证与 Windows MSI 打包研究

## 结论

- 本任务同时触及 Rust 侧供应商删除持久化、TanStack Query 缓存和请求日志决策链 UI，不能只跑单层测试。
- 推荐顺序是：定向 Vitest/Rust 测试 -> 生成绑定校验 -> `check:precommit:full` -> `check:prepush` -> CI 差异项 -> Windows x64 MSI。
- 官方 Windows x64 本地构建命令是 `pnpm tauri:build:win:x64`。该脚本在没有更新器私钥且不处于 CI 时自动关闭 updater artifacts，但仍产出 MSI。
- 本地原始 MSI 应位于 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi`。正式发布流程把它重命名为 `stable-assets/aio-coding-hub-win64.msi`，同时要求 `aio-coding-hub-win64.msi.sig`。
- `.github/workflows/dev-build.yml` 只执行 `--no-bundle` 并上传便携 ZIP，不能满足本任务的 MSI 验收。
- 本次研究没有安装依赖、运行测试或执行打包，因为任务限定只读研究且只允许写本文件。

## 仓库工具链与当前机器状态

仓库声明/CI 基线：

| 项目 | 仓库基线 | 来源 |
| --- | --- | --- |
| Node.js | CI 使用 22；README 最低 18+ | `.github/workflows/ci.yml`、`release.yml`、`README.md` |
| pnpm | 10.34.3 | 根 `package.json#packageManager` |
| Rust | 1.90.0，含 `rustfmt`、`clippy` | `src-tauri/rust-toolchain.toml` |
| Windows Rust target | `x86_64-pc-windows-msvc` | `package.json`、支持矩阵、Release workflow |
| Tauri CLI | lockfile 解析为 2.9.6 | `pnpm-lock.yaml` |
| Windows 原生工具 | VS Build Tools，Desktop development with C++ | `README.md` |

2026-07-31 在当前 worktree 的只读探测结果：

- Node 为 `v24.14.1`，不是 CI 的 Node 22；pnpm 为 `10.34.3`。
- `src-tauri` 目录内 Rust override 正确解析为 `1.90.0-x86_64-pc-windows-msvc`，目标已安装。
- Visual Studio Build Tools 2022 17.14、MSVC 14.44 和 Windows SDK 10.0.22621.0 已安装。
- 普通 PowerShell 的 `PATH` 中没有 `cl.exe`/`link.exe`/`rc.exe`；如 Rust/Tauri 未能自动发现工具，应改用 VS 2022 Developer PowerShell。
- `node_modules`、`dist` 和任何 `src-tauri/target*` 目录都不存在，因此当前不能直接执行 Tauri CLI、前端测试或打包。必须先执行 frozen install。
- 当前 `CI`、`GITHUB_ACTIONS`、`TAURI_SIGNING_PRIVATE_KEY` 和密码均未设置，符合本地无签名 MSI 路径。

建议在正式验证前确认：

```powershell
node --version
pnpm --version

Push-Location src-tauri
rustup show active-toolchain
rustup target list --installed
Pop-Location

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
& $vswhere -latest -products * `
  -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
  -property installationPath

Get-ChildItem Env:CI,Env:GITHUB_ACTIONS,Env:TAURI_SIGNING_PRIVATE_KEY,Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD `
  -ErrorAction SilentlyContinue
pnpm install --frozen-lockfile
```

为最大化 CI 一致性，最终验证应使用 Node 22。Node 24 满足 README 的最低版本，但不是当前 Actions 基线。

## Package scripts 与质量门禁

### 前端/TypeScript

根 `package.json` 中与本任务直接相关的命令：

| 命令 | 实际行为 |
| --- | --- |
| `pnpm typecheck` | `tsc -p tsconfig.json` |
| `pnpm lint` | `eslint src/` |
| `pnpm build` | `tsc && vite build`，输出 `dist/` |
| `pnpm test:unit` | `vitest run` |
| `pnpm test:unit:coverage` | 全量 Vitest + V8 coverage gate |
| `pnpm test:unit:coverage:shards` | 4 个顺序 shard，合并报告后统一应用覆盖率阈值 |
| `pnpm test:e2e` | `vitest run src/e2e` |
| `pnpm format:check` | `prettier --check .` |

Vitest coverage 阈值为 statements 90%、branches 85%、functions 90%、lines 90%。`src/generated/**` 不计入 coverage。

### Rust/Tauri

| 命令 | 实际行为 |
| --- | --- |
| `pnpm tauri:fmt` | `cargo fmt -- --check` |
| `pnpm tauri:check` | `cargo check --locked` |
| `pnpm tauri:clippy` | `cargo clippy --all-targets --locked -- -D warnings` |
| `pnpm tauri:test` | Windows 下以 `CARGO_BUILD_JOBS=1`、`CARGO_INCREMENTAL=0` 执行 `cargo test --locked`，target 为 `src-tauri/target-tests` |

`src-tauri/rust-toolchain.toml` 会在 crate 目录自动切换到 Rust 1.90.0。不要用仓库根目录显示的默认 Rust 版本判断实际构建版本。

### 聚合门禁

`scripts/run-checks.mjs` 是聚合命令的单一来源：

- `pnpm check:precommit`：lint、typecheck、禁止 instant-now 相减检查、`cargo check --locked`。
- `pnpm check:precommit:full`：另含全仓格式、release changelog/spec/support matrix/Homebrew/gateway error code、Rust fmt、生成绑定和 Clippy。
- `pnpm check:prepush`：lint/typecheck/support matrix/Homebrew/gateway error code/plugin docs/plugin API/plugin SDK typecheck、分片全量 coverage、plugin SDK/scaffolder 测试、生成绑定、全量 Rust 测试和 Clippy。

注意两套 full aggregate 是互补关系：

- `precommit:full` 有 format、release changelog、spec links 和 `cargo check`，`prepush` 没有。
- `prepush` 有 coverage、plugin 测试、plugin docs/API 和全量 Rust 测试，`precommit:full` 没有。
- 两者都不包含 `pnpm audit:deps`、`pnpm test:e2e`、`pnpm build` 和 `cargo audit`；这些是 CI 差异项。

Git hooks 的实际行为：

- `.githooks/pre-commit` 根据 staged path 只跑 `precommit-src`、`precommit-tauri` 和/或支持矩阵检查，并可能自动格式化已完整 staged 的文件；它不是 full gate。
- `.githooks/pre-push` 直接执行 `pnpm check:prepush`。

## 生成绑定

生成命令：

```powershell
pnpm tauri:gen-types
```

实际执行 `cargo run --locked --example export-bindings`，输出：

- 受版本控制文件：`src/generated/bindings.ts`
- Rust 中间产物：`src-tauri/target-bindings/`（已被 `src-tauri/target*/` 忽略）

Windows 上脚本还会：

- 从 `PATH` 去掉 Windows Performance Toolkit 条目，规避同名 linker 干扰；
- 默认设置 `CARGO_BUILD_JOBS=1`、`CARGO_INCREMENTAL=0`。

校验命令：

```powershell
pnpm check:generated-bindings
```

该校验不是纯读取：它先记录旧内容，再重新生成 `bindings.ts`，用 Prettier 原地格式化，检查 `HomeUsagePeriod` 固定字面量，最后做字节比较。若绑定过期，命令以 1 退出并把新文件留在 worktree 中，供审阅和提交。因此应在运行前记录 `git status --short`，失败后检查 `git diff -- src/generated/bindings.ts`，不能把修改误当作测试噪声回滚。

本任务如果不改变 Rust IPC 类型，`bindings.ts` 预期无 diff；仍必须运行校验以证明没有漂移。

## 本任务的 focused checks

### 前端缓存与决策链

供应商删除 mutation 位于 `src/query/providers.ts`。目前只同步移除 provider list、account usage 和 model catalog 缓存；默认路由缓存键为 `providersKeys.defaultRoute(cliKey)`，所有排序模板成员缓存以 `sortModesKeys.all` 为前缀。右侧调用顺序从这些缓存直接读取，因此删除成功后应验证默认路由和所有已缓存模板都不再保留目标 ID，同时其他 provider ID 不受影响。

决策链由 `RequestLogDetailDialog` -> `RequestLogDetailChainTab` -> `ProviderChainView` 渲染。`ProviderChainView` 已同时持有 `provider_name` 与 `provider_id`，但 Attempt 详情主体当前只显式显示 `Provider ID`；已存在 `未知（id=N）` 回退。新增测试应覆盖可读名称 + 稳定 ID、同名/同域名配置和已删除/缺失 provider 的回退。

建议每轮实现后执行：

```powershell
pnpm exec vitest run `
  src/query/__tests__/providers.test.tsx `
  src/query/__tests__/sortModes.test.tsx `
  src/components/__tests__/ProviderChainView.test.tsx `
  src/components/home/__tests__/RequestLogDetailDialog.test.tsx
```

如果改动 `attemptsJson` 解析或字段兼容逻辑，再加入：

```powershell
pnpm exec vitest run src/services/gateway/__tests__/attemptsJson.test.ts
```

### Rust 删除持久化

数据库中的三类调用顺序是：

- `provider_pool_order`：供应商池展示顺序；
- `default_route_providers`：Default 调用顺序；
- `sort_mode_providers`：每个排序模板、每个 CLI 的调用顺序与 enabled 状态。

三张表都声明 `provider_id -> providers(id) ON DELETE CASCADE`，连接初始化也执行 `PRAGMA foreign_keys = ON`。但当前删除测试只证明 provider 本身和可选 request logs 的行为，没有显式断言同一 provider 从 Default 及多个排序模板中全部消失，也没有验证其他同名/同域名 provider 保留。应把这些断言放在删除事务的 Rust 测试中，避免只证明 UI cache 被刷新。

建议 focused Rust 命令：

```powershell
Push-Location src-tauri
cargo test --locked --lib delete_
cargo test --locked --test providers_crud providers_crud_roundtrip
cargo test --locked --test sort_modes_crud
Pop-Location
```

若新增测试名更精确，可用其全名替代宽泛的 `delete_` filter；最终仍由 `pnpm tauri:test` 跑全量。

## 推荐 full checks

### 仓库本地完整门禁

先确保 focused tests 全绿，再按以下顺序执行：

```powershell
pnpm check:precommit:full
pnpm check:prepush
```

这两条的并集覆盖本仓库声明的格式、lint、类型检查、静态合约、生成绑定、coverage、前端 workspace 测试、Rust check/fmt/test/clippy。

### CI 等价补充

`.github/workflows/ci.yml` 的 frontend job 还执行以下未被上述并集覆盖的命令：

```powershell
pnpm audit:deps
pnpm test:e2e
pnpm build
```

CI 使用单次 `pnpm test:unit:coverage`，而 `check:prepush` 使用等价目的的 4-shard 合并 coverage gate。通常不需要本地重复跑单次 coverage；若要求逐命令复刻 CI，则额外运行：

```powershell
pnpm test:unit:coverage
```

Rust CI 还会更新 workspace lockfile并检查无 diff、串行运行测试、执行 cargo audit：

```powershell
Push-Location src-tauri
cargo update --workspace
git diff --exit-code -- Cargo.lock
cargo test --locked -- --test-threads=1
cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195
Pop-Location
```

风险与取舍：

- `cargo update --workspace` 会访问网络且可能真实改写 `Cargo.lock`。若出现 diff，必须审阅并按 CI 约定提交，不能盲目恢复或忽略。
- Windows Clippy 只编译 Windows `cfg` 分支，CI Rust job 在 Ubuntu 22.04 编译 Unix 分支。本任务预期修改平台无关的 SQLite/React 逻辑；若最终 diff 引入/触及 target-gated Rust，仍需 Linux CI 等价环境补验。
- dependency audit 依赖实时 registry/advisory 数据，可能因外部状态失败；应记录 advisory 和时间，而不是把它与功能回归混为一谈。

## Windows x64 MSI 构建

### 本地验收路径（推荐）

根 `package.json`：

```text
tauri:build:win:x64 = node scripts/tauri-build.mjs --target x86_64-pc-windows-msvc
```

`scripts/tauri-build.mjs` 根据目标自动补充 `--bundles msi`。在本地且未配置 `TAURI_SIGNING_PRIVATE_KEY` 时，它生成被忽略的 `.local/tauri.build.local.json`，仅覆盖：

```json
{
  "bundle": {
    "createUpdaterArtifacts": false
  }
}
```

因此无需发布私钥即可构建可安装 MSI。执行：

```powershell
$buildStartedAt = Get-Date
pnpm tauri:build:win:x64
if ($LASTEXITCODE -ne 0) { throw "Tauri MSI build failed: exit $LASTEXITCODE" }
```

Tauri 会先按 `tauri.conf.json#build.beforeBuildCommand` 执行 `pnpm build`，然后构建 Rust release binary 和 WiX MSI。配置还要求以下仓库资产存在：

- `src-tauri/icons/icon.ico` 及其他跨平台 icons；
- `src-tauri/resources/plugins/official/privacy-filter/**/*`；
- `src-tauri/wix/remove-stale-start-menu-shortcut.wxs`；
- WiX `upgradeCode`：`c1b4a027-411b-5de5-94b9-b6953c022c17`。

原始预期产物：

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi
```

仓库没有断言 Tauri 原始 MSI 的具体文件名，因此验收应使用受限目录 glob，而不是硬编码 locale/架构后缀。构建后报告路径、文件名、字节数和 SHA-256：

```powershell
$msiFiles = @(
  Get-ChildItem -File `
    -Path "src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi" `
    -Filter "*.msi" |
    Where-Object LastWriteTime -ge $buildStartedAt
)

if ($msiFiles.Count -ne 1) {
  throw "Expected exactly one MSI from this build, found $($msiFiles.Count)"
}

$msi = $msiFiles[0]
$hash = Get-FileHash -Algorithm SHA256 -LiteralPath $msi.FullName
[pscustomobject]@{
  Path = $msi.FullName
  Name = $msi.Name
  Bytes = $msi.Length
  SHA256 = $hash.Hash
}
```

`dist/`、`.local/` 和 `src-tauri/target*/` 均已被 `.gitignore` 忽略。MSI 验收后仍应执行 `git status --short`，确认产品源码和绑定没有意外变化。

### 正式 Release 路径

`.github/workflows/release.yml` 的 Windows 矩阵项为：

```json
{
  "platform": "windows-latest",
  "target": "x86_64-pc-windows-msvc",
  "bundles": "msi",
  "updater_platform": "windows-x86_64",
  "stable_label": "win64"
}
```

发布 job：

1. checkout 已解析的不可变 commit SHA；draft Release 的 tag 不可 fetch 时先创建/解析 tag；
2. 使用 Node 22、pnpm frozen install、Rust 1.90.0 和 Windows MSVC target；
3. 要求并预验 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；
4. `tauri-apps/tauri-action` 执行 `--target x86_64-pc-windows-msvc --bundles msi`；
5. `support-matrix.mjs prepare-stable-assets` 从 action 的 `artifactPaths` 中挑选 MSI 和 `.msi.sig`，重命名为：
   - `stable-assets/aio-coding-hub-win64.msi`
   - `stable-assets/aio-coding-hub-win64.msi.sig`
6. 另生成 `stable-assets/aio-coding-hub-win64-portable.zip`；
7. 上传 workflow artifact `stable-assets-windows-x86_64` 和 GitHub Release assets。

`.msi.sig` 是 Tauri updater 签名，不等同于 Windows Authenticode 代码签名。仓库未配置证书型 Windows code signing。

本任务只需要验证 MSI，且 PRD 明确未授权创建/推送 Release，因此应走本地命令，不应触发 `release.yml`。

## 现有 CI/发布路径摘要

| 文件 | 触发 | 与本任务关系 |
| --- | --- | --- |
| `.github/workflows/ci.yml` | push/PR 到 `dev`、`main` | frontend coverage/build/audit + Ubuntu Rust fmt/clippy/test/audit；本地 full checks 的最终参照 |
| `.github/workflows/dev-build.yml` | 除 `main`、`upstream/*` 外的 push 或手动 | Windows x64 `--no-bundle`，只产便携 ZIP，不产 MSI |
| `.github/workflows/release.yml` | `workflow_dispatch` | 正式签名 MSI/更新器资产/portable ZIP；会操作 GitHub Release |
| `.github/workflows/release-pr-sync-cargo-lock.yml` | release-please PR | `cargo update --workspace` 并提交 `Cargo.lock`；说明 CI 对 lockfile 同步的要求 |
| `scripts/support-matrix.mjs` | CI/Release/README 共用 | Windows x64 是唯一官方 Windows Release target；ARM64 仅本地构建 |
| `scripts/tauri-build.mjs` | 本地 package scripts | 本地无私钥时禁用 updater artifacts，并按目标选择 bundle 类型 |

## 主要风险清单

1. **缓存只清一处**：只刷新当前 Default 或当前选中的排序模板，其他已缓存模板仍可能在 UI 中残留；应按 provider ID 清理/失效整个相关 query family。
2. **仅验证 UI、不验证事务**：SQLite schema 虽声明 cascade，也必须新增后端断言覆盖多个排序模板、Default、pool order 和其他 provider 保留。
3. **按名称/域名误删**：测试 fixture 必须包含同名或同域名的另一 provider，断言只按稳定 ID 删除。
4. **历史日志回退漂移**：已删除 provider 不应被错误映射到复用 ID 或同域名对象；优先使用日志快照名称，名称缺失时稳定显示 ID。
5. **生成校验会写文件**：失败时 `bindings.ts` 会留有修改，不能把它当作无副作用检查。
6. **CI 环境变量污染本地构建**：只要 `CI` 非空，`tauri-build.mjs` 就不会自动禁用 updater artifacts；没有签名 key 时会失败。
7. **误认旧 MSI 为成功**：必须检查构建退出码，并用构建开始时间筛选产物；不能只看目录中是否已有 `.msi`。
8. **首次 MSI bundling 的网络/缓存依赖**：仓库没有单独安装 WiX/WebView2 bundler 工具的步骤，依赖 Tauri CLI 和 Windows runner；首次本地构建可能需要网络获取工具。
9. **普通 PowerShell 未加载 VS 环境**：当前 MSVC 已安装但不在 `PATH`；若自动发现失败，应从 Developer PowerShell 重跑。
10. **资源/自定义 WiX 片段**：缺失资源 glob、icon 或 `remove-stale-start-menu-shortcut.wxs` 会在最后打包阶段失败，即使前端/Rust 门禁全绿。
11. **成本与磁盘**：Windows wrapper 为 tests/bindings 使用独立 `target-tests`、`target-bindings`，MSI 使用标准 `target`；完整验证会产生三套 Rust 中间产物且 Windows 默认单 job，耗时和磁盘占用较高。

## 建议最终执行顺序

```text
1. pnpm install --frozen-lockfile（Node 22）
2. focused Vitest + focused Rust
3. pnpm check:generated-bindings
4. pnpm check:precommit:full
5. pnpm check:prepush
6. pnpm audit:deps + pnpm test:e2e + pnpm build
7. 需要逐命令 CI 等价时：Cargo.lock sync check + serial Rust test + cargo audit
8. pnpm tauri:build:win:x64
9. 记录 MSI Path/Name/Bytes/SHA256，并确认 git status 仅含预期源码/测试/任务文档
```
