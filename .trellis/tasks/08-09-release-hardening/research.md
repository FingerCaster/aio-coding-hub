# 研究摘要

- `release.yml:19-21` concurrency 使用 `github.ref`，同一 tag 的 push/dispatch 可能不同。
- `release.yml:251-289` 把 signing key/password 写入 `$GITHUB_ENV`。
- 当前 fork 已满足 draft Release tag 先解析/创建、再向 build 传 immutable SHA 的强制规则；不得回退。
- 候选 `cec2353f` 增加候选制品 exact-SHA promotion、统一 concurrency 和 no-overwrite；候选 `d5c9cfe0` 用 runner-temp 0600 key、step env 与 always cleanup。
- 候选 workflow/资产矩阵与 fork 不同，应适配当前 release jobs，不整文件替换。
