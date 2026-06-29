# aio-coding-hub Extension Host-only Plugin Architecture Design

日期：2026-06-29

## Status

This spec supersedes these earlier plugin architecture documents for the current
unreleased branch:

- `docs/superpowers/specs/2026-06-27-aio-coding-hub-plugin-extension-host-platform-design.md`
- `docs/superpowers/specs/2026-06-27-aio-coding-hub-plugin-runtime-lifecycle-registry-design.md`

Those documents are still useful historical context, but their legacy
`declarativeRules` compatibility path is no longer the product direction. The
new direction is Extension Host-only for community plugins.

## Summary

The plugin system should converge on one public runtime: a host-managed
Extension Host process running JavaScript or TypeScript bundle output. The
plugin authoring model is `plugin.json + main extension module + host-declared
contributions + capabilities`.

`declarativeRules` should be removed from the public plugin API, SDK,
scaffolding, devtools, runtime dispatch, examples, and user-facing
documentation. Its old gateway behavior must be replaced by Extension Host
gateway hooks so existing plugin use cases, such as prompt helpers, redactors,
and response guards, remain possible through the new architecture.

The target is not an embedded browser plugin platform. aio-coding-hub is a
Tauri 2 desktop GUI application for Linux, macOS, and Windows. Plugins run
outside the main Rust process and outside the main React component tree. The
host owns UI rendering, lifecycle, permissions, resource limits, diagnostics,
installation, disable/uninstall behavior, and compatibility checks.

## Goals

1. Make Extension Host the only community plugin runtime.
2. Remove `declarativeRules` and `gatewayRules` as public plugin APIs.
3. Replace old rule-based gateway mutation with Extension Host `gatewayHooks`.
4. Enforce capabilities at manifest validation time and at runtime Host API
   call time.
5. Route Extension Host command execution through a managed lifecycle registry
   instead of starting a fresh process for every command.
6. Keep official host-owned features, such as the official privacy filter,
   working without exposing third-party native runtime.
7. Define clear UI extension boundaries for page-level extension without letting
   plugins import or patch internal React components.
8. Define a protocol bridge path for future OpenAI, Claude, Gemini, and custom
   protocol translation.
9. Preserve functional coverage for existing plugin use cases through the new
   Extension Host model.
10. Add testable resource limits to avoid unbounded process growth, stale
    instances, and OOM-prone behavior.

## Non-Goals

This phase does not build a general multi-language runtime platform.

Specifically, this phase does not:

- Keep `declarativeRules` as a legacy community runtime.
- Keep `gatewayRules` as a nested legacy contribution.
- Support third-party `native` plugins.
- Promote WASM as a public plugin language.
- Promote arbitrary process plugins as a public plugin language.
- Allow plugin-authored React components to run inside the main frontend bundle.
- Allow plugins to monkey patch host pages.
- Allow plugins to control the Tauri WebView directly.
- Add marketplace accounts, rating, comments, payment, or remote moderation.
- Add unrestricted network, file system, shell command, or secret access.
- Solve every protocol bridge case in the first implementation.

## Assumptions

- This branch has not been released, so breaking the previous unpublished plugin
  shape is acceptable.
- Existing user-facing app behavior must remain stable. The plugin authoring API
  can change because it has not shipped as a stable public ecosystem.
- A personal project does not need enterprise-grade secret redaction, policy
  administration, or marketplace governance in this phase.
- Host-side enforcement is required even when SDK and devtools also validate
  manifests. SDK validation is developer help, not security or correctness
  authority.

## Architecture Decision

Use a single community plugin path:

```text
plugin package
  plugin.json
  dist/extension.js
  optional assets
  optional examples/fixtures

installed plugin
  manifest validation
  contribution indexing
  capability registry validation
  ExtensionHostInstanceRegistry
  host-rendered UI slots
  gateway hook dispatch
  protocol bridge dispatch
  command dispatch
  diagnostics/runtime reports
```

The central boundary is:

```text
Plugin code can declare contributions and call allowed Host APIs.
Host code decides when contributions appear, when plugin code runs, what data is
provided, what mutations are accepted, and when the process is destroyed.
```

This keeps the platform extensible without turning the app into an unbounded
browser-like runtime.

## Current State Audit

The current branch already contains useful Extension Host foundations:

- `packages/plugin-sdk/src/index.ts` defines `extensionHost`,
  `contributes`, `capabilities`, commands, provider contributions, UI slots,
  `gatewayHooks`, and `protocol.bridge` naming.
- `src-tauri/src/domain/plugins.rs` validates Extension Host manifests
  separately from legacy runtimes.
- `src-tauri/src/app/plugins/extension_host.rs` can start an Extension Host
  process and execute commands.
- `src-tauri/src/app/plugins/extension_host_worker.rs` injects a JavaScript API
  surface and supports command registration.
- `src/plugins/contributions/ContributionSlot.tsx` can render host-owned UI
  contributions.
- `src/pages/providers/ProviderEditorDialog.tsx` already has a provider editor
  contribution slot.
- `src-tauri/src/app/plugins/runtime_lifecycle.rs` has the beginning of a
  lifecycle registry abstraction.

The main gaps are:

1. `declarativeRules` still exists across SDK, docs, scaffold templates,
   devtools, runtime executor, Rust domain types, tests, and older specs.
2. `gatewayRules` still exists as a compatibility-shaped contribution.
3. Capabilities are mostly checked as known strings, not enforced as runtime
   access control.
4. `ExtensionHostApiHandler` currently exposes storage and diagnostics without
   checking the owning plugin's declared capabilities.
5. `execute_plugin_command` uses a cold-start path:
   `execute_extension_host_command_once` starts a host, executes one command,
   then disposes it.
6. Extension Host is not yet wired into gateway hook execution, so deleting
   rule runtime without replacement would remove existing gateway plugin
   behavior.
7. Lifecycle registry has sync cache/instance hooks, but not an async,
   host-managed Extension Host instance registry.

## Public Plugin Language

The public language should be JavaScript runtime code generated from TypeScript.
The official docs and scaffolding should teach TypeScript first.

Accepted public package shape:

```json
{
  "id": "publisher.plugin-name",
  "name": "Plugin Name",
  "version": "0.1.0",
  "apiVersion": "1.0.0",
  "main": "dist/extension.js",
  "runtime": {
    "kind": "extensionHost",
    "language": "typescript"
  },
  "activationEvents": ["onStartup"],
  "contributes": {},
  "capabilities": [],
  "hostCompatibility": {
    "app": ">=0.62.0 <1.0.0",
    "pluginApi": "^1.0.0"
  }
}
```

Runtime values that should not be accepted for community plugins:

- `declarativeRules`
- `wasm`
- `process`
- `native`
- `native:privacyFilter`

The official privacy filter remains a host-owned built-in capability. It should
not become a third-party runtime kind.

## Manifest v1 Target Shape

### Required Fields

- `id`
- `name`
- `version`
- `apiVersion`
- `main`
- `runtime.kind = "extensionHost"`
- `runtime.language = "typescript"` or `"javascript"`
- `hostCompatibility`

### Optional Fields

- `activationEvents`
- `contributes`
- `capabilities`
- `configSchema`
- `engines`
- `display`
- `publisher`
- `description`
- `homepage`
- `repository`

### Removed Public Fields

These fields should be removed from the public community plugin contract:

- top-level `hooks`
- top-level `permissions`
- `runtime.kind = "declarativeRules"`
- `runtime.rules`
- `contributes.gatewayRules`

Gateway integration moves to `contributes.gatewayHooks`, and data access moves
to capabilities.

## Contribution Points

### Commands

Manifest:

```json
{
  "contributes": {
    "commands": [
      {
        "command": "acme.echo",
        "title": "Echo"
      }
    ]
  },
  "capabilities": ["commands.execute"]
}
```

Runtime behavior:

- Plugin code registers command handlers during activation.
- Host rejects command execution if the command is not declared.
- Host rejects command execution if the plugin is disabled.
- Host rejects command execution if the plugin lacks `commands.execute`.
- Command execution uses a managed Extension Host instance.

### UI Slots

Manifest:

```json
{
  "contributes": {
    "ui": {
      "providers.editor.sections": [
        {
          "id": "openrouter-routing",
          "title": "Routing",
          "schema": {
            "type": "section",
            "fields": [
              {
                "type": "text",
                "key": "route",
                "label": "Route"
              }
            ]
          }
        }
      ]
    }
  },
  "capabilities": ["provider.extensionValues"]
}
```

Runtime behavior:

- UI is host-rendered from schema.
- Plugin code does not mount React components in the host tree.
- UI contributions disappear immediately when the plugin is disabled,
  quarantined, uninstalled, or updated to a manifest without that contribution.
- UI actions can invoke declared commands if `commands.execute` is present.

Supported first-class slot families:

- `providers.editor.sections`
- `providers.card.badges`
- `settings.sections`
- `logs.detail.tabs`
- `home.overview.cards`
- `plugins.detail.actions`

Slots outside this list are rejected until the host page explicitly opts in.

### Providers

Manifest:

```json
{
  "contributes": {
    "providers": [
      {
        "providerType": "openrouter",
        "displayName": "OpenRouter",
        "targetCliKeys": ["claude", "codex"],
        "extensionNamespace": "openrouter"
      }
    ]
  },
  "capabilities": ["provider.extensionValues"]
}
```

Runtime behavior:

- The host stores provider extension values under a plugin/provider namespace.
- Extension values are only available to the owning plugin and host-owned
  provider execution paths.
- Provider editor schema is host-rendered.
- Provider execution can read extension values only through host-mediated APIs.

### Gateway Hooks

Manifest:

```json
{
  "contributes": {
    "gatewayHooks": [
      {
        "name": "gateway.request.beforeSend",
        "priority": 100
      }
    ]
  },
  "capabilities": ["gateway.hooks"]
}
```

Gateway hook names for the first implementation:

- `gateway.request.afterBodyRead`
- `gateway.request.beforeSend`
- `gateway.response.chunk`
- `gateway.response.after`
- `gateway.error`

Reserved hook names remain rejected until the host wires them:

- `gateway.request.received`
- `gateway.request.beforeProviderResolution`
- `gateway.response.headers`

Execution contract:

```ts
type GatewayHookRequest = {
  hook: string;
  traceId: string;
  providerId?: string;
  protocol?: "openai" | "claude" | "gemini" | "unknown";
  headers?: Record<string, string>;
  bodyText?: string;
  streamChunkText?: string;
  metadata: Record<string, unknown>;
};

type GatewayHookResult =
  | { action: "continue" }
  | { action: "warn"; message: string }
  | { action: "block"; statusCode?: number; message: string }
  | {
      action: "replace";
      headers?: Record<string, string>;
      bodyText?: string;
      streamChunkText?: string;
    }
  | {
      action: "appendMessage";
      role: "system" | "user" | "assistant";
      content: string;
    };
```

Host rules:

- Gateway hooks require `gateway.hooks`.
- Hook runtime timeout is 150 ms by default.
- Hook failure policy is fail-open by default.
- `block` is only accepted for hooks whose host contract allows blocking.
- Streaming hooks receive bounded chunks and must return bounded chunks.
- Every execution records an extension execution report.
- Trace replay stores enough input/output budget metadata to debug behavior
  without turning reports into unbounded payload storage.

This is the replacement for old `declarativeRules` behavior.

### Protocol Bridges

Manifest:

```json
{
  "contributes": {
    "protocolBridges": [
      {
        "id": "openai-to-claude",
        "from": "openai",
        "to": "claude"
      }
    ]
  },
  "capabilities": ["protocol.bridge"]
}
```

First implementation boundary:

- Define manifest validation.
- Define contribution indexing.
- Define diagnostics and replay naming.
- Implement only a minimal host dispatch path where the gateway explicitly asks
  a bridge to normalize request or response payloads.

This phase does not promise complete OpenAI, Claude, Gemini, and Responses API
feature parity. The important architectural decision is that protocol
translation belongs behind an Extension Host contribution point, not behind
hardcoded provider-specific UI fields or JSON rules.

## Capability Model

Capabilities are active contracts, not labels. They must be enforced in three
places:

1. SDK validation for developer feedback.
2. Rust manifest validation for installation and enablement.
3. Host API dispatch for runtime enforcement.

### Capability Matrix

| Capability | Allows contributions | Allows Host APIs |
| --- | --- | --- |
| `commands.execute` | `contributes.commands`, UI action commands | command registration and command execution |
| `storage.plugin` | none required | `api.storage.get`, `api.storage.set` |
| `diagnostics.read` | diagnostic UI actions | `api.diagnostics.getRuntimeReports` |
| `provider.extensionValues` | `contributes.providers`, provider editor UI sections | provider extension value read/write through host APIs |
| `gateway.hooks` | `contributes.gatewayHooks` | gateway hook activation and execution |
| `protocol.bridge` | `contributes.protocolBridges` | protocol bridge activation and execution |

### Validation Rules

- A manifest with `contributes.commands` must include `commands.execute`.
- A manifest with UI actions that invoke plugin commands must include
  `commands.execute`.
- A manifest with `contributes.providers` must include
  `provider.extensionValues`.
- A manifest with UI slot `providers.editor.sections` must include
  `provider.extensionValues`.
- A manifest with `contributes.gatewayHooks` must include `gateway.hooks`.
- A manifest with `contributes.protocolBridges` must include
  `protocol.bridge`.
- Unknown capabilities are rejected.
- Reserved future capabilities are rejected until wired.

### Runtime Enforcement

The worker may avoid injecting unauthorized API modules, but that is only a
developer ergonomics layer. The Rust host handler is the authority.

Host API calls without the required capability return:

```text
PLUGIN_EXTENSION_HOST_FORBIDDEN
```

The error message should identify the missing capability without leaking
unrelated plugin state.

## Extension Host Lifecycle

### Registry

Add an `ExtensionHostInstanceRegistry` and register it with the app-level
runtime lifecycle boundary.

Responsibilities:

- Start Extension Host instances.
- Reuse warm instances.
- Serialize per-plugin execution in the first implementation.
- Dispose instances on plugin disable, quarantine, uninstall, update, rollback,
  refresh removal, app shutdown, and idle timeout.
- Replace stale instances when manifest identity changes.
- Record cold start versus warm execution in diagnostics.
- Bound global process count.

### Instance Key

Use this key:

```text
pluginId
version
installedDir
main
runtime.kind
runtime.language
contributionHash
```

`contributionHash` is computed from manifest fields that affect runtime API
shape and dispatch:

- `main`
- `activationEvents`
- `contributes`
- `capabilities`
- `runtime`

If any of these change, the old instance must not be reused.

### Resource Limits

First implementation constants:

- Extension Host start timeout: 5 seconds.
- Command execution timeout: 10 seconds.
- Gateway hook execution timeout: 150 ms.
- Protocol bridge execution timeout: 500 ms for MVP normalization calls.
- Per-plugin concurrent execution: 1.
- Global warm Extension Host instance limit: 8.
- Idle recycle after: 120 seconds without use.
- Storage budget: keep existing 64 KiB plugin storage budget unless a later
  storage spec changes it.
- JSON-RPC line size: keep existing bounded worker line size and reject
  over-limit messages.

When the global warm instance limit is exceeded, dispose the least recently used
idle instance. Active executions are not killed only to satisfy the warm limit;
they are governed by their own timeouts.

### Dispose Triggers

The registry must dispose by plugin id when:

- plugin is disabled
- plugin is quarantined
- plugin is uninstalled
- plugin update starts
- plugin rollback starts
- plugin manifest is refreshed and no longer matches the active key

The registry must dispose all instances when:

- app is shutting down
- plugin subsystem is reloaded
- runtime lifecycle registry receives `dispose_all`

### Crash and Failure Behavior

- If an Extension Host process exits unexpectedly, mark the instance dead and
  remove it from the registry.
- The next execution may cold-start a new instance unless the plugin is disabled
  or quarantined.
- Consecutive startup failures should be recorded in runtime reports.
- This phase records failures but does not need a complex circuit breaker beyond
  avoiding reuse of dead instances.

## Command Execution Flow

Target flow:

```text
plugin_execute_command IPC
  normalize command
  find enabled plugin that declares command
  validate runtime is extensionHost
  validate commands.execute
  registry.execute_command(plugin detail, command, args)
    get or start instance
    activate plugin if needed
    execute registered command
    record report
```

The old flow `start -> execute one command -> dispose` should be removed.

`find_declared_command_owner` can remain a simple scan for this phase, but
command indexing is allowed if implementation naturally needs it. The critical
fix is lifecycle ownership, not command lookup performance.

## UI Extension Boundary

There are two UI extension levels:

1. Host-rendered schema contributions.
2. Future sandboxed custom views.

This phase implements and supports host-rendered schema contributions only.

Rules:

- Plugins cannot import host React components.
- Plugins cannot provide arbitrary JSX.
- Plugins cannot patch existing pages.
- Each page exposes stable slot ids.
- Each slot defines allowed schema element types.
- The host owns layout, styling, validation, disabled states, loading states,
  empty states, and error rendering.

This still supports page-level extension in the product sense. For example, a
plugin can add sections to the provider editor or tabs to request logs. It just
does so through stable host slots rather than arbitrary frontend code injection.

Future sandbox custom views may use a separate WebView or iframe-like boundary,
but that is a later spec because it needs stricter memory, messaging, CSP,
focus, accessibility, and teardown rules.

## Deleting declarativeRules

### Delete From Public Contract

Remove or reject:

- `runtime.kind = "declarativeRules"`
- `runtime.rules`
- top-level `hooks`
- top-level `permissions`
- `contributes.gatewayRules`
- rule JSON schema docs
- rule replay/explain devtools commands
- rule scaffold templates

### Delete From Host Runtime

Remove community rule runtime dispatch:

- `PluginRuntime::DeclarativeRules`
- `RuleRuntimeGatewayPluginExecutor`
- `rule_runtime.rs` community execution path
- rule runtime cache registration
- rule runtime tests that only prove the removed runtime

Keep or migrate tests that validate gateway behavior by rewriting them against
Extension Host gateway hooks.

### Keep Official Host Runtime

Do not remove official privacy filter behavior. It is not the community
`declarativeRules` runtime. It should remain host-owned and isolated from the
public plugin language decision.

### Compatibility Behavior

Because the branch is unreleased, no automatic migration is required.

Behavior:

- New installation of a `declarativeRules` plugin is rejected.
- Existing local DB entries with `declarativeRules` are marked disabled and
  unsupported during plugin refresh/list normalization.
- UI copy should explain that the plugin uses an unsupported pre-release plugin
  runtime.
- The app should not try to execute unsupported legacy plugins.

## Devtools and Scaffolding

`create-aio-plugin` should generate Extension Host TypeScript plugins by
default.

Default templates:

1. command plugin
2. provider editor plugin
3. gateway hook plugin
4. protocol bridge skeleton

Devtools should support:

- validate manifest
- doctor package
- pack package
- publish check
- run command locally where possible
- gateway hook fixture replay through Extension Host
- protocol bridge fixture replay once MVP bridge dispatch exists

Devtools should not support:

- `create-aio-plugin rule`
- declarative rule replay
- declarative rule explain
- JSON rule templates

## Diagnostics and Replay

Every Extension Host execution path should record runtime reports:

- command execution
- gateway hook execution
- protocol bridge execution
- provider contribution execution when host dispatch exists

Report fields should include:

- plugin id
- contribution type
- contribution id
- command or hook name
- trace id when available
- status
- failure kind
- error code
- started timestamp
- duration
- cold start boolean
- input budget summary
- output budget summary
- mutation summary
- replayable boolean

Replay fixtures should be bounded. Store shape and budget summaries in normal
reports; export full replay payloads only through explicit debug/export paths.

## Error Model

Use stable error codes for predictable UI and tests:

- `PLUGIN_UNSUPPORTED_RUNTIME`
- `PLUGIN_INVALID_RUNTIME`
- `PLUGIN_INVALID_CONTRIBUTION`
- `PLUGIN_MISSING_CAPABILITY`
- `PLUGIN_EXTENSION_HOST_FORBIDDEN`
- `PLUGIN_EXTENSION_HOST_START_TIMEOUT`
- `PLUGIN_EXTENSION_HOST_COMMAND_TIMEOUT`
- `PLUGIN_EXTENSION_HOST_GATEWAY_TIMEOUT`
- `PLUGIN_EXTENSION_HOST_PROTOCOL_TIMEOUT`
- `PLUGIN_EXTENSION_HOST_PROCESS_EXITED`
- `PLUGIN_COMMAND_NOT_FOUND`
- `PLUGIN_COMMAND_PLUGIN_DISABLED`
- `PLUGIN_COMMAND_RUNTIME_UNSUPPORTED`

Validation errors should happen as early as possible. Runtime errors should be
recorded and surfaced without crashing the Tauri app.

## Product Impact

For normal users:

- Plugin page should present fewer technical runtime choices.
- Installed plugins should show what they add: provider, UI section, command,
  gateway hook, protocol bridge, diagnostics.
- Unsupported pre-release rule plugins should be visibly disabled.

For plugin authors:

- There is one recommended path: TypeScript Extension Host.
- The SDK manifest types match the host.
- Devtools generate and validate the same model the app runs.
- Gateway and protocol work is code-based and testable through fixtures.

For maintainers:

- Runtime dispatch has one community execution model.
- Capability checks are centralized and testable.
- Resource lifecycle has one owner.
- Official host-owned features remain separate from community runtime policy.

## Testing Strategy

### SDK Tests

Add or update tests for:

- Extension Host manifest passes.
- `declarativeRules` manifest fails.
- `gatewayRules` contribution fails.
- top-level `hooks` and `permissions` fail for community Extension Host
  manifest.
- each contribution requiring a capability fails when missing.
- unknown capabilities fail.
- valid command, UI, provider, gateway hook, and protocol bridge contributions
  pass with required capabilities.

### Rust Domain Tests

Add or update tests for:

- Rust manifest validation mirrors SDK validation.
- unsupported legacy runtimes are rejected.
- unsupported local DB legacy plugins normalize to disabled/unsupported.
- contribution slot ids outside the allowed set fail.
- reserved gateway hooks still fail.

### Extension Host API Tests

Add tests for:

- storage API succeeds with `storage.plugin`.
- storage API fails without `storage.plugin`.
- diagnostics API succeeds with `diagnostics.read`.
- diagnostics API fails without `diagnostics.read`.
- command registration/execution fails without `commands.execute`.
- plugin id mismatch still fails.

### Lifecycle Tests

Add tests for:

- two commands for the same plugin reuse one warm instance.
- manifest contribution hash change starts a new instance.
- disable/uninstall/update dispose the instance.
- idle recycle disposes unused instance.
- global warm instance limit evicts least recently used idle instance.
- unexpected process exit removes the dead instance.

### Gateway Hook Tests

Add tests for:

- Extension Host gateway hook can return `continue`.
- Extension Host gateway hook can return `replace` for request body.
- Extension Host gateway hook can return `warn`.
- unsupported action for a hook is rejected.
- timeout records a report and applies fail-open behavior.
- stream chunk hook respects bounded chunk size.

### UI Tests

Add tests for:

- provider editor renders host schema contribution.
- contribution disappears when plugin is disabled.
- UI action command is disabled or rejected when capability is missing.
- unsupported slot id is ignored or rejected according to validation path.

### Verification Commands

Expected command families:

```bash
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter create-aio-plugin test
pnpm test
cd src-tauri && cargo test --lib
```

Use narrower test commands during development, then run the full relevant set
before claiming completion.

## Acceptance Criteria

The architecture work is complete when:

1. Public SDK types no longer accept `declarativeRules`.
2. Public docs no longer recommend `declarativeRules`.
3. Scaffolded plugins are Extension Host TypeScript plugins.
4. New installs with `declarativeRules` are rejected.
5. Existing legacy local entries are disabled/unsupported and never executed.
6. Commands execute through `ExtensionHostInstanceRegistry`.
7. Repeated command execution reuses a warm instance.
8. Runtime Host APIs are denied without required capabilities.
9. `gatewayHooks` can cover the core old rule use cases: prompt append,
   request replace, response warn/block, response replace, and stream chunk
   inspect/modify where supported.
10. Official privacy filter still works.
11. The app does not expose WASM, process, or native as community runtime
    choices.
12. Tests prove lifecycle dispose paths for disable, uninstall, update, idle,
    and app-level dispose.

## Implementation Plan Boundary

The implementation plan should be split into small reviewable tasks:

1. Contract and SDK cleanup.
2. Rust manifest/domain cleanup.
3. Devtools and scaffold migration.
4. Capability registry and runtime enforcement.
5. Extension Host instance registry.
6. Command execution migration.
7. Gateway hook dispatch MVP.
8. Protocol bridge manifest and minimal dispatch skeleton.
9. Example plugin migration.
10. Docs and product UI copy cleanup.
11. Full verification and review.

Each task should leave the repo in a compilable or narrowly testable state. The
plan may temporarily keep removed code behind failing references only within a
single task, but no task should end with both old and new public plugin models
presented as supported choices.

