import { createHash } from "node:crypto";
import {
  copyFileSync,
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  assertReleaseState,
  parseReleaseTag,
  requirePlainObject,
  requirePositiveInteger,
  requireSha256,
  requireSourceSha,
} from "./release-contract.mjs";

export const RELEASE_CANDIDATE_MANIFEST = "release-candidate.json";
export const EXPECTED_RELEASE_ASSET_NAMES = Object.freeze([
  "aio-coding-hub-linux-amd64.AppImage",
  "aio-coding-hub-linux-amd64.AppImage.sig",
  "aio-coding-hub-linux-amd64-wayland.AppImage",
  "aio-coding-hub-linux-amd64.deb",
  "aio-coding-hub-macos-arm.tar.gz",
  "aio-coding-hub-macos-arm.tar.gz.sig",
  "aio-coding-hub-macos-arm.zip",
  "aio-coding-hub-macos-intel.tar.gz",
  "aio-coding-hub-macos-intel.tar.gz.sig",
  "aio-coding-hub-macos-intel.zip",
  "aio-coding-hub-win64-portable.zip",
  "aio-coding-hub-win64.msi",
  "aio-coding-hub-win64.msi.sig",
  "latest.json",
]);
const MANIFEST_VERSION = 2;
const WINDOWS_RESERVED_SEGMENT = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;
const modulePath = fileURLToPath(import.meta.url);

function requireExactKeys(value, expectedKeys, label) {
  const actual = Object.keys(value).sort();
  const expected = [...expectedKeys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new Error(`${label} fields are invalid`);
  }
}

function requireAssetName(value, label = "Release asset name") {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 255 ||
    value !== value.trim() ||
    value === "." ||
    value === ".." ||
    value.toLowerCase() === RELEASE_CANDIDATE_MANIFEST ||
    value !== basename(value) ||
    value.includes("/") ||
    value.includes("\\") ||
    /[\u0000-\u001f\u007f<>:"|?*]/.test(value) ||
    /[. ]$/.test(value) ||
    WINDOWS_RESERVED_SEGMENT.test(value)
  ) {
    throw new Error(`${label} is invalid`);
  }
  return value;
}

function sameArray(left, right) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function compareNames(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function listCandidateFiles(directory) {
  const entries = readdirSync(directory, { withFileTypes: true });
  const names = [];
  const caseInsensitiveNames = new Set();
  for (const entry of entries) {
    if (!entry.isFile()) {
      throw new Error(`Candidate directory contains a non-file entry: ${entry.name}`);
    }
    if (entry.name !== RELEASE_CANDIDATE_MANIFEST) {
      const name = requireAssetName(entry.name, "Candidate asset name");
      const foldedName = name.toLowerCase();
      if (caseInsensitiveNames.has(foldedName)) {
        throw new Error(`Candidate directory contains case-colliding assets: ${name}`);
      }
      caseInsensitiveNames.add(foldedName);
      names.push(name);
    }
  }
  names.sort(compareNames);
  if (names.length === 0) {
    throw new Error("Release candidate has no assets");
  }
  return names;
}

function requireCurrentAssetMatrix(names, label) {
  const expected = [...EXPECTED_RELEASE_ASSET_NAMES].sort(compareNames);
  if (!sameArray(names, expected)) {
    throw new Error(`${label} does not match the current release asset matrix`);
  }
}

async function sha256File(path) {
  const digest = createHash("sha256");
  await new Promise((resolveStream, rejectStream) => {
    const stream = createReadStream(path);
    stream.on("data", (chunk) => digest.update(chunk));
    stream.on("error", rejectStream);
    stream.on("end", resolveStream);
  });
  return digest.digest("hex");
}

function normalizeIdentity({
  channel,
  tag,
  sourceSha,
  releaseVersion,
  overlayDigest,
  runId,
  runAttempt,
}) {
  const identity = parseReleaseTag(tag, channel);
  if (releaseVersion !== identity.version) {
    throw new Error(
      `Candidate release version mismatch: expected ${identity.version}, received ${releaseVersion}`
    );
  }
  return {
    channel: identity.channel,
    tag: identity.tag,
    sourceSha: requireSourceSha(sourceSha),
    releaseVersion: identity.version,
    overlayDigest: requireSha256(overlayDigest, "Candidate overlay digest"),
    runId: requirePositiveInteger(runId, "Candidate run ID"),
    runAttempt: requirePositiveInteger(runAttempt, "Candidate run attempt"),
  };
}

export function releaseConcurrencyGroup({ releaseTag, refName }) {
  if (typeof releaseTag !== "string" || typeof refName !== "string") {
    throw new Error("Release concurrency inputs must be strings");
  }
  const identity = releaseTag || refName;
  if (identity.length === 0 || /[\r\n]/.test(identity)) {
    throw new Error("Release concurrency identity is invalid");
  }
  return `release-${identity}`;
}

export async function createReleaseCandidateManifest({
  directory,
  channel,
  tag,
  sourceSha,
  releaseVersion,
  overlayDigest,
  runId,
  runAttempt,
}) {
  const identity = normalizeIdentity({
    channel,
    tag,
    sourceSha,
    releaseVersion,
    overlayDigest,
    runId,
    runAttempt,
  });
  const names = listCandidateFiles(directory);
  requireCurrentAssetMatrix(names, "Release candidate assets");
  const assets = [];
  for (const name of names) {
    const path = join(directory, name);
    const size = statSync(path).size;
    if (!Number.isSafeInteger(size) || size <= 0) {
      throw new Error(`Release candidate asset must be non-empty: ${name}`);
    }
    assets.push({ name, size, sha256: await sha256File(path) });
  }
  return { version: MANIFEST_VERSION, ...identity, assets };
}

function normalizeReleaseCandidateManifest(parsed) {
  const manifest = requirePlainObject(parsed, "Release candidate manifest");
  requireExactKeys(
    manifest,
    [
      "version",
      "channel",
      "tag",
      "sourceSha",
      "releaseVersion",
      "overlayDigest",
      "runId",
      "runAttempt",
      "assets",
    ],
    "Release candidate manifest"
  );
  if (manifest.version !== MANIFEST_VERSION) {
    throw new Error(`Unsupported release candidate manifest version: ${manifest.version}`);
  }

  const identity = normalizeIdentity(manifest);
  if (!Array.isArray(manifest.assets) || manifest.assets.length === 0) {
    throw new Error("Release candidate manifest must contain assets");
  }
  const assets = [];
  const names = new Set();
  for (const [index, value] of manifest.assets.entries()) {
    const asset = requirePlainObject(value, `Release candidate asset ${index}`);
    requireExactKeys(asset, ["name", "size", "sha256"], `Release candidate asset ${index}`);
    const name = requireAssetName(asset.name, `Release candidate asset ${index} name`);
    const foldedName = name.toLowerCase();
    if (names.has(foldedName)) {
      throw new Error(`Release candidate manifest contains duplicate asset: ${name}`);
    }
    names.add(foldedName);
    assets.push({
      name,
      size: requirePositiveInteger(asset.size, `Release candidate asset ${name} size`),
      sha256: requireSha256(asset.sha256, `Release candidate asset ${name}`),
    });
  }
  const sortedAssets = [...assets].sort((left, right) => compareNames(left.name, right.name));
  if (
    !sameArray(
      assets.map((asset) => asset.name),
      sortedAssets.map((asset) => asset.name)
    )
  ) {
    throw new Error("Release candidate manifest assets must be sorted by name");
  }
  return { version: MANIFEST_VERSION, ...identity, assets };
}

export function parseReleaseCandidateManifest(manifestText) {
  if (typeof manifestText !== "string") {
    throw new Error("Release candidate manifest must be text");
  }

  try {
    return normalizeReleaseCandidateManifest(JSON.parse(manifestText));
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new Error(`Release candidate manifest is invalid JSON: ${error.message}`);
    }
    throw error;
  }
}

export async function verifyReleaseCandidate({
  directory,
  channel,
  tag,
  sourceSha,
  releaseVersion,
  overlayDigest,
  runId,
  runAttempt,
}) {
  const expected = normalizeIdentity({
    channel,
    tag,
    sourceSha,
    releaseVersion,
    overlayDigest,
    runId,
    runAttempt,
  });
  const manifestPath = join(directory, RELEASE_CANDIDATE_MANIFEST);
  const manifest = parseReleaseCandidateManifest(readFileSync(manifestPath, "utf8"));

  for (const field of [
    "channel",
    "tag",
    "sourceSha",
    "releaseVersion",
    "overlayDigest",
    "runId",
    "runAttempt",
  ]) {
    if (manifest[field] !== expected[field]) {
      throw new Error(
        `Release candidate ${field} mismatch: expected ${expected[field]}, received ${manifest[field]}`
      );
    }
  }

  const actualNames = listCandidateFiles(directory);
  requireCurrentAssetMatrix(actualNames, "Release candidate assets");
  const manifestNames = manifest.assets.map((asset) => asset.name);
  if (!sameArray(actualNames, manifestNames)) {
    throw new Error("Release candidate asset set does not match its manifest");
  }
  for (const asset of manifest.assets) {
    const path = join(directory, asset.name);
    if (statSync(path).size !== asset.size) {
      throw new Error(`Release candidate asset size mismatch: ${asset.name}`);
    }
    const actual = await sha256File(path);
    if (actual !== asset.sha256) {
      throw new Error(`Release candidate asset digest mismatch: ${asset.name}`);
    }
  }
  return manifest;
}

export async function stageReleaseCandidate({ stagingDirectory, ...candidate }) {
  if (existsSync(stagingDirectory)) {
    throw new Error(`Promotion staging directory already exists: ${stagingDirectory}`);
  }
  const manifest = await verifyReleaseCandidate(candidate);
  const parent = dirname(stagingDirectory);
  mkdirSync(parent, { recursive: true });
  const temporaryDirectory = mkdtempSync(join(parent, ".release-promotion-"));
  try {
    for (const asset of manifest.assets) {
      const stagedPath = join(temporaryDirectory, asset.name);
      copyFileSync(join(candidate.directory, asset.name), stagedPath);
      if ((await sha256File(stagedPath)) !== asset.sha256) {
        throw new Error(`Staged release asset digest mismatch: ${asset.name}`);
      }
    }
    renameSync(temporaryDirectory, stagingDirectory);
  } finally {
    if (existsSync(temporaryDirectory)) {
      rmSync(temporaryDirectory, { recursive: true, force: true });
    }
  }
  return manifest;
}

function requireReleaseTarget({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
}) {
  return assertReleaseState({
    release,
    expectedReleaseId,
    expectedTag,
    expectedChannel,
    expectedSourceSha,
    state: "draft",
  }).release;
}

function normalizeCandidateAssetNames(candidateAssetNames) {
  if (!Array.isArray(candidateAssetNames) || candidateAssetNames.length === 0) {
    throw new Error("Candidate release asset names are required");
  }
  const candidateNames = new Set();
  const candidateNameList = [];
  for (const name of candidateAssetNames) {
    const normalized = requireAssetName(name, "Candidate release asset name");
    const foldedName = normalized.toLowerCase();
    if (candidateNames.has(foldedName)) {
      throw new Error(`Candidate release asset names contain a duplicate: ${normalized}`);
    }
    candidateNames.add(foldedName);
    candidateNameList.push(normalized);
  }
  candidateNameList.sort(compareNames);
  requireCurrentAssetMatrix(candidateNameList, "Candidate release assets");
  return candidateNameList;
}

export function assertReleasePromotionTarget({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
  candidateAssetNames,
}) {
  const target = requireReleaseTarget({
    release,
    expectedReleaseId,
    expectedTag,
    expectedChannel,
    expectedSourceSha,
  });
  normalizeCandidateAssetNames(candidateAssetNames);
  if (!Array.isArray(target.assets)) {
    throw new Error("GitHub Release assets must be an array");
  }
  if (target.assets.length > 0) {
    const existingNames = target.assets.map((asset) => asset?.name ?? "<unnamed>");
    throw new Error(
      `GitHub Release already contains assets; refusing overwrite: ${existingNames.join(", ")}`
    );
  }
}

function normalizeUploadedAssets(uploadedAssets) {
  if (!Array.isArray(uploadedAssets)) {
    throw new Error("Uploaded release assets must be an array");
  }
  const ids = new Set();
  const assets = [];
  const foldedNames = new Set();
  for (const [index, value] of uploadedAssets.entries()) {
    const asset = requirePlainObject(value, `Uploaded release asset ${index}`);
    const id = requirePositiveInteger(asset.id, `Uploaded release asset ${index} ID`);
    const name = requireAssetName(asset.name, `Uploaded release asset ${index} name`);
    const foldedName = name.toLowerCase();
    if (ids.has(id) || foldedNames.has(foldedName)) {
      throw new Error("Uploaded release assets contain duplicate IDs or names");
    }
    ids.add(id);
    foldedNames.add(foldedName);
    assets.push({ id, name });
  }
  assets.sort((left, right) => compareNames(left.name, right.name));
  requireCurrentAssetMatrix(
    assets.map((asset) => asset.name),
    "Uploaded release assets"
  );
  return assets;
}

function normalizePublishedAssets(releaseAssets) {
  if (!Array.isArray(releaseAssets)) {
    throw new Error("GitHub Release assets must be an array");
  }
  const assets = [];
  const ids = new Set();
  const foldedNames = new Set();
  for (const [index, value] of releaseAssets.entries()) {
    const asset = requirePlainObject(value, `GitHub Release asset ${index}`);
    const id = requirePositiveInteger(asset.id, `GitHub Release asset ${index} ID`);
    const name = requireAssetName(asset.name, `GitHub Release asset ${index} name`);
    const foldedName = name.toLowerCase();
    if (ids.has(id) || foldedNames.has(foldedName)) {
      throw new Error("GitHub Release assets contain duplicate IDs or names");
    }
    if (asset.state !== "uploaded") {
      throw new Error(`GitHub Release asset is not uploaded: ${name}`);
    }
    const digest = typeof asset.digest === "string" ? asset.digest : "";
    if (!digest.startsWith("sha256:")) {
      throw new Error(`GitHub Release asset is missing a SHA-256 digest: ${name}`);
    }
    ids.add(id);
    foldedNames.add(foldedName);
    assets.push({
      id,
      name,
      size: requirePositiveInteger(asset.size, `GitHub Release asset ${name} size`),
      sha256: requireSha256(digest.slice("sha256:".length), `GitHub Release asset ${name}`),
    });
  }
  assets.sort((left, right) => compareNames(left.name, right.name));
  requireCurrentAssetMatrix(
    assets.map((asset) => asset.name),
    "GitHub Release assets"
  );
  return assets;
}

export function assertReleasePublicationTarget({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
  candidateManifest,
  uploadedReleaseId,
  uploadedAssets,
}) {
  const target = requireReleaseTarget({
    release,
    expectedReleaseId,
    expectedTag,
    expectedChannel,
    expectedSourceSha,
  });
  const manifest = normalizeReleaseCandidateManifest(candidateManifest);
  const expectedIdentity = normalizeIdentity({
    ...manifest,
    channel: expectedChannel,
    tag: expectedTag,
    sourceSha: expectedSourceSha,
  });
  for (const field of ["channel", "tag", "sourceSha", "releaseVersion"]) {
    if (manifest[field] !== expectedIdentity[field]) {
      throw new Error(`Release candidate ${field} does not match the promotion target`);
    }
  }
  const candidateNames = normalizeCandidateAssetNames(manifest.assets.map((asset) => asset.name));
  if (uploadedReleaseId !== undefined) {
    const normalizedUploadedReleaseId = requirePositiveInteger(
      uploadedReleaseId,
      "Uploaded release ID"
    );
    if (normalizedUploadedReleaseId !== expectedReleaseId) {
      throw new Error(
        `Uploaded release ID mismatch: expected ${expectedReleaseId}, received ${normalizedUploadedReleaseId}`
      );
    }
  }

  let normalizedUploadedAssets;
  if (uploadedAssets !== undefined) {
    normalizedUploadedAssets = normalizeUploadedAssets(uploadedAssets);
    if (
      !sameArray(
        normalizedUploadedAssets.map((asset) => asset.name),
        candidateNames
      )
    ) {
      throw new Error("Uploaded release asset set does not match the candidate manifest");
    }
  }

  const publishedAssets = normalizePublishedAssets(target.assets);
  if (
    !sameArray(
      publishedAssets.map((asset) => asset.name),
      candidateNames
    )
  ) {
    throw new Error("GitHub Release asset set does not match the candidate manifest");
  }
  if (
    normalizedUploadedAssets !== undefined &&
    normalizedUploadedAssets.some((asset, index) => asset.id !== publishedAssets[index].id)
  ) {
    throw new Error("GitHub Release assets do not match the current upload IDs");
  }
  for (let index = 0; index < manifest.assets.length; index += 1) {
    if (publishedAssets[index].size !== manifest.assets[index].size) {
      throw new Error(`GitHub Release asset size mismatch: ${manifest.assets[index].name}`);
    }
    if (publishedAssets[index].sha256 !== manifest.assets[index].sha256) {
      throw new Error(`GitHub Release asset digest mismatch: ${manifest.assets[index].name}`);
    }
  }
}

export function assertPublishedReleaseTarget({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
  candidateManifest,
}) {
  const { release: target, assets } = assertPublishedReleaseAssetMatrix({
    release,
    expectedReleaseId,
    expectedTag,
    expectedChannel,
    expectedSourceSha,
  });
  const manifest = normalizeReleaseCandidateManifest(candidateManifest);
  const expectedIdentity = normalizeIdentity({
    ...manifest,
    channel: expectedChannel,
    tag: expectedTag,
    sourceSha: expectedSourceSha,
  });
  for (const field of ["channel", "tag", "sourceSha", "releaseVersion", "overlayDigest"]) {
    if (manifest[field] !== expectedIdentity[field]) {
      throw new Error(`Published release candidate ${field} does not match`);
    }
  }
  for (let index = 0; index < manifest.assets.length; index += 1) {
    const expected = manifest.assets[index];
    const actual = assets[index];
    if (
      actual.name !== expected.name ||
      actual.size !== expected.size ||
      actual.sha256 !== expected.sha256
    ) {
      throw new Error(`Published GitHub Release asset mismatch: ${expected.name}`);
    }
  }
  return { release: target, manifest, assets };
}

export function assertPublishedReleaseAssetMatrix({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
}) {
  const target = assertReleaseState({
    release,
    expectedReleaseId,
    expectedTag,
    expectedChannel,
    expectedSourceSha,
    state: "published",
  }).release;
  return { release: target, assets: normalizePublishedAssets(target.assets) };
}

function parseCliArgs(values) {
  const args = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value === undefined || args.has(key.slice(2))) {
      throw new Error("Release promotion arguments are invalid");
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

function cliPositiveInteger(args, name) {
  const value = cliValue(args, name);
  if (!/^[1-9][0-9]*$/.test(value)) {
    throw new Error(`Argument --${name} must be a positive integer`);
  }
  return requirePositiveInteger(Number(value), `Argument --${name}`);
}

async function runCli() {
  const command = process.argv[2];
  const args = parseCliArgs(process.argv.slice(3));
  const candidate = {
    directory: resolve(cliValue(args, "directory")),
    channel: cliValue(args, "channel"),
    tag: cliValue(args, "tag"),
    sourceSha: cliValue(args, "source-sha"),
    releaseVersion: cliValue(args, "release-version"),
    overlayDigest: cliValue(args, "overlay-digest"),
    runId: cliPositiveInteger(args, "run-id"),
    runAttempt: cliPositiveInteger(args, "run-attempt"),
  };

  if (command === "create-manifest") {
    const allowed = new Set([
      "directory",
      "channel",
      "tag",
      "source-sha",
      "release-version",
      "overlay-digest",
      "run-id",
      "run-attempt",
    ]);
    if ([...args.keys()].some((key) => !allowed.has(key))) {
      throw new Error("create-manifest received an unknown argument");
    }
    const manifest = await createReleaseCandidateManifest(candidate);
    writeFileSync(
      join(candidate.directory, RELEASE_CANDIDATE_MANIFEST),
      `${JSON.stringify(manifest, null, 2)}\n`,
      "utf8"
    );
    console.log(`[release-promotion] wrote ${manifest.assets.length} asset digests`);
    return;
  }

  if (command === "verify-and-stage") {
    const allowed = new Set([
      "directory",
      "staging-directory",
      "channel",
      "tag",
      "source-sha",
      "release-version",
      "overlay-digest",
      "run-id",
      "run-attempt",
    ]);
    if ([...args.keys()].some((key) => !allowed.has(key))) {
      throw new Error("verify-and-stage received an unknown argument");
    }
    const stagingDirectory = resolve(cliValue(args, "staging-directory"));
    const manifest = await stageReleaseCandidate({ ...candidate, stagingDirectory });
    console.log(`[release-promotion] verified and staged ${manifest.assets.length} assets`);
    return;
  }

  throw new Error(
    "Usage: node scripts/release-promotion.mjs <create-manifest|verify-and-stage> --directory <path> --channel <stable|beta> --tag <tag> --source-sha <sha> --release-version <version> --overlay-digest <sha256> --run-id <id> --run-attempt <attempt> [--staging-directory <path>]"
  );
}

if (process.argv[1] && resolve(process.argv[1]) === modulePath) {
  try {
    await runCli();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
