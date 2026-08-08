# 自定义 JavaScript 账户用量查询

## Goal

为没有内置 sub2api/NewAPI 查询契约的直接 API Key 供应商提供受约束的本地 JavaScript 账户用量适配器，并建立后端拥有的共享账户用量快照，使桌面展示和后续路由门控读取同一份有界、可失效的数据。

## Background

- 当前账户用量结果、前端展示和 Provider 扩展配置已经存在，但查询能力只覆盖内置适配器。
- KNaiFen 提交 `c75150897145420d630f9927519493f154032227` 提供了完整的脚本请求/解析、安全确认、QuickJS 子进程、配置净化和测试基线；后续提交 `3cc35e4920b98f7f29cbe575cfb04c542ec3f95d` 将账户用量提升为后端共享运行时。
- 当前主线与该功能的账户用量核心代码同源，但 Provider、IPC、分享、配置迁移和生成绑定已经演进，必须选择性移植并重新生成绑定，不能原样 cherry-pick。

## Requirements

### R1. 脚本契约

- JavaScript 源码求值为一个对象，提供同步 `request(ctx)` 和 `parse(response)` 两个函数。
- `request(ctx)` 只能返回一个 GET/POST 请求计划；脚本运行时接收 API Key 与 Base URL 的不透明占位符。父进程只替换最终有界请求计划中精确、完整的占位符出现位置；脚本变换后的值不参与替换，明文永不进入 worker。
- `parse(response)` 只接收成功 HTTP 状态和经过大小限制、JSON 解析及精确敏感值清理的响应数据，不接收凭据、请求头、请求 URL 或运行上下文。
- 输出只允许映射到现有账户用量 DTO 字段和稳定状态；非法类型、非有限数字、非整数过期时间、超长文本或失败状态夹带金额时失败关闭。

### R2. 执行与网络边界

- `request` 与 `parse` 分别在一次性子进程中的独立 QuickJS 运行时执行，具备明确的内存、栈、引擎执行、父进程调用和启动上限。
- 运行时不暴露 fetch、文件、模块、进程、环境、计时器、WebSocket 或 Tauri IPC；父进程在超时或协议错误时终止子进程，并对 terminate/reap 自身设置硬上限。
- 最多并行执行四个完整自定义查询工作流，超过上限立即返回稳定 busy 状态，不形成无界队列。
- 请求只允许 HTTPS，目标 Origin 必须等于当前 Base URL Origin 或用户确认的额外 Origin；禁止 userinfo、重定向、Cookie/Set-Cookie、Host、代理认证及逐跳头，并限制 URL、Header、Body、响应与输出大小。

### R3. 权限与本地配置

- 自定义适配器只对直接 API Key Provider 可用；OAuth Provider 必须强制保持禁用。
- 脚本、规范化额外 Origin 集和 Base URL Origin 共同形成授权边界。启用、草稿测试或授权边界改变时，由后端弹出原生确认并列出完整 Origin 和权限指纹。
- 原生确认串行单飞；确认取消、关闭或失败时不得读取明文 API Key 或发送请求。
- Renderer 提交的权限 proof 一律丢弃；只有后端可在确认后注入瞬时 proof，持久化仅保存派生指纹与已确认 Base URL Origin。
- API Key 单独轮换不撤销脚本/目标授权；Provider 身份、认证模式、来源 Provider 或 Base URL Origin 变化必须使旧授权及旧异步结果失效。
- 脚本、额外 Origin、启用状态及授权元数据保持本机私有。Provider 分享、完整配置导出/备份和导入统一输出禁用适配器，并拒绝导入载荷中的自定义字段。
- 便携载荷不得保留“禁用但可恢复”的脚本源码或额外 Origin；换机与备份恢复后必须重新录入脚本、重新选择目标 Origin 并完成原生确认。
- 本机 Provider 复制不是便携导出：可以复制脚本草稿和启用意图，但新 Provider 身份不得继承授权。源 custom 已启用时，复制流程必须为新身份重新原生确认，取消确认则不创建副本；源 custom 未启用时可直接复制草稿并保持禁用。

### R4. 共享账户用量运行时

- Tauri 管理的运行时拥有 Provider 级结果缓存、远端刷新时序、同 Provider in-flight 合并和进程内单调配置 generation；React Query 只拥有前端镜像，不拥有远端刷新时序。
- 桌面消费者使用独立的有界租约续期，不通过复制旧提交的 React Query 轮询来维持租约；没有消费者时停止普通定时刷新。手动刷新、首次查询和硬过期刷新保持可用。
- 相同 Provider 同时只允许一个远端查询。in-flight 期间收到的显式强刷请求必须合并为当前请求之后至多一次尾随刷新，且调用者等待该尾随结果；不能把强刷悄悄折叠为旧完成。
- 不同 Provider 同时最多四个远端查询；scheduler 只在取得 permit 后启动任务，并以每 Provider 一个合并 due/force 位表达待办，不得创建任意数量等待 semaphore 的任务。
- generation 使用运行时 checked-increment，不依赖秒级数据库 `updated_at`。每条 Provider mutation 比较查询语义与凭据身份：Base URL/auth/source/API Key、adapter/mode/interval/custom 授权等变化必须 invalidate，delete/disable 与配置导入成功提交/reset 必须清理；完整导入复用 Provider ID 时旧结果仍不得提交。name/note 与仅展示的 `timedRefreshEnabled` 变化不得无故丢弃有效结果。
- 共享快照包含结果状态、新鲜度、获取时间以及显示所需的非敏感金额字段，不包含 API Key、脚本源、Origin、请求/响应或上游错误文本。
- 运行时提供 generation 与非敏感配置指纹校验后的后端只读快照接口；快照同时保存单调完成时刻和仅供显示的墙钟获取时间。后续消费者不得另建远端缓存，本任务不接入任何路由消费者。
- IPC 将“读取当前快照”和“触发远端刷新”分开；手动刷新不借用 TanStack Query `meta` 传递命令语义，现有 exact-key cancellation 与逆序完成保护继续有效。
- 本子任务保持账户用量纯展示，不修改路由、Provider 可用性、熔断、Session、顺序或启用状态；后续路由子任务只能通过明确的运行时读取接口消费快照。

## Acceptance Criteria

- [x] 用户确认后，自定义适配器能完成一次受限请求并显示归一化余额、套餐剩余、周期额度或过期状态。
- [x] 脚本语法/运行错误、死循环、原生内建阻塞、子进程异常、输出超限和并发超限均在有界时间内失败；终止与回收本身也有硬上限且不遗留子进程。
- [x] HTTP、Origin、重定向、Cookie/Set-Cookie/Header、请求/响应大小、占位符精确替换、JSON 和状态映射的正反路径均有合成测试。
- [x] 测试或启用前取消原生确认不会加载凭据；确认期间配置变化会拒绝陈旧执行。
- [x] 脚本或 Origin 变化撤销旧确认；仅 API Key 轮换仍使用当前密钥和原授权目标。
- [x] 分享、完整导出、备份和导入不包含脚本、额外 Origin 或授权材料，也不会激活自定义适配器。
- [x] 手工构造的导入载荷无法保留自定义源码或 Origin；导入后必须从无脚本的禁用状态重新配置。
- [x] 本机复制保留自定义脚本草稿但不继承授权；源 custom 已启用时只有新确认成功才创建副本，取消为零写入，源 custom 未启用时副本保持禁用。
- [x] 桌面初始、定时和手动读取同一 Provider 时共享后端请求和结果；in-flight 强刷产生至多一次尾随刷新，旧 generation 结果不能覆盖新配置。
- [x] 四个 Provider 并发上限不会形成无界等待任务；后端快照只返回当前 generation/配置指纹的归一化结果，不存在、失效或锁不可读时不返回陈旧快照。
- [x] 秒内连续保存、API Key 轮换、enable/disable 和完整导入复用 Provider ID 都能拒绝旧完成；name/note 与 display timed 开关变化不误清有效结果。
- [x] 现有 sub2api、NewAPI、手动刷新、缓存清理和展示测试保持通过。
- [x] 生成绑定由 Rust 重建，前端类型、日志脱敏、Rust 测试、TypeScript、lint、格式和敏感信息审计通过。

## Out of Scope

- 多请求、循环翻页或网页登录脚本。
- Cookie、DOM、浏览器会话或任意系统能力。
- 让脚本修改 Provider、路由、网关请求或响应。
- 在本子任务中根据余额状态跳过 Provider。
