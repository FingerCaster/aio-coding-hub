import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  RELEASE_CHANNELS,
  parseReleaseTag,
  parseReleaseVersion,
  requirePlainObject,
  requireSha256,
  requireSourceSha,
} from "./release-contract.mjs";

export const RELEASE_VERSION_ATTESTATION_SCHEMA = 1;
export const RELEASE_VERSION_OVERLAY_SCRIPT_VERSION = 1;
export const RELEASE_VERSION_FILES = Object.freeze([
  "package.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
  "src-tauri/tauri.conf.json",
]);

const PLATFORM_PATTERN = /^[a-z0-9][a-z0-9_-]*$/;
const modulePath = fileURLToPath(import.meta.url);

function normalizeLf(text) {
  return text.replace(/\r\n?/g, "\n");
}

function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

function sha256File(path) {
  return sha256Bytes(readFileSync(path));
}

function run(cwd, command, args) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status}): ${result.stderr || result.stdout}`
    );
  }
  return result.stdout;
}

function gitChanges(repoRoot) {
  const output = run(repoRoot, "git", ["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
  if (output.length === 0) return [];
  const entries = output.split("\0").filter(Boolean);
  const paths = [];
  for (const entry of entries) {
    if (entry.length < 4 || entry[2] !== " ") {
      throw new Error(`Unsupported git status entry during version overlay: ${entry}`);
    }
    const path = entry.slice(3).replaceAll("\\", "/");
    if (entry.startsWith("R") || entry.startsWith("C")) {
      throw new Error("Version overlay does not accept renamed or copied files");
    }
    paths.push(path);
  }
  return [...new Set(paths)].sort();
}

function requireCleanCheckout(repoRoot) {
  const changes = gitChanges(repoRoot);
  if (changes.length !== 0) {
    throw new Error(`Version overlay requires a clean checkout: ${changes.join(", ")}`);
  }
}

function parseJsonFile(path, label) {
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new Error(`${label} is invalid JSON: ${error instanceof Error ? error.message : error}`);
  }
  return requirePlainObject(parsed, label);
}

function readJsonVersion(path, label) {
  const parsed = parseJsonFile(path, label);
  if (typeof parsed.version !== "string") {
    throw new Error(`${label} version must be a string`);
  }
  return parsed.version;
}

function writeJsonVersion(path, label, version) {
  const parsed = parseJsonFile(path, label);
  if (typeof parsed.version !== "string") {
    throw new Error(`${label} version must be a string`);
  }
  parsed.version = version;
  writeFileSync(path, `${JSON.stringify(parsed, null, 2)}\n`, "utf8");
}

function packageSection(text) {
  const normalized = normalizeLf(text);
  const headers = [...normalized.matchAll(/^\[package\][ \t]*$/gm)];
  if (headers.length !== 1) {
    throw new Error("src-tauri/Cargo.toml must contain exactly one [package] section");
  }
  const start = headers[0].index + headers[0][0].length;
  const remainder = normalized.slice(start);
  const nextHeader = /^\[[^\n]+\][ \t]*$/m.exec(remainder);
  const end = nextHeader ? start + nextHeader.index : normalized.length;
  return { normalized, start, end };
}

function readCargoTomlVersion(path) {
  const { normalized, start, end } = packageSection(readFileSync(path, "utf8"));
  const section = normalized.slice(start, end);
  const versions = [...section.matchAll(/^version[ \t]*=[ \t]*"([^"]+)"[ \t]*$/gm)];
  if (versions.length !== 1) {
    throw new Error("src-tauri/Cargo.toml [package] must contain exactly one version line");
  }
  return versions[0][1];
}

function writeCargoTomlVersion(path, version) {
  const { normalized, start, end } = packageSection(readFileSync(path, "utf8"));
  const section = normalized.slice(start, end);
  const versions = [...section.matchAll(/^version[ \t]*=[ \t]*"([^"]+)"[ \t]*$/gm)];
  if (versions.length !== 1) {
    throw new Error("src-tauri/Cargo.toml [package] must contain exactly one version line");
  }
  const relativeStart = versions[0].index;
  const relativeEnd = relativeStart + versions[0][0].length;
  const updatedSection = `${section.slice(0, relativeStart)}version = "${version}"${section.slice(relativeEnd)}`;
  writeFileSync(
    path,
    `${normalized.slice(0, start)}${updatedSection}${normalized.slice(end)}`,
    "utf8"
  );
}

function cargoLockRootPackage(text) {
  const normalized = normalizeLf(text);
  const starts = [...normalized.matchAll(/^\[\[package\]\][ \t]*$/gm)].map((match) => match.index);
  const matches = [];
  for (let index = 0; index < starts.length; index += 1) {
    const start = starts[index];
    const end = starts[index + 1] ?? normalized.length;
    const block = normalized.slice(start, end);
    const name = /^name[ \t]*=[ \t]*"([^"]+)"[ \t]*$/m.exec(block)?.[1];
    if (name !== "aio-coding-hub") continue;
    const versions = [...block.matchAll(/^version[ \t]*=[ \t]*"([^"]+)"[ \t]*$/gm)];
    if (versions.length !== 1) {
      throw new Error("src-tauri/Cargo.lock root package must contain exactly one version line");
    }
    matches.push({
      normalized,
      version: versions[0][1],
      versionStart: start + versions[0].index,
      versionEnd: start + versions[0].index + versions[0][0].length,
    });
  }
  if (matches.length !== 1) {
    throw new Error("src-tauri/Cargo.lock must contain exactly one aio-coding-hub package");
  }
  return matches[0];
}

function readCargoLockVersion(path) {
  return cargoLockRootPackage(readFileSync(path, "utf8")).version;
}

function expectedCargoLockText(originalText, version) {
  const rootPackage = cargoLockRootPackage(originalText);
  return `${rootPackage.normalized.slice(0, rootPackage.versionStart)}version = "${version}"${rootPackage.normalized.slice(rootPackage.versionEnd)}`;
}

function readVersions(repoRoot) {
  return {
    packageJson: readJsonVersion(join(repoRoot, "package.json"), "package.json"),
    cargoToml: readCargoTomlVersion(join(repoRoot, "src-tauri/Cargo.toml")),
    cargoLock: readCargoLockVersion(join(repoRoot, "src-tauri/Cargo.lock")),
    tauriConfig: readJsonVersion(
      join(repoRoot, "src-tauri/tauri.conf.json"),
      "src-tauri/tauri.conf.json"
    ),
  };
}

function requireSameVersion(versions, expectedVersion, label) {
  for (const [file, version] of Object.entries(versions)) {
    if (version !== expectedVersion) {
      throw new Error(
        `${label} version mismatch for ${file}: expected ${expectedVersion}, received ${version}`
      );
    }
  }
}

function cargoMetadata(repoRoot, locked) {
  const manifestPath = join(repoRoot, "src-tauri/Cargo.toml");
  const args = ["metadata", "--manifest-path", manifestPath, "--format-version", "1"];
  if (locked) args.push("--locked", "--no-deps");
  const output = run(repoRoot, "cargo", args);
  let metadata;
  try {
    metadata = JSON.parse(output);
  } catch (error) {
    throw new Error(
      `cargo metadata returned invalid JSON: ${error instanceof Error ? error.message : error}`
    );
  }
  return requirePlainObject(metadata, "cargo metadata output");
}

function requireCargoMetadataVersion(metadata, repoRoot, expectedVersion) {
  if (!Array.isArray(metadata.packages)) {
    throw new Error("cargo metadata packages must be an array");
  }
  const manifestPath = resolve(repoRoot, "src-tauri/Cargo.toml")
    .replaceAll("\\", "/")
    .toLowerCase();
  const packages = metadata.packages.filter(
    (item) =>
      item?.name === "aio-coding-hub" &&
      typeof item.manifest_path === "string" &&
      resolve(item.manifest_path).replaceAll("\\", "/").toLowerCase() === manifestPath
  );
  if (packages.length !== 1 || packages[0].version !== expectedVersion) {
    throw new Error(`cargo metadata did not resolve aio-coding-hub at ${expectedVersion}`);
  }
}

function normalizeFileDigests(value) {
  if (!Array.isArray(value) || value.length !== RELEASE_VERSION_FILES.length) {
    throw new Error("Version attestation files are invalid");
  }
  return value.map((entry, index) => {
    const file = requirePlainObject(entry, `Version attestation file ${index}`);
    const keys = Object.keys(file).sort();
    if (keys.join(",") !== "path,sha256") {
      throw new Error(`Version attestation file ${index} fields are invalid`);
    }
    if (file.path !== RELEASE_VERSION_FILES[index]) {
      throw new Error(`Version attestation file order is invalid at ${index}`);
    }
    return { path: file.path, sha256: requireSha256(file.sha256) };
  });
}

function attestationPayload({ channel, tag, sourceSha, version, overlayMode, files }) {
  return {
    schemaVersion: RELEASE_VERSION_ATTESTATION_SCHEMA,
    scriptVersion: RELEASE_VERSION_OVERLAY_SCRIPT_VERSION,
    channel,
    tag,
    sourceSha,
    version,
    overlayMode,
    files,
  };
}

export function parseReleaseVersionAttestation(text) {
  if (typeof text !== "string") {
    throw new Error("Version attestation must be text");
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `Version attestation is invalid JSON: ${error instanceof Error ? error.message : error}`
    );
  }
  const attestation = requirePlainObject(parsed, "Version attestation");
  const keys = Object.keys(attestation).sort();
  const expectedKeys = [
    "channel",
    "files",
    "overlayDigest",
    "overlayMode",
    "schemaVersion",
    "scriptVersion",
    "sourceSha",
    "tag",
    "version",
  ].sort();
  if (keys.join(",") !== expectedKeys.join(",")) {
    throw new Error("Version attestation fields are invalid");
  }
  if (
    attestation.schemaVersion !== RELEASE_VERSION_ATTESTATION_SCHEMA ||
    attestation.scriptVersion !== RELEASE_VERSION_OVERLAY_SCRIPT_VERSION
  ) {
    throw new Error("Version attestation schema or script version is unsupported");
  }
  const identity = parseReleaseTag(attestation.tag, attestation.channel);
  requireSourceSha(attestation.sourceSha);
  if (attestation.version !== identity.version) {
    throw new Error("Version attestation tag and version do not match");
  }
  const expectedMode = identity.channel === RELEASE_CHANNELS.beta ? "apply" : "none";
  if (attestation.overlayMode !== expectedMode) {
    throw new Error("Version attestation overlay mode does not match its channel");
  }
  const files = normalizeFileDigests(attestation.files);
  const payload = attestationPayload({ ...attestation, files });
  const overlayDigest = requireSha256(attestation.overlayDigest, "Version overlay digest");
  if (sha256Bytes(JSON.stringify(payload)) !== overlayDigest) {
    throw new Error("Version attestation overlay digest is invalid");
  }
  return { ...payload, overlayDigest };
}

export function applyReleaseVersionOverlay({ repoRoot, channel, tag, sourceSha, outputPath }) {
  const root = resolve(repoRoot);
  const identity = parseReleaseTag(tag, channel);
  requireSourceSha(sourceSha);
  requireCleanCheckout(root);

  const originalVersions = readVersions(root);
  const cargoLockPath = join(root, "src-tauri/Cargo.lock");
  const originalCargoLock = readFileSync(cargoLockPath, "utf8");
  const uniqueOriginalVersions = [...new Set(Object.values(originalVersions))];
  if (uniqueOriginalVersions.length !== 1) {
    throw new Error("Version overlay source files do not agree before mutation");
  }

  if (identity.channel === RELEASE_CHANNELS.beta) {
    if (parseReleaseVersion(uniqueOriginalVersions[0]).channel !== RELEASE_CHANNELS.stable) {
      throw new Error("Beta overlay source version must be stable");
    }
    writeJsonVersion(join(root, "package.json"), "package.json", identity.version);
    writeJsonVersion(
      join(root, "src-tauri/tauri.conf.json"),
      "src-tauri/tauri.conf.json",
      identity.version
    );
    writeCargoTomlVersion(join(root, "src-tauri/Cargo.toml"), identity.version);
    cargoMetadata(root, false);
    const actualCargoLock = normalizeLf(readFileSync(cargoLockPath, "utf8"));
    const expectedCargoLock = expectedCargoLockText(originalCargoLock, identity.version);
    if (actualCargoLock !== expectedCargoLock) {
      throw new Error("cargo metadata changed Cargo.lock beyond the root package version");
    }
    writeFileSync(cargoLockPath, actualCargoLock, "utf8");
  } else {
    requireSameVersion(originalVersions, identity.version, "Stable source");
  }

  const metadata = cargoMetadata(root, true);
  requireCargoMetadataVersion(metadata, root, identity.version);
  requireSameVersion(readVersions(root), identity.version, "Overlay output");

  const expectedChanges =
    identity.channel === RELEASE_CHANNELS.beta ? [...RELEASE_VERSION_FILES].sort() : [];
  const actualChanges = gitChanges(root);
  if (actualChanges.join("\n") !== expectedChanges.join("\n")) {
    throw new Error(
      `Version overlay changed an unexpected file set. Expected: ${expectedChanges.join(", ") || "<clean>"}. Actual: ${actualChanges.join(", ") || "<clean>"}.`
    );
  }

  const files = RELEASE_VERSION_FILES.map((path) => ({
    path,
    sha256: sha256File(join(root, path)),
  }));
  const payload = attestationPayload({
    channel: identity.channel,
    tag: identity.tag,
    sourceSha,
    version: identity.version,
    overlayMode: identity.channel === RELEASE_CHANNELS.beta ? "apply" : "none",
    files,
  });
  const attestation = { ...payload, overlayDigest: sha256Bytes(JSON.stringify(payload)) };
  const serialized = `${JSON.stringify(attestation, null, 2)}\n`;
  parseReleaseVersionAttestation(serialized);
  writeFileSync(resolve(outputPath), serialized, "utf8");
  return attestation;
}

function normalizePlatforms(platforms) {
  if (!Array.isArray(platforms) || platforms.length === 0) {
    throw new Error("Expected attestation platforms are required");
  }
  const normalized = platforms.map((platform) => {
    if (typeof platform !== "string" || !PLATFORM_PATTERN.test(platform)) {
      throw new Error(`Invalid attestation platform: ${platform}`);
    }
    return platform;
  });
  if (new Set(normalized).size !== normalized.length) {
    throw new Error("Expected attestation platforms contain duplicates");
  }
  return normalized.sort();
}

export function verifyReleaseVersionAttestations({
  directory,
  platforms,
  channel,
  tag,
  sourceSha,
}) {
  const root = resolve(directory);
  const expectedPlatforms = normalizePlatforms(platforms);
  const expectedNames = expectedPlatforms.map(
    (platform) => `release-version-attestation-${platform}.json`
  );
  const actualNames = readdirSync(root, { withFileTypes: true })
    .map((entry) => {
      if (!entry.isFile()) {
        throw new Error(`Attestation directory contains a non-file entry: ${entry.name}`);
      }
      return entry.name;
    })
    .sort();
  if (actualNames.join("\n") !== expectedNames.join("\n")) {
    throw new Error("Version attestation platform file set is incomplete or unexpected");
  }

  let referenceText;
  let reference;
  for (const name of expectedNames) {
    const text = readFileSync(join(root, name), "utf8");
    const attestation = parseReleaseVersionAttestation(text);
    if (referenceText !== undefined && text !== referenceText) {
      throw new Error(`Cross-platform version attestation drift: ${name}`);
    }
    referenceText = text;
    reference = attestation;
  }
  const identity = parseReleaseTag(tag, channel);
  requireSourceSha(sourceSha);
  if (
    reference.channel !== identity.channel ||
    reference.tag !== identity.tag ||
    reference.version !== identity.version ||
    reference.sourceSha !== sourceSha
  ) {
    throw new Error("Version attestation identity does not match the release candidate");
  }
  return reference;
}

function parseCliArgs(values) {
  const args = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value === undefined || args.has(key.slice(2))) {
      throw new Error("Version overlay arguments are invalid");
    }
    args.set(key.slice(2), value);
  }
  return args;
}

function cliValue(args, name) {
  const value = args.get(name);
  if (value === undefined || value.length === 0) {
    throw new Error(`Missing required argument: --${name}`);
  }
  return value;
}

function assertOnlyArgs(args, allowed, command) {
  if ([...args.keys()].some((key) => !allowed.has(key))) {
    throw new Error(`${command} received an unknown argument`);
  }
}

function runCli() {
  const command = process.argv[2];
  const args = parseCliArgs(process.argv.slice(3));
  const channel = cliValue(args, "channel");
  const tag = cliValue(args, "tag");
  const sourceSha = cliValue(args, "source-sha");
  if (command === "apply") {
    assertOnlyArgs(args, new Set(["root", "channel", "tag", "source-sha", "output"]), command);
    const attestation = applyReleaseVersionOverlay({
      repoRoot: cliValue(args, "root"),
      channel,
      tag,
      sourceSha,
      outputPath: cliValue(args, "output"),
    });
    console.error(`[release-version-overlay] verified ${attestation.version}`);
    return;
  }
  if (command === "verify-attestations") {
    assertOnlyArgs(
      args,
      new Set(["directory", "platforms", "channel", "tag", "source-sha"]),
      command
    );
    const attestation = verifyReleaseVersionAttestations({
      directory: cliValue(args, "directory"),
      platforms: cliValue(args, "platforms").split(","),
      channel,
      tag,
      sourceSha,
    });
    process.stdout.write(`${attestation.overlayDigest}\n`);
    return;
  }
  throw new Error(
    "Usage: node scripts/release-version-overlay.mjs <apply|verify-attestations> --channel <stable|beta> --tag <tag> --source-sha <sha> ..."
  );
}

if (process.argv[1] && resolve(process.argv[1]) === modulePath) {
  try {
    runCli();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
