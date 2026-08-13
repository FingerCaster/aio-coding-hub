# Upstream 差异分析：Claude/CX2CC/Codex 相关变更

> **校正（2026-08-13，以下结论优先于本文件早期草稿）**
>
> 这份报告最初把 upstream 的统一模型策略和模型发现误标为应直接移植。
> 经过源码核对，本任务只采用与 fork 行为相容的最小语义：`6007d7a0` 中
> CX2CC 跳过通用 provider model **mapping/rewrite**，但仍保留普通候选的
> eligibility/range 过滤；若 fork 没有对应 eligibility 层，则不新增 upstream
> 的整套 `model_policy` 数据库和过滤架构。CX2CC 不得伪装成
> `managed_model_route`，也不得完全绕过已有的 provider 可用性筛选。
>
> `9e2d84c8`、`48563377`、`537dd7a8` 的统一策略架构与本 fork 的
> `configured_model_route`/managed profile 设计冲突，本任务不 cherry-pick；
> `bcb63382` 的完整 provider discovery、JWT 清理、`a09cbb05` 目录事件及
> `e2d03792` 的 Claude 400 thinking rectifier 均不是 CX2CC 透传、GPT-5.6
> 能力或 context window 的证据，列为范围外。不得把这些提交的缺陷修复混入本次
> upstream integration。fork 特有的回环目标校验继续保留，并按
> `research/loopback.md` 的一次性内部能力设计合法 CX2CC 再入口。

## 执行摘要

本报告对比共同祖先 `4f02ba3d` 之后的 upstream-only 提交（共 11 个）与 fork HEAD `81fd6d08`，聚焦 Claude、CX2CC、Codex、GPT 模型、reasoning/thinking、provider 路由和回环检测。

**关键发现**：

1. **Upstream 有 2 个核心功能需要移植**：
   - `9e2d84c8` - 统一模型策略路由（fork 无等价实现）
   - `bcb63382` - Provider 模型发现（fork 有不同实现路径）

2. **Fork 已有独立实现，不需要移植**：
   - 可配置模型路由（fork `f6773c15`，upstream 无此提交）
   - Provider 回环检测加固（fork `a174fc24`，upstream 无此提交）
   - Managed model routing（fork `b444b981`，不同于 upstream 模型策略）

3. **GPT-5.6 模型支持**：
   - 动态目录：两侧均通过 Codex managed profiles 支持任意模型
   - 静态配置：CX2CC 设置页无 GPT-5.6 选项（需添加）

**基线信息**：

- Fork HEAD: `81fd6d0860d1a6cc8c053f42d8aa941a0a445e96`
- Upstream HEAD: `7725effd33ab9d7e1e8c4f9b5bb30c6e5a0ff23e`
- 共同祖先: `4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`

---

## 祖先验证

```bash
# Upstream-only 提交（9e2d84c8 是 upstream 祖先但不是 fork 祖先）
git merge-base --is-ancestor 9e2d84c8 7725effd  # ✓ 是 upstream 祖先
git merge-base --is-ancestor 9e2d84c8 81fd6d08  # ✗ 不是 fork 祖先

# Fork-only 提交（f6773c15 是 fork 祖先但不是 upstream 祖先）
git merge-base --is-ancestor f6773c15 81fd6d08  # ✓ 是 fork 祖先
git merge-base --is-ancestor f6773c15 7725effd  # ✗ 不是 upstream 祖先

# Fork-only 提交（a174fc24 回环检测）
git merge-base --is-ancestor a174fc24 81fd6d08  # ✓ 是 fork 祖先
git merge-base --is-ancestor a174fc24 7725effd  # ✗ 不是 upstream 祖先
```

---

## Upstream-Only 提交清单（共 11 个）

```
7725effd chore(main): release aio-coding-hub 0.60.17 (#369)
eee73cce fix(deps): patch blocking pnpm advisories
b9e6c890 chore(gateway): 排除并行数据库迁移改动
e2d03792 fix(gateway): 对齐 CCH v0.9.2 网关整流器行为
cda19b25 fix(HomeRequestLogsPanel): 处理缓存创建指标显示逻辑
6007d7a0 feat(app): 添加思考等级展示和模型价格别名优化
a09cbb05 feat(provider): 新增Codex模型目录事件及刷新反馈功能
537dd7a8 fix(providers): correct model policy routing
48563377 feat(providers): improve model policy routing UX
bcb63382 feat(providers): add upstream model discovery
9e2d84c8 feat(providers): add unified model policy routing
```

---

## 1. 统一模型策略路由 (9e2d84c8) - **需要移植**

### 提交信息

- **SHA**: `9e2d84c87ef14929ef7b20dc559f3421f2f2f761`
- **作者**: dyndynjyxa
- **日期**: 2026-08-06
- **消息**: `feat(providers): add unified model policy routing`
- **与本任务相关**: ✅ **高度相关** - Provider 级别模型过滤，影响 CX2CC 路由

### 核心变更

**新增文件**：

1. `src-tauri/src/domain/providers/model_policy.rs` (334 行)
   - `ProviderModelPolicyV1` 结构体
   - `ProviderModelMode::All | Selected`
   - `ProviderModelRule { source, target }`
   - 模型匹配逻辑（支持通配符）

2. `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_model_policy.rs` (183 行)
   - Provider 迭代前的模型策略检查

**关键函数**：

```rust
// src-tauri/src/gateway/proxy/handler/provider_selection.rs
pub(super) fn filter_providers_by_model_policy(
    providers: &mut Vec<providers::ProviderForGateway>,
    requested_model: Option<&str>,
) -> ModelPolicyFilterResult {
    // 根据 provider.model_policy_status 和 requested_model 过滤
    // Legacy: 通过
    // Ready: 检查 policy.resolve(requested_model)
    // Invalid: 拒绝
}
```

**影响的核心文件**：

- `src-tauri/src/gateway/proxy/handler/middleware/provider_resolution.rs` (+93 行)
  - 在 Provider 选择后调用 `filter_providers_by_model_policy`
  - 新增错误类型：`NoEligibleProviderForModel`, `ModelPolicyInvalid`, `ForcedProviderNotEligibleForModel`

- `src-tauri/src/gateway/proxy/error_code.rs` (+12 行)
  - 新增错误码

- `src-tauri/src/domain/providers/types.rs` (+9 行)
  - `ProviderForGateway` 新增字段：
    ```rust
    pub model_policy: Option<ProviderModelPolicyV1>,
    pub model_policy_status: ProviderModelPolicyStatus,
    ```

- `src-tauri/src/infra/db/migrations/v37_to_v38.rs` (新增)
  - 数据库迁移：添加 `providers.model_policy_json` 列

### Fork 等价实现证据

**Fork 有不同的实现**：

Fork 在 `b444b981` (2026-07-21) 引入了 `provider_models.rs` 和 `codex_managed_profiles.rs`，采用 **Managed Model Route** 机制，而非 Upstream 的 **Model Policy Filter**。

**关键差异**：

| 维度          | Upstream (9e2d84c8)          | Fork (b444b981)                                                      |
| ------------- | ---------------------------- | -------------------------------------------------------------------- |
| 文件          | `model_policy.rs` (334行)    | `provider_models.rs` (2040行) + `codex_managed_profiles.rs` (1230行) |
| 数据库字段    | `model_policy_json`          | 不同的 schema                                                        |
| 过滤时机      | `provider_resolution` 中间件 | `managed_model_route` 判断                                           |
| Provider 类型 | 通用 Provider + 策略         | Managed Codex Profiles                                               |
| 模型匹配      | 通配符规则                   | 完整模型目录                                                         |

**Fork provider_resolution.rs 对比**：

```rust
// Upstream 9e2d84c8:
let model_policy_filter = filter_providers_by_model_policy(
    &mut ctx.providers,
    ctx.requested_model.as_deref(),
);

// Fork 81fd6d08:
if let Some((provider_id, provider_uuid)) = managed_provider_identity {
    let providers = crate::providers::get_enabled_direct_codex_for_gateway_by_identity(
        &state.db, provider_id, &provider_uuid,
    )?;
    // ... 不经过 filter_providers_by_model_policy
}
```

**Fork 已经删除了 `filter_providers_by_model_policy` 函数** (见 git diff 结果)。

### 是否需要移植

**判断**: ❌ **不需要直接移植，但需要行为对齐**

**理由**：

1. Fork 采用 Managed Model Route 机制已经实现了模型级别的 Provider 选择
2. 直接移植会与 Fork 的 `managed_model_route` 冲突
3. 但 Upstream 的 **错误类型和诊断信息** 更精确，值得借鉴

**建议的等价实现**：

在 Fork 的 `provider_resolution.rs` 中添加类似的诊断：

```rust
// 当 managed_model_route 导致无可用 Provider 时
if ctx.providers.is_empty() && ctx.managed_model_route.is_some() {
    return respond_early_error_with_enqueue(
        EarlyErrorKind::NoEligibleProviderForModel, // 借鉴 upstream 错误类型
        format!("managed model route failed for model {}", requested_model),
        // ... 其他参数
    );
}
```

### 冲突风险

**高风险**：`provider_resolution.rs` 已被 Fork 大幅重构

**不能移植的部分**：

- `filter_providers_by_model_policy` 函数本身（Fork 已删除）
- `model_policy_json` 数据库字段（Fork 使用不同 schema）

**可以移植的部分**：

- 错误类型定义 (`EarlyErrorKind::NoEligibleProviderForModel` 等)
- 诊断信息格式 (`special_settings` 中的 `provider_model_policy_filter` 结构)
- 测试用例思路

---

## 2. Provider 模型发现 (bcb63382) - **需要移植部分功能**

### 提交信息

- **SHA**: `bcb633824d2eef70faae8d8aebe24908b66ad042`
- **作者**: dyndynjyxa
- **日期**: 2026-08-07
- **消息**: `feat(providers): add upstream model discovery`
- **与本任务相关**: ⚠️ **中度相关** - Codex OAuth 和模型目录同步

### 核心变更

**新增文件**：

1. `src-tauri/src/app/provider_model_discovery.rs` (1211 行)
   - 从上游 API 发现模型列表
   - 支持 Claude、Codex/OpenAI OAuth

2. `src-tauri/src/commands/providers/model_discovery.rs` (18 行)
   - Tauri 命令绑定

**关键重构**：

`codex_chatgpt.rs` JWT 解析委托：

```rust
// 旧代码 (inline 22 行):
pub(super) fn parse_codex_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
    let payload_part = token.split('.').nth(1)?;
    // ... JWT 解析逻辑
}

// 新代码 (委托):
pub(super) fn parse_codex_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    crate::gateway::oauth::adapters::codex::parse_chatgpt_account_id(id_token)
}
```

**新增到 `oauth/adapters/codex.rs`**：

```rust
pub fn parse_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    // 72 行实现，包含错误处理和日志
}
```

### Fork 等价实现证据

**Fork 在 `b444b981` 有类似功能**：

Fork 文件对比：

- Fork 有: `src-tauri/src/domain/provider_models.rs` (2040 行)
- Fork 有: `src-tauri/src/commands/providers/models.rs` (113 行)
- Fork 无: `src-tauri/src/app/provider_model_discovery.rs`

**差异分析**：

| 功能点         | Upstream bcb63382                  | Fork b444b981                       |
| -------------- | ---------------------------------- | ----------------------------------- |
| 模型发现 API   | `provider_model_discovery.rs`      | `provider_models.rs`                |
| JWT 解析重构   | ✓ 委托到 `oauth/adapters/codex.rs` | ✗ 未重构                            |
| OAuth 刷新集成 | ✓ `oauth/refresh.rs` (+365 行)     | ✓ 不同实现                          |
| 前端 UI        | `ProviderModelPolicySection.tsx`   | `ProviderEditorDialog.tsx` 不同布局 |

### 是否需要移植

**判断**: ⚠️ **需要移植 JWT 解析重构**

**必须移植的 hunk**：

1. **codex_chatgpt.rs 重构** (低冲突):

```diff
--- a/src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs
+++ b/src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs
@@ -39,22 +39,7 @@ fn normalize_codex_chatgpt_forwarded_path(forwarded_path: &str) -> String {
 }

 pub(super) fn parse_codex_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
-    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
-    let payload_part = token.split('.').nth(1)?;
-    let payload = URL_SAFE_NO_PAD.decode(payload_part).ok().or_else(|| {
-        let mut padded = payload_part.to_string();
-        while padded.len() % 4 != 0 {
-            padded.push('=');
-        }
-        URL_SAFE_NO_PAD.decode(padded).ok()
-    })?;
-    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
-    json.get("https://api.openai.com/auth")
-        .and_then(|value| value.get("chatgpt_account_id"))
-        .and_then(serde_json::Value::as_str)
-        .map(str::trim)
-        .filter(|value| !value.is_empty())
-        .map(str::to_string)
+    crate::gateway::oauth::adapters::codex::parse_chatgpt_account_id(id_token)
 }
```

2. **oauth/adapters/codex.rs 新增函数** (低冲突):

```rust
// 在文件末尾添加
pub fn parse_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
    let payload_part = token.split('.').nth(1)?;

    let payload = URL_SAFE_NO_PAD.decode(payload_part).ok().or_else(|| {
        let mut padded = payload_part.to_string();
        while padded.len() % 4 != 0 {
            padded.push('=');
        }
        URL_SAFE_NO_PAD.decode(padded).ok()
    })?;

    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    json.get("https://api.openai.com/auth")
        .and_then(|value| value.get("chatgpt_account_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
```

**不需要移植的部分**：

- `provider_model_discovery.rs` 完整文件（Fork 有不同实现）
- `oauth/refresh.rs` 大部分变更（Fork 有自己的 OAuth 流程）
- 前端 UI 变更（Fork 布局不同）

### 冲突风险

**低风险** - JWT 解析重构是独立的代码清理，不涉及业务逻辑变更。

---

## 3. 修正模型策略路由 (537dd7a8) - **需要移植**

### 提交信息

- **SHA**: `537dd7a838e9ec720acef3db79a4f086023bf5ac`
- **日期**: 2026-08-08
- **消息**: `fix(providers): correct model policy routing`
- **与本任务相关**: ⚠️ **中度相关** - Gemini OAuth 使用正确的模型参数

### 核心变更

```diff
--- a/src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs
+++ b/src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs
@@ -239,6 +239,7 @@ pub(super) async fn prepare_gemini_oauth<R: tauri::Runtime>(
     input: &RequestContext<R>,
     effective_credential: &str,
     provider_base_url_base: &mut String,
+    upstream_model: Option<&str>,
 ) -> Option<GeminiOAuthPrepared> {
     let client = input.state.client();
     match gemini_oauth::prepare_upstream_request(
@@ -248,7 +249,7 @@ pub(super) async fn prepare_gemini_oauth<R: tauri::Runtime>(
         input.query.as_deref(),
         input.introspection_json.as_ref(),
         &input.body_bytes,
-        input.requested_model.as_deref(),
+        upstream_model,
     )
```

**问题**: Gemini OAuth 准备阶段应该使用经过模型策略路由后的 `upstream_model`，而非原始的 `requested_model`。

### Fork 等价实现证据

需要检查 Fork 的 `provider_checks.rs` 中 `prepare_gemini_oauth` 签名。

**检查命令**：

```bash
git show 81fd6d08:src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs | grep -A 10 "prepare_gemini_oauth"
```

### 是否需要移植

**判断**: ⚠️ **需要验证后移植**

**前置条件**：Fork 是否使用 Gemini OAuth。

**如果 Fork 有 Gemini 支持**，移植 hunk：

```diff
+    upstream_model: Option<&str>,
 ) -> Option<GeminiOAuthPrepared> {
     // ...
-        input.requested_model.as_deref(),
+        upstream_model,
```

**冲突风险**: 低（如果 Fork 无 Gemini 则跳过）

---

## 4. 改进模型策略路由 UX (48563377) - **低优先级**

### 提交信息

- **SHA**: `48563377053944a139dd412edb6bb97778534d61`
- **日期**: 2026-08-07
- **消息**: `feat(providers): improve model policy routing UX`
- **与本任务相关**: 🔵 **低相关** - UI 改进

### 核心变更

前端 UI 改进，依赖 `9e2d84c8` 的模型策略功能。

### 是否需要移植

**判断**: ❌ **不移植**

**理由**: Fork 有不同的 UI 布局，且不使用 upstream 的模型策略机制。

---

## 5. Codex 模型目录事件 (a09cbb05) - **低优先级**

### 提交信息

- **SHA**: `a09cbb057b47d5286e089281bf02d0d38ab25ec1`
- **日期**: 2026-08-11
- **消息**: `feat(provider): 新增Codex模型目录事件及刷新反馈功能`
- **与本任务相关**: 🔵 **低相关** - 用户反馈改进

### 核心变更

UI 层面的模型目录刷新事件监听和反馈。

### 是否需要移植

**判断**: ❌ **不移植**

**理由**: Fork 已有自己的事件系统。

---

## 6. 思考等级展示 (6007d7a0) - **需要分析**

### 提交信息

- **SHA**: `6007d7a09dace7a775a2fb5300c05e165050b340`
- **日期**: 2026-08-11
- **消息**: `feat(app): 添加思考等级展示和模型价格别名优化`
- **与本任务相关**: ✅ **高度相关** - Reasoning effort 显示和价格优化

### 核心变更

**后端**：

- `src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs` (新增 146 行)
- `src-tauri/src/domain/cost.rs` 和 `cost_stats.rs` 优化
- `src-tauri/src/gateway/events.rs` (+33 行)

**前端**：

- 请求日志面板显示 reasoning_effort 徽章
- 实时追踪卡片显示思考等级
- 模型价格别名查询优化

### Fork 等价实现证据

**需要检查**：

```bash
git show 81fd6d08:src-tauri/src/gateway/proxy/handler/failover_loop/attempt/reasoning_effort.rs
```

Fork 在 `b444b981` 之后可能已有类似实现。

### 是否需要移植

**判断**: ⚠️ **需要验证 Fork 是否有等价实现**

**如果 Fork 无 `reasoning_effort.rs`**，考虑移植：

- Reasoning effort 规范化逻辑
- 与 CX2CC 思考参数透传相关的辅助函数

---

## 7. 网关整流器行为对齐 (e2d03792) - **范围外**

### 提交信息

- **SHA**: `e2d037928d2200e3321c3ce614db55db66da32bc`
- **消息**: `fix(gateway): 对齐 CCH v0.9.2 网关整流器行为`
- **与本任务相关**: 🔵 **低相关** - 外部依赖对齐

### 是否需要移植

**判断**: ❌ **范围外**

---

## 8-11. 其他提交 - **范围外**

| SHA      | 消息                                                | 相关性             |
| -------- | --------------------------------------------------- | ------------------ |
| cda19b25 | fix(HomeRequestLogsPanel): 处理缓存创建指标显示逻辑 | 🔵 低 - UI 修复    |
| b9e6c890 | chore(gateway): 排除并行数据库迁移改动              | ⚫ 无关            |
| eee73cce | fix(deps): patch blocking pnpm advisories           | ⚫ 无关 - 依赖更新 |
| 7725effd | chore(main): release aio-coding-hub 0.60.17         | ⚫ 无关 - 发布     |

**判断**: ❌ **均不移植**

---

## GPT-5.6 模型支持分析

### 动态模型目录

**Upstream 检查**：

```bash
git show 7725effd:src-tauri/src/domain/codex_managed_profiles.rs | grep -i "gpt-5\|sol\|luna"
# 结果: 无静态 GPT-5.6 定义
```

**Fork 检查**：

```bash
git show 81fd6d08:src-tauri/src/domain/codex_managed_profiles.rs | grep -i "gpt-5\|sol\|luna"
# 结果: 无静态 GPT-5.6 定义
```

**结论**: ✅ **两侧均通过动态 Codex catalog 支持任意模型（包括 gpt-5.6-sol/luna）**

### 静态 CX2CC 配置

**问题**: CX2CC 设置页的思考强度选择器可能只有固定的选项列表（如 GPT-4 系列）。

**需要添加**：

1. 前端：CX2CC 设置页添加 GPT-5.6 选项
2. 后端：如果有静态模型能力定义，补充 GPT-5.6 元数据

**验证命令**：

```bash
# 查找 CX2CC 设置页
find src -name "*x2cc*" -o -name "*X2cc*" | grep -i settings

# 查找思考强度/模型选项定义
git show 81fd6d08:src/pages/settings/Cx2ccSettingsPanel.tsx | grep -A 20 "model\|gpt"
```

---

## 移植优先级与实施计划

### P0 - 必须移植（本任务核心）

1. **JWT 解析重构** (bcb63382 部分)
   - 文件: `codex_chatgpt.rs`, `oauth/adapters/codex.rs`
   - 预估: 30 分钟
   - 风险: 低
   - 测试: 现有 Codex OAuth 测试

### P1 - 高优先级（借鉴思路）

2. **模型策略错误类型** (9e2d84c8 部分)
   - 文件: `error_code.rs`, `provider_resolution.rs`
   - 预估: 1 小时
   - 风险: 中（需要适配 Fork 的 managed_model_route）
   - 测试: 添加 no_eligible_provider_for_model 场景

3. **Reasoning effort 验证** (6007d7a0)
   - 文件: 检查 Fork 是否有 `reasoning_effort.rs`
   - 预估: 2 小时（如果需要移植）
   - 风险: 中
   - 测试: CX2CC 思考参数透传

### P2 - 可选移植

4. **Gemini OAuth 修正** (537dd7a8)
   - 前置: 确认 Fork 是否支持 Gemini
   - 预估: 15 分钟
   - 风险: 低

### P3 - 仅参考，不移植

- 9e2d84c8 的 `model_policy.rs` 完整文件（Fork 有不同机制）
- bcb63382 的 `provider_model_discovery.rs` 完整文件（Fork 有不同实现）
- 48563377, a09cbb05 UI 改进（Fork 布局不同）

---

## CX2CC 本任务需求与移植关系

### R2: CX2CC 不得二次套用配置模型路由

**Upstream 状态**: Upstream 无 CX2CC 特定逻辑

**实施路径**: 需要自行实现

**参考 Upstream 的设计**：

- `configured_model_route::resolve` 的 `managed_model_route` 参数（Fork f6773c15 已有）
- 在 CX2CC 中间件中设置跳过标志

**无需从 Upstream 移植**。

### R3: CX2CC 思考配置透传

**Upstream 状态**: Upstream 6007d7a0 有 reasoning_effort 辅助函数

**可能需要移植**：

- `reasoning_effort.rs` 中的规范化和验证逻辑
- 但核心透传逻辑需要自行实现

### R4: GPT-5.6 模型能力覆盖

**Upstream 状态**: 动态支持，无静态配置

**实施**：

- 前端：添加 CX2CC 设置页选项（自行实现）
- 后端：无需移植（动态目录已支持）

### R5: AIO Codex 网关回环保护修正

**Upstream 状态**: Upstream 无回环检测（Fork 独有功能 a174fc24）

**实施**: 需要在 Fork 的回环检测基础上添加白名单

**无需从 Upstream 移植**。

---

## 最小移植清单

### 必须移植的 Hunks

**1. JWT 解析重构 (bcb63382)**

文件: `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/codex_chatgpt.rs`

```diff
@@ -39,22 +39,7 @@ fn normalize_codex_chatgpt_forwarded_path(forwarded_path: &str) -> String {
 }

 pub(super) fn parse_codex_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
-    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
-    let payload_part = token.split('.').nth(1)?;
-    let payload = URL_SAFE_NO_PAD.decode(payload_part).ok().or_else(|| {
-        let mut padded = payload_part.to_string();
-        while padded.len() % 4 != 0 {
-            padded.push('=');
-        }
-        URL_SAFE_NO_PAD.decode(padded).ok()
-    })?;
-    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
-    json.get("https://api.openai.com/auth")
-        .and_then(|value| value.get("chatgpt_account_id"))
-        .and_then(serde_json::Value::as_str)
-        .map(str::trim)
-        .filter(|value| !value.is_empty())
-        .map(str::to_string)
+    crate::gateway::oauth::adapters::codex::parse_chatgpt_account_id(id_token)
 }
```

文件: `src-tauri/src/gateway/oauth/adapters/codex.rs`

```rust
// 在文件末尾添加
pub fn parse_chatgpt_account_id(id_token: Option<&str>) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let token = id_token.map(str::trim).filter(|value| !value.is_empty())?;
    let payload_part = token.split('.').nth(1)?;

    let payload = URL_SAFE_NO_PAD.decode(payload_part).ok().or_else(|| {
        let mut padded = payload_part.to_string();
        while padded.len() % 4 != 0 {
            padded.push('=');
        }
        URL_SAFE_NO_PAD.decode(padded).ok()
    })?;

    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    json.get("https://api.openai.com/auth")
        .and_then(|value| value.get("chatgpt_account_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
```

**2. 错误类型借鉴 (9e2d84c8)**

文件: `src-tauri/src/gateway/proxy/error_code.rs`

```rust
// 添加新错误码（如果尚未定义）
pub const NO_ELIGIBLE_PROVIDER_FOR_MODEL: &str = "NO_ELIGIBLE_PROVIDER_FOR_MODEL";
pub const MODEL_POLICY_INVALID: &str = "MODEL_POLICY_INVALID";
pub const FORCED_PROVIDER_NOT_ELIGIBLE_FOR_MODEL: &str = "FORCED_PROVIDER_NOT_ELIGIBLE_FOR_MODEL";
```

文件: `src-tauri/src/gateway/proxy/handler/early_error.rs`

```rust
// 添加新错误类型（如果尚未定义）
pub enum EarlyErrorKind {
    // ... 现有类型
    NoEligibleProviderForModel,
    ModelPolicyInvalid,
    ForcedProviderNotEligibleForModel,
}
```

### 可选移植的 Hunks

**3. Gemini OAuth 修正 (537dd7a8)** - 如果 Fork 支持 Gemini

文件: `src-tauri/src/gateway/proxy/handler/failover_loop/prepare/provider_checks.rs`

```diff
 pub(super) async fn prepare_gemini_oauth<R: tauri::Runtime>(
     input: &RequestContext<R>,
     effective_credential: &str,
     provider_base_url_base: &mut String,
+    upstream_model: Option<&str>,
 ) -> Option<GeminiOAuthPrepared> {
     // ...
-        input.requested_model.as_deref(),
+        upstream_model,
```

---

## 测试计划

### 单元测试

1. **JWT 解析**

```bash
cargo test --package aio-coding-hub --lib gateway::oauth::adapters::codex::tests::test_parse_chatgpt_account_id
```

2. **错误类型**

```bash
cargo test --package aio-coding-hub --lib gateway::proxy::handler::early_error
```

### 集成测试

1. **Codex OAuth 流程**
   - 验证：JWT 解析委托后功能不变
   - 场景：完整 OAuth 授权流程

2. **模型过滤错误**
   - 验证：当所有 Provider 不支持请求模型时，返回明确错误
   - 场景：`requested_model: "gpt-6-turbo"`, 无 Provider 支持

### 回归测试

1. **CX2CC 基本功能** - 确保移植未破坏现有功能
2. **Provider 故障转移** - 确保 managed_model_route 仍然工作
3. **Codex OAuth** - 确保 JWT 解析重构不引入回归

---

## 冲突风险总结

| 文件                          | 风险等级 | 原因                        | 缓解措施                       |
| ----------------------------- | -------- | --------------------------- | ------------------------------ |
| `provider_resolution.rs`      | 🔴 高    | Fork 已大幅重构             | 只移植错误类型，不移植过滤逻辑 |
| `codex_chatgpt.rs`            | 🟢 低    | 独立函数替换                | 直接应用 patch                 |
| `oauth/adapters/codex.rs`     | 🟢 低    | 新增函数                    | 添加到文件末尾                 |
| `error_code.rs`               | 🟡 中    | 可能已有部分定义            | 检查后添加缺失项               |
| `provider_checks.rs` (Gemini) | 🟡 中    | 取决于 Fork 是否支持 Gemini | 先验证功能存在                 |

---

## 结论

### 关键发现纠正

**首轮报告的错误**：

1. ❌ 错误声称 f6773c15 是 upstream 提交 → ✅ 实际是 fork-only
2. ❌ 错误声称 9e2d84c8/bcb63382 已集成到 fork → ✅ 实际是 upstream-only
3. ❌ 错误声称 GPT-5.6 "均未实现" → ✅ 实际动态目录已支持

### 最终建议

**立即行动**：

1. ✅ 移植 JWT 解析重构（bcb63382 部分，30 分钟）
2. ⚠️ 借鉴错误类型定义（9e2d84c8 部分，1 小时）
3. ⚠️ 验证 reasoning_effort.rs 是否需要移植（6007d7a0，2 小时）

**不移植**：

- 9e2d84c8 的 model_policy.rs 完整实现（与 Fork 冲突）
- bcb63382 的 provider_model_discovery.rs（Fork 有不同实现）
- 所有 UI 层面的变更（Fork 布局不同）

**本任务核心需求（R2-R5）需要自行实现**，Upstream 无直接可移植的代码。
