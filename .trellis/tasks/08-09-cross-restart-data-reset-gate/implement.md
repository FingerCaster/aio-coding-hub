# 实施清单

1. [ ] 追踪 setup → logging → DB → startup tasks 的真实启动顺序。
2. [ ] 定义最小 marker schema、原子写入和严格读取 helper。
3. [ ] 将当前 reset command 接入 prepare-first 状态机。
4. [ ] 在普通启动副作用前加入 maintenance gate。
5. [ ] 补阶段 failpoint、跨启动 replay、marker 损坏和成功清理测试。
6. [ ] 运行 Rust format、聚焦/full tests、Clippy、bindings 和 diff check。
