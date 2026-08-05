# Technical Design

## Boundaries

父任务拥有分类安全不变量、现有全量 CI 保真、跨任务验收与最终交付。研究、实现、验证子任务分别拥有证据、代码和质量门禁，按顺序执行。

## Classification Flow

GitHub 事件元数据决定差异范围，Git 输出通过 NUL 分隔的 `name-status` 记录进入解析器。解析器把 rename/copy 的旧路径和新路径都作为影响路径，delete 仍以被删除路径分类。每个路径先经过不可配置控制面判定，再按机器策略求最高风险档；任何不完整或不可解释状态直接返回 `full`。

## Workflow Contract

分类 job 始终运行并输出 scope 与每个 job 的选择布尔值。`process-docs` 只保留分类、自测与终态门禁；`checked-docs` 额外运行无安装依赖的文档合同；`full` 运行所有当前 job。固定名 `ci-gate` 在 `always()` 下读取所有 needs result，逐项验证选中 job 为 success、未选 job 为 skipped；PR title 的事件条件和 desktop matrix 的聚合结果必须显式处理。

## Compatibility

保留 job 名称和触发分支，只增加稳定的 `ci-gate`。若远端未来将旧 job 设为 required，非全量分类下 skipped job 可能影响保护规则，必须以当前只读查询结果为准记录风险，不擅自更改 GitHub 配置。

## Rollback

策略、分类器、自测、workflow 和 spec 作为一个行为合同提交；回退该提交即可恢复始终全量的旧 CI。任务归档和 journal 在工作提交之后独立提交。
