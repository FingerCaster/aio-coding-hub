# Privacy Filter Extension Host Migration Design

日期：2026-06-30

## Summary

Privacy Filter 必须迁移为普通 Extension Host 插件。迁移后，Plugin API v1 不再保留官方 host-native Privacy Filter runtime 特例；`official.privacy-filter` 的官方身份只代表宿主随包发布、可从官方入口安装、可带默认配置，不代表它能绕过 runtime、manifest、capability、hook 或生命周期校验。

本次迁移的核心不是重写脱敏算法，而是去掉运行时分叉。现有 Rust 脱敏引擎可以保留为标准 Extension Host host API 能力，但该能力必须通过公开 capability 声明、统一校验和统一生命周期暴露，不能按插件 ID 特判。

## Goals

1. `official.privacy-filter` 使用 `runtime.kind = "extensionHost"` 和 `main` 入口运行。
2. 删除官方 Privacy Filter 的 host-native runtime dispatch、manifest validation 特判和 runtime cache 特例。
3. 删除 Plugin API v1 contract、SDK、自检脚本和文档中的 official native runtime 概念。
4. 保持用户可见功能一致：请求发送前脱敏、日志持久化前脱敏、配置 UI、官方安装入口和插件详情行为不回退。
5. 把脱敏能力整理为标准 Extension Host capability，例如 `privacy.redact`，供任何声明并获授权的 Extension Host 插件使用。
6. 确保生命周期、超时、dispose、缓存清理都走 Extension Host Registry 和 Runtime Lifecycle Registry，不再有 Privacy Filter 专属 runtime 生命周期。

## Non-Goals

- 不把 Tauri2 GUI 做成浏览器容器。
- 不引入第二种社区插件语言或 runtime。
- 不用 JS 从零重写完整 secret detector、gitleaks rule parser 和误报规避算法。
- 不改变普通用户安装官方 Privacy Filter 的产品路径。
- 不放宽插件权限、capability 或 package 安装校验。
- 不新增完整插件市场功能。

## Current State

当前分支已经把社区插件主线切到 Extension Host，但 Privacy Filter 仍是最后一个官方 runtime 例外：

- 官方 manifest 仍声明 host-native privacy engine。
- `validate_manifest_for_official_plugin` 对官方 Privacy Filter 放行 host-native runtime。
- `PluginRuntimeManager` 会把官方 Privacy Filter 分发到专属 runtime。
- `RuntimeGatewayPluginExecutor` 持有专属 Privacy Filter runtime cache。
- 文档和 contract 仍把官方 Privacy Filter 描述为 host-owned built-in runtime。
- SDK 主线已经只允许 Extension Host，但 contract selftest 和宿主 Rust 层仍保留旧 official runtime。

这些残留会让架构继续分叉：社区插件走 Extension Host，官方插件走专属 runtime。长期看，这会削弱 runtime lifecycle registry、capability model、diagnostics、trace replay 和插件 API 文档的一致性。

## Architecture Decision

采用 **Extension Host 插件 + 标准 host privacy API**。

`official.privacy-filter` 自身变成一个普通 Extension Host 插件：

- manifest 声明 `main: "dist/extension.js"`。
- manifest 声明 `runtime.kind = "extensionHost"`。
- manifest 通过 `contributes.gatewayHooks` 声明三个 hook：
  - `gateway.request.afterBodyRead`
  - `gateway.request.beforeSend`
  - `log.beforePersist`
- manifest 声明 `capabilities`：
  - `gateway.hooks`
  - `privacy.redact`
- manifest 不声明 top-level `hooks` 或 top-level `permissions`。

Extension Host worker 在激活时，如果插件声明并获得 `privacy.redact` capability，就注入 `api.privacy`：

```ts
api.privacy.redactText(text, options)
api.privacy.redactRequestBody(body, options)
```

该 host API 是标准能力，不按 `official.privacy-filter` 特判。任何 Extension Host 插件只要声明 `privacy.redact` 并通过宿主授权，就可以调用同一套脱敏能力。

Rust 层保留现有脱敏引擎，但将 hook 相关逻辑从专属 runtime 中拆出为普通 service：

```text
privacy_filter.rs
  低层 detector、rules、PII、secret redaction

privacy_redaction_service.rs
  load rules from plugin root
  cache compiled filter by plugin runtime key
  redactText
  redactRequestBody
  options/scopes parsing
```

`ExtensionHostApiHandler` 负责：

- 校验 caller pluginId 与当前 Extension Host 实例一致。
- 校验 `privacy.redact` capability。
- 从该插件安装目录加载 `rules/gitleaks.toml`，继续使用 1 MiB rule file 上限。
- 对 text 或 request body 执行脱敏。
- 返回结构化结果给 JS 插件。

## No Special-Case Rule

迁移完成后必须满足：

- 没有 runtime dispatch 分支按 `official.privacy-filter` 选择专属 Privacy Filter executor。
- 没有 manifest validation 分支按 `official.privacy-filter` 放行 host-native runtime。
- 没有 Plugin API contract 字段表达 official native runtime。
- 没有 SDK 类型允许 Privacy Filter 使用非 Extension Host runtime。
- 没有 gateway pipeline 为官方 Privacy Filter 绕过 Extension Host。
- 没有生命周期 cache 专门服务官方 Privacy Filter runtime。

允许保留的官方差异只有：

- 官方 catalog 仍可列出 `official.privacy-filter`。
- 官方安装入口仍可安装它。
- 官方资源目录仍可随包带上它的 manifest、JS 入口、rules 和 config schema。
- 官方默认配置仍可由宿主提供。

这些差异属于分发和默认体验，不属于 runtime 特例。

## Data Flow

### Install

1. 用户点击官方 Privacy Filter 安装。
2. 宿主从 official resources 读取 package files。
3. 使用普通 Extension Host manifest validation。
4. materialize 到插件安装目录。
5. 保存插件详情、config schema、默认配置、capability grants 和 audit。

### Request Redaction

1. Gateway pipeline 命中 `gateway.request.afterBodyRead` 或 `gateway.request.beforeSend`。
2. `RuntimeGatewayPluginExecutor` 只看到 Extension Host runtime。
3. `ExtensionHostInstanceRegistry` 启动或复用 `official.privacy-filter` 的 Extension Host worker。
4. `dist/extension.js` 的 hook handler 读取 `ctx.config` 和 `ctx.context.request.body`。
5. 如果 `redactBeforeUpstream` 为 false 或没有 body，返回 pass。
6. 否则调用 `api.privacy.redactRequestBody(body, options)`。
7. 有变化时返回 `{ action: "replace", requestBody }`。

### Log Redaction

1. Gateway log pipeline 命中 `log.beforePersist`。
2. Extension Host 插件读取 `ctx.context.log.message`。
3. 如果 `redactLogs` 为 false 或没有 message，返回 pass。
4. 否则调用 `api.privacy.redactText(message, options)`。
5. 有变化时返回 `{ action: "replace", logMessage }`。

### Dispose

1. 插件禁用、卸载、quarantine、版本变化或 registry idle recycle 时，Extension Host Registry dispose worker。
2. Privacy redaction service cache 跟随 runtime lifecycle key 清理。
3. 不再有独立官方 Privacy Filter runtime cache。

## API Shape

SDK 增加 capability：

```ts
export type PluginCapability =
  | "commands.execute"
  | "storage.plugin"
  | "diagnostics.read"
  | "provider.extensionValues"
  | "provider.requestPreparation"
  | "provider.modelDiscovery"
  | "provider.healthCheck"
  | "protocol.bridge"
  | "gateway.hooks"
  | "privacy.redact";
```

SDK 增加 host API 类型：

```ts
export type PrivacyRedactionOptions = {
  sensitiveTypes?: string[];
  redactionScopes?: string[];
};

export type PrivacyRedactionResult = {
  hit: boolean;
  count: number;
  redacted: string;
};

export type PrivacyApi = {
  redactText(text: string, options?: PrivacyRedactionOptions): PrivacyRedactionResult;
  redactRequestBody(body: string, options?: PrivacyRedactionOptions): PrivacyRedactionResult;
};
```

Extension Host 插件作者使用：

```ts
export function activate(api) {
  api.gateway.registerHook("gateway.request.beforeSend", (ctx) => {
    const body = ctx.context.request?.body;
    if (!body) return { action: "pass" };
    const result = api.privacy.redactRequestBody(body, {
      sensitiveTypes: ctx.config?.sensitiveTypes,
      redactionScopes: ctx.config?.redactionScopes,
    });
    return result.hit
      ? { action: "replace", requestBody: result.redacted }
      : { action: "pass" };
  });
}
```

## Functional Compatibility

Privacy Filter 迁移后必须继续支持：

- Codex/OpenAI Responses payload：
  - `instructions`
  - `input` string
  - `input[].content[].text`
  - `function_call_output.output`
- Claude-style payload：
  - `system`
  - `messages[].content`
  - `tool_result.content`
- OpenAI-compatible chat payload：
  - `messages[].content`
  - role `tool` content
- legacy raw text / `prompt`
- log message redaction
- `sensitiveTypes`
- `redactionScopes`
- gzip decoded request mutation path
- no-op behavior when no selected field contains sensitive text

必须继续避免脱敏：

- tool schema
- tool call arguments
- metadata
- reasoning/thinking blocks
- file/image IDs
- URLs/base64 binary payloads，除非已有 Rust allowlist 明确处理

## Testing Strategy

### Rust Domain And Service Tests

- 普通 manifest validation 接受 official Privacy Filter 的 Extension Host manifest。
- 普通 manifest validation 拒绝所有 host-native runtime。
- official install path 不再调用 runtime 特判 validation。
- privacy redaction service 能从插件 root 加载 rules。
- rules 文件超过 1 MiB 时返回明确错误。
- text redaction 保持 emails、手机号、身份证、银行卡、IPv4 和 secret 检测行为。
- request body redaction 保持 Codex、Claude、OpenAI-compatible 和 legacy prompt allowlist 行为。

### Extension Host Tests

- worker 只在 `privacy.redact` capability 存在时注入 `api.privacy`。
- 未声明 capability 调用 privacy host API 返回 forbidden。
- `api.privacy.redactText` 返回 `{ hit, count, redacted }`。
- `api.privacy.redactRequestBody` 返回 request body redaction result。
- hook handler 返回 `requestBody` 和 `logMessage` 后能被 gateway result parser 接受。

### Gateway Tests

- 官方 Privacy Filter 安装后通过 Extension Host 执行 request hooks。
- gzipped Codex request 仍在 upstream 前被脱敏。
- beforeSend 阶段仍能脱敏最终 upstream body。
- log.beforePersist 仍能脱敏持久化日志 message。
- Extension Host timeout/failure policy 仍按通用 gateway hook 策略生效。

### Contract And Docs Tests

- `check-plugin-api-contract` 不再包含 official native runtime。
- `check-plugin-system-docs` 不再要求 Privacy Filter 是 host-owned native runtime。
- `plugin-manifest-v1.md` 只描述 Extension Host runtime。
- SDK typecheck 覆盖 `privacy.redact` capability 和 `api.privacy` 类型。

### Residual Search Acceptance

最终 tracked files 中不应再出现旧官方 host-native Privacy Filter runtime contract、dispatch 或 manifest engine 声明。允许出现：

- `official.privacy-filter` 作为插件 ID。
- `Privacy Filter` 作为产品名称。
- `privacy-filter` 作为资源目录名或文档 slug。
- `privacy_filter.rs` 作为 Rust 脱敏引擎文件名。

## Rollout And Migration

当前分支尚未发布，所以不做复杂线上迁移。安装库中如存在旧 runtime 记录，启动或安装官方插件时可以采用简单策略：

- 若检测到旧官方 Privacy Filter 记录，重新 materialize 官方 Extension Host package。
- 保留用户 config。
- 更新 manifest/runtime summary。
- 写入 audit event。

因为该版本未发布，不需要支持长期双轨兼容，也不需要保留旧 runtime 执行路径。

## Acceptance Criteria

1. 官方 Privacy Filter manifest 是 Extension Host manifest。
2. 官方 Privacy Filter 有实际 `dist/extension.js` 入口并注册三个 hooks。
3. Gateway request 和 log redaction 行为与迁移前测试矩阵一致。
4. Runtime manager 不再存在 Privacy Filter 专属 dispatch。
5. Runtime executor 不再持有 Privacy Filter 专属 runtime。
6. Manifest validation 不再对官方 Privacy Filter 放行 host-native runtime。
7. Plugin API contract 不再包含 official native runtime。
8. SDK 暴露 `privacy.redact` capability 和 privacy host API 类型。
9. 文档不再说 Privacy Filter 是 host-owned native runtime。
10. 所有相关 Rust、SDK、contract、docs、frontend tests 通过。
