import { createPublicKey, verify } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
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
  it("creates an extension host command template", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "command",
    });
    const manifest = readManifest(files);

    expect(manifest).toMatchObject({
      id: "acme.redactor",
      name: "Redactor",
      version: "0.1.0",
      apiVersion: "1.0.0",
      main: "dist/extension.js",
      runtime: { kind: "extensionHost", language: "typescript" },
      activationEvents: ["onCommand:acme.redactor.hello"],
      contributes: {
        commands: [
          {
            command: "acme.redactor.hello",
            title: "Hello from Redactor",
          },
        ],
      },
      capabilities: ["commands.execute"],
      hostCompatibility: { app: ">=0.60.0 <1.0.0", pluginApi: "^1.0.0" },
    });
    expect(manifest).not.toHaveProperty("hooks");
    expect(manifest).not.toHaveProperty("permissions");
    expect(files["dist/extension.js"]).toContain(
      'api.commands.registerCommand("acme.redactor.hello"'
    );
    expect(files["README.md"]).toContain("acme.redactor");
    expectNoGeneratedLegacyFields(files);
    expect(validatePluginFilesStrict(files).ok).toBe(true);
  });

  it("keeps the legacy rule alias on the extension host command template", () => {
    const files = createPluginScaffold({
      id: "acme.legacy",
      name: "Legacy Alias",
      template: "rule",
    });
    const manifest = readManifest(files);

    expect(manifest.runtime).toEqual({ kind: "extensionHost", language: "typescript" });
    expect(manifest.main).toBe("dist/extension.js");
    expect(files).not.toHaveProperty("rules/main.json");
    expect(files["dist/extension.js"]).toContain(
      'api.commands.registerCommand("acme.legacy.hello"'
    );
    expectNoGeneratedLegacyFields(files);
  });

  it("documents the legacy rule template as a command alias in CLI usage", () => {
    const output: string[] = [];

    expect(
      runCreateAioPluginCli([], process.cwd(), {
        log: (line) => output.push(line),
        error: (line) => output.push(line),
      })
    ).toBe(1);

    expect(output[0]).toContain("rule");
    expect(output[0]).toContain("legacy alias for command");
  });

  it("rejects wasm as a public CLI template", () => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-wasm-unsupported-"));
    const output: string[] = [];

    expect(
      runCreateAioPluginCli(["acme.policy", "wasm"], cwd, {
        log: (line) => output.push(line),
        error: (line) => output.push(line),
      })
    ).toBe(1);

    expect(output[0]).toContain("PLUGIN_TEMPLATE_UNSUPPORTED");
    expect(output[0]).toContain("Extension Host");
    expect(existsSync(join(cwd, "acme.policy", "plugin.json"))).toBe(false);
  });

  it("writes the default command template through the CLI helper", () => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-default-"));

    expect(
      runCreateAioPluginCli(["acme.default"], cwd, {
        log: () => undefined,
        error: () => undefined,
      })
    ).toBe(0);

    expect(existsSync(join(cwd, "acme.default", "plugin.json"))).toBe(true);
    expect(existsSync(join(cwd, "acme.default", "dist/extension.js"))).toBe(true);
    expect(existsSync(join(cwd, "acme.default", "rules/main.json"))).toBe(false);
  });

  it("packs binary artifacts without utf8 rewriting", () => {
    const binaryBytes = new Uint8Array([0x00, 0x61, 0x69, 0x6f, 0xff, 0x00, 0x80]);
    const packed = packPluginBytes({
      "plugin.json": new TextEncoder().encode(JSON.stringify(validExtensionManifest())),
      "dist/extension.js": new TextEncoder().encode("module.exports.activate = function() {};\n"),
      "dist/payload.bin": binaryBytes,
    });

    expect(packed.checksum).toMatch(/^sha256:/);
    expect(readStoredZipEntry(packed.bytes, "dist/payload.bin")).toEqual(binaryBytes);
  });

  it("validates manifests, packs extension source, and verifies package signatures", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "command",
    });

    expect(validatePluginFiles(files).ok).toBe(true);

    const packed = packPlugin(files);
    const entries = unpackStoredZipEntries(packed.bytes);

    expect(entries.get("plugin.json")).toContain('"kind": "extensionHost"');
    expect(entries.get("dist/extension.js")).toContain("registerCommand");
    expect(entries.has("rules/main.json")).toBe(false);

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

  it("publish-check emits extension host package metadata", () => {
    const files = createPluginScaffold({
      id: "acme.redactor",
      name: "Redactor",
      template: "example:redactor",
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
      runtime: "extensionHost",
      capabilities: ["gateway.hooks"],
      hooks: ["gateway.request.beforeSend", "log.beforePersist"],
    });
  });

  it("publish-check reports runtime metadata from invalid manifests", () => {
    const files = createPluginScaffold({
      id: "acme.metadata",
      name: "Metadata",
      template: "command",
    });
    const packed = packPlugin(files);

    const wasmRuntime = publishCheckPluginBytes(packed.bytes, {
      checksum: packed.checksum,
      manifest: JSON.stringify({
        ...validExtensionManifest(),
        runtime: { kind: "wasm", abiVersion: "1.0.0" },
      }),
    });
    const missingRuntime = publishCheckPluginBytes(packed.bytes, {
      checksum: packed.checksum,
      manifest: JSON.stringify({
        ...validExtensionManifest(),
        runtime: undefined,
      }),
    });

    expect(wasmRuntime).toMatchObject({ ok: false, runtime: "wasm" });
    expect(missingRuntime).toMatchObject({ ok: false, runtime: "unknown" });
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

  it("pack and publish-check commands require extension host package shape", () => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-pack-shape-"));
    writeScaffold(
      join(cwd, "acme.redactor"),
      createPluginScaffold({
        id: "acme.redactor",
        name: "Redactor",
        template: "command",
      })
    );
    const packOutput: string[] = [];
    const publishCwd = mkdtempSync(join(tmpdir(), "aio-plugin-publish-shape-"));
    writeScaffold(
      join(publishCwd, "acme.redactor"),
      createPluginScaffold({
        id: "acme.redactor",
        name: "Redactor",
        template: "command",
      })
    );
    const publishOutput: string[] = [];

    expect(
      runCreateAioPluginCli(["pack", "./acme.redactor"], cwd, {
        log: (line) => packOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const packResult = JSON.parse(packOutput[0] ?? "{}") as {
      path: string;
      checksum: string;
      sizeBytes: number;
    };

    expect(packResult.path).toBe(join(cwd, "acme.redactor.aio-plugin"));
    expect(packResult.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
    expect(packResult.sizeBytes).toBeGreaterThan(0);
    expect(existsSync(packResult.path)).toBe(true);

    expect(
      runCreateAioPluginCli(["publish-check", "./acme.redactor"], publishCwd, {
        log: (line) => publishOutput.push(line),
        error: () => undefined,
      })
    ).toBe(0);

    const publishResult = JSON.parse(publishOutput[0] ?? "{}") as {
      artifactPath: string;
      checksum: string;
      manifestId: string;
      runtime: string;
      signatureVerified: boolean;
    };

    expect(publishResult).toMatchObject({
      artifactPath: join(publishCwd, "acme.redactor.aio-plugin"),
      manifestId: "acme.redactor",
      runtime: "extensionHost",
      signatureVerified: false,
    });
    expect(publishResult.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
    expect(existsSync(publishResult.artifactPath)).toBe(false);
  });

  it("pack rejects packages missing the extension host main file", () => {
    const files = createPluginScaffold({
      id: "acme.broken",
      name: "Broken",
      template: "command",
    });
    delete files["dist/extension.js"];

    expect(() => packPlugin(files)).toThrow(/PLUGIN_MAIN_FILE_MISSING/);
  });

  it("normalizes dot-prefixed extension host main paths for doctor, pack, and publish-check", () => {
    const files = createPluginScaffold({
      id: "acme.normalized",
      name: "Normalized",
      template: "command",
    });
    const manifest = readManifest(files);
    manifest.main = "./dist/extension.js";
    writeManifest(files, manifest);

    expect(doctorPluginFiles(files).ok).toBe(true);
    expect(validatePluginFiles(files).ok).toBe(true);

    const packed = packPlugin(files);
    const result = publishCheckPluginBytes(packed.bytes, {
      checksum: packed.checksum,
      manifest: files["plugin.json"] ?? "",
    });

    expect(result).toMatchObject({
      ok: true,
      manifestId: "acme.normalized",
      runtime: "extensionHost",
    });
  });

  it.each(["dist/extension.txt", "../extension.js"] as const)(
    "rejects unsafe or non-JavaScript extension host main path %s",
    (main) => {
      const files = createPluginScaffold({
        id: "acme.invalid-main",
        name: "Invalid Main",
        template: "command",
      });
      const manifest = readManifest(files);
      manifest.main = main;
      writeManifest(files, manifest);
      files[main] = "module.exports.activate = function() {};\n";

      const doctorResult = doctorPluginFiles(files);
      const validateResult = validatePluginFiles(files);

      expect(doctorResult.ok).toBe(false);
      expect(doctorResult.diagnostics).toContainEqual(
        expect.objectContaining({
          severity: "error",
          code: "PLUGIN_INVALID_MAIN",
        })
      );
      expect(validateResult).toMatchObject({
        ok: false,
        error: { code: "PLUGIN_INVALID_MAIN" },
      });
      expect(() => packPlugin(files)).toThrow(/PLUGIN_INVALID_MAIN/);
    }
  );

  it("validate command reads plugin.json from a real plugin directory", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-"));
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));

    const result = validatePluginDirectory(root);

    expect(result).toEqual({ ok: true });
  });

  it("validate strict rejects legacy runtime and contribution fields", () => {
    const legacy = validatePluginFilesStrict(legacyWasmFiles());

    expect(legacy.ok).toBe(false);
    expect(legacy.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_UNSUPPORTED_LEGACY_RUNTIME",
        path: "plugin.json#/runtime",
      })
    );

    const unknownContribution = validatePluginFilesStrict({
      "plugin.json": `${JSON.stringify(
        {
          ...validExtensionManifest(),
          contributes: { legacyRules: [{ rules: ["rules/main.json"] }] },
        },
        null,
        2
      )}\n`,
      "dist/extension.js": "module.exports.activate = function() {};\n",
    });

    expect(unknownContribution.ok).toBe(false);
    expect(unknownContribution.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_INVALID_CONTRIBUTION",
        path: "plugin.json",
      })
    );
  });

  it("validate command rejects packages missing the extension host main file", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-strict-"));
    const files = createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" });
    delete files["dist/extension.js"];
    writeScaffold(root, files);
    const normalOutput: string[] = [];
    const strictOutput: string[] = [];

    expect(
      runCreateAioPluginCli(["validate", root], process.cwd(), {
        log: (line) => normalOutput.push(line),
        error: (line) => normalOutput.push(line),
      })
    ).toBe(1);
    expect(JSON.parse(normalOutput[0] ?? "{}")).toMatchObject({
      ok: false,
      error: { code: "PLUGIN_MAIN_FILE_MISSING" },
    });

    expect(
      runCreateAioPluginCli(["validate", "--strict", root], process.cwd(), {
        log: (line) => strictOutput.push(line),
        error: (line) => strictOutput.push(line),
      })
    ).toBe(1);
    expect(JSON.parse(strictOutput[0] ?? "{}")).toMatchObject({
      ok: false,
      diagnostics: [expect.objectContaining({ code: "PLUGIN_MAIN_FILE_MISSING" })],
    });
  });

  it("doctor reports unsupported diagnostics for legacy manifests", () => {
    const legacyResult = doctorPluginFiles(legacyWasmFiles());

    expect(legacyResult.ok).toBe(false);
    expect(legacyResult.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_UNSUPPORTED_LEGACY_RUNTIME",
        message: expect.stringContaining("Extension Host"),
        path: "plugin.json#/runtime",
      })
    );
    expect(legacyResult.diagnostics).not.toContainEqual(
      expect.objectContaining({ code: "PLUGIN_RULE_FILE_MISSING" })
    );

    const wasmResult = doctorPluginFiles({
      "plugin.json": `${JSON.stringify(
        {
          id: "acme.policy",
          name: "Policy",
          version: "0.1.0",
          apiVersion: "1.0.0",
          runtime: { kind: "wasm", abiVersion: "1.0.0" },
          entry: "plugin.wasm",
          hostCompatibility: { app: ">=0.62.0 <1.0.0", pluginApi: "^1.0.0" },
        },
        null,
        2
      )}\n`,
    });

    expect(wasmResult.ok).toBe(false);
    expect(wasmResult.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_UNSUPPORTED_LEGACY_RUNTIME",
        path: "plugin.json#/runtime",
      })
    );
    expect(wasmResult.diagnostics).not.toContainEqual(
      expect.objectContaining({ code: "PLUGIN_WASM_ENTRY_MISSING" })
    );
  });

  it("doctor accepts extension host manifests and reports missing main files", () => {
    const files = createPluginScaffold({
      id: "acme.real",
      name: "Real",
      template: "command",
    });

    const result = doctorPluginFiles(files);

    expect(result.ok).toBe(true);
    expect(result.manifest).toMatchObject({
      id: "acme.real",
      runtime: "extensionHost",
    });

    delete files["dist/extension.js"];
    const missingMain = doctorPluginFiles(files);

    expect(missingMain.ok).toBe(false);
    expect(missingMain.diagnostics).toContainEqual(
      expect.objectContaining({
        severity: "error",
        code: "PLUGIN_MAIN_FILE_MISSING",
        path: "dist/extension.js",
      })
    );
  });

  it("doctor command reads a real plugin directory and returns non-zero for errors", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-doctor-"));
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));
    const output: string[] = [];

    expect(doctorPluginDirectory(root).ok).toBe(true);
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

  it("replay is disabled for extension host and legacy packages", () => {
    const extensionFiles = createPluginScaffold({
      id: "acme.real",
      name: "Real",
      template: "example:redactor",
    });

    expect(() =>
      replayHook(extensionFiles, "gateway.request.beforeSend", { request: { body: "token=abc" } })
    ).toThrow(/PLUGIN_REPLAY_UNSUPPORTED/);
    expect(() =>
      replayHookExplain(extensionFiles, "gateway.request.beforeSend", {
        request: { body: "token=abc" },
      })
    ).toThrow(/PLUGIN_REPLAY_UNSUPPORTED/);
    expect(() =>
      replayHook(legacyWasmFiles(), "gateway.request.afterBodyRead", {
        request: { body: "SECRET_TOKEN" },
      })
    ).toThrow(/PLUGIN_REPLAY_UNSUPPORTED/);
  });

  it("replay command reports that extension host gateway hooks are not run locally", () => {
    const root = mkdtempSync(join(tmpdir(), "aio-plugin-replay-"));
    const fixturePath = join(root, "fixture.json");
    writeScaffold(root, createPluginScaffold({ id: "acme.real", name: "Real", template: "rule" }));
    writeFileSync(fixturePath, JSON.stringify({ request: { body: "token=abc" } }));
    const output: string[] = [];

    expect(
      runCreateAioPluginCli(
        ["replay", "--explain", root, fixturePath, "gateway.request.beforeSend"],
        process.cwd(),
        {
          log: (line) => output.push(line),
          error: (line) => output.push(line),
        }
      )
    ).toBe(1);

    expect(output[0]).toContain("PLUGIN_REPLAY_UNSUPPORTED");
    expect(output[0]).toContain("Extension Host gateway hook replay is not executed locally");
  });
});

describe("create-aio-plugin example templates", () => {
  it.each([
    [
      "acme.prompt-helper",
      "Prompt Helper",
      "example:prompt-helper",
      ["gateway.request.afterBodyRead"],
    ],
    [
      "acme.redactor",
      "Redactor",
      "example:redactor",
      ["gateway.request.beforeSend", "log.beforePersist"],
    ],
    ["acme.response-guard", "Response Guard", "example:response-guard", ["gateway.response.after"]],
  ] as const)(
    "generates extension host gateway hook example %s",
    (pluginId, name, template, hooks) => {
      const files = createPluginScaffold({
        id: pluginId,
        name,
        template,
      });
      const manifest = readManifest(files);

      expect(manifest).toMatchObject({
        id: pluginId,
        main: "dist/extension.js",
        runtime: { kind: "extensionHost", language: "typescript" },
        capabilities: ["gateway.hooks"],
        hostCompatibility: { app: ">=0.60.0 <1.0.0", pluginApi: "^1.0.0" },
      });
      expect(manifest.contributes.gatewayHooks.map((hook: { name: string }) => hook.name)).toEqual(
        hooks
      );
      for (const hook of hooks) {
        expect(files["dist/extension.js"]).toContain(`api.gateway.registerHook("${hook}"`);
      }
      expect(files["dist/extension.js"]).toContain('action: "pass"');
      expect(validatePluginFilesStrict(files).ok).toBe(true);
      expectNoGeneratedLegacyFields(files);
      expectExampleReadmeDocumentsDevtoolsLoop(files);
      expectExampleCanPackAndPublishCheck(files, pluginId, hooks);
    }
  );

  it.each([
    ["acme.prompt-helper", "example:prompt-helper"],
    ["acme.redactor", "example:redactor"],
    ["acme.response-guard", "example:response-guard"],
  ] as const)("writes %s through the CLI example template %s", (pluginId, template) => {
    const cwd = mkdtempSync(join(tmpdir(), "aio-plugin-example-"));
    const validateOutput: string[] = [];
    const packOutput: string[] = [];
    const publishOutput: string[] = [];

    expect(
      runCreateAioPluginCli([pluginId, template], cwd, {
        log: () => undefined,
        error: () => undefined,
      })
    ).toBe(0);

    expect(existsSync(join(cwd, pluginId, "plugin.json"))).toBe(true);
    expect(existsSync(join(cwd, pluginId, "dist/extension.js"))).toBe(true);
    expect(existsSync(join(cwd, pluginId, "rules/main.json"))).toBe(false);

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
      runtime: string;
      signatureVerified: boolean;
    };
    expect(publishResult).toMatchObject({
      artifactPath: join(cwd, `${pluginId}.aio-plugin`),
      manifestId: pluginId,
      runtime: "extensionHost",
      signatureVerified: false,
    });
    expect(publishResult.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
  });
});

function expectExampleCanPackAndPublishCheck(
  files: Record<string, string>,
  manifestId: string,
  hooks: readonly string[]
) {
  const packed = packPlugin(files);
  const result = publishCheckPluginBytes(packed.bytes, {
    checksum: packed.checksum,
    manifest: files["plugin.json"] ?? "",
  });

  expect(packed.checksum).toMatch(/^sha256:[a-f0-9]{64}$/);
  expect(result).toMatchObject({
    ok: true,
    manifestId,
    runtime: "extensionHost",
    checksumVerified: true,
    signatureVerified: false,
    unsigned: true,
    capabilities: ["gateway.hooks"],
    hooks,
  });
}

function expectExampleReadmeDocumentsDevtoolsLoop(files: Record<string, string>) {
  const readme = files["README.md"] ?? "";

  expect(readme).toContain("development template, not a default installable marketplace package");
  expect(readme).toContain("pnpm --filter create-aio-plugin cli validate --strict .");
  expect(readme).toContain("pnpm --filter create-aio-plugin cli pack .");
  expect(readme).toContain("pnpm --filter create-aio-plugin cli publish-check .");
  expect(readme).not.toContain("create-aio-plugin replay");
}

function expectNoGeneratedLegacyFields(files: Record<string, string>): void {
  for (const path of Object.keys(files)) {
    expect(path).not.toContain("rules/");
  }
  const manifest = readManifest(files);
  expect(manifest.runtime).toEqual({
    kind: "extensionHost",
    language: "typescript",
  });
}

function readManifest(files: Record<string, string>): Record<string, any> {
  return JSON.parse(files["plugin.json"] ?? "{}") as Record<string, any>;
}

function writeManifest(files: Record<string, string>, manifest: Record<string, any>): void {
  files["plugin.json"] = `${JSON.stringify(manifest, null, 2)}\n`;
}

function writeScaffold(root: string, files: Record<string, string>): void {
  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(root, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
}

function validExtensionManifest() {
  return {
    id: "acme.binary",
    name: "Binary Payload",
    version: "0.1.0",
    apiVersion: "1.0.0",
    main: "dist/extension.js",
    runtime: { kind: "extensionHost", language: "typescript" },
    activationEvents: ["onStartup"],
    capabilities: [],
    hostCompatibility: { app: ">=0.60.0 <1.0.0", pluginApi: "^1.0.0" },
  };
}

function legacyWasmFiles(): Record<string, string> {
  return {
    "plugin.json": `${JSON.stringify(
      {
        id: "acme.legacy",
        name: "Legacy",
        version: "0.1.0",
        apiVersion: "1.0.0",
        runtime: { kind: "wasm", abiVersion: "1.0.0" },
        hooks: [{ name: "gateway.request.afterBodyRead", priority: 100 }],
        permissions: ["request.body.read", "request.body.write"],
        hostCompatibility: { app: ">=0.56.0 <1.0.0", pluginApi: "^1.0.0" },
      },
      null,
      2
    )}\n`,
    "plugin.wasm": "",
  };
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
  return (bytes[offset] ?? 0) | ((bytes[offset + 1] ?? 0) << 8);
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
