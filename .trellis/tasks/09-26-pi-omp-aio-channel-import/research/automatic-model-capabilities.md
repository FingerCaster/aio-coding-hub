# 自动发现与下拉编辑验收补充

日期：2026-09-26。用户在测试包验收中提出，已直接授权实施。

## 实现

- 新增 `native_channel_models_discover(targetId, providerId, providerUuid, protocol)`，复用已有 15 秒、8 MiB、无重定向模型发现请求；不增加推理请求、不刷新 OAuth、不写入模型库。
- 同一目录响应保留白名单能力字段：显示名、输入类型、上下文、最大输出、工具与推理布尔值、思考等级与默认值等。未知值为 null；不根据模型名字或“并行工具”字段推断工具支持。重复 ID 的冲突能力不作为自动填写依据。
- 来源中非过期、明确配置的模型容量和思考等级优先；来源配置与 API Key 在同一数据库快照读取。网络期间不持目标锁/事务，返回前复核目标、UUID、协议、声明、凭证/账号与来源能力快照。
- 模型路由匹配复用网关现有规则，命中模型或思考改写时仅保留候选 ID，并要求核对；普通推理/网关行为未改变。全局路由在发现前后核对。
- 新模型选择器自动填入已知能力；手动字段（包括清空操作）、高级 JSON、已保存声明保持优先。发现缓存与声明查询独立，保存无需再等待一次上游发现。
- Pi 七档映射、OMP 五种有效思考模式、等级映射和默认等级使用表单；移除无效的 `thought` 模板。前端校验与后端约束对齐。
- 已有模型也可以从上游详情进入管理，无需先删除声明。Gemini OAuth 资格限制保持原状。

## 验证证据

- 前端全量及后端全量结果见 `research/validation.md` 本轮记录。
- `NativeChannelDiscovery.test.tsx`：自动加载、完整 OMP 保存、Pi 七档、手动/清空保护、JSON 保留、失败保留、换目标、换模型 ID、revision 隔离、手动下拉与无效规格。
- `native_channel_discovery`：只读快照、UUID/协议、密钥轮换、非过期配置、来源能力变化、Gemini OAuth 阻断、路由核对。
- `provider_model_discovery::metadata`：Codex 等级、Gemini 容量、未知/冲突字段；既有网络发现测试覆盖超时、凭证、重定向等边界。
- Edge + Playwright 使用真实 React 组件及本地 IPC fixture，Pi 浅色/OMP 深色、宽窄窗口、自动填入、刷新保护、焦点与 Escape 均通过。截图已人工目视检查：`.trellis/.runtime/research/omp-pi/channel-ui/automatic-capabilities/`。
- 所有本轮发现验证使用本地 fixture，没有读取真实账号凭证或进行付费推理。

## 交付边界

旧目录 `.local/test-builds/pi-omp-20260926-212827/` 不包含本改进。新包路径、源码指纹和校验结果在构建完成后记录于验证报告；用户体验验收仍为 pending。

交付完成：D:/OrcaProjects/aio-coding-hub-fork/omp-pi-channel-integration/.local/test-builds/pi-omp-20260926-224335，MSI 与免安装 ZIP 均包含本轮自动获取改进。前端 3024 项、Rust 3186 项通过（6 ignored），最终相关 30 项通过。验证记录已补齐，用户体验验收仍为 pending。
