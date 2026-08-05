# Validation Design

## Layers

1. 纯函数/错误注入自测验证策略和 Git range 构造。
2. YAML 解析与静态合同验证 job 图、输出、条件和 gate。
3. 现有无安装 Node 合同验证 docs/support 治理。
4. 冻结依赖后的 frontend 全套与 Rust fmt/clippy/test/audit 验证 full 不退化。
5. diff/spec/task 全范围自审与复测。

## Failure Handling

不得通过删测试、改参数、忽略错误或扩大文档 allowlist 消除失败。实现缺陷在本任务修复；平台/工具不可用或基线既有问题记录命令、输出和残余风险。验证产生的临时/生成差异必须先确认归属，不能覆盖其他会话文件。

## Remote Compatibility

仅用显式 `FingerCaster/aio-coding-hub` API 再读 classic protection 与 rulesets。当前无 required checks 时，结论限于固定 `ci-gate` 不冲突；未来启用保护由管理员另行配置。
