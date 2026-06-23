import {
  createHash,
  createPrivateKey,
  createPublicKey,
  generateKeyPairSync,
  sign,
  verify,
} from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import type { GatewayHookName, PluginManifest, ValidationResult } from "@aio-coding-hub/plugin-sdk";
import { validateManifest } from "@aio-coding-hub/plugin-sdk";
import { createPluginScaffold, type ScaffoldFiles, type ScaffoldTemplate } from "./scaffold";

export type PackedPlugin = {
  bytes: Uint8Array;
  checksum: string;
};

export type PluginFileBytes = Record<string, Uint8Array>;

export type SigningKeyPair = {
  privateKey: string;
  publicKey: string;
};

export type CliIo = {
  log: (line: string) => void;
  error: (line: string) => void;
};

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

export type StrictValidationResult =
  | { ok: true; diagnostics: PluginDiagnostic[] }
  | { ok: false; error: { code: string; message: string }; diagnostics: PluginDiagnostic[] };

type DoctorOptions = {
  strict?: boolean;
};

const USAGE = [
  "Usage:",
  "  create-aio-plugin <publisher.plugin-name> [rule|wasm]",
  "  create-aio-plugin doctor <plugin-dir>",
  "  create-aio-plugin validate [--strict] <plugin-dir>",
  "  create-aio-plugin replay [--explain] <plugin-dir> <fixture.json> <hook>",
  "  create-aio-plugin pack <plugin-dir>",
].join("\n");

export function runCreateAioPluginCli(args: string[], cwd: string, io: CliIo = console): number {
  const [commandOrId, firstArg, secondArg, thirdArg] = args;

  if (!commandOrId) {
    io.error(USAGE);
    return 1;
  }

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

  if (commandOrId === "replay") {
    if (!firstArg || !secondArg || !thirdArg) {
      io.error("Usage: create-aio-plugin replay <plugin-dir> <fixture.json> <hook>");
      return 1;
    }
    try {
      const files = readPluginDirectory(resolve(cwd, firstArg));
      const fixture = JSON.parse(readFileSync(resolve(cwd, secondArg), "utf8")) as unknown;
      io.log(JSON.stringify(replayHook(files, thirdArg as GatewayHookName, fixture)));
      return 0;
    } catch (error) {
      io.error(`failed to replay plugin hook: ${errorMessage(error)}`);
      return 1;
    }
  }

  if (commandOrId === "pack") {
    try {
      const root = resolve(cwd, firstArg ?? ".");
      const files = readPluginDirectoryBytes(root);
      const manifest = JSON.parse(
        textFromBytes(files["plugin.json"]) ?? "{}"
      ) as Partial<PluginManifest>;
      const packed = packPluginBytes(files);
      const outputPath = join(cwd, `${manifest.id ?? firstArg ?? "plugin"}.aio-plugin`);
      writeFileSync(outputPath, packed.bytes);
      io.log(
        JSON.stringify({
          path: outputPath,
          checksum: packed.checksum,
          sizeBytes: packed.bytes.length,
        })
      );
      return 0;
    } catch (error) {
      io.error(`failed to pack plugin directory: ${errorMessage(error)}`);
      return 1;
    }
  }

  if (commandOrId === "sign") {
    const bytes = new TextEncoder().encode(firstArg ?? "");
    const keyPair = secondArg
      ? { privateKey: secondArg, publicKey: createPublicKeyFromPrivateKey(secondArg) }
      : generateSigningKeyPair();
    io.log(JSON.stringify(signPackage(bytes, keyPair.privateKey, keyPair.publicKey)));
    return 0;
  }

  if (commandOrId === "verify") {
    if (!secondArg || !thirdArg) {
      io.error("Usage: create-aio-plugin verify <bytes> <signature> <publicKey>");
      return 1;
    }
    const bytes = new TextEncoder().encode(firstArg ?? "");
    io.log(JSON.stringify(verifyPackage(bytes, secondArg, thirdArg)));
    return 0;
  }

  const idArg = commandOrId;
  const template = (firstArg ?? "rule") as ScaffoldTemplate;
  const files = createPluginScaffold({
    id: idArg,
    name: titleFromId(idArg),
    template,
  });

  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(cwd, idArg, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return 0;
}

export function validatePluginFiles(files: ScaffoldFiles): ValidationResult {
  const manifestText = files["plugin.json"];
  if (!manifestText) {
    return {
      ok: false,
      error: { code: "PLUGIN_MISSING_MANIFEST", message: "missing plugin.json" },
    };
  }
  try {
    return validateManifest(JSON.parse(manifestText) as PluginManifest);
  } catch (error) {
    return {
      ok: false,
      error: {
        code: "PLUGIN_INVALID_MANIFEST",
        message: error instanceof Error ? error.message : "invalid manifest",
      },
    };
  }
}

export function readPluginDirectory(root: string): ScaffoldFiles {
  const files: ScaffoldFiles = {};
  walkPluginDirectory(root, root, files);
  return files;
}

export function readPluginDirectoryBytes(root: string): PluginFileBytes {
  const files: PluginFileBytes = {};
  walkPluginDirectoryBytes(root, root, files);
  return files;
}

export function validatePluginDirectory(root: string): ValidationResult {
  return validatePluginFiles(readPluginDirectory(root));
}

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

export function doctorPluginDirectory(root: string, options: DoctorOptions = {}): DoctorResult {
  return doctorPluginFiles(readPluginDirectory(root), options);
}

export function doctorPluginFiles(files: ScaffoldFiles, options: DoctorOptions = {}): DoctorResult {
  const diagnostics: PluginDiagnostic[] = [];
  const manifestText = files["plugin.json"];

  if (manifestText == null) {
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
    const parsed = JSON.parse(manifestText) as unknown;
    if (!asRecord(parsed)) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
        message: "plugin.json must contain a manifest object",
        path: "plugin.json",
        hint: "Fix plugin.json so it matches Plugin API v1.",
      });
      return { ok: false, diagnostics };
    }
    manifest = parsed as PluginManifest;
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

  diagnostics.push(...manifestShapeDiagnostics(manifest));

  let validation: ValidationResult;
  try {
    validation = validateManifest(manifest);
  } catch (error) {
    validation = {
      ok: false,
      error: {
        code: "PLUGIN_INVALID_MANIFEST",
        message: errorMessage(error),
      },
    };
  }
  if (!validation.ok) {
    diagnostics.push({
      severity: "error",
      code: validation.error.code,
      message: validation.error.message,
      path: "plugin.json",
      hint: "Update the manifest so it matches Plugin API v1.",
    });
  }

  const runtimeKind = manifestRuntimeKind(manifest);
  const runtime = manifest.runtime;
  if (!runtimeKind) {
    diagnostics.push(runtimeShapeDiagnostic(manifest));
  }

  if (runtimeKind === "declarativeRules" && runtime.kind === "declarativeRules") {
    for (const rulePath of runtime.rules) {
      if (!hasPluginFile(files, rulePath)) {
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

  if (runtimeKind === "wasm") {
    const entry = typeof manifest.entry === "string" ? manifest.entry : "plugin.wasm";
    if (!hasPluginFile(files, entry)) {
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

  const manifestSummary = doctorManifestSummary(manifest, runtimeKind);

  return {
    ok: !hasErrorDiagnostics(diagnostics),
    diagnostics,
    ...(manifestSummary ? { manifest: manifestSummary } : {}),
  };
}

export function packPluginDirectory(root: string): PackedPlugin {
  return packPluginBytes(readPluginDirectoryBytes(root));
}

function hasErrorDiagnostics(diagnostics: readonly PluginDiagnostic[]): boolean {
  return diagnostics.some((diagnostic) => diagnostic.severity === "error");
}

function hasPluginFile(files: ScaffoldFiles, path: string): boolean {
  return Object.prototype.hasOwnProperty.call(files, path);
}

const RULE_TARGET_FIELDS_BY_HOOK: Record<string, readonly string[]> = {
  "gateway.request.afterBodyRead": ["request.body"],
  "gateway.request.beforeSend": ["request.body"],
  "gateway.response.after": ["response.body"],
  "gateway.response.chunk": ["stream.chunk"],
  "gateway.error": ["response.body"],
  "log.beforePersist": ["log.message"],
};

function strictRuleDiagnostics(files: ScaffoldFiles, manifest: PluginManifest): PluginDiagnostic[] {
  const runtime = asRecord(manifest.runtime);
  if (
    runtime?.kind !== "declarativeRules" ||
    !Array.isArray(runtime.rules) ||
    !runtime.rules.every((rulePath) => typeof rulePath === "string")
  ) {
    return [];
  }

  const diagnostics: PluginDiagnostic[] = [];
  const declaredHooks = new Set<string>(
    Array.isArray(manifest.hooks)
      ? manifest.hooks
          .map((hook) => asRecord(hook)?.name)
          .filter((name): name is string => typeof name === "string")
      : []
  );
  const grantedPermissions = new Set<string>(
    Array.isArray(manifest.permissions)
      ? manifest.permissions.flatMap((permission) =>
          typeof permission === "string" ? [permission] : []
        )
      : []
  );

  for (const rulePath of runtime.rules) {
    if (!hasPluginFile(files, rulePath)) continue;
    const text = files[rulePath] ?? "";

    let document: Record<string, unknown> | null;
    try {
      document = asRecord(JSON.parse(text) as unknown);
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

    if (!document || !Array.isArray(document.rules)) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_RULES_MISSING_ARRAY",
        message: "rule document must contain a rules array",
        path: `${rulePath}#/rules`,
        hint: 'Use { "rules": [...] } as the rule document shape.',
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

      if (typeof rule.id !== "string" || rule.id.length === 0) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_ID_MISSING",
          message: "rule id is required",
          path: `${rulePath}#/rules/${index}/id`,
          hint: "Set id to a stable string so host diagnostics can identify this rule.",
        });
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
      if (!target) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_TARGET_MISSING",
          message: "rule target must be an object",
          path: `${rulePath}#/rules/${index}/target`,
          hint: "Set target.field to the hook-visible field the rule should inspect.",
        });
      }
      const matcher = asRecord(rule.match);
      if (!matcher) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_MATCHER_MISSING",
          message: "rule match must be an object",
          path: `${rulePath}#/rules/${index}/match`,
          hint: "Set match.regex to the pattern the rule should evaluate.",
        });
      } else if (typeof matcher.regex !== "string") {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_MATCHER_INVALID",
          message: "rule match.regex must be a string",
          path: `${rulePath}#/rules/${index}/match/regex`,
          hint: "Use a JavaScript-compatible regex string.",
        });
      }
      const action = asRecord(rule.action);
      if (!action) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_ACTION_MISSING",
          message: "rule action must be an object",
          path: `${rulePath}#/rules/${index}/action`,
          hint: "Set action.kind and its required payload.",
        });
      }
      const targetField = typeof target?.field === "string" ? target.field : "";
      if (target && !targetField) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_TARGET_INVALID",
          message: "rule target.field must be a string",
          path: `${rulePath}#/rules/${index}/target/field`,
          hint: "Use a hook-visible target field such as request.body.",
        });
      }
      const actionKind = typeof action?.kind === "string" ? action.kind : "";
      const actionDiagnostics = ruleActionDiagnostics(action, actionKind, rulePath, index);
      diagnostics.push(...actionDiagnostics);
      const allowedFields = RULE_TARGET_FIELDS_BY_HOOK[hook] ?? [];
      let targetCompatible = true;
      if (hook && targetField && allowedFields.length > 0 && !allowedFields.includes(targetField)) {
        targetCompatible = false;
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_RULE_TARGET_INCOMPATIBLE_WITH_HOOK",
          message: `target field ${targetField} is not compatible with hook ${hook}`,
          path: `${rulePath}#/rules/${index}/target/field`,
          hint: `Use one of: ${allowedFields.join(", ")}.`,
        });
      }

      if (!targetCompatible || !targetField || !action || actionDiagnostics.length > 0) return;

      for (const permission of permissionsForRuleTarget(targetField, actionKind)) {
        if (!grantedPermissions.has(permission)) {
          diagnostics.push({
            severity: "error",
            code: "PLUGIN_RULE_PERMISSION_MISMATCH",
            message: `rule targeting ${targetField} with action ${
              actionKind || "unknown"
            } requires ${permission}`,
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

function ruleActionDiagnostics(
  action: Record<string, unknown> | null,
  actionKind: string,
  rulePath: string,
  index: number
): PluginDiagnostic[] {
  if (!action) return [];
  const path = `${rulePath}#/rules/${index}/action`;
  if (!actionKind) {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: "rule action.kind must be a string",
        path: `${path}/kind`,
        hint: "Use replace, block, warn, or appendMessage.",
      },
    ];
  }
  if (!["replace", "block", "warn", "appendMessage"].includes(actionKind)) {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: `unsupported rule action kind: ${actionKind}`,
        path: `${path}/kind`,
        hint: "Use replace, block, warn, or appendMessage.",
      },
    ];
  }
  if (actionKind === "replace" && typeof action.replacement !== "string") {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: "replace action requires a string replacement",
        path: `${path}/replacement`,
        hint: "Set action.replacement to the replacement text.",
      },
    ];
  }
  if (actionKind === "block" && typeof action.reason !== "string") {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: "block action requires a string reason",
        path: `${path}/reason`,
        hint: "Set action.reason to the block reason.",
      },
    ];
  }
  if (actionKind === "warn" && typeof action.message !== "string") {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: "warn action requires a string message",
        path: `${path}/message`,
        hint: "Set action.message to the warning message.",
      },
    ];
  }
  if (
    actionKind === "appendMessage" &&
    (typeof action.role !== "string" || typeof action.content !== "string")
  ) {
    return [
      {
        severity: "error",
        code: "PLUGIN_RULE_ACTION_INVALID",
        message: "appendMessage action requires string role and content",
        path,
        hint: "Set action.role and action.content.",
      },
    ];
  }
  return [];
}

function manifestRuntimeKind(
  manifest: Partial<PluginManifest>
): PluginManifest["runtime"]["kind"] | null {
  const runtime = asRecord(manifest.runtime);
  if (
    runtime?.kind === "declarativeRules" &&
    Array.isArray(runtime.rules) &&
    runtime.rules.every((rulePath) => typeof rulePath === "string" && rulePath.length > 0)
  ) {
    return "declarativeRules";
  }
  if (runtime?.kind === "wasm" && typeof runtime.abiVersion === "string") {
    return "wasm";
  }
  return null;
}

function manifestShapeDiagnostics(manifest: Partial<PluginManifest>): PluginDiagnostic[] {
  const diagnostics: PluginDiagnostic[] = [];
  for (const field of ["id", "name", "version", "apiVersion"] as const) {
    if (typeof manifest[field] !== "string") {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
        message: `${field} must be a string`,
        path: `plugin.json#/${field}`,
        hint: "Use the Plugin API v1 manifest field types.",
      });
    }
  }
  if (!Array.isArray(manifest.hooks)) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST",
      message: "hooks must be an array",
      path: "plugin.json#/hooks",
      hint: "Declare at least one Plugin API v1 hook.",
    });
  } else {
    manifest.hooks.forEach((hook, index) => {
      const hookRecord = asRecord(hook);
      if (!hookRecord) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_INVALID_MANIFEST",
          message: "hook entries must be objects",
          path: `plugin.json#/hooks/${index}`,
          hint: "Declare hooks as Plugin API v1 hook objects.",
        });
        return;
      }
      if (typeof hookRecord.name !== "string") {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_INVALID_MANIFEST",
          message: "hook name must be a string",
          path: `plugin.json#/hooks/${index}/name`,
          hint: "Use a Plugin API v1 hook name.",
        });
      }
      if (hookRecord.priority != null && typeof hookRecord.priority !== "number") {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_INVALID_MANIFEST",
          message: "hook priority must be a number when present",
          path: `plugin.json#/hooks/${index}/priority`,
          hint: "Use a numeric hook priority.",
        });
      }
      if (
        hookRecord.failurePolicy != null &&
        hookRecord.failurePolicy !== "fail-open" &&
        hookRecord.failurePolicy !== "fail-closed"
      ) {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_INVALID_MANIFEST",
          message: "hook failurePolicy must be fail-open or fail-closed when present",
          path: `plugin.json#/hooks/${index}/failurePolicy`,
          hint: "Use fail-open or fail-closed.",
        });
      }
    });
  }
  if (!Array.isArray(manifest.permissions)) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST",
      message: "permissions must be an array",
      path: "plugin.json#/permissions",
      hint: "Declare Plugin API v1 permissions as strings.",
    });
  }
  const compatibility = asRecord(manifest.hostCompatibility);
  if (!compatibility) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST",
      message: "hostCompatibility must be an object",
      path: "plugin.json#/hostCompatibility",
      hint: "Set hostCompatibility.app and hostCompatibility.pluginApi.",
    });
  } else {
    for (const field of ["app", "pluginApi"] as const) {
      if (typeof compatibility[field] !== "string") {
        diagnostics.push({
          severity: "error",
          code: "PLUGIN_INVALID_MANIFEST",
          message: `hostCompatibility.${field} must be a string`,
          path: `plugin.json#/hostCompatibility/${field}`,
          hint: "Use Plugin API v1 compatibility range strings.",
        });
      }
    }
    if (
      compatibility.platforms != null &&
      (!Array.isArray(compatibility.platforms) ||
        !compatibility.platforms.every((platform) => typeof platform === "string"))
    ) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
        message: "hostCompatibility.platforms must be an array of strings when present",
        path: "plugin.json#/hostCompatibility/platforms",
        hint: "Use platform names as strings.",
      });
    }
  }
  if (manifest.entry != null && typeof manifest.entry !== "string") {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST",
      message: "entry must be a string when present",
      path: "plugin.json#/entry",
      hint: "Use a package-relative entry path.",
    });
  }
  const runtime = asRecord(manifest.runtime);
  if (runtime?.kind === "wasm" && typeof runtime.abiVersion !== "string") {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_RUNTIME",
      message: "wasm runtime requires a string abiVersion",
      path: "plugin.json#/runtime/abiVersion",
      hint: 'Use runtime: { kind: "wasm", abiVersion: "1.0.0" }.',
    });
  }
  if (
    runtime?.kind === "wasm" &&
    runtime.memoryLimitBytes != null &&
    typeof runtime.memoryLimitBytes !== "number"
  ) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_RUNTIME",
      message: "wasm memoryLimitBytes must be a number when present",
      path: "plugin.json#/runtime/memoryLimitBytes",
      hint: "Use a byte count number.",
    });
  }
  return diagnostics;
}

function runtimeShapeDiagnostic(manifest: Partial<PluginManifest>): PluginDiagnostic {
  const runtime = asRecord(manifest.runtime);
  if (!runtime) {
    return {
      severity: "error",
      code: "PLUGIN_INVALID_RUNTIME",
      message: "plugin runtime must be an object",
      path: "plugin.json#/runtime",
      hint: "Set runtime to a Plugin API v1 runtime object.",
    };
  }
  if (runtime.kind === "declarativeRules") {
    return {
      severity: "error",
      code: "PLUGIN_INVALID_RUNTIME",
      message: "declarativeRules runtime requires a rules array",
      path: "plugin.json#/runtime",
      hint: 'Use runtime: { kind: "declarativeRules", rules: ["rules/main.json"] }.',
    };
  }
  return {
    severity: "error",
    code: "PLUGIN_INVALID_RUNTIME",
    message: "plugin runtime kind is not supported by create-aio-plugin doctor",
    path: "plugin.json#/runtime",
    hint: "Use declarativeRules or wasm for community plugin packages.",
  };
}

function doctorManifestSummary(
  manifest: Partial<PluginManifest>,
  runtimeKind: PluginManifest["runtime"]["kind"] | null
): DoctorResult["manifest"] | undefined {
  if (
    typeof manifest.id !== "string" ||
    typeof manifest.name !== "string" ||
    typeof manifest.version !== "string" ||
    !runtimeKind
  ) {
    return undefined;
  }
  return {
    id: manifest.id,
    name: manifest.name,
    version: manifest.version,
    runtime: runtimeKind,
  };
}

function walkPluginDirectory(root: string, dir: string, files: ScaffoldFiles): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === ".git") continue;
    const fullPath = join(dir, entry.name);
    const relativePath = relative(root, fullPath).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      walkPluginDirectory(root, fullPath, files);
    } else if (entry.isFile()) {
      files[relativePath] = readFileSync(fullPath, "utf8");
    }
  }
}

function walkPluginDirectoryBytes(root: string, dir: string, files: PluginFileBytes): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === ".git") continue;
    const fullPath = join(dir, entry.name);
    const relativePath = relative(root, fullPath).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      walkPluginDirectoryBytes(root, fullPath, files);
    } else if (entry.isFile()) {
      files[relativePath] = new Uint8Array(readFileSync(fullPath));
    }
  }
}

export function replayHook(files: ScaffoldFiles, hook: GatewayHookName, context: unknown): unknown {
  const validation = validatePluginFiles(files);
  if (!validation.ok) {
    throw new Error(`${validation.error.code}: ${validation.error.message}`);
  }
  const manifest = JSON.parse(files["plugin.json"] ?? "{}") as PluginManifest;
  if (manifest.runtime.kind !== "declarativeRules") {
    return { action: "pass" };
  }
  for (const rulePath of manifest.runtime.rules) {
    const document = JSON.parse(files[rulePath] ?? '{"rules":[]}') as { rules?: unknown[] };
    for (const rule of document.rules ?? []) {
      const result = replayDeclarativeRule(rule, hook, context);
      if (result.action !== "pass") return result;
    }
  }
  return { action: "pass" };
}

export function packPlugin(files: ScaffoldFiles): PackedPlugin {
  const bytes = createStoredZipBytes(textFilesToBytes(files));
  return {
    bytes,
    checksum: sha256(bytes),
  };
}

export function packPluginBytes(files: PluginFileBytes): PackedPlugin {
  const bytes = createStoredZipBytes(
    Object.entries(files)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([path, content]) => [path, content] as const)
  );
  return {
    bytes,
    checksum: sha256(bytes),
  };
}

function textFilesToBytes(files: ScaffoldFiles): readonly (readonly [string, Uint8Array])[] {
  return Object.entries(files)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, content]) => [path, new TextEncoder().encode(content)] as const);
}

function textFromBytes(bytes: Uint8Array | undefined): string | undefined {
  return bytes == null ? undefined : new TextDecoder().decode(bytes);
}

export function generateSigningKeyPair(): SigningKeyPair {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  return {
    privateKey: privateKey.export({ format: "der", type: "pkcs8" }).toString("base64"),
    publicKey: rawPublicKeyFromSpki(
      publicKey.export({ format: "der", type: "spki" }) as Buffer
    ).toString("base64"),
  };
}

export function signPackage(
  bytes: Uint8Array,
  privateKey: string,
  publicKey?: string
): { checksum: string; signature: string; publicKey: string } {
  const checksum = sha256(bytes);
  const key = createPrivateKey({
    key: Buffer.from(privateKey, "base64"),
    format: "der",
    type: "pkcs8",
  });
  const signature = sign(null, Buffer.from(bytes), key).toString("base64");
  return {
    checksum,
    signature,
    publicKey:
      publicKey ??
      rawPublicKeyFromSpki(
        createPublicKey(key).export({ format: "der", type: "spki" }) as Buffer
      ).toString("base64"),
  };
}

export function verifyPackage(
  bytes: Uint8Array,
  signature: string,
  publicKey: string
): { ok: boolean; checksum: string } {
  const key = createPublicKey({
    key: spkiFromRawPublicKey(Buffer.from(publicKey, "base64")),
    format: "der",
    type: "spki",
  });
  return {
    ok: verify(null, Buffer.from(bytes), key, Buffer.from(signature, "base64")),
    checksum: sha256(bytes),
  };
}

function createPublicKeyFromPrivateKey(privateKey: string): string {
  return signPackage(new Uint8Array(), privateKey).publicKey;
}

function titleFromId(id: string): string {
  const segments = id.split(".");
  const slug = segments[segments.length - 1] ?? id;
  return slug
    .split("-")
    .map((part: string) => `${part.slice(0, 1).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

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
  const regex = compileReplayRegex(matcher);
  if (!regex) return { action: "pass" };

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

type ReplayRuleResult =
  | { action: "pass" }
  | { action: "replace"; requestBody: string }
  | { action: "replace"; responseBody: string }
  | { action: "replace"; streamChunk: string }
  | { action: "replace"; logMessage: string }
  | { action: "block"; reason: string }
  | { action: "warn"; message: string };

type JsonPathSegment = { kind: "key"; key: string } | { kind: "wildcardArray" };

function compileReplayRegex(matcher: Record<string, unknown>): RegExp | null {
  if (typeof matcher.regex !== "string") return null;
  try {
    return new RegExp(matcher.regex, matcher.caseSensitive === false ? "gi" : "g");
  } catch {
    return null;
  }
}

function textFromFixture(context: unknown, field: string): string | undefined {
  if (typeof context === "string") return context;
  const contextRecord = asRecord(context);
  if (field === "request.body") {
    if (typeof contextRecord?.body === "string") return contextRecord.body;
    const request = asRecord(contextRecord?.request);
    return typeof request?.body === "string" ? request.body : undefined;
  }
  if (field === "response.body") {
    const response = asRecord(contextRecord?.response);
    return typeof response?.body === "string" ? response.body : undefined;
  }
  if (field === "stream.chunk") {
    const stream = asRecord(contextRecord?.stream);
    return typeof stream?.chunk === "string" ? stream.chunk : undefined;
  }
  if (field === "log.message") {
    const log = asRecord(contextRecord?.log);
    return typeof log?.message === "string" ? log.message : undefined;
  }
  return undefined;
}

function replayTextAction(
  body: string,
  regex: RegExp,
  action: Record<string, unknown>,
  field: string
): ReplayRuleResult {
  if (!regex.test(body)) return { action: "pass" };
  regex.lastIndex = 0;
  if (action.kind === "replace" && typeof action.replacement === "string") {
    return replaceResult(field, body.replace(regex, action.replacement));
  }
  if (action.kind === "block" && typeof action.reason === "string") {
    return { action: "block", reason: action.reason };
  }
  if (action.kind === "warn" && typeof action.message === "string") {
    return { action: "warn", message: action.message };
  }
  if (
    action.kind === "appendMessage" &&
    typeof action.role === "string" &&
    typeof action.content === "string"
  ) {
    return appendMessageToBody(body, action.role, action.content);
  }
  return { action: "pass" };
}

function replayJsonPathAction(
  body: string,
  path: string,
  regex: RegExp,
  action: Record<string, unknown>,
  field: string
): ReplayRuleResult {
  const segments = parseReplayJsonPath(path);
  if (!segments) return { action: "pass" };
  let root: unknown;
  try {
    root = JSON.parse(body) as unknown;
  } catch {
    return { action: "pass" };
  }

  let matched = false;
  let changed = false;
  applyToJsonStrings(root, segments, (candidate) => {
    if (!regex.test(candidate.value)) return;
    regex.lastIndex = 0;
    matched = true;
    if (action.kind === "replace" && typeof action.replacement === "string") {
      const next = candidate.value.replace(regex, action.replacement);
      if (next !== candidate.value) {
        candidate.set(next);
        changed = true;
      }
    }
  });

  if (!matched) return { action: "pass" };
  if (action.kind === "replace") {
    return changed ? replaceResult(field, JSON.stringify(root)) : { action: "pass" };
  }
  if (action.kind === "block" && typeof action.reason === "string") {
    return { action: "block", reason: action.reason };
  }
  if (action.kind === "warn" && typeof action.message === "string") {
    return { action: "warn", message: action.message };
  }
  if (
    action.kind === "appendMessage" &&
    typeof action.role === "string" &&
    typeof action.content === "string"
  ) {
    return appendMessageToBody(body, action.role, action.content);
  }
  return { action: "pass" };
}

function replaceResult(field: string, value: string): ReplayRuleResult {
  switch (field) {
    case "response.body":
      return { action: "replace", responseBody: value };
    case "stream.chunk":
      return { action: "replace", streamChunk: value };
    case "log.message":
      return { action: "replace", logMessage: value };
    case "request.body":
    default:
      return { action: "replace", requestBody: value };
  }
}

function appendMessageToBody(body: string, role: string, content: string): ReplayRuleResult {
  if (role !== "system" && role !== "developer") return { action: "pass" };
  if (!content.trim()) return { action: "pass" };
  let root: unknown;
  try {
    root = JSON.parse(body) as unknown;
  } catch {
    return { action: "pass" };
  }
  const record = asRecord(root);
  const messages = Array.isArray(record?.messages) ? record.messages : null;
  if (!messages) return { action: "pass" };
  messages.push({ role, content });
  return { action: "replace", requestBody: JSON.stringify(root) };
}

function parseReplayJsonPath(path: string): JsonPathSegment[] | null {
  if (!path.startsWith("$")) return null;
  const segments: JsonPathSegment[] = [];
  let index = 1;
  while (index < path.length) {
    if (path[index] === ".") {
      index += 1;
      const start = index;
      while (index < path.length && path[index] !== "." && path[index] !== "[") {
        index += 1;
      }
      if (start === index) return null;
      const key = path.slice(start, index);
      if (key.includes('"') || key.includes("'")) return null;
      segments.push({ kind: "key", key });
      continue;
    }
    if (path.slice(index, index + 3) === "[*]") {
      segments.push({ kind: "wildcardArray" });
      index += 3;
      continue;
    }
    return null;
  }
  return segments;
}

function applyToJsonStrings(
  value: unknown,
  path: JsonPathSegment[],
  visit: (candidate: { value: string; set: (next: string) => void }) => void
): void {
  if (path.length === 0) return;
  const [segment, ...rest] = path;
  if (!segment) return;

  if (segment.kind === "key") {
    const record = asRecord(value);
    if (!record || !(segment.key in record)) return;
    if (rest.length === 0) {
      const current = record[segment.key];
      if (typeof current === "string") {
        visit({
          value: current,
          set: (next) => {
            record[segment.key] = next;
          },
        });
      }
      return;
    }
    applyToJsonStrings(record[segment.key], rest, visit);
    return;
  }

  if (!Array.isArray(value)) return;
  for (const item of value) {
    applyToJsonStrings(item, rest, visit);
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function sha256(bytes: Uint8Array): string {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

function createStoredZipBytes(entries: readonly (readonly [string, Uint8Array])[]): Uint8Array {
  const chunks: Uint8Array[] = [];
  const centralDirectory: Uint8Array[] = [];
  let offset = 0;

  for (const [path, data] of entries) {
    const name = new TextEncoder().encode(path.replace(/\\/g, "/"));
    const crc = crc32(data);
    const localHeader = concatBytes([
      u32(0x04034b50),
      u16(20),
      u16(0x0800),
      u16(0),
      u16(0),
      u16(0),
      u32(crc),
      u32(data.length),
      u32(data.length),
      u16(name.length),
      u16(0),
      name,
    ]);
    chunks.push(localHeader, data);

    centralDirectory.push(
      concatBytes([
        u32(0x02014b50),
        u16(20),
        u16(20),
        u16(0x0800),
        u16(0),
        u16(0),
        u16(0),
        u32(crc),
        u32(data.length),
        u32(data.length),
        u16(name.length),
        u16(0),
        u16(0),
        u16(0),
        u16(0),
        u32(0),
        u32(offset),
        name,
      ])
    );
    offset += localHeader.length + data.length;
  }

  const centralDirectoryOffset = offset;
  const centralDirectoryBytes = concatBytes(centralDirectory);
  const endOfCentralDirectory = concatBytes([
    u32(0x06054b50),
    u16(0),
    u16(0),
    u16(entries.length),
    u16(entries.length),
    u32(centralDirectoryBytes.length),
    u32(centralDirectoryOffset),
    u16(0),
  ]);

  return concatBytes([...chunks, centralDirectoryBytes, endOfCentralDirectory]);
}

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function concatBytes(chunks: readonly Uint8Array[]): Uint8Array {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const output = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    output.set(chunk, offset);
    offset += chunk.length;
  }
  return output;
}

function u16(value: number): Uint8Array {
  return new Uint8Array([value & 0xff, (value >>> 8) & 0xff]);
}

function u32(value: number): Uint8Array {
  return new Uint8Array([
    value & 0xff,
    (value >>> 8) & 0xff,
    (value >>> 16) & 0xff,
    (value >>> 24) & 0xff,
  ]);
}

function rawPublicKeyFromSpki(spki: Buffer): Buffer {
  const prefix = Buffer.from("302a300506032b6570032100", "hex");
  if (spki.length !== prefix.length + 32 || !spki.subarray(0, prefix.length).equals(prefix)) {
    throw new Error("Unsupported Ed25519 SPKI public key format");
  }
  return spki.subarray(prefix.length);
}

function spkiFromRawPublicKey(raw: Buffer): Buffer {
  if (raw.length !== 32) {
    throw new Error("Ed25519 public key must be 32 bytes");
  }
  return Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]);
}
