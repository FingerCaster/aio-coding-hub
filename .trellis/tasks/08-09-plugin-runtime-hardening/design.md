# 技术设计

## 子边界

1. 安装服务拥有 preview digest、确认重算和版本 identity。
2. repository 事务拥有 plugin/config/storage 原子更新，不把并发控制放在 UI。
3. gateway plugin context 拥有 wire casing 和序列化预算。
4. pipeline 拥有 fail policy、事务 header mutation 和日志屏障。
5. extension host runtime 拥有 absolute deadline、实例身份和 idle sweeper。

## 顺序

先落安装/版本完整性，再落 context/policy，最后落 deadline/idle/config/log。每一段必须可独立测试，避免一次性搬候选最终插件栈。

## 兼容性

- canonical camelCase 之外仅在 QuickJS 边界为已安装旧插件提供 snake_case aliases，不在公开 SDK 扩展双格式。
- 运行时 package 源码为本任务所有；被用户删除的 SDK/scaffold 路径不是本任务所有。
- activation quarantine 属于候选最终 recovery 架构，不在本任务内，除非实现 deadline 时出现不可分离的现行契约证据并由协调者批准。

## 回滚

按安装完整性、Hook 合同、host 生命周期三个提交边界回滚；禁止用整仓插件目录替换。
