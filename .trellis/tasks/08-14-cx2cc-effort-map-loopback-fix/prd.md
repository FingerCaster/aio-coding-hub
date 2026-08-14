# 增加 CX2CC 思考强度映射并修复回环慢请求

## Goal

让 CX2CC 能按用户可维护的规则转换请求自带的思考强度，同时修复“当前 AIO 服务 Codex 网关”来源在长请求后被误记为自回环并失败的问题。

## Background

- CX2CC 当前会透传显式 `output_config.effort`，不会再使用旧的固定思考强度设置覆盖请求。
- GPT-5.6 Sol、Terra、Luna 支持 `max`，因此 `max` 不得降级；Claude/Codex 侧可能发送 `ultra`，默认需要转换为 `max`。
- 当前 AIO 网关来源使用一次性授权进行合法本机第二跳，但外层仍套用普通 Provider 的首字节超时与重试。第一次请求等待配置的 120 秒后超时，授权已消费，后续重试被记录为 `provider_target_self_loop`。
- 普通 Provider 指向本机网关仍属于错误配置，必须继续被回环保护拒绝。

## Requirements

### R1. 可配置思考强度映射

- CX2CC 设置页提供思考强度转换规则编辑器。
- 用户可以新增、修改和删除规则，并可一键恢复默认规则。
- 默认规则为：`low -> low`、`medium -> medium`、`high -> high`、`xhigh -> xhigh`、`max -> max`、`ultra -> max`。
- 映射按去除首尾空白后的精确字符串匹配；来源值不得为空或重复，目标值不得为空。
- 未匹配到规则的显式强度原样透传，以兼容未来新增值。
- 配置必须持久化、随完整配置备份导入导出，并由后端与前端共同校验。

### R2. 保持思考存在性语义

- 未提供 `thinking` 和 `output_config.effort` 时，不生成 `reasoning`。
- `thinking.type=enabled|adaptive` 且带显式 effort 时，仅转换该显式值。
- `thinking.type=enabled|adaptive` 且未带 effort 时，不猜测或注入默认强度。
- `thinking.type=disabled` 始终转换为 `reasoning.effort=none`，不受用户映射影响。
- 旧 `cx2cc_model_reasoning_effort` 字段继续只做兼容持久化，不恢复运行时固定覆盖。
- 请求日志继续显示最终实际发往上游的映射后强度。

### R3. 修复当前 AIO 网关合法第二跳

- 只有 CX2CC 明确选择“当前 AIO 服务 Codex 网关”时，才允许一次经过认证的本机第二跳。
- 外层 CX2CC 将第二跳视为委托：只发送一次，不重复套用普通 Provider 的首字节超时或同 Provider 重试。
- 内层 Codex 网关继续负责真实 Provider 的选择、超时、重试、故障转移、熔断和日志。
- 普通 Provider、本机伪造请求、错误方法/路径/查询、过期或重放授权仍被防回环机制拒绝。
- 不新增或依赖响应缓存；修复后不得再产生“先等待外层超时，再因同一授权已消费而自回环失败”的链路。

## Acceptance Criteria

- [x] 新安装和旧设置迁移后均得到完整默认映射，且 `max -> max`、`ultra -> max`。
- [x] 设置页可新增、修改、删除、保存规则，并可恢复默认；重复来源、空值、超长值和超量规则会被阻止。
- [x] 默认、自定义、删除后未命中和恢复默认均有前后端测试。
- [x] Anthropic 输入经过真实 CX2CC bridge 后，显式 effort 使用映射后的值；未知值原样、缺省仍缺省、disabled 始终为 none。
- [x] 最终请求日志观察到映射后的 outbound effort，而不是原始输入或旧固定设置。
- [x] 当前 AIO 网关第二跳在超过外层首字节超时阈值时不会被外层取消，也不会进行一次必败的自回环重试。
- [x] 内层真实 Provider 的超时/重试仍生效；普通自回环 Provider 的既有拒绝测试继续通过。
- [x] 设置 schema、生成绑定、配置备份和相关 UI/Rust 回归测试全部通过。

## Out Of Scope

- 不恢复固定的全局思考强度覆盖。
- 不改变 CX2CC 四槽模型映射、模型上下文自定义或通用 Provider 模型资格策略。
- 不放宽普通 Provider 的本机回环保护。
- 不增加模型响应缓存。
