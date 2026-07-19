# 自启动副作用边界修复设计

## Boundary

`settings_set` 仍是普通设置的共享持久化入口，但 `auto_start` 从完整快照字段改为可选意图：

- `Some(bool)`：本次 writer 明确拥有 `auto_start`，进入现有 autostart coordinator。
- `None`：本次 writer 不拥有 `auto_start`，在 settings lock 内保留最新 canonical 值，且不进入 autostart owner/OS 路径。

旧客户端发送布尔值仍走原语义；新前端只有真正修改 `auto_start` 时才发送布尔值。

## Frontend Data Flow

CLI Manager 的 `useSettingsPatchMutation` 已持有原始 `AppSettingsPatch`，可在 `createSettingsSetInput` 中根据 `hasOwnProperty("auto_start")` 决定是否保留 `autoStart`。设置页持久化器已计算 `changedKeys`，由 `buildPersistedSettingsMutationInput(desired, changedKeys)` 决定是否携带 `autoStart`。`toGeneratedSettingsUpdate` 将未携带值编码为 `null`。

这样不依赖“新值是否恰好等于旧值”猜测用户意图；显式同值写入仍被视为修复 OS 漂移的请求。

## Backend Commit And Rollback

`apply_settings_update_owned_patch` 以 `update.auto_start.unwrap_or(previous.auto_start)` 计算 canonical 值。

- `Some` 分支复用 `commit_auto_start_with_owner`，返回 `Some(AutoStartCommitToken)`，保留 generation、强制 OS sync、correction 与 rollback。
- `None` 分支直接执行同一个 settings-owned commit closure，返回 `None` token；不持有 `AUTO_START_LOCK`。

运行态失败回滚接收该可选 token。`None` 时只在 settings lock 内执行 ordinary-owned CAS：比较普通 committed token时忽略 `auto_start`，恢复 ordinary fields 时保留当时最新 `auto_start`，不调用 `perform_os_sync`。`Some` 时维持现有原子 generation + ordinary-owned 检查和 OS 收敛。

网关成功启动后若实际端口不同，既有 `repair_preferred_port_if_current` 是独立 post-commit writer。它为使较旧 autostart token 失去回滚所有权而继续推进 generation，但不会调用 OS autostart sync；本任务不拆除该并发保护。纯重试策略保存不会触发 preferred-port repair。

## Windows Adapter Defense

本地 `auto-launch` patch 将注册表键或值缺失的 `ErrorKind::NotFound` 解释为目标状态已满足。helper 接受可注入 open/delete 操作以做无注册表副作用的单元测试；其他 `io::Error` 原样返回。

## Compatibility And Risk

- Rust `Option<bool>` 接受旧布尔 payload，也接受新 `null`/缺失 payload。
- 配置文件 schema 与 `AppSettings.auto_start` 不变，无数据迁移。
- config import 的 whole-settings 路径不变。
- 最大风险在 settings runtime-failure rollback；测试必须证明无 token 时不触碰 OS 且保留并发 auto-start winner，有 token 时既有 ABA/generation 测试不回归。

## Rollback

该修复可通过回退单一功能提交恢复。没有 schema migration 或不可逆数据变化；Windows adapter 的幂等行为也可独立回退。
