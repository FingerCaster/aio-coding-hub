# Technical Design

## Policy And Classifier

策略版本固定为 1，顶层只允许 `processDocumentation`、`checkedDocumentation`、`version`。每个规则集只允许 `exactPaths` 和 `prefixRules`；prefix 必须以 `/` 结尾，每项至少一个受限扩展名，重复或未知字段均报错。

分类器先校验路径是安全的仓库相对 POSIX 路径，再检查不可配置控制面：`.github/**` 与 CI scope/gate 脚本。随后匹配两个规则集；双匹配、未匹配均为 `full`。多路径只要跨档就失败闭合为 `full`，并保留 `docs_checks` 表示 mixed 变更中是否仍需文档合同。

`collectChangedPaths` 对 PR 验证 base/head SHA 并求 merge-base；对 push 验证 before/head。Git diff 固定使用 `--name-status -z --find-renames --find-copies-harder`。所有异常由顶层转换成 `classification-error` 的 full 结果，CLI 仍输出安全值供 workflow 运行完整检查。

## Workflow Job Graph

```
change-scope -> docs-contract (checked path present)
change-scope -> support-contract -> desktop-support-contract (full)
                                -> frontend (full)
                                -> rust (full)
pr-title (PR only) ---------------------------------> ci-gate
all selectable jobs --------------------------------> ci-gate (always)
```

`change-scope` checkout 完整历史且不持久化凭据，先跑自测再分类。`docs-contract` 只需 checkout/Node。现有 full job 步骤原样保留，仅增加 `needs`/`if`。

`ci-gate` 读取所有直接 needs 的 `result` 和分类输出，并调用无依赖
`scripts/ci-gate.mjs`：分类 job 必须 success；scope/布尔输出必须互相一致；PR title 按事件 success/skipped；docs 按 `docs_checks` success/skipped；四个 full job（含 desktop matrix 聚合结果）按 `full_ci` success/skipped。任何 cancelled/failure/空输出均不满足断言。

`scripts/ci-workflow-contract.selftest.mjs` 使用缩进感知的最小结构解析器读取当前 workflow，校验 job 集合、outputs、needs/if、gate 全依赖及环境绑定；再直接调用 `ci-gate.mjs` 的同一断言函数覆盖 process/checked/full、PR/push、selected/unselected 及故障结果矩阵。

## Documentation Contract

新增 `.trellis/spec/aio-coding-hub/cross-layer/ci-change-scope-contract.md`，登记到 index，明确机器合同和运行时 Markdown 不属于纯文档。`package.json` 提供本地分类器自测入口，但 CI 直接调用无依赖 Node 脚本以避免 install。
