# Process Runtime Legacy Note

The process runtime PoC is an unsupported pre-release legacy runtime. It is not part of the public Plugin API v1 community runtime surface. Community plugins must use Extension Host.

This page is retained only to explain why older JSON-RPC over stdio references are not a current authoring path. Process execution remains disabled by default and has no marketplace enablement.

## Historical Shape

The old PoC explored child processes using JSON-RPC over stdio:

```json
{"jsonrpc":"2.0","id":1,"method":"plugin.handleHook","params":{"hook":"gateway.request.afterBodyRead","context":{}}}
```

That protocol is not accepted for community plugins in the current public contract.

## Current Migration Target

Use Extension Host instead:

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

`dist/extension.js` registers behavior with `api.gateway.registerHook`. Host lifecycle, activation, per-invocation hook timeout budget, failure policy, and dispose remain host-owned.
