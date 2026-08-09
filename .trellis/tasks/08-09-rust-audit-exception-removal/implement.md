# 实施清单

1. [ ] 读取 Cargo manifests/lock 反向依赖并核对候选最小版本集合。
2. [ ] 运行精确 Cargo update，审查锁文件变化。
3. [ ] 删除所有目标 RustSec ignore，搜索仓库遗漏入口。
4. [ ] 运行 cargo audit、fmt、check/Clippy/tests 和 workflow 合同测试。
5. [ ] 记录无法在 Windows 覆盖的平台目标。
