# 实施清单

1. [ ] 读取 CI change-scope 和 release tag 合同，绘制当前 job/asset 数据流。
2. [ ] 统一 tag/version concurrency key 并补事件 fixture。
3. [ ] 将 updater key/password 收窄到 build step 和 runner temp，补静态泄漏检查。
4. [ ] 适配 exact-SHA candidate manifest/promotion，保持现有资产矩阵。
5. [ ] 补 mismatched SHA、资产缺失、已存在资产和 draft tag 回归。
6. [ ] 运行 Node/workflow 合同、actionlint（可用时）和 diff check。
