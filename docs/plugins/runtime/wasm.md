# WASM Legacy Runtime Note

WASM is an unsupported pre-release legacy runtime in the public plugin docs. It is not part of the public Plugin API v1 community runtime surface, and community plugins must migrate to Extension Host.

Current community manifests must use `runtime.kind = "extensionHost"` with `main` pointing to bundled JavaScript output. Gateway integration belongs in `contributes.gatewayHooks`, and runtime behavior is registered from `activate(api)` with `api.gateway.registerHook`.

## Why This Page Still Exists

Older design notes and experimental packages may mention WASM ABI shapes. Those packages are not a supported community distribution path for Plugin API v1. Keeping this page avoids broken links while making the current contract explicit.

## Migration Shape

Use Extension Host:

```json
{
  "main": "dist/extension.js",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  },
  "contributes": {
    "gatewayHooks": [{ "name": "gateway.request.afterBodyRead" }]
  },
  "capabilities": ["gateway.hooks"]
}
```

Move deterministic hook behavior into `dist/extension.js`:

```js
module.exports.activate = function(api) {
  api.gateway.registerHook("gateway.request.afterBodyRead", function(context) {
    return { action: "continue" };
  });
};
```

Any future binary or sandboxed runtime would need a new public contract, lifecycle registry ownership, signing policy, compatibility story, timeout model, resource limits, and marketplace policy before it could be presented as a community runtime.
