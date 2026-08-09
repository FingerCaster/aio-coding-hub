# 技术设计

## 状态机

`normal` → 原子写入 `reset-pending` marker → 当前进程停止长期 owner 并执行删除 → 成功后退出/清 marker。启动 bootstrap 在任何普通副作用前读取 marker：成功完成维护后继续或请求重启；失败则仅暴露 maintenance error/retry/exit。

## 所有权

- marker 位于受管理 app data 路径，使用临时文件 + rename/replace 原子提交。
- 维护执行复用现有 `infra::data_management` 删除清单；不复制第二套路径列表。
- `app_state` 负责启动顺序，command 只发起事务，不绕过状态机。

## 故障处理

prepare 未提交时不删除；marker 已提交后所有失败保留 marker。清除失败仍视为维护未完成。任何未知 marker schema 阻断普通启动。

## 验证

用 failpoints 覆盖 prepare、各删除阶段、marker clear 和 next-process replay；检查启动顺序与零后台 owner。
