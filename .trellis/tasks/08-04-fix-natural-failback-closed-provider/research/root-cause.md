# Root Cause Evidence

- 截图中的首选 Provider 为 `CLOSED 0/5`，但 session 复用低优先级 Provider。
- `probe_planner.rs` 的自然 deadline 分支只寻找 `OPEN/HALF_OPEN` 候选；`CLOSED` 候选没有 explicit trigger 时返回 `not_triggered`。
- `circuit_breaker.rs` 只在 `set_open_deadlines` 中建立 `natural_probe_due_at`；低于 threshold 的 `CLOSED` failure 只记录 failure timestamp。
- `record_success` 清理 failure timestamps，但当前不存在供 `CLOSED` failback 使用的 pending deadline。
- 现有测试 `natural_session_does_not_directly_fail_back_to_closed_candidate_without_trigger` 证明无 trigger 的健康候选保持黏性是有意行为，因此修复必须只作用于“发生过失败且期限待处理”的 `CLOSED` 候选。
- 设置页文案称该值为 Provider 全局最大等待兜底，但没有说明它当前只在 `OPEN` 状态建立，造成用户对 60 秒回切的合理误解。
