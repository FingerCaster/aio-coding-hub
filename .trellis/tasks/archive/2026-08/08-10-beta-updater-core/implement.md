# Beta 更新频道后端实施清单

- [x] 在 settings types/defaults/service/view 中加入 UpdateChannel，并补专用 writer、generated type 和普通 patch 排除断言。
- [x] 在 config_migrate export/prepare import 加入 stable normalization，补 round-trip、legacy、invalid/rollback tests。
- [x] 改造 desktop updater endpoint/metadata/resource/download/discard，保留 stable 默认路径和现有 risky confirmation。
- [x] 更新 app updater service 与 normalizer，锁定 pipeline 输出字段和 Release URL。
- [x] 运行 cargo fmt/check/test/clippy（适用时）、pnpm check:generated-bindings、相关 service tests。
- [x] 将 command/type/error contract 交给 beta-update-ui，并在父任务记录稳定回归结果。
