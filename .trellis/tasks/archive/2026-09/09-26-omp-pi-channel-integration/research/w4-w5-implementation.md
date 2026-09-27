# W4/W5 网关上游与原生发布实施记录

日期：2026-09-26。负责人 dispatch `ctx_213e20abea0f`。本记录只覆盖本 worker 的后端实现；真实 CLI / AIO wire、数据库迁移、中央注册和前端由其他工作包分别验收。

## 已实现

- Provider 增加显式 `gateway_protocol`，Pi/OMP 仅接受四种约定协议及用户在 AIO 填写的字面 API key，不接受 OAuth 或桥接；旧四 CLI 保持 nullable 协议。查询、更新、复制和运行时重置判定已贯通，复制同时保留显式模型声明。
- `native_gateway_model_specs.metadata_json` 保存能力及 transport/model-policy binding；模型编辑验证 provider UUID，并以同一 DB 事务内的能力 revision 执行 CAS。上游 URL、协议或映射发生变化后，旧声明不能继续成为有效候选。
- 目录复用既有 active-mode provider 选择和单一模型映射，公开模型 ID 不另建映射层；容量取下限、模态/工具/思考能力取可证明交集。Pi 思考映射要求全七档，OMP 限真实 custom-model schema 可表达字段。未知工具能力可归档，但发布前必须明确确认。
- 候选必须覆盖同客户端/协议/公开模型在所有仍发布 target 的能力和全局模型策略 hash，包括尚未结算的 intent。零发布快照不许可转发；发布前另证明替换当前 target 后仍至少存在有效候选，其他 target 的旧约束不会被忽略。
- 原生导入按 snapshot revision 绑定预览/确认，按实际协议与 URL 分组；从不执行原生表达式、不读取原生认证存储、不复制源 key。每组凭证由用户明确填写，所有新供应商与声明在同一事务中创建且默认 disabled。动态 URL、额外 headers、不可表达 transport/metadata 等明确拒绝。原生价格不写入 AIO 价格策略，并显示对应预览提示。
- 一个 target 至多四个独立 AIO 协议节点；生成只使用白名单、实际本地监听地址及无效占位 key。OMP 显式 `auth: apiKey`、模型 `preferWebsockets: false`；不生成上游地址、上游秘密、自定义 headers、discovery 或其他旁路配置。
- 发布/撤回统一调用 native worker 的 target lock、snapshot、patch、compensate、digest API。使用 durable intent → node CAS → manifest finalize，DB/FS 部分失败仅条件补偿所属节点；不恢复整份文件。重试按 before/after digest 恢复未结算 intent，外改冲突保留记录并拒绝覆盖。撤回不依赖当前目录有效性或监听状态。
- 获取目标锁后复核 selected target，防止旧排队操作在目标切换成功后写入旧目录。原生默认项和其他供应商节点不变。共享 schema 不足以完整表达 Pi/OMP 时，单供应商分享明确返回 `PROVIDER_SHARE_NATIVE_UNSUPPORTED`。

## 对外接口

- `native_gateway_models_get(providerId, providerUuid)` → `GatewayModelsSnapshot`。
- `native_gateway_models_set(providerId, providerUuid, expectedRevision, models)` → 同一 snapshot。
- `native_gateway_catalog_preview(targetId)` → 目录、白名单生成预览、native revision、catalog revision、listener readiness 与 manifest 状态。
- `native_gateway_apply(input)` / `native_gateway_remove(input)`；input 为 `targetId / expectedRevision / catalogRevision`。
- `native_gateway_import_preview(targetId, nativeKey)`。
- `native_gateway_import_confirm(input)`；input 为 `targetId / nativeKey / expectedRevision / credentials[{groupId,apiKey}]`。
- Wire：`candidate_eligible(conn, providerId, sourceCli, protocol, requestModelId, &globalModelPolicy)`；requestModelId 必须是映射前公开 ID。
- Native：`managed_native_keys(db,targetId)` 返回所有状态下的精确托管 key。

## 检查状态

- 定向 `rustfmt` 和本包 `git diff --check` 已通过。
- 已编写协议验证/旧客户端/复制/共享回归，metadata 交集、全部发布快照、policy 与 provider binding、身份/CAS、导入事务/秘密边界、发布幂等/碰撞/外改/撤回/故障补偿及跨 target 候选证明测试。
- Rust 统一构建由协调者/native worker 串行完成，本 worker 使用最后写入时间为 2026-09-26 09:01:46 UTC 的 `aio_coding_hub_lib-25a5c65b4b903819.exe` 直接执行定向回归，避免争抢 cargo 锁：`native_gateway` 22/22、`domain::providers` 132/132、`app::provider_service` 5/5 全部通过（合计 159 项，0 失败；native_gateway 包含一项 portable bundle 拒绝回归）。
- 初轮测试发现 fixture 未显式加入 Default route、测试持有唯一 DB 连接后再次调用服务；已仅修正 fixture membership/连接作用域，未放松生产候选筛选。最新程序覆盖这些修复和 selected-target 锁后复核，复测全部通过。
- 执行摘要位于 `.trellis/.runtime/research/omp-pi/w4-w5-verified-suites.log`；全项目构建、前端及真实 AIO wire 验收仍由协调者汇总。
- W0 已核实固定版本四协议以及工具/图像/thinking 原生字段；这不替代 W3 的真实 CLI 经 AIO 端到端验收。

## 语义边界

- apply 成功只说明生成节点及 manifest 完成，并非实际请求连通性证明；前端须区分节点、监听器和真实观察流量。
- 一个旧 target 仍发布更强能力或旧全局策略时，可能阻止较弱候选或新 target 发布；应明确撤回或更新该旧入口。
- 不支持的原生能力/协议/自定义传输明确拒绝导入或发布，不静默删除语义后伪装成功。
- 未修改 gateway/native_cli 实现、DB migrations/settings、中央注册或前端，未提交/推送，未改全局 CLI 安装。
