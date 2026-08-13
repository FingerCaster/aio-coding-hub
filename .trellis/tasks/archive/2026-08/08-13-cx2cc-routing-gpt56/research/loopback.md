# CX2CC 本机 Codex 网关回环误判调研

## 基线与范围

- 当前分支：`81fd6d0860d1a6cc8c053f42d8aa941a0a445e96`
- `upstream/main`：`7725effd33ab9d7e1e8c4f9b5bb30c6e5a0ff23e`
- 共同祖先：`4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`
- 本报告只调研 Provider 保存、CX2CC 转译、本机 Codex 网关再入口、Provider
  target 校验和递归标记；不修改业务代码。
- 行号均指当前分支，明确写出 `upstream/main:` 的除外。

## 结论

### 已确认事实

1. 用户选择“当前 AIO 服务 Codex 网关（跟随当前分流）”后，保存结果是一个
   `bridge_type="cx2cc"`、`source_provider_id=NULL`、空 `base_urls` 的 Claude
   Provider。这不是普通 Provider 指向自身，而是项目自 2026-04-11 起明确支持的
   内部协议再入口。
2. 运行时会把 Anthropic 请求转成 OpenAI Responses 请求，并由运行中 Gateway
   状态生成 `<当前 base_url>/v1/responses`。该请求本应通过 `/v1/*path` 以
   `cli_key="codex"` 再进入 Gateway，然后使用当前 Codex 分流。
3. 当前 fork 在每次 Provider 发送前无条件调用 `validate_gateway_target`。它只知道
   最终 URL，不知道 URL 是 CX2CC 有意生成的内部入口，因此将上述 URL 判为
   `SelfLoop`，在网络发送前以 `provider_target_self_loop` 健康中性跳过。请求实际
   从未进入 Codex 路由；这才是本次误判的直接根因。
4. `x-aio-gateway-forwarded: aio-coding-hub` 的旧递归中间件不是这次误判的直接
   根因：当前生产发送链已经不再注入该头，而且它只拦截 `cli_key="claude"`；合法
   再入口目标是 `cli_key="codex"`。该守卫目前是残留的、可被客户端伪造的固定值
   判断，不能作为放行能力或完整递归防线。
5. 目标 URL/DNS 自环校验是 fork 特有加固；pinned `upstream/main` 没有
   `validate_gateway_target`。上游仍保留相同的 CX2CC 本机网关入口。因此不能简单
   删除 fork 校验来“对齐上游”，否则会恢复真实 Provider 自引用风险。

### 建议摘要

保留普通 Provider 的严格 URL/DNS 校验；给准备结果增加运行时生成的、类型化的
`InternalCodexReentry` 意图，只允许由 CX2CC `source_provider_id=None` 分支生成的
当前 Gateway authority、`POST /v1/responses` 通过一次。同时使用每进程随机密钥
认证的路由谱系标记，入站原子消费、出站在插件之后注入，第二次进入同一 Gateway
实例必须失败关闭。不能按 `localhost`、相同端口、固定请求头或数据库布尔值宽泛
放行。

## 一、保存链路

### 1. 前端语义

1. `src/pages/providers/Cx2ccSection.tsx:71-96` 渲染来源选择；特殊项位于
   `:84-86`，值来自 `CX2CC_GLOBAL_SOURCE_VALUE`，文本正是“当前 AIO 服务 Codex
   网关（跟随当前分流）”。显式 Provider 列表只展示启用且本身非 bridge 的直接
   Codex Provider（`:87-95`）。
2. `src/pages/providers/providerEditorUtils.ts:31` 将特殊值定义为
   `__codex_gateway__`；`deriveCx2ccSourceValue` 在 `:177-187` 把
   `bridge_type="cx2cc"` 且无 source ID 的已保存 Provider 反向映射回该特殊值。
3. UI 对这个选择的契约不是“直连某个 Provider”。
   `src/pages/providers/Cx2ccSection.tsx:155-195` 展示当前 Gateway base URL、App
   Token、免费倍率，并在 `:195` 明确写明：转译后进入当前 Codex Gateway，再按
   当前 Codex 分流继续路由。

### 2. 保存载荷

`buildProviderEditorUpsertInput` 的关键行为如下：

- `src/pages/providers/providerEditorSubmitModel.ts:94-120`：CX2CC 清空
  `baseUrls`，且显式 Provider ID 与特殊 Gateway 来源二选一。
- `:148-154`：特殊 Gateway 来源强制 `costMultiplier=0`，bridge 类型为
  `cx2cc`。
- `:156-198`：最终 `authMode` 落为 `api_key`、`apiKey=null`；最关键的是
  `:191-195` 将特殊来源保存为 `sourceProviderId=null`、`bridgeType="cx2cc"`。
- `src/pages/providers/providerEditorSaveRunner.ts:11-24,48-51`：保存动作构造该
  载荷后调用 `persistProvider`。
- `src/services/providers/providers.ts:279-309`：服务层调用生成的
  `providerUpsert` IPC。
- `src-tauri/src/commands/providers/crud.rs:18-26`：IPC 薄层进入
  `provider_service::provider_upsert`。
- `src-tauri/src/app/provider_service.rs:386-470`：服务层原样拆出
  `source_provider_id`/`bridge_type`，最终调用 `providers::upsert_with_provider_uuid`。

现有前端测试只证明保存语义：

- `src/pages/providers/__tests__/providerEditorSubmitModel.test.ts:83-103` 断言特殊来源
  得到零倍率、`bridgeType="cx2cc"`、`sourceProviderId=null`。
- `src/pages/providers/__tests__/ProviderEditorDialog.test.tsx:1048-1127` 覆盖 UI 选择与
  保存载荷，但没有发起 Gateway 请求。

### 3. 后端静态图校验

`src-tauri/src/domain/providers/queries.rs` 的 upsert 校验已经区分显式 source 图：

- `:1654-1666`：拒绝未知 bridge 类型，以及“有 source、无 bridge”。
- `:1671-1680`：编辑时拒绝 `source_provider_id` 直接等于自身 ID。
- `:1682-1719`：source 必须存在，且 CX2CC source 必须属于 Codex CLI；映射函数
  `source_cli_key_for_bridge_type` 位于 `:1250-1256`。
- `:1720-1731`：source 必须启用，且不能再带 `source_provider_id` 或
  `bridge_type`。因此经受支持的 upsert 路径不能保存 A -> B -> C bridge 链或
  A -> B -> A Provider-ID 环。
- `:1736-1749`：CX2CC 只允许挂在 Claude 上，并强制 bridge Provider 的
  `base_urls` 为空。
- 运行时再次读取显式 source 时，SQL 在 `:1258-1326` 要求它启用、无 source、无
  bridge。

因此 `bridge_type="cx2cc" + source_provider_id=NULL` 是有意保留的另一种语义，
不是遗漏了直接 self-reference 校验。对该记录做普通 Provider 图环检测不会发现
任何边，因为“当前 Gateway”不是 providers 表中的一行。

## 二、运行时复现与根因

### 1. 预期数据流

```text
Claude 入站
  -> 选择 CX2CC bridge Provider
  -> Anthropic -> OpenAI Responses 转译
  -> 当前 Gateway /v1/responses
  -> routes.rs 将 /v1/* 映射为 cli_key=codex
  -> 当前 Codex 分流选择实际 Provider
  -> 响应转回 Anthropic
```

路由契约证据：`src-tauri/src/gateway/routes.rs:75-90` 将 `/v1/*path` 交给
`proxy_impl(..., "codex", ...)`，路由注册在 `:103-118`。

### 2. 实际数据流

1. `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_iterator.rs:258-293`
   识别 CX2CC，并把 `provider.source_provider_id` 交给 `cx2cc_preparation::prepare`
   （`:264-276`）。
2. 无 source ID 时，
   `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:160-180`
   读取 `app_gateway_status(...).base_url`，生成 `<base_url>/v1`，source 设为
   `None`，source CLI 语义设为 `codex`。
3. 这个 base URL 来自真实监听状态：
   `src-tauri/src/gateway/control_service.rs:46-79` 绑定端口并在 `:58-62` 生成
   `http://<base_host>:<实际端口>`；`:390-411` 用相同端口、bind host 和 base host
   同步 HTTP client 的 self-check context。
4. 协议桥 target 是固定的 `/v1/responses`：
   `src-tauri/src/gateway/proxy/protocol_bridge/outbound/openai_responses.rs:36-43`。
   准备结果在 `cx2cc_preparation.rs:228-242,325-337` 携带该 target path 与 Gateway
   base URL。
5. `provider_iterator.rs:278-288` 把准备结果写入 `PreparedProvider`。但本机来源的
   `cx2cc_source=None` 又被复制成 `bridge_source=None`（`:282-283`）。
   `PreparedProvider` 只有 `cx2cc_active`、`active_bridge_type` 和可空 source
   （`:14-52`），没有“这是经授权的本机 Codex 再入口”这一目标类型，语义在发送层
   前已经丢失。
6. `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/attempt_executor.rs:610-617`
   经 `build_target_url` 组合为当前 Gateway 的 `/v1/responses`。路径去重规则见
   `src-tauri/src/gateway/util.rs:497-532`，所以 `<base>/v1 + /v1/responses` 最终仍是
   `<base>/v1/responses`。
7. `attempt_executor.rs:370-383` 在发送前无条件调用 `validate_gateway_target`。
   `src-tauri/src/gateway/http_client.rs:1033-1053` 发现有效端口等于 Gateway 端口且
   host 在本机 context 中，立即返回 `SelfLoop`。因为 URL 正是实时状态生成的同一
   authority，这个误判是确定性的，不是竞态。
8. `attempt_executor.rs:416-445` 在任何网络调用前返回
   `ProviderTargetRejected`。真正的发送只在 `:472-502` 发生，因此 `/v1/responses`
   从未重入。
9. `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/retry_engine.rs:180-200`
   将它记录为 `provider_target_self_loop`、`target_validation`、健康中性 skip，然后
   切下一个 Provider。原因码定义在 `src-tauri/src/gateway/events.rs:44-47`。

### 3. 地址识别边界

self-check context 的构造覆盖范围较完整：

- `src-tauri/src/gateway/http_client.rs:858-894` 收集 `localhost`、`127.0.0.1`、
  `::1`、本机网卡 IP、hostname、`HOSTNAME`/`COMPUTERNAME`。
- `:897-939` 把 wildcard listen 地址扩展为全部本机 token，并把 loopback 别名互相
  展开。
- `:941-958` 统一大小写、方括号 IP，并把 IPv4-mapped IPv6 归一为 IPv4。
- `:1055-1112` 仅在有效端口等于 Gateway 端口时做 DNS 校验；同端口 hostname
  解析失败会失败关闭。
- `:1114-1135` 只要 DNS 结果中有一个本机地址就拒绝；空结果或超过地址上限也
  失败关闭。

现有测试 `http_client.rs:1416-1469` 证明同端口的 `127.0.0.1`、大小写
`localhost`、`::1`、`::ffff:127.0.0.1`、wildcard 和 LAN 本机地址会被识别，
而不同端口与远端地址会放行。`:1486-1522` 覆盖 `localhost` DNS alias 与不同端口；
`:1524-1560` 覆盖混合本地/远端 DNS、空结果和过多结果的失败关闭。本次调研实际
运行：

```text
cargo test test_gateway_target_detection_covers_ipv4_ipv6_and_remote_addresses --lib
1 passed; 0 failed
```

这证明地址分类按现有契约工作；问题是它缺少目标来源/路由意图，而不是
IPv4/IPv6 归一错误。

## 三、场景分类

| 场景 | 保存时 | 当前运行时 | 正确语义 |
| --- | --- | --- | --- |
| CX2CC 当前 AIO Codex Gateway | 保存为 CX2CC + null source | 同 host/port 必然被 target validator 拒绝 | 只允许一次 Claude -> Codex 内部再入口 |
| Provider `source_provider_id == self.id` | `queries.rs:1671-1680` 拒绝 | 不应到达运行时 | 拒绝 |
| A -> B -> C 或 A -> B -> A 的 bridge ID 图 | B 作为 source 时因它本身是 bridge 被 `:1725-1731` 拒绝 | 受支持写入路径不可达 | 拒绝；导入/迁移也必须走同一校验 |
| 普通 Provider URL 直接指向当前 Gateway | URL 语法可保存 | `validate_gateway_target` 拒绝 | 拒绝并保持健康中性 |
| 同端口 `localhost`/IPv4/IPv6/DNS alias | URL 语法可保存 | 归一或 DNS 后拒绝 | 除类型化内部入口外均拒绝 |
| 本机不同端口的真实上游 | 可保存 | `http_client.rs:1059-1061` 直接允许 | 允许，不能仅因“本机”误伤 |
| 本机不同端口代理回当前 Gateway | 可保存 | 首跳允许；当前无可靠端到端谱系 | 第二次进入原 Gateway 时拒绝 |
| 远端地址使用与 Gateway 相同端口 | 可保存 | 非本机则允许 | 允许 |
| 远端或不同端口返回 3xx 到当前 Gateway | 可保存 | 只校验初始 URL，自动 redirect 可绕开 target gate | 每个 redirect hop 重验，回本机则拒绝 |
| 客户端伪造旧固定头 | 不涉及保存 | Claude 请求会被旧中间件返回 508 | 固定公开值不能授予放行能力 |

### 多 hop 与 redirect 的已确认缺口

`gateway_client_builder` 在 `src-tauri/src/gateway/http_client.rs:967-975` 没有设置
redirect policy。项目锁定 `reqwest 0.12.28`（`src-tauri/Cargo.toml:44`）；该版本
`reqwest-0.12.28/src/redirect.rs:3-5,21-26` 的默认行为是自动跟随最多 10 hop。
当前 target validation 只在 `attempt_executor.rs:370-383` 对初始 URL 执行一次。
所以 3xx 目标和不同端口反向代理不能靠现有“首跳 host + port”证明安全。

不同端口本身必须继续允许；从地址静态推断“该端口以后会不会转回本机”既不可靠，
也会误伤合法本机服务。要覆盖 multi-hop，必须验证每个可见 redirect target，并在
请求中携带可认证的 Gateway 访问谱系。若一个不受控中间节点主动删除所有谱系头，
纯应用层无法证明它之后不会回流；这是方案需要明确记录的信任边界。

## 四、两套保护的历史与原意

### 1. 旧固定头递归守卫

| 提交 | 变化与原意 |
| --- | --- |
| `107d89202defcd4bbb8727b15ec88bcf6bfe49e0`（2026-03-29） | 引入 Claude 请求观察，同时增加 `mark_internal_forwarded_request` 和入站判断。提交说明明确是阻止“由内部转发标记的 Claude 请求造成递归代理”。当时每个 Claude outbound 都会注入固定头。 |
| `7f00bdc872106e32ee1476b136ac1c42cc4927b8`（2026-04-10） | 将守卫拆入 middleware chain。 |
| `07e309c4c8ec92bc273faca601b34f305ad6bd05`（2026-06-03） | 增加内部转发请求不记录的测试。 |
| `5f1625707dd1df85de460463e2901864080eb7c6`（2026-06-03） | 明确删除 `mark_internal_forwarded_request` 及 attempt executor 调用，但保留识别函数和测试。 |

当前残留位置：

- 常量与识别：`src-tauri/src/gateway/proxy/mod.rs:44-45,103-111`。
- 只拦 Claude、返回 508：
  `src-tauri/src/gateway/proxy/handler/middleware/recursion_guard.rs:1-45`。
- 它是 middleware 第一关：`src-tauri/src/gateway/proxy/handler/mod.rs:215-220`。
- 全仓生产代码没有固定头发送者；命中只剩识别、测试、插件 spoof 用例和 fingerprint
  过滤。`src-tauri/src/gateway/routes.rs:11169` 的 route test 是手工注入该头。

因此旧守卫目前既不能发现 Codex multi-hop，也不应被改造成“有这个固定值就允许”
的能力。任何客户端都能复制公开字符串。

### 2. Provider target 自环校验

| 提交 | 变化与原意 |
| --- | --- |
| `a174fc24a07a4a0ac5acd7f97cbc846946d2711b`（2026-08-09） | `fix(gateway): enforce provider routing safety`，在每次 transport send 前增加最终 Provider URL/DNS 自环拒绝与健康中性 skip。 |
| `43e7380f152da83b40c941e86d630488a2f1ec37`（2026-08-09） | `fix(gateway): harden provider routing safety checks`，增加有界 DNS、缓存、并发限制及 DNS pinning 等。 |

原任务证据很明确：

- `.trellis/tasks/archive/2026-08/08-09-provider-routing-master-switch-self-loop/prd.md:11-19`
  认为旧 attempt executor 只有代理递归保护，缺少“每次 Provider send 前”的最终
  URL/DNS 自环验证；要求失败健康中性。
- 同任务 `design.md:5-16` 把 validator 限定为地址安全判断，放在 transport commit
  前，并要求无法安全判断时失败关闭。
- 当前契约
  `.trellis/spec/aio-coding-hub/cross-layer/gateway-failover-route-contract.md:148-160`
  固定了同端口、本机 host/DNS、解析失败、DNS pinning、健康中性语义。

原意是正确的：防止用户配置的任意 Provider URL 指回当前 Gateway。缺陷来自该
地址层安全判断后来遇到了一个本来就合法、但没有把“内部再入口”意图传到发送层的
CX2CC 路径。

## 五、upstream 对比与分类

### 可审计差异

| 分类 | 结论 | 证据 |
| --- | --- | --- |
| 两侧已有 | CX2CC 使用当前 Codex Gateway | `2bf7117585f20e03971831a04bd721fb6f620d67`（2026-04-11，`feat: 支持 CX2CC 使用当前 AIO 服务 Codex 网关作为来源 (#194)`）是 HEAD 和 upstream 的共同祖先。当前 `cx2cc_preparation.rs:160-180` 与 `upstream/main:...:158-176` 都生成当前 Gateway `/v1`。 |
| 两侧已有 | 固定头 Claude recursion middleware | `107d8920`、`5f162570` 均为两侧祖先；`upstream/main:src-tauri/src/gateway/proxy/mod.rs:40-41,99-107` 和 `upstream/main:.../recursion_guard.rs:1-45` 与当前语义相同，也没有生产发送者。 |
| fork 特有 | Provider target URL/DNS validator | `a174fc24`、`43e7380f` 是 HEAD 祖先但不是 `upstream/main` 祖先；`git grep` 在 upstream 找不到 `validate_gateway_target` 或 `GatewayTargetValidationError`。 |
| upstream 可移植 | 无现成回环修复 | upstream 没有类型化内部目标、可信 hop marker 或 redirect hop 校验，不能从 pinned upstream cherry-pick 本问题修复。 |
| 不应移植 | 删除 fork target guard | upstream 缺少该加固不是一个可接受的“修复”；会破坏真实 self-reference 的失败关闭及现有 fork 契约。 |

这也纠正了把问题归因于 `recursion_guard.rs` 的表面结论：即使给该 Claude
middleware 加 `source_id.is_none()` 白名单，也无法修复发送前
`validate_gateway_target` 的拒绝；middleware 在第二次 HTTP 入站才有机会运行，而
当前请求在第一次发送前已经被拦下。

## 六、失败关闭且不误伤的方案

### 方案 A：类型化目标意图（必要）

在 CX2CC 准备结果和 `PreparedProvider` 中引入不可由用户 URL 直接构造的运行时
类型，例如：

```rust
enum GatewayTargetIntent {
    ExternalProvider,
    InternalCodexReentry {
        bridge_provider_id: i64,
        route_id: RouteId,
    },
}
```

生成规则必须同时满足：

1. 当前选中 Provider 的有效 `bridge_type` 是 `cx2cc`；
2. `source_provider_id` 为 `None`；
3. source CLI 语义固定为 `codex`；
4. authority 直接来自当前运行中 `GatewayStatus.base_url`，不是 DB/base URL/插件输入；
5. method 为 `POST`，最终 path 严格为 `/v1/responses`，query 为空；
6. transition 严格为 `claude -> codex`。

发送边界行为：

- `ExternalProvider` 继续无条件走现有 URL/DNS validator；显式 CX2CC source 也是
  External，不享有例外。
- `InternalCodexReentry` 仍先验证上述全部不变量和实时 Gateway authority，只对这
  一次精确 target 接受 `SelfLoop`；任何不一致都返回结构化、健康中性的本地拒绝。
- 不要把 `source_provider_id=None`、`cx2cc_active`、`localhost` 或“端口相同”中的
  任意单项当作白名单；这些条件单独都不足以证明目标来源。
- 不要持久化 `allow_gateway_loop=true`。运行端口可变化，导入/手工 DB 数据也不应
  获得传输能力；意图必须在每个请求中由受信准备分支重新生成。

### 方案 B：可信路由谱系（必要，用于真实递归）

类型化意图只解决第一次合法再入口。为拒绝第二次及 multi-hop，建议替换固定公开
头为每 Gateway 实例可认证的谱系：

1. Gateway 启动时生成随机 `instance_id` 和 CSPRNG secret，保存在 runtime state，
   不进入设置、日志、fingerprint 或插件输入。
2. 对合法 `InternalCodexReentry` 生成短 TTL、一次性 capability，至少绑定
   `instance_id`、`route_id`、原 trace、bridge Provider ID、`claude -> codex`、
   method、path、目标 authority 和剩余 hop budget。可使用 HMAC，或 256-bit 随机
   token + 有界服务端状态表。
3. capability 头在 request-before-send 插件之后、transport send 之前注入；入站在
   插件和日志观察之前验证并移除，防止插件伪造、泄漏或回显。合法首次 Codex 入站
   原子消费 token，并把已访问的 instance/route 写入请求上下文。
4. 从该 Codex 请求继续发往真实 Provider 时，附带只用于“已访问”判定的签名谱系。
   若请求经同端口、不同端口代理、redirect 或另一 Gateway 后回到本实例，本实例
   验证到自己的谱系条目后返回 508/结构化 loop error；不得再次授权 CX2CC transition。
5. 外部客户端伪造、篡改、过期、重复使用、错误 path/CLI/method/authority 的 token
   全部失败关闭。未知的外实例谱系只能在严格长度/hop 上限内保留，不能授予本实例
   放行；声称属于当前 instance 但签名无效必须拒绝。
6. token 不含密钥和用户内容；服务端状态、header 长度、总 hop 数和 TTL 必须有界。

固定的 `x-aio-gateway-forwarded: aio-coding-hub` 最多保留为兼容期“拒绝提示”，不能
用于授权。观察/日志抑制应改看已验证的内部 request context，确保外层 Claude 请求
只产生一条用户可见生命周期，而不是信任可伪造头。

### 方案 C：逐 hop target 校验（必要，用于 redirect）

当前 reqwest 自动 redirect 绕过异步 DNS target validator。要宣称失败关闭，需要：

- 对 Gateway Provider transport 禁止库自动 redirect，并在受控的有界循环中解析
  `Location`，每一 hop 重新执行同一 URL/DNS/intent 校验；或提供等价的逐 hop
  异步验证机制。
- 保持现有 redirect 次数、method/body 和敏感 header 语义，避免普通 Provider
  行为回归；跨 origin 不得盲目转发 auth。
- redirect 到当前 Gateway 只允许仍满足尚未消费的精确 InternalCodexReentry；普通
  Provider 的 3xx self target 必须拒绝。

仅设置同步 `redirect::Policy::custom` 不足以复用当前异步 DNS 失败关闭逻辑；仅依赖
自定义 header 也不能覆盖会删除 header 的不受控代理。

### 更强但范围更大的替代方案

将“跟随当前 Codex 分流”实现为进程内 typed dispatch，直接把已转译 request 交给
Codex provider selection，并通过 request extensions 携带 route lineage，可以避免
第一次真实 HTTP self-call 和 hostname/端口歧义。这在结构上最清晰，但必须重新审计
嵌套 failover、流式响应转换、取消、用量、日志和 Session 归属，实施面明显大于
类型化一次性 capability。可作为后续演进，不应在未补齐这些契约时仓促替换。

## 七、回归测试设计

### 现有可复用基线

1. 保留并扩展 `http_client.rs:1416-1560` 的 IPv4/localhost/IPv6/DNS 正反矩阵。
2. 保留 `src-tauri/src/gateway/routes.rs:4492-4583`
   `provider_self_loop_switches_without_circuit_or_session_pollution`：普通 Codex Provider
   指向当前 Gateway 时零 self-loop 网络发送、切到 fallback、不增加 circuit failure，
   Session 只绑定成功 fallback。
3. 现有 CX2CC route helper
   `routes.rs:1335-1375` 强制要求一个 `i64 source_provider_id`，现有 route 测试因而只
   覆盖显式 Codex source；没有覆盖 null source 的本机 Gateway 成功链。
4. `routes.rs:11169` 只验证手工固定头触发旧 508，不证明生产链可检测真实递归。

### 正向测试（必须新增）

1. **保存契约**：保留前端特殊值测试；后端直接 upsert
   `bridge_type=cx2cc, source_provider_id=None` 成功，读回仍能恢复特殊来源。
2. **完整本机链**：启动真实/测试 Gateway，在 Claude 路由选择 null-source CX2CC，
   Codex 分流中放一个计数 upstream。断言：内部 `/v1/responses` 恰好进入一次、实际
   Codex upstream 恰好一次、返回正确 Anthropic 响应，不出现
   `provider_target_self_loop`。
3. **别名与 listen mode**：分别覆盖 Gateway base host 为 `127.0.0.1`、
   `localhost`、IPv6 `::1`（平台支持时）、LAN/wildcard 的合法内部入口；能力依据
   realtime authority 与 typed intent，而不是只对某个字符串特判。
4. **不同端口真实上游**：普通 Codex Provider 指向本机另一端口且直接成功，证明
   multi-hop 加固没有误伤合法开发服务。
5. **显式 CX2CC source**：仍直连该 Codex Provider，不获得本机 Gateway capability，
   鉴权、失败转移和请求参数保持现状。
6. **普通路径**：Claude、Codex、其他 bridge 的远端 target、同端口远端 IP 继续
   成功；现有 Provider 专用路由仍经过严格 target gate。
7. **可观测性**：外层 Claude 请求只有一条用户可见生命周期；内部 Codex hop 可有
   受控诊断但不重复计费/统计/Session，谱系和 secret 不进入日志或 fingerprint。

### 反向测试（必须新增）

1. 普通 Claude/Codex Provider 直接使用当前 Gateway authority：对
   `127.0.0.1`、`localhost`、`::1`、IPv4-mapped IPv6、wildcard、本机 LAN IP 及
   DNS alias 全部拒绝；fallback、circuit、account usage、Session 语义与现有测试一致。
2. DNS 返回“远端 + 任一本机地址”、空结果、超上限、timeout：全部失败关闭；保留
   DNS pinning，不能验证后再二次解析。
3. 合法 CX2CC 首次再入口后，Codex 分流选中一个 URL 指回当前 Gateway 的普通
   Provider：第二次必须拒绝，不能因 route 起源是 CX2CC 而继承放行能力。
4. 不同端口反向代理回当前 Gateway：首次不同端口可发送，回到已访问实例时稳定
   508/结构化拒绝，调用数有硬上限，不产生无限任务或日志。
5. 3xx 到当前 Gateway：逐 hop validator 在 follow 前拒绝；3xx 到合法远端仍按现有
   redirect 契约工作。
6. capability 缺失、伪造、签名篡改、过期、重放、错误 instance/CLI/path/method、
   authority 在签发后变化：全部拒绝；合法 token 只能原子消费一次，并发重放只能有
   一个成功。
7. request-before-send 插件尝试注入/覆盖谱系头，或外部客户端发送旧固定头：不能
   获得放行。内部头在插件可见面和真实外部上游日志中按设计过滤。
8. 两个 Gateway 实例/不同端口 A -> B -> A：A 能识别自己已签名的谱系并拒绝；B
   不把无法验证的 A 条目当作授权。谱系长度和 hop budget 超限失败关闭。
9. 内部 Codex CLI 开关关闭、Gateway 已重启/端口切换、签发状态丢失：请求不得降级
   为宽泛 localhost 放行，应返回可诊断的本地配置/能力失效错误。

## 八、风险、范围外与实施约束

### 风险

- 只在 `recursion_guard.rs` 加 null-source 白名单既修不到 pre-send 误判，也会把来源
  证明放在错误层级。
- 只给 `validate_gateway_target` 增加 `allow_self=true` 会让后续 Codex Provider 的
  自引用继承例外；例外必须绑定一次具体 transition 和 target。
- 当前本机 CX2CC 准备结果把 `bridge_source=None` 兼作“全局 Gateway source”，发送
  层无法据此区分“无来源信息”和“可信内部入口”；必须补显式类型，不能继续靠
  `Option` 猜测。
- 多实例/不受控代理若主动删除所有谱系信息，应用层无法绝对识别后续回流；逐 hop
  redirect 校验、同实例签名谱系、hop 上限和现有 direct/DNS gate 应共同缩小边界，
  文档不得声称静态地址检查能覆盖任意代理拓扑。
- redirect 改造容易改变 POST、body、Authorization 及跨 origin 行为，必须用现有
  reqwest 行为作兼容基线，不能为了安全静默破坏普通 Provider。

### 范围外

- 不删除 fork 的 Provider master switch、DNS pinning、健康中性 skip 或 Session/
  circuit 契约。
- 不修复 pinned upstream 自身的固定头残留设计；本任务只应在需要可信谱系时以
  fork 集成方式替换/收敛它。
- 不在本报告处理 CX2CC 模型路由、thinking、GPT-5.6 或 context window；它们由本
  任务其他调研/实现分支负责。
- 不把多个独立 AIO 实例之间的任意互代理拓扑自动认定为合法。未经显式、可认证
  transition 的再次进入必须失败关闭。

## 最终判断

误判是“合法协议再入口缺少类型化来源，撞上 fork 的通用 Provider 地址硬门”，不是
地址归一错误，也不是旧 Claude 固定头守卫直接触发。最小安全修复不能是删除硬门或
按 localhost 白名单放行，而应是：**精确的一次性内部 Codex target intent + 可认证
路由谱系 + 每个 redirect hop 复验**。这样合法的第一跳可用，普通直接 self-reference、
第二次进入、跨端口回流和可见 redirect 回流仍失败关闭。
