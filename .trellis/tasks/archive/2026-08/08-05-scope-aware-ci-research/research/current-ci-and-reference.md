# 当前 CI 与参考实现研究

## 研究基线

- 当前 worktree：`D:\OrcaProjects\aio-coding-hub-fork\scope-aware-ci`
- 当前提交：`12e565c0e7fbcb461f0ccb0fccaa5274846f8185`，与用户指定的本地 `main` 基线一致。
- 当前实现真相：本仓库 `.github/workflows/ci.yml` 与 `package.json`。
- 只读参考：`D:\UGit\aio-coding-hub-knaifen-reference` 的 `80c4cbd5`，以及后续修正 `a92ec8f`。参考仓库未被修改，也没有 cherry-pick。

## 当前 workflow 合同

当前 `ci.yml` 只监听 `push` 和 `pull_request`，目标分支均为 `dev`、`main`；没有 `workflow_dispatch`。并发组为 `ci-${{ github.event.pull_request.number || github.ref }}`，同一 PR/分支的新运行会取消旧运行。实现分级 CI 时必须保留这些触发与并发语义，不能从参考仓库带入手动候选发布输入。

| Job | 当前条件/依赖 | 必须保留的行为 |
| --- | --- | --- |
| `pr-title` | 仅 `pull_request` | 校验 Conventional Commits 标题；push 上应为 skipped。 |
| `support-contract` | 所有当前触发事件 | checkout、Node 22；导出 desktop matrix；运行 support matrix 合同、Homebrew Cask 自测、upstream 同步策略自测和实际策略检查。 |
| `desktop-support-contract` | `needs: support-contract` | 由 support matrix 生成 Windows、macOS、Linux 三项矩阵，逐项运行 `pnpm check:support-matrix`。矩阵整体结果必须进入最终 gate。 |
| `frontend` | `needs: support-contract` | 安装 Linux/Tauri 系统依赖和冻结的 pnpm 依赖，运行现有全部前端检查。 |
| `rust` | `needs: support-contract` | 安装 Linux/Tauri 系统依赖和 Rust 1.90.0，执行 fmt、Cargo.lock 同步/漂移检查、clippy、test、cargo audit。 |

`support-contract` 中以下两步是本 fork 的产品/治理合同，参考实现没有等价替代，必须在 `full` 保留：

1. `node scripts/check-sync-upstream-policy.selftest.mjs`
2. `node scripts/check-sync-upstream-policy.mjs`

它们确保 `.github/workflows/sync-upstream.yml` 保留定时与手动触发、以跨仓 PR 同步、明确要求人工审查/合并、不自动 merge，并让有冲突或需要语义审查的 PR 保持打开。分级不能把这两步遗漏，也不能因 `support-contract` 产出 matrix 而让下游 full jobs 意外静默跳过。

当前 `frontend` 的完整检查清单是：

- `pnpm check:support-matrix`
- `pnpm audit:deps`
- `pnpm lint`
- `pnpm check:gateway-error-codes`
- `pnpm check:plugin-system-docs`
- `pnpm check:plugin-api-contract`
- `pnpm check:generated-bindings`
- `pnpm plugin-sdk:typecheck`
- `pnpm plugin-sdk:test`
- `pnpm --filter create-aio-plugin test`
- `pnpm test:e2e`
- `pnpm test:unit:coverage`
- `pnpm build`

当前 `rust` 的完整检查清单是 `cargo fmt -- --check`、`cargo update --workspace` 后的受条件 Cargo.lock 漂移检查、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --locked -- --test-threads=1` 和带当前两个临时 ignore 的 `cargo audit`。`full` 必须继续运行所有这些现有检查；不能以分级为名降低参数或删减步骤。

## checked-docs 可运行合同

当前仓库真实存在且只依赖 Node 内置模块/`git`、不需要 `pnpm install` 或 Cargo build 的文档相关合同有：

| 命令 | 覆盖范围 | 结论 |
| --- | --- | --- |
| `node scripts/check-plugin-system-docs.mjs` | 插件文档必需/禁止表述，并与少量源码、历史文档边界保持一致 | 可直接用于 `checked-docs`。 |
| `node scripts/check-plugin-api-contract.mjs` | 插件文档、`plugin-api-v1-contract.json` 与 Rust/TS 实现的一致性 | 可直接用于 `checked-docs`；合同 JSON 或源码本身发生变化仍必须分类为 `full`。 |
| `node scripts/check-spec-links.mjs` | `.trellis/spec/**/*.md` 与 `src/templates/markdown/spec/**/*.md` 的本地链接 | 可直接用于 `checked-docs`，当前 workflow 尚未调用它，但脚本和 `package.json` 的 `check:spec-links` 均真实存在。 |

三条命令已在基线提交上直接运行并通过。参考实现还运行 `scripts/check-tui-release-contract.mjs`，但当前仓库不存在该文件；不得引用。`check-plugin-system-completion.mjs` 虽然存在，但它同时校验 package/workspace/source/CI 完整性，且不属于当前 CI 已选用的文档 job，因此不应凭参考实现之外扩张 `checked-docs`。

路径政策应只声明确实纯文档的路径。最终规划采用比参考提交更保守的边界：

- `process-docs` 精确覆盖 `AGENTS.md`，并以严格扩展名覆盖 `.trellis/tasks/` 下的 `.md/.json/.jsonl` 与 `.trellis/workspace/` 下的 `.md`。这些 JSON/JSONL 是任务记录的特例，不代表任意 JSON 可降级。
- `checked-docs` 覆盖 `README.md`、`README_EN.md`、`.trellis/spec/**/*.md` 和 `docs/**/*.md`。
- `CHANGELOG.md` 属于发布记录，`.trellis/workflow.md` 会被 Trellis 运行时解析，`.trellis/agents/**/*.md` 会影响代理执行；它们都不是纯文档，保持 `full`。
- 当前 `docs/` 与 `.trellis/spec/` 合计除 Markdown 外只有 `docs/plugins/plugin-api-v1-contract.json`；它是机器可读产品合同，必须保持 `full`。图片、其他扩展、源码、配置、脚本、manifest、锁文件、构建/发布文件也一律落入 `full`。
- 前缀规则必须同时限制扩展名，不能把整个 `docs/` 或 `.trellis/` 无条件降级。未知路径和一切未显式允许的扩展默认 `full`。

## 参考提交 80c4cbd5

`80c4cbd5` 提供了可复用的基础结构：

- 新增 `change-scope`，checkout 使用 `fetch-depth: 0` 和 `persist-credentials: false`，Node 分类器输出 `scope`、`full_ci`、`docs_checks`、`reason`。
- 新增严格 schema 的 `.github/ci-scope.json`，规则只支持 exact path 与带 extension allowlist 的 prefix。
- 新增无第三方依赖分类器与自测。
- `docs-contract` 仅在 `docs_checks == true` 时运行，重型 jobs 仅在 `full_ci == true` 时运行。
- 新增固定名 `ci-gate`，使用 `if: always()` 汇总分类器、条件 job 和候选发布 job 的 success/skipped 结果。

参考分类器值得沿用的 fail-closed 合同：

1. 路径先做仓库相对路径安全校验；空路径、绝对路径、反斜杠、控制字符、空/`.`/`..` segment 都不能降级。
2. `.github/**` 以及分类器、自测脚本是代码内硬编码的控制面，优先于策略文件；即使策略误配也返回 `full`。当前实现还应明确覆盖 CI 策略自身（已由 `.github/**` 包含）。
3. 策略版本、字段集合、数组、重复 exact/prefix/extension 都严格校验；策略读取、JSON 解析、schema 或 Git 命令错误统一由顶层捕获并返回 `full`/`classification-error`。
4. 单路径未分类为 `full`；同一路径同时匹配 process 与 checked 规则为 `full`/`ambiguous-policy`；任意混合中只要出现 full 路径，整体就是 `full`。
5. 空差异返回 `full`/`empty-diff`；`workflow_dispatch` 返回 `full`/`manual-dispatch`；不支持事件返回 `full`/`unsupported-event`。

### Git 事件与 name-status 边界

- PR：验证 base/head SHA 后执行 `git merge-base <base> <head>`，再比较 `<merge-base>..<head>`。这避免把目标分支在 PR 分叉后的无关推进算进 PR，也不能简化成固定 `base..head`。
- push：比较事件 payload 的 `before` 与 `head`，覆盖一次 push 中的全部提交，不能用 `HEAD^1..HEAD`。全零 SHA、缺失/畸形 SHA 或对象不可用都应失败并回退 full。
- checkout 必须有完整历史（参考实现使用 `fetch-depth: 0`），否则 merge-base 或 push `before` 可能不存在；Git 失败由分类器 fail closed。
- diff 使用 `git diff --name-status -z --find-renames --find-copies-harder <from> <head> --`。NUL 分隔是支持空格、换行等合法文件名所必需，不能按行解析。
- 普通状态记录消费一个路径；`Rnnn`/`Cnnn` 同时消费旧、新两个路径并都参与分类。这样从源码 rename/copy 到文档、或反向移动，不会因只看目标路径而降级。delete 仍按被删除路径分类。未知状态、非法 score、字段数错误或缺失末尾 NUL 均抛错并回退 full。

参考自测已覆盖 exact/prefix/extension、unknown/mixed、控制面优先、策略歧义、空差异、rename/copy/delete、PR merge-base、push before/head、手动事件、零 SHA 和 malformed name-status。当前实现应补齐/保留错误注入与 unsupported event 的显式断言，并确保修改 `.github/**`、分类器、自测或策略会选择 full，从而运行分类器自测。

## a92ec8f 的后续修正

`a92ec8f` 不是分类器基础逻辑修正，而是修复参考仓库新出现的 provider trend 百万行 benchmark 选路：

- 分类输出新增 `provider_trend_benchmark`。
- `src-tauri/src/domain/usage_stats/**` 及若干 ledger/migration 精确路径触发 benchmark。
- manual、error、empty 等 fail-closed full 结果也强制 benchmark 为 true。
- Rust job 不再用局部的 `git diff HEAD^1 HEAD` 决定 benchmark，而消费分类器基于真实事件 range 的输出。

这项修正证明所有“按变更决定是否运行”的子门禁也必须共享正确的 PR/push range，不能在 job 内另做 `HEAD^1` 判断。不过当前仓库的 `ci.yml` 没有 provider trend benchmark，因此不应移植该输出或路径表。

## 为什么必须适配当前 ci.yml

参考 workflow 有 `candidate-plan`、GUI 候选构建、TUI 候选构建、候选产物组装及手动发布输入；当前仓库一个也没有。相反，当前仓库有参考实现没有的 `desktop-support-contract` 三平台矩阵，以及 fork 独有的 upstream 人工审查同步策略检查。直接照搬会同时造成“运行不存在脚本/job”和“漏掉当前必要检查”。

实现时应在当前 job 图上最小改造：

- `change-scope` 始终运行并输出分类合同。
- `pr-title` 继续按事件运行，不因 docs 分级绕过。
- `support-contract`、`desktop-support-contract`、`frontend`、`rust` 只在 full 选择；full 下保持现有步骤完整。
- `docs-contract` 只运行上文三条真实、无安装文档合同，不引用 TUI/候选发布脚本。
- 固定 job id/name `ci-gate`，`if: always()`，`needs` 至少包含分类器、PR title、docs、support、desktop matrix、frontend、rust。gate 必须校验分类输出域和相互关系；逐一要求被选择 job 为 `success`、未选择 job 为 `skipped`。`desktop-support-contract` 的聚合 matrix 结果不能遗漏。
- 非 full 时 `support-contract` 被跳过，其 matrix 消费者也必须明确得到预期 skipped；full 时则要求 support 成功、matrix 三项整体成功，再允许 gate 成功。

该结构既保留当前行为，又把条件 job 的 skipped 从“可能静默通过”变成 `ci-gate` 明确验证的合同。
