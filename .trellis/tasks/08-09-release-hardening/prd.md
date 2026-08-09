# 发布链并发与制品不可变

## Goal

阻止同一版本的自动/手动发布并发写入、签名私钥跨步骤暴露及与目标 SHA 不一致的候选制品晋升，同时保留 fork 已有的 release tag 解析安全合同。

## Evidence

- `.github/workflows/release.yml:19-21` 使用 `release-${{ github.ref }}`，tag push 与 workflow_dispatch 可能为同一发布生成不同 key。
- workflow 将私钥和密码写入 `$GITHUB_ENV`，后续 job steps/Actions 均可读取。
- 当前直接 build/upload Release；缺少以候选 SHA/asset manifest 为权威的不可变晋升边界。
- 候选参考：`cec2353f`、`d5c9cfe0`。当前 fork 已有 release tag 先解析/创建、再把 immutable commit SHA 传给 build job 的规则，必须保留。

## Requirements

- `R1`：自动 tag 和手动 dispatch 对同一目标 tag/version 计算相同 concurrency key。
- `R2`：私钥保存到 runner temp 的权限受限文件，仅 build/sign step 可见，`always()` 清理；密码同样不进入 job 级环境。
- `R3`：正式 Release 只接受目标 SHA 精确匹配且验证过的候选 manifest/assets；已存在资产不得静默覆盖。
- `R4`：draft Release 早于可 fetch tag 时仍先解析或创建 tag，再以 immutable SHA 构建。
- `R5`：保持当前平台资产矩阵和 Homebrew 架构，不采用候选 ARM-only 假设。

## Acceptance Criteria

- [ ] push/dispatch 同 tag 的静态 fixture 得到同一 concurrency group。
- [ ] workflow 静态扫描证明密钥/密码不写入 `$GITHUB_ENV`，临时文件权限和清理存在。
- [ ] SHA、manifest 或资产集合不一致时 promotion 零上传；同一目标资产已存在时 fail closed。
- [ ] annotated/lightweight/missing draft tag 的现有合同测试继续通过。
- [ ] workflow 语法、release 脚本自测、actionlint（可用时）和 diff check 通过。
