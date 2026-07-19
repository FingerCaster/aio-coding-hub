# 修复自启动校正阻断网关策略保存

## Goal

解除普通设置保存与操作系统自启动副作用之间的错误耦合：只有请求明确修改或重申 `auto_start` 时才进入自启动协调器并访问 Windows 注册表。保存瞬时重试规则等无关字段时，从提交到失败回滚全过程都不得读写自启动状态。

## Background

- 用户在 `0.60.28` 本地 MSI 中保存通用网关参数时收到 `failed to disable autostart: 系统找不到指定的文件 (os error 2); autostart correction also failed`。
- 前端的 `createSettingsSetInput` 和设置页持久化器会为局部 patch 重建完整 `SettingsUpdate`，因此只修改重试策略也会携带当前 `autoStart=false`。后端将 `auto_start` 定义为必填 `bool`，无法区分“明确关闭”和“本次未修改”。
- 普通设置提交随后无条件通过共享自启动协调器，并以 `force_sync=true` 将 canonical `auto_start` 同步到操作系统；失败回滚也始终传入自启动 token。结果是任何普通字段保存都可能访问注册表。
- 现有 settings/autostart 契约要求所有**实际拥有 `auto_start` 的 writer**共享 generation、锁序、校正和回滚协议，并不要求无关 writer 冒充 `auto_start` owner。可选 token 与 settings-only rollback 已有结构基础，但生产路径尚未使用。
- Windows `auto-launch` 本地补丁在 `src-tauri/patches/auto-launch/src/windows.rs` 中直接调用 `Run` 注册表键的 `delete_value`。键或值不存在时返回 `std::io::ErrorKind::NotFound`，当前被当作同步失败；随后的校正再次执行相同删除并再次失败，最终阻断设置保存。
- 即使正确收窄副作用，明确关闭自启动时删除一个已经不存在的启动项仍应幂等成功；权限拒绝、注册表访问失败及其他 I/O 错误必须继续上抛。

## Requirements

- R1：`SettingsUpdate.auto_start` 改为可选补丁字段。缺失/`null` 表示本次不拥有也不修改 `auto_start`；布尔值表示明确的自启动写入。旧客户端继续发送布尔值时保持兼容。
- R2：前端局部设置保存只有在原始 patch 或实际 changed-key 集合包含 `auto_start` 时才发送 `autoStart`；瞬时重试规则、熔断参数及其他无关保存必须发送 `null`/缺失。
- R3：无 `auto_start` 意图的 settings commit 直接走 settings-owned 更新，不获取/推进 autostart generation，不执行 OS sync，并返回空自启动 token。其运行态失败 rollback 只比较/恢复普通 owned fields，保留并发 `auto_start` winner，且不得执行 OS sync。后续仅在网关实际选择不同端口时发生的独立 preferred-port repair 继续沿用既有 generation 失权协议，但同样不得执行 OS autostart sync。
- R4：明确携带 `auto_start` 的提交继续使用现有 autostart coordinator、强制收敛、generation token、锁序、校正和回滚协议；显式重申同值仍可作为修复操作同步 OS 状态。
- R5：Windows `AutoLaunch::disable()` 在 `Run` 注册表键不存在或应用对应值不存在时返回成功；除 `NotFound` 外的打开/删除错误原样传播，不得按错误文本或无条件吞错。
- R6：增加前后端及 Windows adapter 回归，覆盖无关策略保存零 autostart 调用、显式自启动保存仍同步、无 token 回滚零 autostart 调用、并发自启动 winner 保留，以及 adapter 的成功/缺失/其他错误矩阵。
- R7：验证生成绑定、前端类型/测试、Rust 格式/检查及相关 autostart/settings 测试，并重新生成 Windows x64 MSI 供安装验证。

## Acceptance Criteria

- AC1（R1-R3）：保存 `upstream_retry_policy` 且不修改 `auto_start` 时，持久化成功并证明 autostart sync 调用数为 0；Windows 注册表是否存在启动项不影响结果。
- AC2（R2）：CLI Manager patch 与设置页 changed-key 保存均只在 `auto_start` 被明确修改时发送布尔值，其他请求发送 `null`/缺失；旧布尔 payload 仍可反序列化。
- AC3（R3）：无自启动 token 的直接 commit 与运行态失败 rollback 不获取自启动所有权、不执行 OS sync，并在恢复普通字段时保留并发写入的 `auto_start`；独立 preferred-port repair 不属于该直接 commit/rollback 计数，且保持零 OS autostart sync。
- AC4（R4）：显式 `auto_start=true/false` 仍产生 generation token 并同步 OS；同值显式写入仍可强制纠正外部漂移。
- AC5（R5、R6）：Windows 注册表键和值两种缺失路径均幂等成功；正常删除成功；权限等非 `NotFound` 错误原样传播。
- AC6（R4、R6）：config import、显式自启动提交、并发 winner 及既有 settings/autostart 回归保持通过，锁序与 token 规则不被削弱。
- AC7（R7）：修复提交合并到本地 `main`，主工作区既有用户改动不受影响，并产出新的未签名 Windows x64 MSI、文件大小和 SHA-256。

## Out Of Scope

- 不改变瞬时错误规则的匹配、重试预算或界面。
- 不改变 config import 的 whole-settings autostart 协调、应用启动时的显式修复或 macOS/Linux adapter 行为。
- 不改变网关 effective preferred-port repair 为防止旧 token 回滚而推进 generation 的既有协议。
- 不忽略权限、损坏或其他真实注册表错误。
- 不进行发布或推送远端。
