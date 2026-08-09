# 研究摘要

- `src/query/settings.ts:63-111` 的 set/patch mutation 无共享 `scope`；patch 使用 Query cache 展开完整设置。
- Settings 页面有局部 runner queue，但普通 mutation 的其他调用者不受其串行保护。
- 候选 `5c756edc` 使用共享 ordinary-settings-write scope 和 changed-key persistence runner。
- `commands/model_prices.rs:79-86` 的编辑 GET 调用 `read_fail_open`；`infra/model_price_aliases` 已有严格有界读取与 schema v2。
- `ModelPriceAliasesDialog` 在 null/default 投影后仍允许保存，前端版本常量需要与 Rust 对齐。
- 候选 `db92a480` 将编辑读取改为严格错误并阻断 add/save；成本统计读取仍可 fail-open。
