# 技术设计

## 发布身份

目标 tag/version 是并发身份，已解析的 commit SHA 是构建和晋升身份。两个身份分别显式传递，禁止从 later `github.ref` 或“最新成功候选”反推。

## 密钥作用域

prepare step 只在 runner temp 写 0600 key file并校验；build step 通过 step-level env/路径读取；无论成功失败均清理。任何诊断不得打印 key/password。

## 候选晋升

候选产出带 exact SHA 和资产清单。promotion 验证 run 结论、SHA、tag、资产名/摘要后一次性上传，设置不覆盖策略。若当前 workflow 暂无候选 job，先引入最小 manifest/验证脚本，不复制候选仓库完整 CI 架构。

## 回滚

发布 workflow 与 helper script 同提交回滚；不修改 release-please 和现有 tag resolution helper 的语义。
