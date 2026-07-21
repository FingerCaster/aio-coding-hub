# Codex 供应商与模型配置语义

## 调研环境

- 调研日期：2026-07-20。
- 官方 Codex 手册通过 `openai-docs` 的手册抓取器刷新到本机缓存：
  `C:\Users\ADMINI~1\AppData\Local\Temp\openai-docs-cache\codex-manual.md`。
- 本机验证版本：`codex-cli 0.144.6`。
- 官方来源页：
  - [Advanced Configuration](https://learn.chatgpt.com/docs/config-file/config-advanced)
  - [Models](https://learn.chatgpt.com/docs/models)
  - [Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)

## 官方文档确认的事实

1. `model` 与 `model_provider` 是两个独立配置值。`model_provider` 选择
   `[model_providers.<id>]` 中的连接定义，`model` 是发送给该连接的模型标识。
2. 自定义 provider 定义连接属性，包括 `base_url`、鉴权、请求头、查询参数、
   重试参数和 `wire_api`。`openai`、`ollama`、`lmstudio` 是保留 ID，不能被自定义
   provider 覆盖。
3. 当前文档明确说 Codex 可以指向支持 Responses API 或 Chat Completions API 的
   任意模型/provider，但 Chat Completions 支持已弃用，未来会移除。新能力应以
   Responses API 兼容为主。
4. `model_provider` 与 `model_providers` 只能放在用户级配置中；项目内
   `.codex/config.toml` 会忽略这些键并给出启动警告。
5. Codex 0.134.0 起不再从 `config.toml` 读取 `[profiles.<name>]`，也不再支持根级
   `profile = "<name>"`。当前 profile 是 `$CODEX_HOME/<name>.config.toml` 独立文件，
   文件内直接写顶层 `model`、`model_provider` 等键，再通过 `--profile <name>` 选择。
6. `model_catalog_json` 是启动时读取的手工模型目录覆盖项，profile 文件也可以覆盖
   它。当前手册没有给出稳定的自定义目录 JSON schema，因此不能仅凭该字段假定可
   安全生成跨版本目录文件。
7. `codex debug models` 显示 Codex 当前模型目录；`--bundled` 会跳过远端刷新。官方
   文档未声称该命令会调用每个自定义 provider 的 OpenAI-compatible `/models`。

## 本机回环实验

### 自定义 provider 的推理路由

对 `codex exec` 临时覆盖以下值，provider 指向只在 `127.0.0.1` 监听的探针服务，
使用虚构 API key：

```toml
model = "grok-4.5"
model_provider = "probe"

[model_providers.probe]
name = "Probe"
base_url = "http://127.0.0.1:47830/v1"
env_key = "PROBE_API_KEY"
wire_api = "responses"
```

探针观察到：

```text
POST /v1/responses
request.model = grok-4.5
Authorization header present = true
```

结论：Codex 原生支持把任意模型字符串与明确的 provider ID 组合起来执行请求；模型
归属不需要也不应由模型名反向猜测。

### 自定义 provider 的模型目录

使用相同的普通 `env_key` 自定义 provider 配置执行 `codex debug models`，探针在整
个命令期间没有收到任何请求，输出仍为 Codex 自身的 GPT 模型目录。

该实验只能证明普通自定义 provider 配置不会自动获得逐 provider `/v1/models` 发
现，不能证明 Codex 在所有认证配置下都不请求模型端点。仓库已有更强的实际证据：
AIO 网关明确接收 Codex 发来的 `GET /v1/models` / `GET /models`，并把它透明转发给
Codex provider；AIO 的受管 provider 配置还使用了 `requires_openai_auth = true`，与
本实验的 `env_key` 配置不同。

因此准确结论是：Codex 没有提供通用的“枚举每个自定义 provider 并保留 provider
归属”机制。AIO 已能把一次模型发现请求 failover 到首个成功上游，但如需“添加供应
商后获取该供应商模型”，仍必须拥有逐 provider 的发现、解析和持久化/缓存策略；现
有 `app-server model/list` 也不能直接替代该能力。

## 对本任务的直接影响

- 用户示例中的 `[profiles.grok]` 属于旧版格式。面向当前 Codex 的输出应是独立的
  `grok.config.toml`，内容例如：

  ```toml
  model = "grok-4.5"
  model_provider = "aio"
  ```

- 如果多个 profile 都通过 AIO 本地网关连接，它们的 Codex `model_provider` 都可以是
  `aio`；真正的上游供应商归属必须由 AIO 使用稳定的 `provider_id` 与模型记录关联，
  不能从 Codex 的 `model_provider = "aio"` 得出。
- 当前网关对 Codex `/models` 的行为是按既有 provider 顺序逐个尝试、每个 provider
  最多一次，并返回首个成功响应；它不聚合多个 provider 的模型，也不把 provider ID
  注入上游的 OpenAI-compatible 响应。
- 供应商模型发现与 Codex 模型能力目录是两个不同领域：前者回答“此上游声称有哪些
  model ID”，后者还可能包含推理档位、上下文窗口、显示名、可见性等 Codex 元数据。
  MVP 不应把只有 `id` 的 `/v1/models` 结果伪装成完整能力目录。

## 证据边界

- 回环实验验证的是当前安装的 0.144.6 和普通 `env_key` 自定义 provider，不代表所
  有历史版本或 `requires_openai_auth` 场景。
- 未向真实第三方供应商发请求，也未使用真实凭据。
- 手册没有承诺统一的第三方模型发现协议；非 OpenAI-compatible provider 需要单独适配
  或手工回退。
