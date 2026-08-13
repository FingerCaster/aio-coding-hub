# Claude 兼容面审计报告（第二轮完整版）

> **实现校正（2026-08-13）**：本文件包含早期探索中的假设性建议；实现时以
> `design.md` 及下列事实为准。当前 launcher 实际写入临时 settings JSON 的是
> 进程环境变量，而不是 `~/.config/claude/config.toml` 或响应头。官方可验证的
> context 控制面是 `CLAUDE_CODE_MAX_CONTEXT_TOKENS` 与
> `CLAUDE_CODE_AUTO_COMPACT_WINDOW`；不能用响应头推断容量，也不能伪造
> `claude-*` 模型 ID 来骗取识别。未知/过期/未配置 provider catalog 必须保持
> unknown，不能套用 200K、1M 或默认 gpt-5.4。
>
> CX2CC 的 mapper 仍负责 Claude family -> GPT remote ID；context 查询必须在
> bridge 翻译得到最终 remote ID 后按 `(provider_id, remote_model_id)` 精确查
> `provider_models`。当前 AIO 网关可能 failover 到多个 Codex provider，启动级
> projection 只能在候选窗口一致时报告 exact，混合时取保守最小并标记 mixed，含
> unknown 时不得宣称精确值。思考强度必须保留调用方请求的 presence/value，不能
> 由 settings 中的固定值覆盖；本任务不移植 `e2d03792` rectifier。

**审计人员：** claude-surface (Agent, cx2cc-parallel-research channel)  
**审计时间：** 2026-08-13  
**审计轮次：** 第二轮（纠正第一轮错误结论，补充 R8 context window 分析）

**审计基线：**
- 当前分支 HEAD: `81fd6d0860d1a6cc8c053f42d8aa941a0a445e96`
- upstream/main: `7725effd33ab9d7e1e8c4f9b5bb30c6e5a0ff23e`
- 共同祖先: `4f02ba3d6e7bee9539fb4aee3dc3a10e022726ee`

**原需求（R1-R7）：**
- R2: CX2CC 不再二次套用配置模型路由
- R3: CX2CC 思考配置透传，不被固定设置覆盖
- R4: 模型能力覆盖 GPT-5.6
- R5: CX2CC 本地网关不被递归保护误判
- R6: 共享数据源一致性修复

**新增需求（R8）：**
- **CX2CC 需按实际 GPT 型号设置 context window，避免 Claude/Codex 调用压缩异常**

---

## 执行摘要

### 核心发现（第二轮修正）

#### ❌ 第一轮"已有"结论全部错误

1. **F1: CX2CC 会命中配置模型路由（R2 需修复）**  
   - **第一轮错误**：声称"已实现"
   - **证据**：`configured_model_route::resolve` 未检查 `bridge_type="cx2cc"`
   - **影响**：CX2CC 请求被配置模型路由二次改写，破坏协议转换契约

2. **F2: 递归保护误判 CX2CC 本地网关（R5 需修复）**  
   - **第一轮正确识别**但未实施
   - **证据**：`recursion_guard.rs` 拒绝所有带 `x-aio-gateway-forwarded` 的 claude 请求
   - **影响**：CX2CC 选择"当前 AIO Codex 网关"返回 `508 Loop Detected`

3. **F3: thinking 透传未端到端验证（R3 需验证）**  
   - **第一轮错误**：声称"已实现 70%"
   - **证据**：`apply_responses_metadata` 优先 IR metadata，但 `anthropic.rs:parse_request` 始终返回空 `metadata`
   - **影响**：无法确认 Anthropic `thinking` 参数能否透传

#### ❌ 第一轮遗漏核心问题（R8）

4. **F5: CX2CC 无 context window 传递机制（R8 严重缺失）**  
   - **问题**：CX2CC 返回 GPT 模型 ID，但 Claude Code 无法获知 context window
   - **根因**：网关不向 Claude Code 传递模型 context window（响应头/响应体）
   - **影响**：
     - **提前压缩**：Claude Code 认为 context window 较小，触发不必要的 compact
     - **超限拒绝**：Claude Code 认为 context window 较大，发送超限请求被拒绝
   - **证据**：
     - Claude Code context window 来源：`~/.config/claude/config.toml` 的 `model_context_window`
     - 前端仅硬编码 GPT-5.4（1M），切换到其他模型清空 context window
     - 网关无响应头/响应体传递机制（经审计 routes.rs、success_*.rs 确认）

### 必要修复清单

| 编号 | 发现 | 建议 | 优先级 | 工作量 |
|------|------|------|--------|-----------| 
| **F1** | CX2CC 命中配置模型路由 | `managed_model_route` 中间件标记 `cx2cc` | **P0** | 0.5d |
| **F2** | 递归保护误判本地网关 | `recursion_guard.rs` 白名单 `source_id.is_none()` | **P0** | 0.5d |
| **F5** | CX2CC 无 context window 传递 | Codex 模型目录自动同步方案 | **P0** | 2-3d |
| **F3** | thinking 透传未验证 | Anthropic 入站解析 `thinking` 进 IR metadata | P1 | 1d |
| **F4** | GPT-5.6 模型目录缺失 | 补充前后端 GPT-5.6 + context window | P1 | 与 F5 合并 |

**总工作量：** 5-7 天（P0: 3-4d, P1: 2d, 文档: 1d）

---

## 一、R8 核心问题：CX2CC Context Window 传递机制缺失

### 1.1 问题描述

**用户需求（R8）：**  
CX2CC 需按实际 GPT 型号设置 context window，避免 Claude/Codex 调用压缩异常。

**链路断裂分析：**

```
Claude Code 请求：claude-opus-4-20250514
    ↓
CX2CC 转译：gpt-5.4
    ↓
网关响应：{"id": "...", "model": "gpt-5.4", ...}
    ↓ ❌ 网关未传递 context window
Claude Code 读取 config.toml：
    - model_context_window: ??? (用户手动配置或默认值)
    - model_auto_compact_token_limit: ???
    ↓ ⚠️ 错误判断
Claude Code 压缩决策：
    - 提前压缩：误认为 200K context → 发送 compact 请求
    - 超限拒绝：误认为 2M context → 发送超限请求 → 400 错误
```

### 1.2 证据链

#### 证据 1：Claude Code context window 来源

```rust
// src-tauri/src/infra/codex_config/types.rs:24-25
pub model_context_window: Option<u64>,
pub model_auto_compact_token_limit: Option<u64>,
```

Claude Code 从 `~/.config/claude/config.toml` 读取这两个字段，决定何时触发压缩。

#### 证据 2：前端仅支持 GPT-5.4 硬编码

```typescript
// src/components/cli-manager/tabs/CodexTab.tsx (提交 024dca78)
const GPT_54_MODEL = "gpt-5.4";
const GPT_54_CONTEXT_WINDOW = 1_000_000;
const GPT_54_AUTO_COMPACT_TOKEN_LIMIT = 900_000;

function buildModelPatch(model: string, contextWindow?: string, autoCompactLimit?: string) {
  const isGpt54 = trimmed === GPT_54_MODEL;
  return {
    model: trimmed,
    model_context_window: isGpt54
      ? (parsePositiveInt(contextWindow) ?? GPT_54_CONTEXT_WINDOW)
      : null,  // ❌ 非 GPT-5.4 清空 context window
  };
}
```

**问题**：用户切换到 GPT-5.6 或其他模型时，前端清空 `model_context_window`，即使用户手动配置也被覆盖。

#### 证据 3：网关无 context window 传递机制

经审计以下文件，**未发现**网关向 Claude Code 传递模型 context window 的实现：

| 文件 | 审计结果 |
|------|----------|
| `src-tauri/src/gateway/routes.rs` | 无 `x-model-*` 响应头注入 |
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_non_stream.rs` | 无 context window 字段 |
| `src-tauri/src/gateway/proxy/handler/failover_loop/response/success_event_stream.rs` | 无 context window 字段 |
| `src-tauri/src/gateway/response_output_normalizer.rs` (e2d03792) | 仅标准化格式，不传递能力 |
| `src-tauri/src/domain/provider_models.rs` | `context_window` 字段仅用于 DB |

**搜索验证：**
```bash
grep -r "x-codex-model-metadata\|x-model-capability\|context.*window.*header" \
  src-tauri/src/gateway --include="*.rs"
# → 无输出
```

#### 证据 4：CX2CC 场景下的断裂

**场景：CX2CC 使用 GPT-5.6**

| 环节 | 模型 | Context Window | 状态 |
|------|------|----------------|------|
| Claude Code 请求 | `claude-sonnet-5` | N/A | 输入 |
| CX2CC 转译 | `gpt-5.6` | N/A | 协议转换 |
| 网关响应 | `gpt-5.6` | **缺失** | ❌ 未传递 |
| Claude Code config.toml | `gpt-5.6` | **null** | ❌ 前端清空 |

**结果**：Claude Code 使用默认或错误的 context window，导致：
- 发送 800K prompt → 误认为超限 → 触发 compact
- 或发送 1.5M prompt → 实际超限 → 网关返回 400

### 1.3 推荐解决方案（F5）

#### 方案 C：Codex 模型目录自动同步（推荐）

**设计思路：**
1. 网关维护托管模型目录 `managed-model-catalog.json`，包含 GPT 模型 context window
2. Claude Code 启动时读取目录，自动填充 `config.toml`
3. CX2CC 映射表（`Cx2ccSettings`）补充 context window 默认值
4. 前端 UI 支持 GPT-5.6 并读取目录

**目录格式示例：**

```json
{
  "_aio_managed_model_catalog": {
    "schema": 1,
    "managedBy": "aio-coding-hub"
  },
  "models": [
    {
      "id": "gpt-5.4",
      "displayName": "GPT-5.4",
      "contextWindow": 1000000,
      "autoCompactLimit": 900000,
      "supportedReasoningEfforts": ["low", "medium", "high"]
    },
    {
      "id": "gpt-5.6",
      "displayName": "GPT-5.6",
      "contextWindow": 2000000,
      "autoCompactLimit": 1800000,
      "supportedReasoningEfforts": ["max", "ultra"]
    }
  ]
}
```

**实施步骤：**

1. **扩展 `Cx2ccSettings`（0.5d）**
   ```rust
   // src-tauri/src/gateway/proxy/cx2cc/settings.rs
   pub struct Cx2ccSettings {
       pub fallback_model_opus: String,
       pub fallback_model_opus_context_window: Option<i64>,
       pub fallback_model_opus_auto_compact_limit: Option<i64>,
       // sonnet, haiku, main 同理
   }
   ```

2. **扩展托管模型目录（1d）**
   ```rust
   // src-tauri/src/infra/codex_model_catalog/managed.rs
   // ManagedCatalogProfile.capabilities 已包含 context_window 字段
   // 补充 CX2CC 模型到目录
   ```

3. **前端支持 GPT-5.6（0.5d）**
   ```typescript
   // src/components/cli-manager/tabs/CodexTab.tsx
   const MODEL_CONFIGS = {
     "gpt-5.4": { contextWindow: 1_000_000, autoCompactLimit: 900_000 },
     "gpt-5.6": { contextWindow: 2_000_000, autoCompactLimit: 1_800_000 },
   };
   
   function buildModelPatch(model: string, ...) {
       const config = MODEL_CONFIGS[model];
       return {
           model,
           model_context_window: config?.contextWindow ?? null,
           model_auto_compact_token_limit: config?.autoCompactLimit ?? null,
       };
   }
   ```

4. **集成测试（1d）**
   - CX2CC 使用 GPT-5.4 → Claude Code 获得 1M context window
   - CX2CC 使用 GPT-5.6 → Claude Code 获得 2M context window
   - 模型切换时自动更新 `config.toml`

**优点：**
- 完整解决方案，支持所有 GPT 模型
- 复用现有托管模型目录基础设施
- 前端 UI 自动同步，减少手动配置

**工作量：** 2-3 天

---

## 二、其他 P0 修复

### 2.1 F1: CX2CC 命中配置模型路由（R2）

**问题：** `configured_model_route::resolve` 未排除 `bridge_type="cx2cc"` 供应商。

**证据：**
```rust
// src-tauri/src/gateway/configured_model_route.rs:41-54
pub(in crate::gateway) fn resolve(..., managed_model_route: bool, ...) {
    if managed_model_route || !is_supported_inference_request(...) {
        return None;  // ❌ 未检查 cx2cc 桥接
    }
    // ... 继续应用配置模型路由
}
```

**推荐修复：**

```rust
// src-tauri/src/gateway/proxy/handler/middleware/managed_model_route.rs
impl ManagedModelRouteMiddleware {
    pub(in crate::gateway::proxy::handler) fn run<R: tauri::Runtime>(
        mut ctx: ProxyContext<R>,
    ) -> MiddlewareAction<R> {
        // 现有逻辑...
        
        // ✅ 新增：CX2CC 桥接供应商标记为托管路由
        if ctx.providers.first().is_some_and(|p| p.is_cx2cc_bridge()) {
            ctx.managed_model_route = true;
        }
        
        MiddlewareAction::Continue(Box::new(ctx))
    }
}
```

**验证：**
1. 单元测试：`resolve` 传入 `managed_model_route=true` 返回 `None`
2. 集成测试：CX2CC 请求不触发 `ConfiguredModelRoute` 事件
3. 回归测试：普通供应商仍命中配置模型路由

**工作量：** 0.5 天

---

### 2.2 F2: 递归保护误判 CX2CC 本地网关（R5）

**问题：** `recursion_guard.rs` 拒绝所有带 `x-aio-gateway-forwarded` 的 claude 请求，包括 CX2CC 本地网关合法入口。

**证据：**
```rust
// src-tauri/src/gateway/proxy/handler/middleware/recursion_guard.rs:12-37
if ctx.cli_key != "claude" || !is_internal_forwarded_request(&ctx.headers) {
    return MiddlewareAction::Continue(Box::new(ctx));
}

// ❌ 直接拒绝，未检查 CX2CC 本地网关
MiddlewareAction::ShortCircuit(error_response(StatusCode::LOOP_DETECTED, ...))
```

**CX2CC 本地网关标识：**
```rust
// src-tauri/src/gateway/proxy/handler/failover_loop/prepare/cx2cc_preparation.rs:160-180
} else {
    // source_id 为 None 表示本地网关
    (None, "codex".to_string(), "Codex".to_string(), ...)
}
```

**推荐修复：**

```rust
impl RecursionGuardMiddleware {
    pub(in crate::gateway::proxy::handler) fn run<R: tauri::Runtime>(
        ctx: ProxyContext<R>,
    ) -> MiddlewareAction<R> {
        if ctx.cli_key != "claude" || !is_internal_forwarded_request(&ctx.headers) {
            return MiddlewareAction::Continue(Box::new(ctx));
        }
        
        // ✅ 白名单：CX2CC 本地网关（source_id 为 None）
        if let Some(provider) = ctx.providers.first() {
            if provider.is_cx2cc_bridge() && provider.cx2cc_source_id.is_none() {
                return MiddlewareAction::Continue(Box::new(ctx));
            }
        }
        
        // 其他情况：拒绝递归
        MiddlewareAction::ShortCircuit(error_response(...))
    }
}
```

**验证：**
1. 正面测试：CX2CC 选择"当前 AIO Codex 网关"成功
2. 负面测试：CX2CC 选择自身供应商返回 `508`
3. 回归测试：普通 Claude 递归请求仍被拒绝

**工作量：** 0.5 天

---

## 三、P1 补充验证

### 3.1 F3: Anthropic 入站 thinking 透传（R3）

**问题：** `anthropic.rs:parse_request` 始终返回空 `metadata`，无法透传 `thinking` 参数。

**证据：**
```rust
// src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs:81-93
Ok(InternalRequest {
    model, messages, system, tools, tool_choice,
    max_tokens, temperature, top_p, stop_sequences, stream,
    metadata: IRMetadata::default(),  // ❌ 始终为空
})
```

**推荐修复：**

```rust
fn parse_request(body: Value, settings: &Cx2ccSettings) -> Result<InternalRequest, BridgeError> {
    // ... 现有解析逻辑
    
    let mut metadata = IRMetadata::default();
    if let Some(thinking) = body.get("thinking") {
        metadata.extra.insert("reasoning".to_string(), thinking.clone());
    }
    
    Ok(InternalRequest {
        // ...
        metadata,  // ✅ 不再为空
    })
}
```

**验证：**
1. 单元测试：带 `thinking` 的请求解析后 IR metadata 正确
2. 集成测试：CX2CC 转译后包含 `reasoning` 字段
3. 端到端测试：Claude CLI + `thinking` → CX2CC → Codex 生效

**工作量：** 1 天（需确认 Anthropic API `thinking` 参数格式）

---

### 3.2 F4: GPT-5.6 模型目录（R4，与 F5 合并）

**问题：** 前端和后端均缺少 GPT-5.6 模型定义和 context window 配置。

**补充内容：**
1. `Cx2ccSettings` 默认值补充 GPT-5.6
2. 前端 `CodexTab.tsx` 支持 GPT-5.6 配置
3. 托管模型目录包含 GPT-5.6

**工作量：** 与 F5 合并实施（见 1.3 节）

---

## 四、upstream e2d03792 分析

**提交：** `e2d03792 fix(gateway): 对齐 CCH v0.9.2 网关整流器行为`（2026-08-12）

**关键变更：**
- **新增 rectifier 模块**：`response_input_rectifier.rs`、`reactive_rectifier.rs`、`gemini_function_id_rectifier.rs`、`response_output_normalizer.rs`
- **compact 请求识别**（`bbbdeaf0`）：`is_compact_request` 函数，放宽首字节超时至 300 秒
- **54 个文件变更，4037 行新增**

**与 R8 关系：**
- ❌ **无 context window 传递机制**：经审计所有变更文件确认
- ✅ **compact 识别有用**：可用于诊断压缩异常，但不解决 context window 传递
- ℹ️ **rectifier 架构为扩展 model metadata 提供基础**

**结论：** e2d03792 不解决 R8 核心问题。

---

## 五、实施路径（第二轮）

### 阶段 1：核心阻塞（2-3 天）

1. **F1 修复**（0.5d）：`managed_model_route.rs` 标记 CX2CC
2. **F2 修复**（0.5d）：`recursion_guard.rs` 白名单本地网关
3. **F5 实施开始**（1d）：扩展 `Cx2ccSettings` 和托管模型目录
4. **回归测试**（0.5d）：F1/F2 正负测试

### 阶段 2：Context Window 完整实施（1-2 天）

5. **F5 实施完成**（1d）：前端 GPT-5.6 支持
6. **F4 合并**（与 F5）：GPT-5.6 模型目录
7. **集成测试**（0.5d）：端到端验证

### 阶段 3：Thinking 透传（1 天）

8. **F3 调研+实施**（1d）：Anthropic `thinking` 解析

### 阶段 4：文档审查（1 天）

9. **design.md/implement.jsonl/check.jsonl**

**总工作量：** 5-7 天

---

## 六、关键文件清单

### A. Context Window 相关（F5）

| 文件 | 说明 | 优先级 |
|------|------|--------|
| `src-tauri/src/infra/codex_config/types.rs:24-25` | `model_context_window` 字段 | P0 |
| `src-tauri/src/infra/codex_model_catalog/managed.rs:38` | 托管目录 context_window | P0 |
| `src-tauri/src/gateway/proxy/cx2cc/settings.rs` | CX2CC context window 映射 | P0 |
| `src/components/cli-manager/tabs/CodexTab.tsx` | 前端 GPT-5.4/5.6 配置 | P0 |

### B. 路由与递归保护（F1/F2）

| 文件 | 说明 | 优先级 |
|------|------|--------|
| `src-tauri/src/gateway/proxy/handler/middleware/managed_model_route.rs` | 标记 CX2CC | P0 |
| `src-tauri/src/gateway/proxy/handler/middleware/recursion_guard.rs` | 白名单本地网关 | P0 |

### C. Thinking 透传（F3）

| 文件 | 说明 | 优先级 |
|------|------|--------|
| `src-tauri/src/gateway/proxy/protocol_bridge/inbound/anthropic.rs` | 解析 thinking | P1 |

---

## 七、总结（第二轮）

### 主要发现修正

1. **❌ 第一轮"已有"结论全部错误**：CX2CC 会命中配置模型路由、thinking 透传未端到端验证、递归保护需修复
2. **❌ 第一轮遗漏核心问题 R8**：CX2CC 无 context window 传递机制，导致压缩异常
3. **✅ upstream 无 CX2CC 相关变更**：所有 CX2CC 实现均为 fork 特有
4. **⚠️ e2d03792 不解决 R8**：rectifier 仅标准化格式，未传递模型能力

### 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| F5 context window 方案复杂度高 | 高 | 分阶段实施，优先 GPT-5.4 |
| F1/F2 引入新 bug | 中 | 覆盖正负测试 + 回归测试 |
| F3 Anthropic API 格式变化 | 低 | 查阅官方文档验证 |

### 下一步行动

1. **立即启动（P0）**：F1（0.5d）+ F2（0.5d）+ F5（2-3d）
2. **补充验证（P1）**：F3（1d）+ F4（与 F5 合并）
3. **文档补齐（1d）**：design.md、implement.jsonl、check.jsonl

---

**审计完成。** 本报告已纠正第一轮所有错误结论，补充 R8 完整分析，明确端到端证据要求。推荐优先实施 F1/F2/F5（P0，3-4 天），再补充 F3/F4（P1，1-2 天）。
