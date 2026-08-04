# 多会话单飞回切回归 - 执行计划

1. 在 `gateway/routes.rs` 测试模块增加可阻塞 counting upstream helper。
2. 增加 winner + 动态 follower 集合的两波路由测试，断言调用数、attempt metadata、route
   顺序、circuit 和每个 session binding。
3. 增加失败/stale winner 不发布恢复事实的反例；优先复用现有 helper，避免生产改动。
4. 运行新增过滤测试、全部 `route_ordered_failback`、rustfmt 和 git diff check。

## File Ownership

只允许修改 `src-tauri/src/gateway/routes.rs`，不得修改生产模块。
