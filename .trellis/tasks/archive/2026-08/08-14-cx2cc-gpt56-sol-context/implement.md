# 实施计划：CX2CC GPT-5.6 Sol 与模型上下文

## 执行顺序

1. 更新共享模型契约：移除裸预设，前后端默认改为 `gpt-5.6-sol`，修正相关 UI/测试
   与 CX2CC spec；保留历史手动值。
2. 在 provider `ClaudeModels` 增加四个 CX2CC-only optional context，补 model/context
   配对校验、普通 Claude 拒绝、生成绑定、Provider share v5 与配置 bundle 回归。
3. 扩展 terminal projection：逐槽应用显式 context 或 provider catalog，组合
   Exact/Mixed/Unknown，并覆盖四槽、多 Provider、去重和 env 注入测试。
4. 在 CX2CC Provider 四个有效模型输入旁增加上下文数字输入；模型改变时清空旧
   context，普通 Claude 隐藏；补保存、边界、失败与历史缺字段测试。
5. 补 reasoning passthrough 边界（尤其 `ultra`/未来值和非字符串 absent），确认没有
   legacy fallback 或枚举换算。
6. 运行格式化、定向 Rust/前端测试、生成绑定校验、typecheck/lint、`git diff --check`
   与完整 `pnpm check:prepush`。
7. Trellis 独立 check 代理复核并自修；更新 CX2CC/settings spec，提交、PR、CI 合并，
   再以修正后的不可变 merge SHA 重新发布 Beta 6。

## 重点回归

- 新默认与历史值兼容。
- context/model 配对、数值边界、普通 Claude 拒绝与旧 JSON 缺字段。
- override-only、catalog-only、两者混合、不同容量 min、unknown fail closed。
- 当前 AIO 动态路由的所有候选参与投影。
- Claude effort 字段转换但值不换算；disabled 唯一映射为 none。
- 稳定发布渠道和 Homebrew 不被 Beta 工作流修改。

## 回滚点

- context map 默认空，因此删除新 UI/字段可恢复原 catalog-only 行为。
- 默认常量与 context 功能分开测试；任一失败都阻止 Beta 6 promotion。
- 发布前不复用旧 Beta 6 tag；tag 必须绑定最终 merge SHA。
