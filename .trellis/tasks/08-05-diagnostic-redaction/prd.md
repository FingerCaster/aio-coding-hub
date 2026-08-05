# 诊断信息全链路脱敏

## Goal

确保诊断内容跨越前端和 Rust 边界时不会泄露凭据、认证头、私密 URL 信息或无界对象内容。

## Requirements

- 敏感键、Bearer/Authorization、常见 token 和 secret 赋值统一脱敏。
- URL 去除用户名、密码、查询和 hash。
- 对象遍历处理循环、异常 getter、深层/超大结构并有固定预算。
- console、generated IPC、前端错误报告和 Rust 接收端全部接入。
- 任一脱敏异常失败闭合，不回显原值。

## Acceptance Criteria

- [x] 单元测试覆盖自由文本、URL、嵌套对象、循环、异常 getter 和预算上限。
- [x] IPC/错误报告测试证明敏感值不出现在日志或传输参数中。
- [x] Rust 测试证明后端会再次清理合成 secret。

## Out Of Scope

- 修改业务错误文案、远端遥测或持久化日志结构。
