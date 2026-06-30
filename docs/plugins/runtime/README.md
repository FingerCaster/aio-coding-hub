# 插件运行时说明

这里解释插件运行时如何执行，以及当前哪些能力已经开放。Extension Host 是唯一 community runtime。旧 WASM、process 和第三方 native 只作为 unsupported pre-release legacy runtime notes 保留，不是当前推荐路径。

- [流式响应插件](./streaming.md)：`gateway.response.chunk`、sliding window 和 `stream.modify` 的边界。
- [WASM legacy note](./wasm.md)：说明旧 WASM runtime 为什么不属于公开 community runtime。
- [Process runtime legacy note](./process-poc.md)：说明旧 JSON-RPC over stdio PoC 为什么不属于公开 community runtime。

## Extension Host Runtime Lifecycle

This is a host-owned lifecycle. Plugins declare `runtime.kind = "extensionHost"`, `main`, activation events, contributions, and capabilities in Plugin API v1, but they do not create, retain, dispose, or inspect runtime instances directly.

0.62.3 treats runtime lifecycle as a host-owned internal contract:

1. **Load**: parse manifest and load the bundled JavaScript output referenced by `main`.
2. **Activate**: call `activate(api)` in an Extension Host worker and expose only capability-gated APIs.
3. **Execute**: run a bounded hook invocation with timeout and mutation checks.
4. **Retain**: keep only worker/runtime state that corresponds to the current enabled plugin snapshot.
5. **Dispose**: clear runtime state when plugins are disabled, updated, uninstalled, or when the gateway plugin snapshot is replaced.

Gateway hooks are registered with `api.gateway.registerHook`. Hook execution evidence is written to `plugin_hook_execution_reports`. These reports let the GUI and developer workflow show duration, status, timeout, circuit state, budget rejection, mutation summary, and replayability without exposing a new plugin-callable diagnostics API.

The lifecycle boundary exists to prevent long-lived plugin state from outliving its installed snapshot. Any future runtime that allocates memory, opens handles, or starts child processes must implement the same Load / Activate / Execute / Retain / Dispose contract before it can participate in gateway execution.

## Release Guard

0.62.3 keeps Plugin API v1 externally stable while hardening host runtime internals. Run `pnpm check:plugin-hardening` before release branches that touch plugin runtime loading, hook context budgets, output mutation budgets, SDK validation, or manifest/runtime documentation.
