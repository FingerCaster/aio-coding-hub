# aio-coding-hub 0.62.1 Plugin Developer Loop Implementation Plan

> Status: Superseded.
>
> MUST NOT be executed. This historical plan contains obsolete
> declarativeRules and local `replay --explain` success-path assumptions. Use
> the Extension Host-only plan in
> `docs/superpowers/plans/2026-06-29-aio-coding-hub-extension-host-only-plugin-architecture.md`
> and the current docs under `docs/plugins/` instead.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the 0.62.1 plugin developer loop and observability release without changing Plugin API v1 or exposing Provider Plugin API.

**Architecture:** Keep Plugin API v1 stable and add internal support around it: `create-aio-plugin` owns local diagnostics, strict package validation, and declarative-rules replay explanation; the Tauri GUI presents persisted runtime evidence already produced by the gateway; contract checks prevent SDK, docs, scaffold, and devtools drift. Provider work is limited to host-owned acceptance tests.

**Tech Stack:** TypeScript, Vitest, React 19, TanStack Query, Tauri 2 generated IPC bindings, Rust, Cargo tests, Node.js contract scripts, Prettier.

---

## Scope Boundaries

- Do not change the public Plugin API v1 manifest shape.
- Do not add Plugin API v2.
- Do not expose Provider Plugin API.
- Do not enable JS/WebView plugin runtime.
- Do not enable arbitrary marketplace WASM execution by default.
- Do not let plugins control provider selection, failover, OAuth, token counting, or session binding.
- Preserve existing `create-aio-plugin validate <plugin-dir>` and `create-aio-plugin replay <plugin-dir> <fixture.json> <hook>` behavior.

## File Structure

- `packages/create-aio-plugin/src/devtools.ts`: CLI command routing, package reads, manifest validation, doctor diagnostics, strict validation, replay, replay explanation, pack/sign/verify.
- `packages/create-aio-plugin/src/scaffold.test.ts`: existing `create-aio-plugin` unit tests; add developer-loop tests here to keep the package small.
- `scripts/check-plugin-api-contract.mjs`: contract drift checker; extend it so devtools metadata stays aligned with `docs/plugins/plugin-api-v1-contract.json`.
- `scripts/check-plugin-api-contract.selftest.mjs`: synthetic contract-check fixtures proving new drift checks fail for mismatches and pass for aligned metadata.
- `src/pages/PluginsPage.tsx`: plugin detail panel; add runtime failure and audit evidence UI.
- `src/pages/__tests__/PluginsPage.test.tsx`: frontend render and action tests for plugin observability.
- `src-tauri/src/gateway/proxy/handler/provider_order.rs`: provider ordering acceptance tests.
- `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs`: session-bound provider acceptance tests.
- `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs`: cx2cc request/response acceptance tests.
- `src-tauri/src/domain/provider_oauth_limits.rs`: OAuth snapshot acceptance tests.
- `docs/plugins/developer-guide.md`: developer workflow docs for doctor, strict validate, replay explain, pack, install, inspect.
- `docs/plugins/reference/sdk.md`: SDK/devtools reference updates.
- `docs/plugins/reference/declarative-rules.md`: replay explain and strict rule diagnostics reference.
- `docs/plugins/reference/compatibility.md`: compatibility boundary updates.

## Task 1: Add Developer Diagnostic Model And `doctor`

**Files:**

- Modify: `packages/create-aio-plugin/src/devtools.ts`
- Test: `packages/create-aio-plugin/src/scaffold.test.ts`

- [ ] **Step 1: Write failing tests for structured diagnostics**

Add `doctorPluginDirectory` and `doctorPluginFiles` to the existing devtools import list in `packages/create-aio-plugin/src/scaffold.test.ts`:

```ts
import {
  doctorPluginDirectory,
  doctorPluginFiles,
  generateSigningKeyPair,
  packPlugin,
  packPluginBytes,
  packPluginDirectory,
  replayHook,
  runCreateAioPluginCli,
  signPackage,
  validatePluginDirectory,
  validatePluginFiles,
  verifyPackage,
} from "./devtools";
```

Add these tests inside the existing `describe("create-aio-plugin scaffold", () => { ... })` block:

```ts
it("doctor reports a structured error when plugin.json is missing", () => {
  const result = doctorPluginFiles({});

  expect(result.ok).toBe(false);
  expect(result.diagnostics).toEqual([
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_MISSING_MANIFEST",
      path: "plugin.json",
    }),
  ]);
});

it("doctor reports missing rule files and policy-gated wasm runtime", () => {
  const ruleFiles = createPluginScaffold({
    id: "acme.redactor",
    name: "Redactor",
    template: "rule",
  });
  delete ruleFiles["rules/main.json"];

  const ruleResult = doctorPluginFiles(ruleFiles);

  expect(ruleResult.ok).toBe(false);
  expect(ruleResult.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_RULE_FILE_MISSING",
      path: "rules/main.json",
    })
  );

  const wasmResult = doctorPluginFiles(
    createPluginScaffold({ id: "acme.policy", name: "Policy", template: "wasm" })
  );

  expect(wasmResult.ok).toBe(false);
  expect(wasmResult.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_WASM_ENTRY_MISSING",
      path: "plugin.wasm",
    })
  );
  expect(wasmResult.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "warn",
      code: "PLUGIN_WASM_POLICY_GATED",
    })
  );
});

it("doctor command reads a real plugin directory and returns non-zero for errors", () => {
  const root = mkdtempSync(join(tmpdir(), "aio-plugin-doctor-"));
  writeScaffold(
    root,
    createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" })
  );
  const output: string[] = [];

  expect(
    runCreateAioPluginCli(["doctor", root], process.cwd(), {
      log: (line) => output.push(line),
      error: (line) => output.push(line),
    })
  ).toBe(0);
  expect(JSON.parse(output[0] ?? "{}")).toMatchObject({ ok: true });

  const brokenRoot = mkdtempSync(join(tmpdir(), "aio-plugin-doctor-broken-"));
  writeFileSync(join(brokenRoot, "README.md"), "# broken\n");
  const brokenOutput: string[] = [];

  expect(
    runCreateAioPluginCli(["doctor", brokenRoot], process.cwd(), {
      log: (line) => brokenOutput.push(line),
      error: (line) => brokenOutput.push(line),
    })
  ).toBe(1);
  expect(JSON.parse(brokenOutput[0] ?? "{}")).toMatchObject({
    ok: false,
    diagnostics: [expect.objectContaining({ code: "PLUGIN_MISSING_MANIFEST" })],
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
```

Expected: FAIL because `doctorPluginFiles` and `doctorPluginDirectory` are not exported.

- [ ] **Step 3: Add the diagnostic types and `doctor` implementation**

In `packages/create-aio-plugin/src/devtools.ts`, update usage text:

```ts
const USAGE = [
  "Usage:",
  "  create-aio-plugin <publisher.plugin-name> [rule|wasm]",
  "  create-aio-plugin doctor <plugin-dir>",
  "  create-aio-plugin validate [--strict] <plugin-dir>",
  "  create-aio-plugin replay [--explain] <plugin-dir> <fixture.json> <hook>",
  "  create-aio-plugin pack <plugin-dir>",
].join("\n");
```

Add these exported types after `CliIo`:

```ts
export type DiagnosticSeverity = "error" | "warn" | "info";

export type PluginDiagnostic = {
  severity: DiagnosticSeverity;
  code: string;
  message: string;
  path?: string;
  hint?: string;
};

export type DoctorResult = {
  ok: boolean;
  diagnostics: PluginDiagnostic[];
  manifest?: {
    id: string;
    name: string;
    version: string;
    runtime: PluginManifest["runtime"]["kind"];
  };
};

type DoctorOptions = {
  strict?: boolean;
};
```

Add the `doctor` command before the `validate` command branch:

```ts
if (commandOrId === "doctor") {
  try {
    const result = doctorPluginDirectory(resolve(cwd, firstArg ?? "."));
    const text = JSON.stringify(result);
    if (result.ok) {
      io.log(text);
      return 0;
    }
    io.error(text);
    return 1;
  } catch (error) {
    io.error(`failed to inspect plugin directory: ${errorMessage(error)}`);
    return 1;
  }
}
```

Add these exported helpers near `validatePluginDirectory`:

```ts
export function doctorPluginDirectory(root: string, options: DoctorOptions = {}): DoctorResult {
  return doctorPluginFiles(readPluginDirectory(root), options);
}

export function doctorPluginFiles(files: ScaffoldFiles, options: DoctorOptions = {}): DoctorResult {
  const diagnostics: PluginDiagnostic[] = [];
  const manifestText = files["plugin.json"];

  if (!manifestText) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_MISSING_MANIFEST",
      message: "missing plugin.json",
      path: "plugin.json",
      hint: "Run create-aio-plugin <publisher.plugin-name> rule or add a Plugin API v1 manifest.",
    });
    return { ok: false, diagnostics };
  }

  let manifest: PluginManifest;
  try {
    manifest = JSON.parse(manifestText) as PluginManifest;
  } catch (error) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST_JSON",
      message: `plugin.json is not valid JSON: ${errorMessage(error)}`,
      path: "plugin.json",
      hint: "Fix plugin.json before running validate, replay, or pack.",
    });
    return { ok: false, diagnostics };
  }

  const validation = validateManifest(manifest);
  if (!validation.ok) {
    diagnostics.push({
      severity: "error",
      code: validation.error.code,
      message: validation.error.message,
      path: "plugin.json",
      hint: "Update the manifest so it matches Plugin API v1.",
    });
  }

  if (manifest.runtime.kind === "declarativeRules") {
    for (const rulePath of manifest.runtime.rules) {
      if (!files[rulePath]) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_FILE_MISSING",
          message: `declarative rule file is missing: ${rulePath}`,
          path: rulePath,
          hint: "Add the rule file or remove it from runtime.rules.",
        });
      }
    }
  }

  if (manifest.runtime.kind === "wasm") {
    const entry = manifest.entry ?? "plugin.wasm";
    if (!files[entry]) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_WASM_ENTRY_MISSING",
        message: `wasm entry file is missing: ${entry}`,
        path: entry,
        hint: "Build the WASM artifact before packing the plugin.",
      });
    }
    diagnostics.push({
      severity: "warn",
      code: "PLUGIN_WASM_POLICY_GATED",
      message: "WASM runtime remains policy-gated by the host.",
      hint: "Do not rely on marketplace WASM execution unless host policy enables it.",
    });
  }

  if (options.strict) {
    diagnostics.push(...strictRuleDiagnostics(files, manifest));
  }

  return {
    ok: !hasErrorDiagnostics(diagnostics),
    diagnostics,
    manifest: {
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      runtime: manifest.runtime.kind,
    },
  };
}

function hasErrorDiagnostics(diagnostics: readonly PluginDiagnostic[]): boolean {
  return diagnostics.some((diagnostic) => diagnostic.severity === "error");
}

function strictRuleDiagnostics(_files: ScaffoldFiles, _manifest: PluginManifest): PluginDiagnostic[] {
  return [];
}
```

- [ ] **Step 4: Run tests to verify the task passes**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
pnpm --filter create-aio-plugin typecheck
```

Expected: PASS for both commands.

- [ ] **Step 5: Commit**

```bash
git add packages/create-aio-plugin/src/devtools.ts packages/create-aio-plugin/src/scaffold.test.ts
git commit -m "feat(plugin-devtools): add plugin doctor diagnostics"
```

## Task 2: Add `validate --strict` Package And Rule Checks

**Files:**

- Modify: `packages/create-aio-plugin/src/devtools.ts`
- Test: `packages/create-aio-plugin/src/scaffold.test.ts`

- [ ] **Step 1: Write failing tests for strict validation**

Add `validatePluginFilesStrict` to the devtools import list.

Add these tests inside the existing `describe("create-aio-plugin scaffold", () => { ... })` block:

```ts
it("validate strict rejects malformed declarative rule documents", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
  files["rules/main.json"] = "{ bad json";

  const result = validatePluginFilesStrict(files);

  expect(result.ok).toBe(false);
  expect(result.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_RULE_FILE_INVALID_JSON",
      path: "rules/main.json",
    })
  );
});

it("validate strict rejects rules whose hook is not declared by the manifest", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
  const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
    hooks: Array<{ name: string; priority?: number }>;
  };
  manifest.hooks = [{ name: "gateway.response.after", priority: 100 }];
  files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

  const result = validatePluginFilesStrict(files);

  expect(result.ok).toBe(false);
  expect(result.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_RULE_HOOK_NOT_DECLARED",
      path: "rules/main.json#/rules/0/hook",
    })
  );
});

it("validate strict rejects missing permissions for mutating declarative rules", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
  const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
    permissions: string[];
  };
  manifest.permissions = ["request.body.read"];
  files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

  const result = validatePluginFilesStrict(files);

  expect(result.ok).toBe(false);
  expect(result.diagnostics).toContainEqual(
    expect.objectContaining({
      severity: "error",
      code: "PLUGIN_RULE_PERMISSION_MISMATCH",
      message: expect.stringContaining("request.body.write"),
    })
  );
});

it("legacy validate remains manifest-only while validate strict reports package errors", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
  delete files["rules/main.json"];

  expect(validatePluginFiles(files)).toEqual({ ok: true });
  expect(validatePluginFilesStrict(files)).toMatchObject({
    ok: false,
    diagnostics: [expect.objectContaining({ code: "PLUGIN_RULE_FILE_MISSING" })],
  });
});

it("validate strict command preserves the old validate command shape unless strict is requested", () => {
  const root = mkdtempSync(join(tmpdir(), "aio-plugin-strict-"));
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
  delete files["rules/main.json"];
  writeScaffold(root, files);
  const normalOutput: string[] = [];
  const strictOutput: string[] = [];

  expect(
    runCreateAioPluginCli(["validate", root], process.cwd(), {
      log: (line) => normalOutput.push(line),
      error: (line) => normalOutput.push(line),
    })
  ).toBe(0);
  expect(JSON.parse(normalOutput[0] ?? "{}")).toEqual({ ok: true });

  expect(
    runCreateAioPluginCli(["validate", "--strict", root], process.cwd(), {
      log: (line) => strictOutput.push(line),
      error: (line) => strictOutput.push(line),
    })
  ).toBe(1);
  expect(JSON.parse(strictOutput[0] ?? "{}")).toMatchObject({
    ok: false,
    diagnostics: [expect.objectContaining({ code: "PLUGIN_RULE_FILE_MISSING" })],
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
```

Expected: FAIL because `validatePluginFilesStrict` and strict CLI routing are missing.

- [ ] **Step 3: Add strict validation result and CLI routing**

In `packages/create-aio-plugin/src/devtools.ts`, add this type after `DoctorResult`:

```ts
export type StrictValidationResult =
  | { ok: true; diagnostics: PluginDiagnostic[] }
  | { ok: false; error: { code: string; message: string }; diagnostics: PluginDiagnostic[] };
```

Replace the current `validate` command branch with:

```ts
if (commandOrId === "validate") {
  const strict = firstArg === "--strict";
  const pluginDir = strict ? secondArg : firstArg;
  try {
    const root = resolve(cwd, pluginDir ?? ".");
    if (strict) {
      const result = validatePluginDirectoryStrict(root);
      const text = JSON.stringify(result);
      if (result.ok) {
        io.log(text);
        return 0;
      }
      io.error(text);
      return 1;
    }
    io.log(JSON.stringify(validatePluginDirectory(root)));
    return 0;
  } catch (error) {
    io.error(`failed to validate plugin directory: ${errorMessage(error)}`);
    return 1;
  }
}
```

Add these exported helpers near `validatePluginDirectory`:

```ts
export function validatePluginDirectoryStrict(root: string): StrictValidationResult {
  return validatePluginFilesStrict(readPluginDirectory(root));
}

export function validatePluginFilesStrict(files: ScaffoldFiles): StrictValidationResult {
  const result = doctorPluginFiles(files, { strict: true });
  const firstError = result.diagnostics.find((diagnostic) => diagnostic.severity === "error");
  if (!firstError) {
    return { ok: true, diagnostics: result.diagnostics };
  }
  return {
    ok: false,
    error: { code: firstError.code, message: firstError.message },
    diagnostics: result.diagnostics,
  };
}
```

- [ ] **Step 4: Add strict declarative-rules diagnostics**

Replace the temporary `strictRuleDiagnostics` helper in `packages/create-aio-plugin/src/devtools.ts` with:

```ts
const RULE_TARGET_FIELDS_BY_HOOK: Record<string, readonly string[]> = {
  "gateway.request.afterBodyRead": ["request.body"],
  "gateway.request.beforeSend": ["request.body"],
  "gateway.response.after": ["response.body"],
  "gateway.response.chunk": ["stream.chunk"],
  "gateway.error": ["request.body", "response.body"],
  "log.beforePersist": ["log.message"],
};

function strictRuleDiagnostics(files: ScaffoldFiles, manifest: PluginManifest): PluginDiagnostic[] {
  if (manifest.runtime.kind !== "declarativeRules") return [];

  const diagnostics: PluginDiagnostic[] = [];
  const declaredHooks = new Set<string>(manifest.hooks.map((hook) => hook.name));
  const grantedPermissions = new Set<string>(manifest.permissions);

  for (const rulePath of manifest.runtime.rules) {
    const text = files[rulePath];
    if (!text) continue;

    let document: { rules?: unknown[] };
    try {
      document = JSON.parse(text) as { rules?: unknown[] };
    } catch (error) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_RULE_FILE_INVALID_JSON",
        message: `rule file is not valid JSON: ${errorMessage(error)}`,
        path: rulePath,
        hint: "Fix the rule JSON before replaying or packing the plugin.",
      });
      continue;
    }

    if (!Array.isArray(document.rules)) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_RULES_MISSING_ARRAY",
        message: "rule document must contain a rules array",
        path: `${rulePath}#/rules`,
        hint: "Use { \"rules\": [...] } as the rule document shape.",
      });
      continue;
    }

    document.rules.forEach((rawRule, index) => {
      const rule = asRecord(rawRule);
      if (!rule) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_INVALID",
          message: "rule entry must be an object",
          path: `${rulePath}#/rules/${index}`,
          hint: "Replace the entry with an object containing hook, target, match, and action.",
        });
        return;
      }

      const hook = typeof rule.hook === "string" ? rule.hook : "";
      if (!hook) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_HOOK_MISSING",
          message: "rule hook is required",
          path: `${rulePath}#/rules/${index}/hook`,
          hint: "Set hook to one of the hooks declared in plugin.json.",
        });
      } else if (!declaredHooks.has(hook)) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_HOOK_NOT_DECLARED",
          message: `rule hook is not declared in plugin.json: ${hook}`,
          path: `${rulePath}#/rules/${index}/hook`,
          hint: "Add the hook to manifest.hooks or change the rule hook.",
        });
      }

      const target = asRecord(rule.target);
      const action = asRecord(rule.action);
      const targetField = typeof target?.field === "string" ? target.field : "request.body";
      const actionKind = typeof action?.kind === "string" ? action.kind : "";
      const allowedFields = RULE_TARGET_FIELDS_BY_HOOK[hook] ?? [];
      if (hook && allowedFields.length > 0 && !allowedFields.includes(targetField)) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_TARGET_INCOMPATIBLE_WITH_HOOK",
          message: `target field ${targetField} is not compatible with hook ${hook}`,
          path: `${rulePath}#/rules/${index}/target/field`,
          hint: `Use one of: ${allowedFields.join(", ")}.`,
        });
      }

      for (const permission of permissionsForRuleTarget(targetField, actionKind)) {
        if (!grantedPermissions.has(permission)) {
          diagnostics.push({
            severity: "error",
            code: "PLUGIN_RULE_PERMISSION_MISMATCH",
            message: `rule targeting ${targetField} with action ${actionKind || "unknown"} requires ${permission}`,
            path: `${rulePath}#/rules/${index}`,
            hint: `Add ${permission} to manifest.permissions or change the rule target/action.`,
          });
        }
      }
    });
  }

  return diagnostics;
}

function permissionsForRuleTarget(field: string, actionKind: string): string[] {
  const mutates = actionKind === "replace" || actionKind === "appendMessage";
  switch (field) {
    case "response.body":
      return mutates ? ["response.body.read", "response.body.write"] : ["response.body.read"];
    case "stream.chunk":
      return mutates ? ["stream.inspect", "stream.modify"] : ["stream.inspect"];
    case "log.message":
      return ["log.redact"];
    case "request.body":
    default:
      return mutates ? ["request.body.read", "request.body.write"] : ["request.body.read"];
  }
}
```

- [ ] **Step 5: Run tests to verify the task passes**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
pnpm --filter create-aio-plugin typecheck
```

Expected: PASS for both commands.

- [ ] **Step 6: Commit**

```bash
git add packages/create-aio-plugin/src/devtools.ts packages/create-aio-plugin/src/scaffold.test.ts
git commit -m "feat(plugin-devtools): add strict plugin validation"
```

## Task 3: Add `replay --explain`

**Files:**

- Modify: `packages/create-aio-plugin/src/devtools.ts`
- Test: `packages/create-aio-plugin/src/scaffold.test.ts`

- [ ] **Step 1: Write failing tests for replay explanation**

Add `replayHookExplain` to the devtools import list.

Add these tests inside the existing `describe("create-aio-plugin scaffold", () => { ... })` block:

```ts
it("replay explain reports evaluated rules when no rule matches", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });

  const result = replayHookExplain(files, "gateway.request.afterBodyRead", {
    request: { body: JSON.stringify({ messages: [{ role: "user", content: "hello" }] }) },
  });

  expect(result).toMatchObject({
    pluginId: "acme.real",
    runtime: "declarativeRules",
    hook: "gateway.request.afterBodyRead",
    evaluatedRuleCount: 1,
    matchedRuleIds: [],
    actionKind: null,
    outputKind: "pass",
    result: { action: "pass" },
  });
});

it("replay explain reports matched rule ids and replacement summary", () => {
  const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });

  const result = replayHookExplain(files, "gateway.request.afterBodyRead", {
    request: {
      body: JSON.stringify({ messages: [{ role: "user", content: "SECRET_TOKEN" }] }),
    },
  });

  expect(result).toMatchObject({
    evaluatedRuleCount: 1,
    matchedRuleIds: ["redact-token-rule"],
    actionKind: "replace",
    outputKind: "replace",
    mutationSummary: {
      changed: true,
      field: "requestBody",
      targetField: "request.body",
      jsonPath: "$.messages[*].content",
    },
  });
  expect(JSON.stringify(result)).toContain("[REDACTED]");
});

it("replay explain reports block and warn actions", () => {
  const blockResult = replayHookExplain(
    rulePluginFilesWithAction({ kind: "block", reason: "blocked" }),
    "gateway.request.afterBodyRead",
    { request: { body: "danger" } }
  );
  expect(blockResult).toMatchObject({
    matchedRuleIds: ["redact-token-rule"],
    actionKind: "block",
    outputKind: "block",
    mutationSummary: { changed: false },
  });

  const warnResult = replayHookExplain(
    rulePluginFilesWithAction({ kind: "warn", message: "careful" }),
    "gateway.request.afterBodyRead",
    { request: { body: "danger" } }
  );
  expect(warnResult).toMatchObject({
    matchedRuleIds: ["redact-token-rule"],
    actionKind: "warn",
    outputKind: "warn",
    mutationSummary: { changed: false },
  });
});

it("replay explain command emits JSON explanation", () => {
  const root = mkdtempSync(join(tmpdir(), "aio-plugin-explain-"));
  writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));
  const fixturePath = join(root, "fixture.json");
  writeFileSync(
    fixturePath,
    JSON.stringify({
      request: { body: JSON.stringify({ messages: [{ role: "user", content: "SECRET_TOKEN" }] }) },
    })
  );
  const output: string[] = [];

  expect(
    runCreateAioPluginCli(
      ["replay", "--explain", root, fixturePath, "gateway.request.afterBodyRead"],
      process.cwd(),
      {
        log: (line) => output.push(line),
        error: (line) => output.push(line),
      }
    )
  ).toBe(0);

  expect(JSON.parse(output[0] ?? "{}")).toMatchObject({
    pluginId: "acme.real",
    outputKind: "replace",
    matchedRuleIds: ["redact-token-rule"],
  });
});

it("legacy replay keeps JavaScript best-effort regex behavior while explain reports Rust-aware diagnostics", () => {
  const toggledFiles = rulePluginFilesWithTarget(undefined);
  const toggledDocument = JSON.parse(toggledFiles["rules/main.json"] ?? "{}") as {
    rules?: Array<Record<string, unknown>>;
  };
  const toggledRule = toggledDocument.rules?.[0];
  if (toggledRule) {
    toggledRule.match = { regex: "(?i)sec(?-i)ret" };
  }
  toggledFiles["rules/main.json"] = `${JSON.stringify(toggledDocument, null, 2)}\n`;

  expect(
    replayHook(toggledFiles, "gateway.request.afterBodyRead", {
      request: { body: "SECRET token" },
    })
  ).toEqual({ action: "pass" });
  expect(
    replayHookExplain(toggledFiles, "gateway.request.afterBodyRead", {
      request: { body: "SECRET token" },
    })
  ).toMatchObject({
    outputKind: "pass",
    warnings: [expect.objectContaining({ code: "PLUGIN_REPLAY_REGEX_UNSUPPORTED" })],
  });

  const unicodeFiles = rulePluginFilesWithTarget(undefined);
  const unicodeDocument = JSON.parse(unicodeFiles["rules/main.json"] ?? "{}") as {
    rules?: Array<Record<string, unknown>>;
  };
  const unicodeRule = unicodeDocument.rules?.[0];
  if (unicodeRule) {
    unicodeRule.match = { regex: "\\p{L}+" };
  }
  unicodeFiles["rules/main.json"] = `${JSON.stringify(unicodeDocument, null, 2)}\n`;

  expect(
    replayHook(unicodeFiles, "gateway.request.afterBodyRead", {
      request: { body: "é token" },
    })
  ).toEqual({ action: "pass" });
  expect(
    replayHookExplain(unicodeFiles, "gateway.request.afterBodyRead", {
      request: { body: "é token" },
    })
  ).toMatchObject({
    matchedRuleIds: ["redact-token-rule"],
    outputKind: "replace",
    result: { action: "replace", requestBody: "[REDACTED] [REDACTED]" },
  });
});
```

Update `rulePluginFilesWithRule` so test rule ids are stable and not coupled to scaffold text:

```ts
if (rule) {
  rule.id = "redact-token-rule";
  rule.hook = options.hook;
  rule.target = options.target;
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
```

Expected: FAIL because `replayHookExplain` and `replay --explain` routing are missing.

- [ ] **Step 3: Add explanation types and CLI routing**

In `packages/create-aio-plugin/src/devtools.ts`, add these types after `ReplayRuleResult`:

```ts
export type ReplayMutationSummary = {
  changed: boolean;
  field?: "requestBody" | "responseBody" | "streamChunk" | "logMessage";
  targetField?: string;
  jsonPath?: string;
  beforeLength?: number;
  afterLength?: number;
};

export type ReplayExplainResult = {
  pluginId: string;
  runtime: PluginManifest["runtime"]["kind"];
  hook: GatewayHookName;
  evaluatedRuleCount: number;
  matchedRuleIds: string[];
  actionKind: string | null;
  outputKind: ReplayRuleResult["action"];
  mutationSummary: ReplayMutationSummary;
  warnings: PluginDiagnostic[];
  result: ReplayRuleResult;
};

type ReplayRuleTrace = {
  result: ReplayRuleResult;
  matched: boolean;
  ruleId: string;
  actionKind: string | null;
  targetField?: string;
  jsonPath?: string;
  warning?: PluginDiagnostic;
};
```

Replace the current `replay` command branch with:

```ts
if (commandOrId === "replay") {
  const explain = firstArg === "--explain";
  const pluginDir = explain ? secondArg : firstArg;
  const fixturePath = explain ? thirdArg : secondArg;
  const hookName = explain ? args[4] : thirdArg;
  if (!pluginDir || !fixturePath || !hookName) {
    io.error("Usage: create-aio-plugin replay [--explain] <plugin-dir> <fixture.json> <hook>");
    return 1;
  }
  try {
    const files = readPluginDirectory(resolve(cwd, pluginDir));
    const fixture = JSON.parse(readFileSync(resolve(cwd, fixturePath), "utf8")) as unknown;
    const hook = hookName as GatewayHookName;
    io.log(JSON.stringify(explain ? replayHookExplain(files, hook, fixture) : replayHook(files, hook, fixture)));
    return 0;
  } catch (error) {
    io.error(`failed to replay plugin hook: ${errorMessage(error)}`);
    return 1;
  }
}
```

Add the exported explanation helper below `replayHook`:

```ts
export function replayHookExplain(
  files: ScaffoldFiles,
  hook: GatewayHookName,
  context: unknown
): ReplayExplainResult {
  const validation = validatePluginFilesStrict(files);
  if (!validation.ok) {
    throw new Error(`${validation.error.code}: ${validation.error.message}`);
  }

  const manifest = JSON.parse(files["plugin.json"] ?? "{}") as PluginManifest;
  const warnings: PluginDiagnostic[] = [];
  if (manifest.runtime.kind !== "declarativeRules") {
    warnings.push({
      severity: "warn",
      code: "PLUGIN_REPLAY_UNSUPPORTED_RUNTIME",
      message: `replay explain only supports declarativeRules runtime, got ${manifest.runtime.kind}`,
      hint: "Use host runtime logs for non-declarative runtimes.",
    });
    return {
      pluginId: manifest.id,
      runtime: manifest.runtime.kind,
      hook,
      evaluatedRuleCount: 0,
      matchedRuleIds: [],
      actionKind: null,
      outputKind: "pass",
      mutationSummary: { changed: false },
      warnings,
      result: { action: "pass" },
    };
  }

  let evaluatedRuleCount = 0;
  const matchedRuleIds: string[] = [];
  let finalTrace: ReplayRuleTrace | null = null;

  for (const rulePath of manifest.runtime.rules) {
    const document = JSON.parse(files[rulePath] ?? "{\"rules\":[]}") as { rules?: unknown[] };
    for (const [index, rule] of (document.rules ?? []).entries()) {
      evaluatedRuleCount += 1;
      const trace = replayDeclarativeRuleWithTrace(rule, hook, context, `${rulePath}#${index + 1}`);
      if (trace.warning) warnings.push(trace.warning);
      if (trace.matched) matchedRuleIds.push(trace.ruleId);
      if (trace.result.action !== "pass") {
        finalTrace = trace;
        break;
      }
    }
    if (finalTrace) break;
  }

  const result = finalTrace?.result ?? { action: "pass" };
  return {
    pluginId: manifest.id,
    runtime: manifest.runtime.kind,
    hook,
    evaluatedRuleCount,
    matchedRuleIds,
    actionKind: finalTrace?.actionKind ?? null,
    outputKind: result.action,
    mutationSummary: mutationSummaryFromReplayResult(result, finalTrace),
    warnings,
    result,
  };
}
```

- [ ] **Step 4: Add an explain-only trace path and keep legacy `replayHook` on its old best-effort path**

Keep `replayDeclarativeRule` as the legacy implementation. It must continue compiling regexes with JavaScript's `RegExp` directly and silently pass when JavaScript cannot compile a pattern. This preserves the old local replay contract:

```ts
function replayDeclarativeRule(
  rawRule: unknown,
  hook: GatewayHookName,
  context: unknown
): ReplayRuleResult {
  const rule = asRecord(rawRule);
  if (rule?.hook !== hook) return { action: "pass" };
  const target = asRecord(rule.target);
  const matcher = asRecord(rule.matcher) ?? asRecord(rule.match);
  const action = asRecord(rule.action);
  if (!target || !matcher || !action) return { action: "pass" };
  if (typeof matcher.regex !== "string") return { action: "pass" };

  let regex: RegExp;
  try {
    regex = new RegExp(matcher.regex, matcher.caseSensitive === false ? "gi" : "g");
  } catch {
    return { action: "pass" };
  }

  const targetField = typeof target.field === "string" ? target.field : "request.body";
  const text = textFromFixture(context, targetField);
  if (!text) return { action: "pass" };

  const path =
    typeof target.jsonPath === "string"
      ? target.jsonPath
      : target.kind === "jsonPath" && typeof target.path === "string"
        ? target.path
        : undefined;
  if (
    !path ||
    (target.field && target.field !== "request.body" && target.field !== "response.body")
  ) {
    return replayTextAction(text, regex, action, targetField);
  }
  return replayJsonPathAction(text, path, regex, action, targetField);
}
```

Add the new explain-only helper after `replayDeclarativeRule`:

```ts
function replayDeclarativeRuleWithTrace(
  rawRule: unknown,
  hook: GatewayHookName,
  context: unknown,
  fallbackRuleId: string
): ReplayRuleTrace {
  const rule = asRecord(rawRule);
  const ruleId = typeof rule?.id === "string" ? rule.id : fallbackRuleId;
  if (rule?.hook !== hook) {
    return { result: { action: "pass" }, matched: false, ruleId, actionKind: null };
  }
  const target = asRecord(rule.target);
  const matcher = asRecord(rule.matcher) ?? asRecord(rule.match);
  const action = asRecord(rule.action);
  const actionKind = typeof action?.kind === "string" ? action.kind : null;
  const targetField = typeof target?.field === "string" ? target.field : "request.body";
  const jsonPath =
    typeof target?.jsonPath === "string"
      ? target.jsonPath
      : target?.kind === "jsonPath" && typeof target.path === "string"
        ? target.path
        : undefined;

  if (!target || !matcher || !action) {
    return { result: { action: "pass" }, matched: false, ruleId, actionKind, targetField, jsonPath };
  }
  const compiled = compileReplayRegex(matcher);
  if (!compiled.ok) {
    return {
      result: { action: "pass" },
      matched: false,
      ruleId,
      actionKind,
      targetField,
      jsonPath,
      ...(compiled.warning ? { warning: compiled.warning } : {}),
    };
  }
  const regex = compiled.regex;

  const text = textFromFixture(context, targetField);
  if (!text) {
    return { result: { action: "pass" }, matched: false, ruleId, actionKind, targetField, jsonPath };
  }

  const result =
    !jsonPath || (target.field && target.field !== "request.body" && target.field !== "response.body")
      ? replayTextAction(text, regex, action, targetField)
      : replayJsonPathAction(text, jsonPath, regex, action, targetField);

  return {
    result,
    matched: result.action !== "pass" || replayRegexMatches(text, regex, jsonPath),
    ruleId,
    actionKind,
    targetField,
    jsonPath,
  };
}

function replayRegexMatches(text: string, regex: RegExp, jsonPath: string | undefined): boolean {
  regex.lastIndex = 0;
  if (!jsonPath) return regex.test(text);
  try {
    const root = JSON.parse(text) as unknown;
    let matched = false;
    const segments = parseReplayJsonPath(jsonPath);
    if (!segments) return false;
    applyToJsonStrings(root, segments, (candidate) => {
      regex.lastIndex = 0;
      if (regex.test(candidate.value)) matched = true;
    });
    return matched;
  } catch {
    return false;
  }
}

function mutationSummaryFromReplayResult(
  result: ReplayRuleResult,
  trace: ReplayRuleTrace | null
): ReplayMutationSummary {
  if (result.action !== "replace") return { changed: false };
  if ("requestBody" in result) {
    return summaryForReplacement("requestBody", result.requestBody, trace);
  }
  if ("responseBody" in result) {
    return summaryForReplacement("responseBody", result.responseBody, trace);
  }
  if ("streamChunk" in result) {
    return summaryForReplacement("streamChunk", result.streamChunk, trace);
  }
  if ("logMessage" in result) {
    return summaryForReplacement("logMessage", result.logMessage, trace);
  }
  return { changed: false };
}

function summaryForReplacement(
  field: ReplayMutationSummary["field"],
  value: string,
  trace: ReplayRuleTrace | null
): ReplayMutationSummary {
  return {
    changed: true,
    field,
    targetField: trace?.targetField,
    jsonPath: trace?.jsonPath,
    afterLength: value.length,
  };
}
```

`compileReplayRegex`, `parseReplayRegexPattern`, and `replayRegexFlags` belong only to the explain path. Do not call them from `replayDeclarativeRule` or legacy `replayHook`.

- [ ] **Step 5: Run tests to verify the task passes**

Run:

```bash
pnpm --filter create-aio-plugin test -- src/scaffold.test.ts
pnpm --filter create-aio-plugin typecheck
```

Expected: PASS for both commands, and existing `replayHook` assertions still pass unchanged.

- [ ] **Step 6: Commit**

```bash
git add packages/create-aio-plugin/src/devtools.ts packages/create-aio-plugin/src/scaffold.test.ts
git commit -m "feat(plugin-devtools): explain declarative rule replay"
```

## Task 4: Improve GUI Plugin Runtime Observability

**Files:**

- Modify: `src/pages/PluginsPage.tsx`
- Test: `src/pages/__tests__/PluginsPage.test.tsx`

- [ ] **Step 1: Write failing frontend tests**

In `src/pages/__tests__/PluginsPage.test.tsx`, add a mock for the existing clipboard service near the other mocks:

```ts
vi.mock("../../services/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));
```

Add these tests inside `describe("pages/PluginsPage", () => { ... })`:

```tsx
it("renders runtime failures with hook, failure kind, and trace id", () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);
  vi.mocked(usePluginQuery).mockReturnValue({
    data: detail({
      runtime_failures: [
        {
          id: 10,
          plugin_id: "community.prompt-helper",
          hook_name: "gateway.request.afterBodyRead",
          failure_kind: "timeout",
          message: "Hook timed out",
          trace_id: "trace-runtime-1",
          created_at: 50,
        },
      ],
    }),
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  expect(screen.getByText("运行观测")).toBeInTheDocument();
  expect(screen.getByText("Hook timed out")).toBeInTheDocument();
  expect(screen.getByText("timeout")).toBeInTheDocument();
  expect(screen.getAllByText("gateway.request.afterBodyRead").length).toBeGreaterThan(0);
  expect(screen.getByText("trace-runtime-1")).toBeInTheDocument();
});

it("renders audit events with event type, risk, and trace id", () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);
  vi.mocked(usePluginQuery).mockReturnValue({
    data: detail({
      audit_logs: [
        {
          id: 11,
          plugin_id: "community.prompt-helper",
          trace_id: "trace-audit-1",
          event_type: "plugin.hook.failed",
          risk_level: "high",
          message: "Rule failed open",
          details: { hookName: "gateway.request.afterBodyRead", failureKind: "timeout" },
          created_at: 60,
        },
      ],
    }),
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  expect(screen.getByText("Rule failed open")).toBeInTheDocument();
  expect(screen.getByText("plugin.hook.failed")).toBeInTheDocument();
  expect(screen.getByText("high")).toBeInTheDocument();
  expect(screen.getByText("trace-audit-1")).toBeInTheDocument();
  expect(screen.getAllByText("gateway.request.afterBodyRead").length).toBeGreaterThan(0);
});

it("shows an empty runtime observability state before a hook has run", () => {
  vi.mocked(usePluginsListQuery).mockReturnValue({
    data: [summary()],
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);
  vi.mocked(usePluginQuery).mockReturnValue({
    data: detail({ audit_logs: [], runtime_failures: [] }),
    isLoading: false,
    isFetching: false,
    error: null,
  } as any);

  renderWithProviders(<PluginsPage />);

  expect(screen.getByText("还没有记录到插件运行事件")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
```

Expected: FAIL because the plugin detail panel does not render the new runtime observability section.

- [ ] **Step 3: Add observability helpers and section**

In `src/pages/PluginsPage.tsx`, add imports:

```ts
import { Copy, Download, RotateCcw, Upload, Power, PowerOff, RefreshCw, ShieldAlert, Trash2 } from "lucide-react";
import { copyText } from "../services/clipboard";
```

Add these helpers before `PluginDetailPanel`:

```tsx
function detailValue(details: JsonValue, key: string): string | null {
  const record = jsonRecord(details);
  const value = record?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function TraceIdButton({ traceId }: { traceId: string | null }) {
  if (!traceId) return <span className="text-muted-foreground">-</span>;
  return (
    <Button
      type="button"
      size="sm"
      variant="ghost"
      className="h-7 gap-1 px-1.5 font-mono text-[11px]"
      onClick={() => {
        copyText(traceId)
          .then(() => toast.success("Trace ID 已复制"))
          .catch((error) => toast.error(formatActionFailureToast("复制 Trace ID", error).toast));
      }}
    >
      <Copy className="h-3.5 w-3.5" />
      {traceId}
    </Button>
  );
}

function RuntimeObservabilitySection({ detail }: { detail: PluginDetail }) {
  const hasFailures = detail.runtime_failures.length > 0;
  const hasAuditLogs = detail.audit_logs.length > 0;

  return (
    <Section title="运行观测">
      {!hasFailures && !hasAuditLogs ? (
        <div className="rounded-md border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
          还没有记录到插件运行事件。启用插件并让请求经过 gateway 后，这里会显示 hook、失败类型、审计事件和 trace ID。
        </div>
      ) : null}

      {hasFailures ? (
        <div className="grid gap-2">
          <div className="text-xs font-semibold text-muted-foreground">运行失败</div>
          {detail.runtime_failures.slice(0, 5).map((failure) => (
            <div key={failure.id} className="rounded-md border border-border px-3 py-2 text-sm">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="font-medium text-foreground">{failure.message}</span>
                <span className="rounded-md bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
                  {failure.failure_kind}
                </span>
              </div>
              <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                <span className="font-mono">{failure.hook_name ?? "-"}</span>
                <TraceIdButton traceId={failure.trace_id} />
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {hasAuditLogs ? (
        <div className="grid gap-2">
          <div className="text-xs font-semibold text-muted-foreground">审计事件</div>
          {detail.audit_logs.slice(0, 8).map((log) => {
            const hookName = detailValue(log.details, "hookName");
            const failureKind = detailValue(log.details, "failureKind");
            return (
              <div key={log.id} className="rounded-md border border-border px-3 py-2 text-sm">
                <div className="flex flex-wrap justify-between gap-2">
                  <span className="font-medium text-foreground">{log.message}</span>
                  <span className="text-xs text-muted-foreground">{log.risk_level}</span>
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                  <span className="font-mono">{log.event_type}</span>
                  {hookName ? <span className="font-mono">{hookName}</span> : null}
                  {failureKind ? <span>{failureKind}</span> : null}
                  <TraceIdButton traceId={log.trace_id} />
                </div>
              </div>
            );
          })}
        </div>
      ) : null}
    </Section>
  );
}
```

- [ ] **Step 4: Render the section in `PluginDetailPanel` and remove the old audit-only preview**

In `PluginDetailPanel`, add this section after `Section title="设置"` and before `Section title="开发者信息"`:

```tsx
<RuntimeObservabilitySection detail={detail} />
```

Remove the old `detail.audit_logs.slice(0, 5)` block inside `Section title="开发者信息"` so audit events are shown in one place.

- [ ] **Step 5: Run tests to verify the task passes**

Run:

```bash
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
pnpm typecheck
```

Expected: PASS for both commands.

- [ ] **Step 6: Commit**

```bash
git add src/pages/PluginsPage.tsx src/pages/__tests__/PluginsPage.test.tsx
git commit -m "feat(plugins): show runtime observability in plugin details"
```

## Task 5: Strengthen Plugin API Contract Drift Gates For Devtools

**Files:**

- Modify: `scripts/check-plugin-api-contract.mjs`
- Modify: `scripts/check-plugin-api-contract.selftest.mjs`
- Test: `scripts/check-plugin-api-contract.selftest.mjs`

- [ ] **Step 1: Write failing selftest coverage for devtools drift**

In `scripts/check-plugin-api-contract.selftest.mjs`, add this helper after `writePassingScaffold`:

```js
function writePassingDevtools(root) {
  writeFileSync(
    join(root, "packages/create-aio-plugin/src/devtools.ts"),
    [
      "gateway.request.afterBodyRead gateway.request.beforeSend gateway.response.chunk gateway.response.after gateway.error log.beforePersist",
      "request.body.read request.body.write response.body.read response.body.write stream.inspect stream.modify log.redact",
      "requestBody responseBody streamChunk logMessage headers",
      "declarativeRules wasm",
      "PLUGIN_RULE_PERMISSION_MISMATCH PLUGIN_REPLAY_UNSUPPORTED_RUNTIME PLUGIN_WASM_POLICY_GATED",
      "validatePluginFilesStrict replayHookExplain doctorPluginFiles",
    ].join("\n")
  );
}
```

Call `writePassingDevtools(root);` at the end of `writePassingScaffold(root)`.

Add this negative fixture near the other negative fixtures:

```js
const missingDevtoolsMetadataRoot = makeRoot("missing-devtools-metadata");
writeJson(missingDevtoolsMetadataRoot, "docs/plugins/plugin-api-v1-contract.json", {
  apiVersion: "1.0.0",
  defaultHookTimeoutMs: 150,
  defaultFailurePolicy: "fail-open",
  activeHooks: ["gateway.request.afterBodyRead"],
  reservedHooks: ["gateway.response.headers"],
  activeMutationFields: ["requestBody"],
  configSchemaTypes: ["object"],
  activePermissions: ["request.body.read", "request.body.write"],
  reservedPermissions: ["network.fetch"],
  hookMatrix: {
    "gateway.request.afterBodyRead": {
      phase: "after request body read and before upstream provider send",
      kind: "request",
      status: "active",
      readPermissions: ["request.body.read"],
      writePermissions: ["request.body.write"],
      permissionDependencies: { "request.body.write": ["request.body.read"] },
      mutationFields: ["requestBody"],
      contextFields: ["traceId"],
      timeoutMs: 150,
      defaultFailurePolicy: "fail-open",
      reservedHeaderPolicy: "block-gateway-owned",
    },
  },
  communityRuntimes: ["declarativeRules"],
  policyGatedRuntimes: ["wasm"],
  officialRuntimes: ["native:privacyFilter"],
});
writePassingScaffold(missingDevtoolsMetadataRoot);
writeFileSync(
  join(missingDevtoolsMetadataRoot, "packages/create-aio-plugin/src/devtools.ts"),
  "declarativeRules validatePluginFilesStrict replayHookExplain doctorPluginFiles"
);

const missingDevtoolsMetadataResult = runCheck(missingDevtoolsMetadataRoot);
if (
  missingDevtoolsMetadataResult.status === 0 ||
  !missingDevtoolsMetadataResult.stderr.includes("packages/create-aio-plugin/src/devtools.ts") ||
  !missingDevtoolsMetadataResult.stderr.includes("requestBody")
) {
  throw new Error(
    `expected devtools contract failure, got status ${missingDevtoolsMetadataResult.status}\n${missingDevtoolsMetadataResult.stderr}`
  );
}
```

- [ ] **Step 2: Run selftest to verify it fails**

Run:

```bash
node scripts/check-plugin-api-contract.selftest.mjs
```

Expected: FAIL because `scripts/check-plugin-api-contract.mjs` does not inspect devtools metadata yet.

- [ ] **Step 3: Add devtools contract checks**

In `scripts/check-plugin-api-contract.mjs`, after the existing scaffold checks, add:

```js
const devtools = readText("packages/create-aio-plugin/src/devtools.ts");
requireIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  contract.activeHooks,
  "developer tool active hook"
);
requireIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  contract.activePermissions,
  "developer tool active permission"
);
requireIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  runtimeTokens(contract),
  "developer tool runtime"
);
requireIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  contract.activeMutationFields ?? [],
  "developer tool mutation field"
);
requireIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  [
    "doctorPluginFiles",
    "validatePluginFilesStrict",
    "replayHookExplain",
    "PLUGIN_RULE_PERMISSION_MISMATCH",
    "PLUGIN_REPLAY_UNSUPPORTED_RUNTIME",
    "PLUGIN_WASM_POLICY_GATED",
  ],
  "developer tool diagnostic surface"
);
requireNotIncludes(
  "packages/create-aio-plugin/src/devtools.ts",
  devtools,
  ["contextPatch"],
  "legacy mutation field"
);
```

- [ ] **Step 4: Run contract checks to verify the task passes**

Run:

```bash
node scripts/check-plugin-api-contract.selftest.mjs
pnpm check:plugin-api-contract
```

Expected: PASS for both commands.

- [ ] **Step 5: Commit**

```bash
git add scripts/check-plugin-api-contract.mjs scripts/check-plugin-api-contract.selftest.mjs
git commit -m "test(plugins): guard devtools contract drift"
```

## Task 6: Add Provider Internal Acceptance Tests Without Public API Changes

**Files:**

- Modify: `src-tauri/src/gateway/proxy/handler/provider_order.rs`
- Modify: `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs`
- Modify: `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs`
- Modify: `src-tauri/src/domain/provider_oauth_limits.rs`

- [ ] **Step 1: Add provider ordering acceptance test**

In `src-tauri/src/gateway/proxy/handler/provider_order.rs`, add this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn acceptance_bound_order_ignores_unknown_and_duplicate_provider_ids() {
    let mut providers = vec![provider(1), provider(2), provider(3), provider(4)];

    reorder_providers_by_bound_order(&mut providers, &[3, 99, 3, 1]);

    assert_eq!(ids(&providers), vec![3, 1, 2, 4]);
}
```

- [ ] **Step 2: Add session binding acceptance test**

In `src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs`, add this test at the end of the file:

```rust
#[test]
fn acceptance_session_bound_provider_falls_back_when_bound_provider_circuit_is_open() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let db = crate::db::init_for_tests(&db_path).expect("init db");

    let p1 = insert_provider(&db, "P1", true);
    let p2 = insert_provider(&db, "P2", true);
    let id1 = p1.id;
    let id2 = p2.id;

    let session = session_manager::SessionManager::new();
    let now = 1000;
    session.bind_success("claude", "sess_1", id1, Some(vec![id1, id2]), now);
    let circuit = open_circuit_for_provider(id1, now);

    let mut enabled =
        providers::list_enabled_for_gateway_in_mode(&db, "claude", None).expect("list enabled");
    let selected = resolve_session_bound_provider_id(
        &session,
        &circuit,
        "claude",
        Some("sess_1"),
        now,
        true,
        None,
        &mut enabled,
        Some(&[id1, id2]),
    );

    assert_eq!(selected, None);
    assert_eq!(ids(&enabled), vec![id2]);
}
```

- [ ] **Step 3: Add cx2cc bridge acceptance test**

In `src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs`, add this test inside the existing test module:

```rust
#[test]
fn acceptance_cx2cc_round_trip_preserves_requested_model_and_usage() {
    let bridge = get_bridge("cx2cc").unwrap();
    let ctx = cx2cc_ctx();

    let anthropic_req = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let translated_req = bridge.translate_request(anthropic_req, &ctx).unwrap();
    assert_eq!(translated_req.target_path, "/v1/responses");

    let openai_resp = json!({
        "id": "resp_acceptance",
        "model": translated_req.body["model"],
        "status": "completed",
        "output": [
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hi"}]
            }
        ],
        "usage": {"input_tokens": 13, "output_tokens": 5}
    });

    let anthropic_resp = bridge.translate_response(openai_resp, &ctx).unwrap();
    assert_eq!(anthropic_resp["model"], "claude-sonnet-4-20250514");
    assert_eq!(anthropic_resp["usage"]["input_tokens"], 13);
    assert_eq!(anthropic_resp["usage"]["output_tokens"], 5);
}
```

- [ ] **Step 4: Add OAuth snapshot acceptance test**

In `src-tauri/src/domain/provider_oauth_limits.rs`, add this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn acceptance_oauth_exhausted_snapshot_is_scoped_to_provider() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = db::init_for_tests(&dir.path().join("oauth-limits-scope.db")).expect("init db");
    let now = now_unix_seconds();
    let exhausted_provider_id = insert_test_provider_named(&db, "OAuth exhausted");
    let healthy_provider_id = insert_test_provider_named(&db, "OAuth healthy");

    save_exhausted_snapshot(&db, exhausted_provider_id, Some(now + 3_600))
        .expect("save exhausted snapshot");
    save_snapshot(
        &db,
        OAuthLimitSnapshotInput {
            provider_id: healthy_provider_id,
            limit_short_label: Some("5h"),
            limit_5h_text: Some("25%"),
            limit_weekly_text: Some("80%"),
            limit_5h_reset_at: None,
            limit_weekly_reset_at: None,
            reset_credit_available_count: Some(3),
        },
    )
    .expect("save healthy snapshot");

    let conn = db.open_connection().expect("open");
    assert_eq!(
        gate_snapshot(&conn, exhausted_provider_id, now).expect("gate exhausted"),
        OAuthLimitGate::Limited {
            reset_at: Some(now + 3_600)
        }
    );
    assert_eq!(
        gate_snapshot(&conn, healthy_provider_id, now).expect("gate healthy"),
        OAuthLimitGate::Allow
    );
}
```

- [ ] **Step 5: Run Rust tests to verify the task passes**

Run:

```bash
cd src-tauri && cargo test provider_order --lib
cd src-tauri && cargo test provider_selection --lib
cd src-tauri && cargo test protocol_bridge --lib
cd src-tauri && cargo test provider_oauth_limits --lib
```

Expected: PASS for all four commands.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/gateway/proxy/handler/provider_order.rs src-tauri/src/gateway/proxy/handler/provider_selection/tests.rs src-tauri/src/gateway/proxy/protocol_bridge/e2e_tests.rs src-tauri/src/domain/provider_oauth_limits.rs
git commit -m "test(provider): add internal acceptance coverage"
```

## Task 7: Update Docs And Run Release Gates

**Files:**

- Modify: `docs/plugins/developer-guide.md`
- Modify: `docs/plugins/reference/sdk.md`
- Modify: `docs/plugins/reference/declarative-rules.md`
- Modify: `docs/plugins/reference/compatibility.md`

- [ ] **Step 1: Update developer guide workflow**

In `docs/plugins/developer-guide.md`, update the command sequence so it reads:

```md
pnpm --filter create-aio-plugin test
pnpm --filter create-aio-plugin cli acme.redactor rule
pnpm --filter create-aio-plugin cli doctor ./acme.redactor
pnpm --filter create-aio-plugin cli validate --strict ./acme.redactor
pnpm --filter create-aio-plugin cli replay --explain ./acme.redactor ./fixtures/claude-request.json gateway.request.afterBodyRead
pnpm --filter create-aio-plugin cli replay --explain ./acme.redactor ./fixtures/codex-request.json gateway.request.afterBodyRead
pnpm --filter create-aio-plugin cli pack ./acme.redactor
```

Add this short explanation near the first `create-aio-plugin validate` section:

```md
`doctor` checks package health and reports structured diagnostics with `severity`, `code`, `message`, `path`, and `hint`.
`validate --strict` keeps Plugin API v1 compatibility but adds package-level checks for rule files, rule hooks, target compatibility, and rule permission mismatches.
Warnings do not fail the command in 0.62.1; any `error` severity diagnostic returns a non-zero exit code.
```

- [ ] **Step 2: Update SDK reference**

In `docs/plugins/reference/sdk.md`, update the devtools command block to:

```md
pnpm --filter create-aio-plugin cli doctor ./acme.redactor
pnpm --filter create-aio-plugin cli validate --strict ./acme.redactor
pnpm --filter create-aio-plugin cli replay --explain ./acme.redactor ./fixtures/request.json gateway.request.afterBodyRead
pnpm --filter create-aio-plugin cli pack ./acme.redactor
```

Add the stable diagnostic shape:

```json
{
  "severity": "error",
  "code": "PLUGIN_RULE_PERMISSION_MISMATCH",
  "message": "rule targeting request.body with action replace requires request.body.write",
  "path": "rules/main.json#/rules/0",
  "hint": "Add request.body.write to manifest.permissions or change the rule target/action."
}
```

- [ ] **Step 3: Update declarative rules reference**

In `docs/plugins/reference/declarative-rules.md`, add this explanation model:

```json
{
  "pluginId": "acme.redactor",
  "runtime": "declarativeRules",
  "hook": "gateway.request.afterBodyRead",
  "evaluatedRuleCount": 1,
  "matchedRuleIds": ["redact-token-rule"],
  "actionKind": "replace",
  "outputKind": "replace",
  "mutationSummary": {
    "changed": true,
    "field": "requestBody",
    "targetField": "request.body",
    "jsonPath": "$.messages[*].content"
  },
  "warnings": [],
  "result": {
    "action": "replace",
    "requestBody": "{\"messages\":[{\"role\":\"user\",\"content\":\"[REDACTED]\"}]}"
  }
}
```

Add this sentence below the JSON:

```md
`replay --explain` is a deterministic local simulator for the supported declarative-rules subset. The Rust gateway remains the source of truth for runtime execution, audit events, failure policy, timeouts, and circuit behavior.
```

- [ ] **Step 4: Update compatibility reference**

In `docs/plugins/reference/compatibility.md`, add this 0.62.1 boundary note:

```md
0.62.1 does not change Plugin API v1. `doctor`, `validate --strict`, and `replay --explain` are developer tooling around the same manifest and hook contract.

Provider behavior remains host-owned. Provider ordering, failover, OAuth limits, token counting, cx2cc translation, and session binding are covered by internal acceptance tests, but no Provider Plugin API is exposed.

WASM remains policy-gated. The scaffold and pack flow can carry WASM artifacts, but marketplace WASM execution is not enabled by default.
```

- [ ] **Step 5: Run documentation and release verification**

Run:

```bash
pnpm exec prettier --check docs/plugins/developer-guide.md docs/plugins/reference/sdk.md docs/plugins/reference/declarative-rules.md docs/plugins/reference/compatibility.md
pnpm --filter create-aio-plugin test
pnpm --filter create-aio-plugin typecheck
pnpm --filter @aio-coding-hub/plugin-sdk test
pnpm --filter @aio-coding-hub/plugin-sdk typecheck
pnpm check:plugin-api-contract
pnpm check:plugin-system-docs
pnpm test:unit -- src/pages/__tests__/PluginsPage.test.tsx
pnpm typecheck
cd src-tauri && cargo test plugin --lib
cd src-tauri && cargo test gateway_plugin --lib
cd src-tauri && cargo test provider --lib
git diff --check
```

Expected: PASS for every command.

- [ ] **Step 6: Commit**

```bash
git add docs/plugins/developer-guide.md docs/plugins/reference/sdk.md docs/plugins/reference/declarative-rules.md docs/plugins/reference/compatibility.md
git commit -m "docs(plugins): document 0.62.1 developer loop"
```

## Final Acceptance Checklist

- [ ] `create-aio-plugin doctor ./plugin` returns structured diagnostics and non-zero exit code when any error diagnostic exists.
- [ ] `create-aio-plugin validate ./plugin` preserves the existing manifest-only success/failure shape.
- [ ] `create-aio-plugin validate --strict ./plugin` checks package health, rule documents, declared hooks, target compatibility, and rule permissions.
- [ ] `create-aio-plugin replay ./plugin fixture.json hook` preserves existing output.
- [ ] `create-aio-plugin replay --explain ./plugin fixture.json hook` reports evaluated rules, matched rule ids, action kind, output kind, mutation summary, and warnings.
- [ ] Plugin detail GUI shows runtime failures and audit events with hook, failure kind, event type, risk, and trace ID.
- [ ] Contract drift gates include devtools metadata in addition to SDK, Rust, docs, scaffold, and WASM SDK.
- [ ] Provider adapter work remains internal and is covered by acceptance tests.
- [ ] Plugin API v1 remains externally compatible.
- [ ] Provider Plugin API remains closed.
- [ ] WASM remains policy-gated.

## Self-Review Notes

- Spec coverage: Tasks 1-3 cover local developer loop; Task 4 covers GUI observability; Task 5 covers contract drift prevention; Task 6 covers internal provider acceptance; Task 7 covers docs and release gates.
- Type consistency: diagnostic types are shared by `doctor`, strict validation, and replay warnings; `ReplayExplainResult` uses the existing `ReplayRuleResult` action union without changing old replay output.
- Compatibility: legacy `validate` and `replay` routes remain available with their existing positional arguments; new behavior is gated by `doctor`, `--strict`, and `--explain`.
