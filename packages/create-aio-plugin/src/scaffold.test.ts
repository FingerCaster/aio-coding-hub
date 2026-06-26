import { createPublicKey, verify } from "node:crypto";
import { dirname } from "node:path";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createPluginScaffold } from "./scaffold";
import {
  doctorPluginDirectory,
  doctorPluginFiles,
  generateSigningKeyPair,
  packPlugin,
  packPluginBytes,
  packPluginDirectory,
  publishCheckPluginBytes,
  replayHook,
  replayHookExplain,
  runCreateAioPluginCli,
  signPackage,
  validatePluginDirectory,
  validatePluginFiles,
  validatePluginFilesStrict,
  verifyPackage,
} from "./devtools";

describe("create-aio-plugin scaffold", () => {
  it("creates a declarative rule plugin template", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });

    expect(files["plugin.json"]).toContain('"id": "acme.redactor"');
    expect(files["plugin.json"]).toContain('"kind": "declarativeRules"');
    expect(files["rules/main.json"]).toContain('"kind": "replace"');
    expect(files["README.md"]).toContain("acme.redactor");
  });

  it("creates a declarative rule template with the host rule ABI", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });

    const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const rule = document.rules?.[0] ?? {};

    expect(rule.target).toEqual({
      field: "request.body",
      jsonPath: "$.messages[*].content",
    });
    expect(rule.match).toMatchObject({
      regex: "SECRET_[A-Za-z0-9_]+",
      caseSensitive: true,
    });
    expect(rule).not.toHaveProperty("matcher");
  });

  it("creates a WASM plugin template without enabling marketplace execution", () => {
    const files = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });

    expect(files["plugin.json"]).toContain('"kind": "wasm"');
    expect(files["src/lib.rs"]).toContain("aio_plugin_handle");
    expect(files["README.md"]).toContain("gateway execution remains policy-gated");
    expect(files["README.md"]).toContain("PLUGIN_RUNTIME_DISABLED");
  });

  it("packs binary wasm artifacts without utf8 rewriting", () => {
    const wasmBytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0xff, 0x00, 0x80]);
    const packed = packPluginBytes({
      "plugin.json": new TextEncoder().encode(JSON.stringify(validWasmManifest())),
      "plugin.wasm": wasmBytes,
    });

    expect(packed.checksum).toMatch(/^sha256:/);
    expect(readStoredZipEntry(packed.bytes, "plugin.wasm")).toEqual(wasmBytes);
  });

  it("validates manifests, replays hook fixtures, and verifies package signatures", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });

    expect(validatePluginFiles(files).ok).toBe(true);
    expect(replayHook(files, "gateway.request.afterBodyRead", { body: "SECRET_TOKEN" })).toEqual({
      action: "pass",
    });

    const packed = packPlugin(files);
    const entries = unpackStoredZipEntries(packed.bytes);

    expect(entries.get("plugin.json")).toContain('"id": "acme.redactor"');
    expect(entries.get("rules/main.json")).toContain('"kind": "replace"');

    const keyPair = generateSigningKeyPair();
    const signed = signPackage(packed.bytes, keyPair.privateKey);

    expect(signed.publicKey).toBe(keyPair.publicKey);
    expect(Buffer.from(signed.publicKey, "base64")).toHaveLength(32);
    expect(
      verify(
        null,
        Buffer.from(packed.bytes),
        createPublicKey({
          key: Buffer.concat([
            Buffer.from("302a300506032b6570032100", "hex"),
            Buffer.from(keyPair.publicKey, "base64"),
          ]),
          format: "der",
          type: "spki",
        }),
        Buffer.from(signed.signature, "base64")
      )
    ).toBe(true);
    expect(verifyPackage(packed.bytes, signed.signature, keyPair.publicKey)).toMatchObject({
      ok: true,
      checksum: packed.checksum,
    });
    expect(
      verifyPackage(new TextEncoder().encode("tampered"), signed.signature, keyPair.publicKey)
    ).toMatchObject({
      ok: false,
    });
  });

  it("publish-check emits package metadata needed by the market flow", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    const packed = packPlugin(files);
    const keyPair = generateSigningKeyPair();
    const signed = signPackage(packed.bytes, keyPair.privateKey);

    const result = publishCheckPluginBytes(packed.bytes, {
      checksum: signed.checksum,
      signature: signed.signature,
      publicKey: signed.publicKey,
      manifest: files["plugin.json"] ?? "",
    });

    expect(result).toMatchObject({
      ok: true,
      checksum: signed.checksum,
      signatureVerified: true,
      manifestId: "acme.redactor",
      version: "0.1.0",
      runtime: "declarativeRules",
    });
  });

  it("signs and verifies package bytes through the CLI helper", () => {
    const keyPair = generateSigningKeyPair();
    const signedOutput: string[] = [];
    const verifyOutput: string[] = [];

    expect(
      runCreateAioPluginCli(["sign", "package-bytes", keyPair.privateKey], process.cwd(), {
        log: (line) => signedOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);
    const signed = JSON.parse(signedOutput[0] ?? "{}") as {
      checksum: string;
      signature: string;
      publicKey: string;
    };

    expect(
      runCreateAioPluginCli(
        ["verify", "package-bytes", signed.signature, signed.publicKey],
        process.cwd(),
        {
          log: (line) => verifyOutput.push(line),
          error: () => undefined,
        }
      )
    ).toBe(0);

    expect(JSON.parse(verifyOutput[0] ?? "{}")).toMatchObject({
      ok: true,
      checksum: signed.checksum,
    });
  });

  it("packs a scaffold into an .aio-plugin file through the CLI helper", () => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-pack-"));
    writeScaffold(
      join(cwd, "acme.redactor"),
      createPluginScaffold({
        id: "acme.redactor",
        name: "Redactor",
        template: "rule",
      })
    );
    const output: string[] = [];

    expect(
      runCreateAioPluginCli(["pack", "./acme.redactor"], cwd, {
        log: (line) => output.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const result = JSON.parse(output[0] ?? "{}") as {
      path: string;
      checksum: string;
      sizeBytes: number;
    };

    expect(result.path).toBe(join(cwd, "acme.redactor.aio-plugin"));
    expect(result.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
    expect(result.sizeBytes).toBeGreaterThan(0);
    expect(existsSync(result.path)).toBe(true);
  });

  it("publish-check command emits release metadata without writing the artifact", () => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-publish-check-"));
    writeScaffold(
      join(cwd, "acme.redactor"),
      createPluginScaffold({
        id: "acme.redactor",
        name: "Redactor",
        template: "rule",
      })
    );
    const output: string[] = [];

    expect(
      runCreateAioPluginCli(["publish-check", "./acme.redactor"], cwd, {
        log: (line) => output.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const result = JSON.parse(output[0] ?? "{}") as {
      artifactPath: string;
      checksum: string;
      manifestId: string;
      signatureVerified: boolean;
    };

    expect(result).toMatchObject({
      artifactPath: join(cwd, "acme.redactor.aio-plugin"),
      manifestId: "acme.redactor",
      signatureVerified: false,
    });
    expect(result.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
    expect(existsSync(result.artifactPath)).toBe(false);
  });

  it("validate command reads plugin.json from a real plugin directory", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-"));
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));

    const result = validatePluginDirectory(root);

    expect(result).toEqual({ ok: true });
  });

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

  it("validate strict rejects empty declarative rule documents", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = "";

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

  it("validate strict rejects non-object declarative rule documents", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = "null";

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_RULES_MISSING_ARRAY",
        path: "rules/main.json#/rules",
      })
    );
  });

  it("validate strict rejects rule documents that exceed the host runtime limit", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: Array.from({ length: 257 }, (_, index) => ({
          id: `rule-${index}`,
          hook: "gateway.request.afterBodyRead",
          target: { field: "request.body" },
          match: { regex: "SECRET" },
          action: { kind: "replace", replacement: "[x]" },
        })),
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_RULE_TOO_MANY_RULES",
        path: "rules/main.json#/rules",
      })
    );
  });

  it("validate strict rejects merged rule files that exceed the host runtime limit", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
      runtime: { kind: "declarativeRules"; rules: string[] };
    };
    manifest.runtime.rules = ["rules/a.json", "rules/b.json"];
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    const rules = (prefix: string) =>
      Array.from({ length: 200 }, (_, index) => ({
        id: `${prefix}-${index}`,
        hook: "gateway.request.afterBodyRead",
        target: { field: "request.body" },
        match: { regex: "SECRET" },
        action: { kind: "replace", replacement: "[x]" },
      }));
    files["rules/a.json"] = `${JSON.stringify({ rules: rules("a") }, null, 2)}\n`;
    files["rules/b.json"] = `${JSON.stringify({ rules: rules("b") }, null, 2)}\n`;
    delete files["rules/main.json"];

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_RULE_TOO_MANY_RULES",
        path: "plugin.json#/runtime/rules",
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

  it("validate strict rejects rules with missing id, target, match, or action", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            hook: "gateway.request.afterBodyRead",
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "missing-match",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "missing-action",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ID_MISSING",
        path: "rules/main.json#/rules/0/id",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_TARGET_MISSING",
        path: "rules/main.json#/rules/0/target",
      })
    );
    expect(result.diagnostics).not.toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_TARGET_INCOMPATIBLE_WITH_HOOK",
        path: "rules/main.json#/rules/0/target/field",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_MISSING",
        path: "rules/main.json#/rules/1/match",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_MISSING",
        path: "rules/main.json#/rules/2/action",
      })
    );
  });

  it("validate strict rejects the legacy matcher alias because the host runtime requires match", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const rule = document.rules?.[0];
    if (rule) {
      rule.matcher = rule.match;
      delete rule.match;
    }
    files["rules/main.json"] = `${JSON.stringify(document, null, 2)}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_MISSING",
        path: "rules/main.json#/rules/0/match",
      })
    );
  });

  it("validate strict rejects malformed matcher and action payloads", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: 7 },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "replace" },
          },
          {
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "warn" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/0/match/regex",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/1/action/replacement",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/2/action/message",
      })
    );
  });

  it("validate strict rejects regex patterns that the host runtime cannot compile", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            id: "lookahead",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "secret(?=token)" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "oversized",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "a".repeat(4097) },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-syntax",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "[" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-group",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "(" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-repeat",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "*" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "named-backref",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "(?<word>secret)\\k<word>" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-alternation-star",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "secret|*token" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-alternation-plus",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "secret|+token" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-alternation-question",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "secret|?token" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-group-star",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "(*token)" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "invalid-group-plus",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "(+token)" },
            action: { kind: "replace", replacement: "[x]" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/0/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/1/match/regex",
        message: expect.stringContaining("too large"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/2/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/3/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/4/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/5/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/6/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/7/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/8/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/9/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/10/match/regex",
        message: expect.stringContaining("unsupported Rust regex syntax"),
      })
    );
  });

  it("validate strict accepts Rust regex inline flags that JavaScript RegExp cannot compile", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.request.afterBodyRead",
      target: { field: "request.body" },
    });
    const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const rule = document.rules?.[0];
    if (rule) {
      rule.match = { regex: "(?i)secret" };
    }
    files["rules/main.json"] = `${JSON.stringify(document, null, 2)}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(true);
  });

  it("validate strict rejects matcher and when fields that host serde cannot parse", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            id: "bad-case-sensitive",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET", caseSensitive: "false" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "bad-when-cli-keys",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
            when: { cliKeys: "codex" },
          },
          {
            id: "bad-when-models",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
            when: { models: [7] },
          },
          {
            id: "bad-when-config",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
            when: { configEquals: [] },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_MATCHER_INVALID",
        path: "rules/main.json#/rules/0/match/caseSensitive",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_WHEN_INVALID",
        path: "rules/main.json#/rules/1/when/cliKeys",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_WHEN_INVALID",
        path: "rules/main.json#/rules/2/when/models",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_WHEN_INVALID",
        path: "rules/main.json#/rules/3/when/configEquals",
      })
    );
  });

  it("validate strict rejects target jsonPath syntax that the host runtime cannot parse", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            id: "missing-root",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body", jsonPath: "messages[*].content" },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "numeric-index",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body", jsonPath: "$.messages[0].content" },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
          },
          {
            id: "quoted-key",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body", jsonPath: '$."messages"' },
            match: { regex: "SECRET" },
            action: { kind: "replace", replacement: "[x]" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_TARGET_INVALID",
        path: "rules/main.json#/rules/0/target/jsonPath",
        message: expect.stringContaining("must start with $"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_TARGET_INVALID",
        path: "rules/main.json#/rules/1/target/jsonPath",
        message: expect.stringContaining("only [*] array wildcards"),
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_TARGET_INVALID",
        path: "rules/main.json#/rules/2/target/jsonPath",
        message: expect.stringContaining("quoted JSON path keys"),
      })
    );
  });

  it("validate strict rejects unsupported action kinds", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "drop" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/0/action/kind",
      })
    );
  });

  it("validate strict rejects appendMessage payloads that the host runtime rejects", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            id: "user-role",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "appendMessage", role: "user", content: "hello" },
          },
          {
            id: "blank-content",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "appendMessage", role: "developer", content: "   " },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/0/action/role",
        message: "appendMessage role must be system or developer",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/1/action/content",
        message: "appendMessage content must not be empty",
      })
    );
  });

  it("validate strict rejects write-only request rules because matching needs body read access", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.request.beforeSend",
      target: { field: "request.body" },
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
      permissions: string[];
    };
    manifest.permissions = ["request.body.write"];
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_PERMISSION_MISMATCH",
        message: expect.stringContaining("request.body.read"),
      })
    );
  });

  it("validate strict rejects write-only gateway error response rules because matching needs body read access", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.error",
      target: { field: "response.body" },
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
      permissions: string[];
    };
    manifest.permissions = ["response.body.write"];
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_PERMISSION_MISMATCH",
        message: expect.stringContaining("response.body.read"),
      })
    );
  });

  it("validate strict rejects request body targets on gateway error hooks", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.error",
      target: { field: "request.body" },
    });

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_RULE_TARGET_INCOMPATIBLE_WITH_HOOK",
        path: "rules/main.json#/rules/0/target/field",
        hint: "Use one of: response.body.",
      })
    );
  });

  it("validate strict skips permission mismatch noise when action is invalid", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.request.afterBodyRead",
      target: { field: "request.body" },
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
      permissions: string[];
    };
    manifest.permissions = [];
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    files["rules/main.json"] = `${JSON.stringify(
      {
        rules: [
          {
            id: "invalid-action",
            hook: "gateway.request.afterBodyRead",
            target: { field: "request.body" },
            match: { regex: "SECRET" },
            action: { kind: "drop" },
          },
        ],
      },
      null,
      2
    )}\n`;

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_ACTION_INVALID",
        path: "rules/main.json#/rules/0/action/kind",
      })
    );
    expect(result.diagnostics).not.toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_PERMISSION_MISMATCH",
      })
    );
  });

  it("validate strict rejects rule targets that do not match the hook", () => {
    const files = rulePluginFilesWithRule({
      hook: "gateway.response.after",
      target: { field: "request.body" },
    });

    const result = validatePluginFilesStrict(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_RULE_TARGET_INCOMPATIBLE_WITH_HOOK",
        path: "rules/main.json#/rules/0/target/field",
      })
    );
    expect(result.diagnostics).not.toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_PERMISSION_MISMATCH",
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

  it("doctor reports an invalid manifest diagnostic for incomplete plugin.json", () => {
    const result = doctorPluginFiles({ "plugin.json": "{}\n" });

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_ID",
        path: "plugin.json",
      })
    );
  });

  it("doctor distinguishes empty and non-object plugin.json content", () => {
    const emptyResult = doctorPluginFiles({ "plugin.json": "" });

    expect(emptyResult.ok).toBe(false);
    expect(emptyResult.diagnostics).toEqual([
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST_JSON",
        path: "plugin.json",
      }),
    ]);

    const nullResult = doctorPluginFiles({ "plugin.json": "null\n" });

    expect(nullResult.ok).toBe(false);
    expect(nullResult.diagnostics).toEqual([
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
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

  it("doctor only treats own runtime file entries as present", () => {
    const ruleFiles = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    delete ruleFiles["rules/main.json"];
    const ruleManifest = JSON.parse(ruleFiles["plugin.json"] ?? "{}") as Record<string, unknown>;
    ruleManifest.runtime = { kind: "declarativeRules", rules: ["toString"] };
    ruleFiles["plugin.json"] = `${JSON.stringify(ruleManifest, null, 2)}\n`;

    const ruleResult = doctorPluginFiles(ruleFiles);

    expect(ruleResult.ok).toBe(false);
    expect(ruleResult.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_RULE_FILE_MISSING",
        path: "toString",
      })
    );

    const wasmFiles = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });
    const wasmManifest = JSON.parse(wasmFiles["plugin.json"] ?? "{}") as Record<string, unknown>;
    wasmManifest.entry = "toString";
    wasmFiles["plugin.json"] = `${JSON.stringify(wasmManifest, null, 2)}\n`;

    const wasmResult = doctorPluginFiles(wasmFiles);

    expect(wasmResult.ok).toBe(false);
    expect(wasmResult.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_WASM_ENTRY_MISSING",
        path: "toString",
      })
    );
  });

  it("doctor treats empty runtime files as present", () => {
    const ruleFiles = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    ruleFiles["rules/main.json"] = "";

    const ruleResult = doctorPluginFiles(ruleFiles);

    expect(ruleResult.diagnostics).not.toContainEqual(
      expect.objectContaining({ code: "PLUGIN_RULE_FILE_MISSING" })
    );

    const wasmFiles = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });
    wasmFiles["plugin.wasm"] = "";

    const wasmResult = doctorPluginFiles(wasmFiles);

    expect(wasmResult.diagnostics).not.toContainEqual(
      expect.objectContaining({ code: "PLUGIN_WASM_ENTRY_MISSING" })
    );
    expect(wasmResult.diagnostics).toContainEqual(
      expect.objectContaining({ code: "PLUGIN_WASM_POLICY_GATED" })
    );
  });

  it("doctor rejects malformed runtime shapes that SDK validation does not catch", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.runtime = { kind: "declarativeRules", rules: "rules/main.json" };
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_RUNTIME",
        path: "plugin.json#/runtime",
      })
    );
  });

  it("doctor rejects non-string declarative rule paths", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.runtime = { kind: "declarativeRules", rules: [0] };
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    files["0"] = "{}";

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_RUNTIME",
        path: "plugin.json#/runtime",
      })
    );
  });

  it("doctor rejects malformed manifest field types that SDK validation can coerce", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.name = 123;
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
        path: "plugin.json#/name",
      })
    );
  });

  it("doctor rejects malformed wasm runtime metadata", () => {
    const files = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.runtime = { kind: "wasm", abiVersion: ["1.0.0"] };
    manifest.entry = 42;
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    files["42"] = "";

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_RUNTIME",
        path: "plugin.json#/runtime/abiVersion",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_MANIFEST",
        path: "plugin.json#/entry",
      })
    );
  });

  it("doctor rejects malformed optional manifest metadata", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "rule",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.hooks = [{ name: "gateway.request.afterBodyRead", priority: "high" }];
    manifest.hostCompatibility = {
      app: ">=0.56.0 <1.0.0",
      pluginApi: "^1.0.0",
      platforms: "linux",
    };
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_INVALID_MANIFEST",
        path: "plugin.json#/hooks/0/priority",
      })
    );
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_INVALID_MANIFEST",
        path: "plugin.json#/hostCompatibility/platforms",
      })
    );
  });

  it("doctor rejects malformed wasm memory limits", () => {
    const files = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.runtime = { kind: "wasm", abiVersion: "1.0.0", memoryLimitBytes: "16MB" };
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    files["plugin.wasm"] = "";

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_INVALID_RUNTIME",
        path: "plugin.json#/runtime/memoryLimitBytes",
      })
    );
  });

  it("doctor does not use malformed wasm entry values as file paths", () => {
    const files = createPluginScaffold({
      id: "acme.policy",
      name: "Policy",
      template: "wasm",
    });
    const manifest = JSON.parse(files["plugin.json"] ?? "{}") as Record<string, unknown>;
    manifest.entry = 42;
    files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
    files["42"] = "";

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(false);
    expect(result.diagnostics).toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_INVALID_MANIFEST",
        path: "plugin.json#/entry",
      })
    );
    expect(result.diagnostics).not.toContainEqual(
      expect.objectContaining({
        code: "PLUGIN_WASM_ENTRY_MISSING",
        path: 42,
      })
    );
  });

  it("doctor command reads a real plugin directory and returns non-zero for errors", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-doctor-"));
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));
    const output: string[] = [];

    const directoryResult = doctorPluginDirectory(root);

    expect(directoryResult.ok).toBe(true);
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

  it("pack command writes package bytes from a real plugin directory", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-"));
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));

    const packed = packPluginDirectory(root);

    expect(packed.checksum).toMatch(/^sha256:/);
    expect(packed.bytes.length).toBeGreaterThan(64);
  });

  it("replay command applies scaffold rule to fixture context", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });

    const result = replayHook(files, "gateway.request.afterBodyRead", {
      request: {
        body: JSON.stringify({
          messages: [{ role: "user", content: "SECRET_TOKEN" }],
        }),
      },
    });

    expect(result).toMatchObject({ action: "replace" });
    expect(JSON.stringify(result)).toContain("[REDACTED]");
  });

  it("replay command emits the active vNext mutation envelope", () => {
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });

    const result = replayHook(files, "gateway.request.afterBodyRead", {
      request: {
        body: JSON.stringify({
          messages: [{ role: "user", content: "SECRET_TOKEN" }],
        }),
      },
    });

    expect(result).toMatchObject({
      action: "replace",
      requestBody: expect.stringContaining("[REDACTED]"),
    });
    expect(result).not.toHaveProperty("contextPatch");
  });

  it("replay applies same-target JSONPath replacement like the host rule runtime", () => {
    const files = createPluginScaffold({ id: "acme.redactor", name: "Redactor", template: "rule" });

    const result = replayHook(files, "gateway.request.afterBodyRead", {
      request: {
        body: JSON.stringify({
          messages: [{ role: "user", content: "SECRET_TOKEN" }],
        }),
      },
    });

    expect(result).toEqual({
      action: "replace",
      requestBody: JSON.stringify({
        messages: [{ role: "user", content: "[REDACTED]" }],
      }),
    });
  });

  it("replay supports block, warn, and appendMessage actions", () => {
    const blockFiles = rulePluginFilesWithAction({ kind: "block", reason: "blocked" });

    expect(
      replayHook(blockFiles, "gateway.request.afterBodyRead", { request: { body: "danger" } })
    ).toEqual({ action: "block", reason: "blocked" });

    const warnFiles = rulePluginFilesWithAction({ kind: "warn", message: "careful" });

    expect(
      replayHook(warnFiles, "gateway.request.afterBodyRead", { request: { body: "danger" } })
    ).toEqual({ action: "warn", message: "careful" });

    const appendFiles = rulePluginFilesWithAction({
      kind: "appendMessage",
      role: "developer",
      content: "Use safe mode",
    });

    const result = replayHook(appendFiles, "gateway.request.afterBodyRead", {
      request: { body: JSON.stringify({ messages: [{ role: "user", content: "hello" }] }) },
    });

    expect(JSON.stringify(result)).toContain("Use safe mode");
    expect(result).not.toHaveProperty("contextPatch");
  });

  it("replay replaces Codex/OpenAI Responses input text like the host rule runtime", () => {
    const files = rulePluginFilesWithTarget("$.input[*].content[*].text");

    const result = replayHook(files, "gateway.request.afterBodyRead", {
      request: {
        body: JSON.stringify({
          input: [
            {
              type: "message",
              role: "user",
              content: [{ type: "input_text", text: "SECRET_TOKEN" }],
            },
          ],
        }),
      },
    });

    expect(result).toEqual({
      action: "replace",
      requestBody: JSON.stringify({
        input: [
          {
            type: "message",
            role: "user",
            content: [{ type: "input_text", text: "[REDACTED]" }],
          },
        ],
      }),
    });
  });

  it("replay maps response, stream, and log targets to their host mutation fields", () => {
    expect(
      replayHook(
        rulePluginFilesWithRule({
          hook: "gateway.response.after",
          target: { field: "response.body" },
        }),
        "gateway.response.after",
        { response: { body: "SECRET_TOKEN" } }
      )
    ).toEqual({ action: "replace", responseBody: "[REDACTED]" });

    expect(
      replayHook(
        rulePluginFilesWithRule({
          hook: "gateway.response.chunk",
          target: { field: "stream.chunk" },
        }),
        "gateway.response.chunk",
        { stream: { chunk: "data: SECRET_TOKEN\n\n" } }
      )
    ).toEqual({ action: "replace", streamChunk: "data: [REDACTED]\n\n" });

    expect(
      replayHook(
        rulePluginFilesWithRule({
          hook: "log.beforePersist",
          target: { field: "log.message" },
        }),
        "log.beforePersist",
        { log: { message: "apiKey=SECRET_TOKEN" } }
      )
    ).toEqual({ action: "replace", logMessage: "apiKey=[REDACTED]" });
  });

  it("replay explain reports a pass when no rule matches", () => {
    const result = replayHookExplain(
      rulePluginFilesWithTarget(undefined),
      "gateway.request.afterBodyRead",
      { request: { body: "ordinary text" } }
    );

    expect(result).toMatchObject({
      pluginId: "acme.redactor",
      runtime: "declarativeRules",
      hook: "gateway.request.afterBodyRead",
      evaluatedRuleCount: 1,
      matchedRuleIds: [],
      actionKind: "pass",
      outputKind: "pass",
      mutationSummary: { changed: false },
      result: { action: "pass" },
    });
  });

  it("replay explain reports replacement mutation details", () => {
    const result = replayHookExplain(
      rulePluginFilesWithTarget("$.messages[*].content"),
      "gateway.request.afterBodyRead",
      {
        request: {
          body: JSON.stringify({
            messages: [{ role: "user", content: "SECRET_TOKEN" }],
          }),
        },
      }
    );

    expect(result).toMatchObject({
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

  it("replay explain accepts exported host fixtures without changing the host contract shape", () => {
    const fixture = {
      schemaVersion: 1,
      source: {
        appVersion: "0.62.3",
        traceId: "trace-replay-1",
        exportedAtMs: 1_000,
        requestLogId: 1,
        createdAtMs: 900,
      },
      traceId: "trace-replay-1",
      hookName: "gateway.request.afterBodyRead",
      pluginId: "acme.redactor",
      request: {
        cliKey: "codex",
        method: "POST",
        path: "/v1/responses",
        body: { messages: [{ role: "user", content: "SECRET_TOKEN" }] },
        normalizedMessages: [{ role: "user", content: "SECRET_TOKEN" }],
      },
      response: null,
      log: null,
      attempts: [],
      runtimeReports: [],
      notes: [],
    };

    const result = replayHookExplain(
      rulePluginFilesWithTarget("$.messages[*].content"),
      "gateway.request.afterBodyRead",
      fixture
    );

    expect(result).toMatchObject({
      pluginId: "acme.redactor",
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

  it("replay explain supports Rust regex inline flags accepted by strict validation", () => {
    const files = rulePluginFilesWithTarget(undefined);
    const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const rule = document.rules?.[0];
    if (rule) {
      rule.match = { regex: "(?i)secret" };
    }
    files["rules/main.json"] = `${JSON.stringify(document, null, 2)}\n`;

    const result = replayHookExplain(files, "gateway.request.afterBodyRead", {
      request: { body: "SECRET token" },
    });

    expect(result).toMatchObject({
      matchedRuleIds: ["redact-token-rule"],
      actionKind: "replace",
      outputKind: "replace",
      result: { action: "replace", requestBody: "[REDACTED] token" },
    });
  });

  it("replay explain handles disabled leading inline flags", () => {
    const disabledFiles = rulePluginFilesWithTarget(undefined);
    const disabledDocument = JSON.parse(disabledFiles["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const disabledRule = disabledDocument.rules?.[0];
    if (disabledRule) {
      disabledRule.match = { regex: "(?-i)secret", caseSensitive: false };
    }
    disabledFiles["rules/main.json"] = `${JSON.stringify(disabledDocument, null, 2)}\n`;

    expect(
      replayHookExplain(disabledFiles, "gateway.request.afterBodyRead", {
        request: { body: "SECRET token" },
      })
    ).toMatchObject({
      matchedRuleIds: [],
      outputKind: "pass",
      result: { action: "pass" },
    });
  });

  it("replay explain warns instead of false passing complex Rust regex flags", () => {
    const toggledFiles = rulePluginFilesWithTarget(undefined);
    const toggledDocument = JSON.parse(toggledFiles["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const toggledRule = toggledDocument.rules?.[0];
    if (toggledRule) {
      toggledRule.match = { regex: "(?i)sec(?-i)ret" };
    }
    toggledFiles["rules/main.json"] = `${JSON.stringify(toggledDocument, null, 2)}\n`;

    const toggledResult = replayHookExplain(toggledFiles, "gateway.request.afterBodyRead", {
      request: { body: "SECRET token" },
    });

    expect(toggledResult).toMatchObject({
      matchedRuleIds: [],
      outputKind: "pass",
      warnings: [
        expect.objectContaining({
          severity: "warn",
          code: "PLUGIN_REPLAY_REGEX_UNSUPPORTED",
        }),
      ],
    });
    expect(
      replayHook(toggledFiles, "gateway.request.afterBodyRead", {
        request: { body: "SECRET token" },
      })
    ).toEqual({ action: "pass" });

    const extendedFiles = rulePluginFilesWithTarget(undefined);
    const extendedDocument = JSON.parse(extendedFiles["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const extendedRule = extendedDocument.rules?.[0];
    if (extendedRule) {
      extendedRule.match = { regex: "(?x)[ a ]" };
    }
    extendedFiles["rules/main.json"] = `${JSON.stringify(extendedDocument, null, 2)}\n`;

    expect(
      replayHookExplain(extendedFiles, "gateway.request.afterBodyRead", {
        request: { body: " " },
      })
    ).toMatchObject({
      matchedRuleIds: [],
      outputKind: "pass",
      warnings: [
        expect.objectContaining({
          severity: "warn",
          code: "PLUGIN_REPLAY_REGEX_UNSUPPORTED",
        }),
      ],
    });
  });

  it("replay handles Rust Unicode property classes without false passing", () => {
    const files = rulePluginFilesWithTarget(undefined);
    const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
      rules?: Array<Record<string, unknown>>;
    };
    const rule = document.rules?.[0];
    if (rule) {
      rule.match = { regex: "\\p{L}+" };
    }
    files["rules/main.json"] = `${JSON.stringify(document, null, 2)}\n`;

    expect(
      replayHookExplain(files, "gateway.request.afterBodyRead", {
        request: { body: "é token" },
      })
    ).toMatchObject({
      matchedRuleIds: ["redact-token-rule"],
      outputKind: "replace",
      result: { action: "replace", requestBody: "[REDACTED] [REDACTED]" },
    });
    expect(
      replayHook(files, "gateway.request.afterBodyRead", { request: { body: "é token" } })
    ).toEqual({
      action: "pass",
    });
  });

  it("replay explain reports block and warn matches without mutations", () => {
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
      result: { action: "block", reason: "blocked" },
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
      result: { action: "warn", message: "careful" },
    });
  });

  it("replay explain command emits JSON explanation", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-replay-explain-"));
    const fixturePath = join(root, "fixture.json");
    writeScaffold(root, rulePluginFilesWithTarget("$.messages[*].content"));
    writeFileSync(
      fixturePath,
      JSON.stringify({
        request: {
          body: JSON.stringify({
            messages: [{ role: "user", content: "SECRET_TOKEN" }],
          }),
        },
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
      pluginId: "acme.redactor",
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
  });
});

describe("create-aio-plugin example templates", () => {
  it("generates and replays the prompt-helper example", () => {
    const files = createPluginScaffold({
      id: "acme.prompt-helper",
      name: "Prompt Helper",
      template: "example:prompt-helper",
    });

    expect(files["plugin.json"]).toContain('"id": "acme.prompt-helper"');
    expect(files["rules/main.json"]).toContain("prompt-helper-claude");
    expect(files["fixtures/claude-request.json"]).toBeDefined();
    expect(files["fixtures/codex-request.json"]).toBeDefined();
    expect(validatePluginFilesStrict(files).ok).toBe(true);

    const claudeFixture = JSON.parse(files["fixtures/claude-request.json"] ?? "{}") as unknown;
    const claudeExplain = replayHookExplain(
      files,
      "gateway.request.afterBodyRead",
      claudeFixture
    );

    expect(claudeExplain).toMatchObject({
      pluginId: "acme.prompt-helper",
      actionKind: "replace",
      matchedRuleIds: ["prompt-helper-claude"],
      mutationSummary: { changed: true, field: "requestBody", targetField: "request.body" },
    });
    expect(JSON.stringify(claudeExplain.result)).toContain("Keep answers concise");

    const codexFixture = JSON.parse(files["fixtures/codex-request.json"] ?? "{}") as unknown;
    const codexExplain = replayHookExplain(
      files,
      "gateway.request.afterBodyRead",
      codexFixture
    );

    expect(codexExplain).toMatchObject({
      actionKind: "replace",
      matchedRuleIds: ["prompt-helper-codex"],
      mutationSummary: {
        changed: true,
        field: "requestBody",
        targetField: "request.body",
        jsonPath: "$.input[*].content[*].text",
      },
    });

    expectExampleCanPackAndPublishCheck(files, "acme.prompt-helper");
  });

  it("generates and replays the redactor example", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "example:redactor",
    });

    expect(files["plugin.json"]).toContain('"id": "acme.redactor"');
    expect(files["rules/main.json"]).toContain("redact-request-secrets");
    expect(files["fixtures/request-hit.json"]).toBeDefined();
    expect(files["fixtures/request-miss.json"]).toBeDefined();
    expect(files["fixtures/log-redact.json"]).toBeDefined();
    expect(validatePluginFilesStrict(files).ok).toBe(true);

    const hitFixture = JSON.parse(files["fixtures/request-hit.json"] ?? "{}") as unknown;
    const hitExplain = replayHookExplain(files, "gateway.request.beforeSend", hitFixture);
    expect(hitExplain).toMatchObject({
      actionKind: "replace",
      matchedRuleIds: ["redact-request-secrets"],
      mutationSummary: { changed: true, field: "requestBody", targetField: "request.body" },
    });
    expect(JSON.stringify(hitExplain.result)).toContain("[REDACTED]");

    const missFixture = JSON.parse(files["fixtures/request-miss.json"] ?? "{}") as unknown;
    const missExplain = replayHookExplain(files, "gateway.request.beforeSend", missFixture);
    expect(missExplain).toMatchObject({
      actionKind: "pass",
      mutationSummary: { changed: false },
    });

    const logFixture = JSON.parse(files["fixtures/log-redact.json"] ?? "{}") as unknown;
    const logExplain = replayHookExplain(files, "log.beforePersist", logFixture);
    expect(logExplain).toMatchObject({
      actionKind: "replace",
      matchedRuleIds: ["redact-log-secrets"],
      mutationSummary: { changed: true, field: "logMessage", targetField: "log.message" },
    });

    expectExampleCanPackAndPublishCheck(files, "acme.redactor");
  });

  it("generates and replays the response-guard example", () => {
    const files = createPluginScaffold({
      id: "acme.response-guard",
      name: "Response Guard",
      template: "example:response-guard",
    });

    expect(files["plugin.json"]).toContain('"id": "acme.response-guard"');
    expect(files["rules/main.json"]).toContain("response-guard-review-marker");
    expect(files["fixtures/response-warn.json"]).toBeDefined();
    expect(files["fixtures/response-pass.json"]).toBeDefined();
    expect(validatePluginFilesStrict(files).ok).toBe(true);

    const warnFixture = JSON.parse(files["fixtures/response-warn.json"] ?? "{}") as unknown;
    const warnExplain = replayHookExplain(files, "gateway.response.after", warnFixture);
    expect(warnExplain).toMatchObject({
      actionKind: "replace",
      matchedRuleIds: ["response-guard-review-marker"],
      mutationSummary: { changed: true, field: "responseBody", targetField: "response.body" },
    });
    expect(JSON.stringify(warnExplain.result)).toContain("[REVIEW_REQUIRED]");

    const passFixture = JSON.parse(files["fixtures/response-pass.json"] ?? "{}") as unknown;
    const passExplain = replayHookExplain(files, "gateway.response.after", passFixture);
    expect(passExplain).toMatchObject({
      actionKind: "pass",
      mutationSummary: { changed: false },
    });

    expectExampleCanPackAndPublishCheck(files, "acme.response-guard");
  });

  it.each([
    ["acme.prompt-helper", "example:prompt-helper"],
    ["acme.redactor", "example:redactor"],
    ["acme.response-guard", "example:response-guard"],
  ] as const)("writes %s through the CLI example template %s", (pluginId, template) => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-example-"));
    const scaffoldOutput: string[] = [];
    const validateOutput: string[] = [];
    const packOutput: string[] = [];
    const publishOutput: string[] = [];

    expect(
      runCreateAioPluginCli([pluginId, template], cwd, {
        log: (line) => scaffoldOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    expect(existsSync(join(cwd, pluginId, "plugin.json"))).toBe(true);
    expect(existsSync(join(cwd, pluginId, "rules/main.json"))).toBe(true);
    expect(existsSync(join(cwd, pluginId, "README.md"))).toBe(true);

    expect(
      runCreateAioPluginCli(["validate", "--strict", `./${pluginId}`], cwd, {
        log: (line) => validateOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);
    expect(JSON.parse(validateOutput[0] ?? "{}")).toMatchObject({ ok: true });

    expect(
      runCreateAioPluginCli(["pack", `./${pluginId}`], cwd, {
        log: (line) => packOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const packResult = JSON.parse(packOutput[0] ?? "{}") as {
      path: string;
      checksum: string;
      sizeBytes: number;
    };
    expect(packResult.path).toBe(join(cwd, `${pluginId}.aio-plugin`));
    expect(packResult.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
    expect(packResult.sizeBytes).toBeGreaterThan(0);
    expect(existsSync(packResult.path)).toBe(true);

    expect(
      runCreateAioPluginCli(["publish-check", `./${pluginId}`], cwd, {
        log: (line) => publishOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const publishResult = JSON.parse(publishOutput[0] ?? "{}") as {
      artifactPath: string;
      manifestId: string;
      checksum: string;
      signatureVerified: boolean;
    };
    expect(publishResult).toMatchObject({
      artifactPath: join(cwd, `${pluginId}.aio-plugin`),
      manifestId: pluginId,
      signatureVerified: false,
    });
    expect(publishResult.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
  });
});

function expectExampleCanPackAndPublishCheck(files: Record<string, string>, manifestId: string) {
  const packed = packPlugin(files);
  const result = publishCheckPluginBytes(packed.bytes, {
    checksum: packed.checksum,
    manifest: files["plugin.json"] ?? "",
  });

  expect(packed.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
  expect(result).toMatchObject({
    ok: true,
    manifestId,
    runtime: "declarativeRules",
    checksumVerified: true,
    signatureVerified: false,
    unsigned: true,
  });
  expect(result.hooks.length).toBeGreaterThan(0);
  expect(result.permissions.length).toBeGreaterThan(0);
}

function writeScaffold(root: string, files: Record<string, string>): void {
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
}

function validWasmManifest() {
  return {
    id: "acme.policy",
    name: "Policy",
    version: "0.1.0",
    apiVersion: "1.0.0",
    runtime: { kind: "wasm", abiVersion: "1.0.0", memoryLimitBytes: 16777216 },
    hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
    permissions: ["request.meta.read"],
    hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
    entry: "plugin.wasm",
  };
}

function rulePluginFilesWithAction(action: Record<string, unknown>): Record<string, string> {
  const files = rulePluginFilesWithTarget(undefined);
  const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
    rules?: Array<Record<string, unknown>>;
  };
  const rule = document.rules?.[0];
  if (rule) {
    rule.target = { field: "request.body" };
    rule.match = { regex: "danger|hello", caseSensitive: true };
    rule.action = action;
  }
  return {
    ...files,
    "rules/main.json": `${JSON.stringify(document, null, 2)}\n`,
  };
}

function rulePluginFilesWithTarget(jsonPath: string | undefined): Record<string, string> {
  return rulePluginFilesWithRule({
    hook: "gateway.request.afterBodyRead",
    target: jsonPath ? { field: "request.body", jsonPath } : { field: "request.body" },
  });
}

function rulePluginFilesWithRule(options: {
  hook: string;
  target: Record<string, unknown>;
}): Record<string, string> {
  const files = createPluginScaffold({ id: "acme.redactor", name: "Redactor", template: "rule" });
  const manifest = JSON.parse(files["plugin.json"] ?? "{}") as {
    hooks?: Array<Record<string, unknown>>;
    permissions?: string[];
  };
  if (manifest.hooks?.[0]) {
    manifest.hooks[0].name = options.hook;
  }
  manifest.permissions = permissionsForReplayTarget(options.target.field);
  const document = JSON.parse(files["rules/main.json"] ?? "{}") as {
    rules?: Array<Record<string, unknown>>;
  };
  const rule = document.rules?.[0];
  if (rule) {
    rule.id = "redact-token-rule";
    rule.hook = options.hook;
    rule.target = options.target;
  }
  return {
    ...files,
    "plugin.json": `${JSON.stringify(manifest, null, 2)}\n`,
    "rules/main.json": `${JSON.stringify(document, null, 2)}\n`,
  };
}

function permissionsForReplayTarget(field: unknown): string[] {
  switch (field) {
    case "response.body":
      return ["response.body.read", "response.body.write"];
    case "stream.chunk":
      return ["stream.inspect", "stream.modify"];
    case "log.message":
      return ["log.redact"];
    case "request.body":
    default:
      return ["request.body.read", "request.body.write"];
  }
}

function unpackStoredZipEntries(bytes: Uint8Array): Map<string, string> {
  const entries = new Map<string, string>();
  let offset = 0;
  while (offset + 30 <= bytes.length) {
    const signature = readU32(bytes, offset);
    if (signature !== 0x04034b50) break;
    const compression = readU16(bytes, offset + 8);
    const compressedSize = readU32(bytes, offset + 18);
    const nameLength = readU16(bytes, offset + 26);
    const extraLength = readU16(bytes, offset + 28);
    expect(compression).toBe(0);
    const nameStart = offset + 30;
    const dataStart = nameStart + nameLength + extraLength;
    const name = new TextDecoder().decode(bytes.subarray(nameStart, nameStart + nameLength));
    const data = bytes.subarray(dataStart, dataStart + compressedSize);
    entries.set(name, new TextDecoder().decode(data));
    offset = dataStart + compressedSize;
  }
  return entries;
}

function readStoredZipEntry(bytes: Uint8Array, expectedName: string): Uint8Array | null {
  let offset = 0;
  while (offset + 30 <= bytes.length) {
    const signature = readU32(bytes, offset);
    if (signature !== 0x04034b50) break;
    const compression = readU16(bytes, offset + 8);
    const compressedSize = readU32(bytes, offset + 18);
    const nameLength = readU16(bytes, offset + 26);
    const extraLength = readU16(bytes, offset + 28);
    expect(compression).toBe(0);
    const nameStart = offset + 30;
    const dataStart = nameStart + nameLength + extraLength;
    const name = new TextDecoder().decode(bytes.subarray(nameStart, nameStart + nameLength));
    const data = bytes.subarray(dataStart, dataStart + compressedSize);
    if (name === expectedName) {
      return new Uint8Array(data);
    }
    offset = dataStart + compressedSize;
  }
  return null;
}

function readU16(bytes: Uint8Array, offset: number): number {
  return bytes[offset] | ((bytes[offset + 1] ?? 0) << 8);
}

function readU32(bytes: Uint8Array, offset: number): number {
  return (
    ((bytes[offset] ?? 0) |
      ((bytes[offset + 1] ?? 0) << 8) |
      ((bytes[offset + 2] ?? 0) << 16) |
      ((bytes[offset + 3] ?? 0) << 24)) >>>
    0
  );
}
