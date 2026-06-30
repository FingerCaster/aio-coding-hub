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

export type ReplayExplainResult = never;

export type PublishCheckResult = {
  ok: boolean;
  checksum: string;
  expectedChecksum: string;
  checksumVerified: boolean;
  signatureVerified: boolean;
  unsigned: boolean;
  manifestId: string;
  name: string;
  version: string;
  runtime: string;
  capabilities: string[];
  hooks: string[];
  hostCompatibility: PluginManifest["hostCompatibility"];
  sizeBytes: number;
};

type DoctorOptions = {
  strict?: boolean;
};

type NormalizedPackagePathResult = { ok: true; path: string } | { ok: false; message: string };

const SUPPORTED_TEMPLATES: readonly ScaffoldTemplate[] = [
  "command",
  "rule",
  "example:prompt-helper",
  "example:redactor",
  "example:response-guard",
];
const UNSUPPORTED_PUBLIC_TEMPLATES = new Set(["wasm", "process", "native"]);
const USAGE = [
  "Usage:",
  "  create-aio-plugin <publisher.plugin-name> [command|rule|example:prompt-helper|example:redactor|example:response-guard]",
  "  rule is a legacy alias for command and generates an Extension Host command template.",
  "  create-aio-plugin doctor <plugin-dir>",
  "  create-aio-plugin validate [--strict] <plugin-dir>",
  "  create-aio-plugin replay [--explain] <plugin-dir> <fixture.json> <hook> (unsupported for Extension Host; returns PLUGIN_REPLAY_UNSUPPORTED)",
  "  create-aio-plugin pack <plugin-dir>",
  "  create-aio-plugin publish-check <plugin-dir>",
].join("\n");

export function runCreateAioPluginCli(args: string[], cwd: string, io: CliIo = console): number {
  const [commandOrId, firstArg, secondArg, thirdArg] = args;

  if (!commandOrId || commandOrId === "--help" || commandOrId === "-h" || commandOrId === "help") {
    io.log(USAGE);
    return commandOrId ? 0 : 1;
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
      const result = validatePluginDirectory(root);
      const text = JSON.stringify(result);
      if (result.ok) {
        io.log(text);
        return 0;
      }
      io.error(text);
      return 1;
    } catch (error) {
      io.error(`failed to validate plugin directory: ${errorMessage(error)}`);
      return 1;
    }
  }

  if (commandOrId === "replay") {
    const explain = firstArg === "--explain";
    const pluginDir = explain ? secondArg : firstArg;
    const fixturePath = explain ? thirdArg : secondArg;
    const hookName = explain ? args[4] : thirdArg;
    if (!pluginDir || !fixturePath || !hookName) {
      io.error(
        "Usage: create-aio-plugin replay <plugin-dir> <fixture.json> <hook> (unsupported for Extension Host; returns PLUGIN_REPLAY_UNSUPPORTED)"
      );
      return 1;
    }
    try {
      const files = readPluginDirectory(resolve(cwd, pluginDir));
      JSON.parse(readFileSync(resolve(cwd, fixturePath), "utf8")) as unknown;
      if (explain) {
        replayHookExplain(files, hookName as GatewayHookName, {});
      } else {
        replayHook(files, hookName as GatewayHookName, {});
      }
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
      const manifest = parseManifestText(textFromBytes(files["plugin.json"]) ?? "{}");
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

  if (commandOrId === "publish-check") {
    try {
      const root = resolve(cwd, firstArg ?? ".");
      const files = readPluginDirectoryBytes(root);
      const manifestText = textFromBytes(files["plugin.json"]) ?? "{}";
      const manifest = parseManifestText(manifestText);
      const packed = packPluginBytes(files);
      io.log(
        JSON.stringify({
          artifactPath: join(cwd, `${manifest.id ?? firstArg ?? "plugin"}.aio-plugin`),
          ...publishCheckPluginBytes(packed.bytes, {
            checksum: packed.checksum,
            manifest: manifestText,
          }),
        })
      );
      return 0;
    } catch (error) {
      io.error(`failed to publish-check plugin directory: ${errorMessage(error)}`);
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

  const templateArg = firstArg ?? "command";
  if (UNSUPPORTED_PUBLIC_TEMPLATES.has(templateArg)) {
    io.error(
      `PLUGIN_TEMPLATE_UNSUPPORTED: ${templateArg} templates are not supported. Use command, the legacy rule alias, or an Extension Host example.`
    );
    return 1;
  }
  if (!isScaffoldTemplate(templateArg)) {
    io.error(`PLUGIN_TEMPLATE_UNKNOWN: unknown Extension Host template: ${templateArg}\n${USAGE}`);
    return 1;
  }

  const idArg = commandOrId;
  const files = createPluginScaffold({
    id: idArg,
    name: titleFromId(idArg),
    template: templateArg,
  });

  for (const [path, content] of Object.entries(files)) {
    const fullPath = join(cwd, idArg, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }
  return 0;
}

export function validatePluginFiles(files: ScaffoldFiles): ValidationResult {
  const result = validatePluginFilesStrict(files);
  return result.ok ? { ok: true } : { ok: false, error: result.error };
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

export function doctorPluginFiles(
  files: ScaffoldFiles,
  _options: DoctorOptions = {}
): DoctorResult {
  const diagnostics: PluginDiagnostic[] = [];
  const manifestText = files["plugin.json"];

  if (manifestText == null) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_MISSING_MANIFEST",
      message: "missing plugin.json",
      path: "plugin.json",
      hint: "Run create-aio-plugin <publisher.plugin-name> or add an Extension Host manifest.",
    });
    return { ok: false, diagnostics };
  }

  let manifest: Partial<PluginManifest>;
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
    manifest = parsed as Partial<PluginManifest>;
  } catch (error) {
    diagnostics.push({
      severity: "error",
      code: "PLUGIN_INVALID_MANIFEST_JSON",
      message: `plugin.json is not valid JSON: ${errorMessage(error)}`,
      path: "plugin.json",
      hint: "Fix plugin.json before running validate, pack, or publish-check.",
    });
    return { ok: false, diagnostics };
  }

  const legacyRuntime = unsupportedLegacyRuntimeDiagnostic(manifest);
  if (legacyRuntime) {
    diagnostics.push(legacyRuntime);
    return { ok: false, diagnostics };
  }

  const validation = safelyValidateManifest(manifest);
  if (!validation.ok) {
    diagnostics.push({
      severity: "error",
      code: validation.error.code,
      message: validation.error.message,
      path: "plugin.json",
      hint: "Update the manifest so it matches Plugin API v1 Extension Host shape.",
    });
  }

  if (isExtensionHostManifest(manifest) && typeof manifest.main === "string") {
    const mainPath = normalizeExtensionMainPath(manifest.main);
    if (!mainPath.ok) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_INVALID_MAIN",
        message: mainPath.message,
        path: "plugin.json#/main",
        hint: 'Use a relative JavaScript entry point such as "dist/extension.js".',
      });
    } else if (!hasPluginFile(files, mainPath.path)) {
      diagnostics.push({
        severity: "error",
        code: "PLUGIN_MAIN_FILE_MISSING",
        message: `extension host main file is missing: ${mainPath.path}`,
        path: mainPath.path,
        hint: "Build or include dist/extension.js before packing the plugin.",
      });
    }
  }

  const manifestSummary = doctorManifestSummary(manifest, manifestRuntimeKind(manifest));

  return {
    ok: !hasErrorDiagnostics(diagnostics),
    diagnostics,
    ...(manifestSummary ? { manifest: manifestSummary } : {}),
  };
}

export function packPluginDirectory(root: string): PackedPlugin {
  return packPluginBytes(readPluginDirectoryBytes(root));
}

export function replayHook(
  _files: ScaffoldFiles,
  _hook: GatewayHookName,
  _context: unknown
): never {
  throw replayUnsupportedError();
}

export function replayHookExplain(
  _files: ScaffoldFiles,
  _hook: GatewayHookName,
  _context: unknown
): ReplayExplainResult {
  throw replayUnsupportedError();
}

export function packPlugin(files: ScaffoldFiles): PackedPlugin {
  assertPackageShape(files);
  const bytes = createStoredZipBytes(textFilesToBytes(files));
  return {
    bytes,
    checksum: sha256(bytes),
  };
}

export function packPluginBytes(files: PluginFileBytes): PackedPlugin {
  assertPackageShape(textFilesFromBytes(files));
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

export function publishCheckPluginBytes(
  bytes: Uint8Array,
  input: {
    checksum: string;
    signature?: string | null;
    publicKey?: string | null;
    manifest: string;
  }
): PublishCheckResult {
  const manifest = parseManifestText(input.manifest);
  const validation = safelyValidateManifest(manifest);
  const zipEntries = storedZipEntryNames(bytes);
  const entryNames = zipEntries.names;
  const mainPath =
    typeof manifest.main === "string" ? normalizeExtensionMainPath(manifest.main) : null;
  const checksum = sha256(bytes);
  const checksumVerified = checksum === input.checksum;
  const signatureVerified =
    input.signature && input.publicKey
      ? verifyPackage(bytes, input.signature, input.publicKey).ok
      : false;
  const unsigned = !input.signature || !input.publicKey;
  const hasPackageShape =
    zipEntries.ok &&
    entryNames.has("plugin.json") &&
    mainPath != null &&
    mainPath.ok &&
    entryNames.has(mainPath.path);

  return {
    ok: validation.ok && hasPackageShape && checksumVerified && (unsigned || signatureVerified),
    checksum,
    expectedChecksum: input.checksum,
    checksumVerified,
    signatureVerified,
    unsigned,
    manifestId: typeof manifest.id === "string" ? manifest.id : "",
    name: typeof manifest.name === "string" ? manifest.name : "",
    version: typeof manifest.version === "string" ? manifest.version : "",
    runtime: manifestRuntimeMetadataKind(manifest),
    capabilities: Array.isArray(manifest.capabilities) ? [...manifest.capabilities] : [],
    hooks: manifest.contributes?.gatewayHooks?.map((hook) => hook.name) ?? [],
    hostCompatibility: manifest.hostCompatibility ?? { app: "", pluginApi: "" },
    sizeBytes: bytes.length,
  };
}

function safelyValidateManifest(manifest: Partial<PluginManifest>): ValidationResult {
  try {
    return validateManifest(manifest as PluginManifest);
  } catch (error) {
    return {
      ok: false,
      error: { code: "PLUGIN_INVALID_MANIFEST", message: errorMessage(error) },
    };
  }
}

function assertPackageShape(files: ScaffoldFiles): void {
  const result = validatePluginFilesStrict(files);
  if (result.ok) return;
  throw new Error(`${result.error.code}: ${result.error.message}`);
}

function hasErrorDiagnostics(diagnostics: readonly PluginDiagnostic[]): boolean {
  return diagnostics.some((diagnostic) => diagnostic.severity === "error");
}

function hasPluginFile(files: ScaffoldFiles, path: string): boolean {
  for (const filePath of Object.keys(files)) {
    const normalized = normalizePackagePath(filePath, "plugin package entry path");
    if (normalized.ok && normalized.path === path) {
      return true;
    }
  }
  return false;
}

function normalizeExtensionMainPath(rawPath: string): NormalizedPackagePathResult {
  const normalized = normalizePackagePath(rawPath);
  if (!normalized.ok) return normalized;
  if (!/\.c?js$/.test(normalized.path)) {
    return {
      ok: false,
      message: "extensionHost main must point to a .js or .cjs file inside the package",
    };
  }
  return normalized;
}

function normalizePackagePath(
  rawPath: string,
  label = "extensionHost main"
): NormalizedPackagePathResult {
  const trimmed = rawPath.trim();
  if (!trimmed) {
    return {
      ok: false,
      message:
        label === "extensionHost main"
          ? "extensionHost runtime requires main"
          : `${label} is required`,
    };
  }
  if (hasWindowsDrivePrefix(trimmed) || trimmed.startsWith("/") || trimmed.startsWith("\\")) {
    return {
      ok: false,
      message: `${label} must be a relative path inside the package`,
    };
  }
  if (trimmed.includes("//") || trimmed.includes("\\\\")) {
    return {
      ok: false,
      message: `${label} must not contain repeated path separators`,
    };
  }

  const normalized = trimmed.replace(/\\/g, "/");
  const segments: string[] = [];
  for (const segment of normalized.split("/")) {
    if (segment === ".") continue;
    if (segment === "" || segment === "..") {
      return {
        ok: false,
        message: `${label} must be a relative path inside the package`,
      };
    }
    segments.push(segment);
  }
  if (segments.length === 0) {
    return {
      ok: false,
      message:
        label === "extensionHost main"
          ? "extensionHost runtime requires main"
          : `${label} is required`,
    };
  }
  return { ok: true, path: segments.join("/") };
}

function hasWindowsDrivePrefix(value: string): boolean {
  return /^[A-Za-z]:/.test(value);
}

function unsupportedLegacyRuntimeDiagnostic(
  manifest: Partial<PluginManifest>
): PluginDiagnostic | null {
  const runtime = asRecord(manifest.runtime);
  const kind = typeof runtime?.kind === "string" ? runtime.kind : "";
  if (!UNSUPPORTED_PUBLIC_TEMPLATES.has(kind)) {
    return null;
  }
  return {
    severity: "error",
    code: "PLUGIN_UNSUPPORTED_LEGACY_RUNTIME",
    message: `runtime ${kind} is no longer supported for community plugins; use Extension Host with main: "dist/extension.js".`,
    path: "plugin.json#/runtime",
    hint: 'Set runtime to { "kind": "extensionHost", "language": "typescript" }, add capabilities: ["gateway.hooks"], and move hook behavior into contributes.gatewayHooks.',
  };
}

function manifestRuntimeKind(
  manifest: Partial<PluginManifest>
): PluginManifest["runtime"]["kind"] | null {
  const runtime = asRecord(manifest.runtime);
  return runtime?.kind === "extensionHost" && runtime.language === "typescript"
    ? "extensionHost"
    : null;
}

function manifestRuntimeMetadataKind(manifest: Partial<PluginManifest>): string {
  const runtime = asRecord(manifest.runtime);
  return typeof runtime?.kind === "string" && runtime.kind.trim() !== "" ? runtime.kind : "unknown";
}

function isExtensionHostManifest(manifest: Partial<PluginManifest>): manifest is PluginManifest {
  return manifestRuntimeKind(manifest) === "extensionHost";
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

function textFilesToBytes(files: ScaffoldFiles): readonly (readonly [string, Uint8Array])[] {
  return Object.entries(files)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, content]) => [path, new TextEncoder().encode(content)] as const);
}

function textFilesFromBytes(files: PluginFileBytes): ScaffoldFiles {
  const textFiles: ScaffoldFiles = {};
  for (const path of Object.keys(files)) {
    textFiles[path] = textFromBytes(files[path]) ?? "";
  }
  return textFiles;
}

function textFromBytes(bytes: Uint8Array | undefined): string | undefined {
  return bytes == null ? undefined : new TextDecoder().decode(bytes);
}

function parseManifestText(text: string): Partial<PluginManifest> {
  const parsed = JSON.parse(text) as unknown;
  return asRecord(parsed) ? (parsed as Partial<PluginManifest>) : {};
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

function isScaffoldTemplate(value: string): value is ScaffoldTemplate {
  return SUPPORTED_TEMPLATES.some((template) => template === value);
}

function replayUnsupportedError(): Error {
  return new Error(
    "PLUGIN_REPLAY_UNSUPPORTED: Extension Host gateway hook replay is not executed locally by create-aio-plugin. Use validate, pack, publish-check, and host runtime traces for hook behavior."
  );
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

function storedZipEntryNames(bytes: Uint8Array): { names: Set<string>; ok: boolean } {
  const names = new Set<string>();
  let ok = true;
  let offset = 0;
  while (offset + 30 <= bytes.length) {
    const signature = readU32(bytes, offset);
    if (signature !== 0x04034b50) break;
    const compressedSize = readU32(bytes, offset + 18);
    const nameLength = readU16(bytes, offset + 26);
    const extraLength = readU16(bytes, offset + 28);
    const nameStart = offset + 30;
    const dataStart = nameStart + nameLength + extraLength;
    const normalizedName = normalizePackagePath(
      new TextDecoder().decode(bytes.subarray(nameStart, nameStart + nameLength)),
      "plugin package entry path"
    );
    if (normalizedName.ok) {
      names.add(normalizedName.path);
    } else {
      ok = false;
    }
    offset = dataStart + compressedSize;
  }
  return { names, ok };
}

function createStoredZipBytes(entries: readonly (readonly [string, Uint8Array])[]): Uint8Array {
  const chunks: Uint8Array[] = [];
  const centralDirectory: Uint8Array[] = [];
  let offset = 0;

  for (const [path, data] of entries) {
    const normalizedPath = normalizePackagePath(path, "plugin package entry path");
    if (!normalizedPath.ok) {
      throw new Error(`PLUGIN_INVALID_PACKAGE_PATH: ${normalizedPath.message}`);
    }
    const name = new TextEncoder().encode(normalizedPath.path);
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
