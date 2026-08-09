# 技术设计

使用 Cargo 的精确 package update 收窄锁文件变化；先确认 `plist` 与 `wayland-scanner` 对 `quick-xml` 的约束，再更新最小依赖集合。随后删除 workflow 的 audit ignore，并以锁文件反向依赖和 diff 审查证明没有无关升级。

此任务不修改 CI change-scope 分类，只调整已选 Rust job 内的审计命令。
