# Provider `model_catalog_url` 不能代替本地受管文件

检查日期：2026-10-09。上游来源是当天的 `openai/codex` `main`，不是本仓库已验证过的某个 Codex 发行版。

## 两个入口

根配置 `model_catalog_json` 是本地 JSON 文件路径。Codex 在构造 models manager 时，只要这个静态目录存在，就直接返回 `StaticModelsManager`，不再看 provider 上的目录 URL。

`model_providers.<id>.model_catalog_url` 是另一个字段。上游 `ModelProviderInfo` 把它定义为 Codex 原生目录的完整 URL。当前 `main` 的 `codex-rs/model-provider/src/provider.rs` 在 `create_models_manager` 里先处理静态目录；只有静态目录不存在时，才在 `model_catalog_url` 有值时调用 `with_provider_catalog()`。

## URL 实际做什么

`codex-rs/model-provider/src/models_endpoint.rs` 对这个 URL 发 HTTP GET。测试使用 `http://<mock>/models` 或 `http://<mock>/codex/models`，并带上 provider 的 Authorization、自定义 header 和 query。响应必须是 Codex 原生 `ModelsResponse`：顶层 `models` 数组，slug 非空且唯一。重定向会被拒绝，响应还有字节上限。

API key 发现还要单独打开。`provider.rs` 的测试把 `set_api_key_model_discovery_enabled(false)` 时，即使 URL 已配置，返回目录里也没有该 provider 模型。LiteLLM 的 Codex 客户端说明把同一要求写成 `[features] api_key_model_discovery = true`，并写明 Codex CLI 0.159 起才有这条路径。本仓库没有用当前安装的 Codex 复验这个版本门槛。

## 和 AIO 受管文件的差别

AIO 生成的是本地文件 `cli-proxy/codex/managed-model-catalog.json`，再把绝对路径写进根 `model_catalog_json`。文件里除了 `models`，还有 `_aio_managed_model_catalog` owner 元数据。`model_catalog_url` 没有本地路径形式，不能把这个文件路径写进去。

AIO 网关现有的 `GET /v1/models` 是 OpenAI 列表转发，不是这份 Codex 原生目录。上游 issue openai/codex#37122 记录了标准 OpenAI `data[].id` 列表无法通过这个原生解析。

因此：可以另做 HTTP 端点，把同一份目录以 URL 交给 `model_providers.aio.model_catalog_url`，并留着根字段不动。这仍然不是“把受管文件写进 URL”。只要用户根配置里还有 `model_catalog_json`，Codex 会忽略这个 URL。
