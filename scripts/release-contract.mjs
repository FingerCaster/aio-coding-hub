import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const RELEASE_CHANNELS = Object.freeze({
  stable: "stable",
  beta: "beta",
});

const STABLE_TAG_PATTERN = /^aio-coding-hub-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;
const BETA_TAG_PATTERN =
  /^aio-coding-hub-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-beta\.([1-9][0-9]*)$/;
const VERSION_PATTERN =
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-beta\.([1-9][0-9]*))?$/;
const SHA_PATTERN = /^[0-9a-f]{40}$/;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const modulePath = fileURLToPath(import.meta.url);

export function requirePlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value;
}

export function requirePositiveInteger(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label} must be a positive integer`);
  }
  return value;
}

export function requireReleaseChannel(value, label = "Release channel") {
  if (value !== RELEASE_CHANNELS.stable && value !== RELEASE_CHANNELS.beta) {
    throw new Error(`${label} must be stable or beta`);
  }
  return value;
}

export function requireSourceSha(value, label = "Release source SHA") {
  if (typeof value !== "string" || !SHA_PATTERN.test(value)) {
    throw new Error(`${label} must be a lowercase full commit SHA`);
  }
  return value;
}

export function requireSha256(value, label = "SHA-256 digest") {
  if (typeof value !== "string" || !SHA256_PATTERN.test(value)) {
    throw new Error(`${label} must be a lowercase SHA-256 digest`);
  }
  return value;
}

function parseNumericIdentifier(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new Error(`${label} is outside the supported integer range`);
  }
  return parsed;
}

export function parseReleaseVersion(value, label = "Release version") {
  if (typeof value !== "string") {
    throw new Error(`${label} is invalid`);
  }
  const match = VERSION_PATTERN.exec(value);
  if (!match) {
    throw new Error(`${label} is invalid`);
  }
  const betaNumber =
    match[4] === undefined ? null : parseNumericIdentifier(match[4], `${label} beta number`);
  return Object.freeze({
    version: value,
    major: parseNumericIdentifier(match[1], `${label} major`),
    minor: parseNumericIdentifier(match[2], `${label} minor`),
    patch: parseNumericIdentifier(match[3], `${label} patch`),
    betaNumber,
    channel: betaNumber === null ? RELEASE_CHANNELS.stable : RELEASE_CHANNELS.beta,
  });
}

export function parseReleaseTag(value, expectedChannel) {
  const channel = requireReleaseChannel(expectedChannel);
  if (typeof value !== "string") {
    throw new Error("Release tag is invalid");
  }
  const match = (channel === RELEASE_CHANNELS.beta ? BETA_TAG_PATTERN : STABLE_TAG_PATTERN).exec(
    value
  );
  if (!match) {
    throw new Error(`Release tag is invalid for ${channel}`);
  }
  const version = value.slice("aio-coding-hub-v".length);
  const parsedVersion = parseReleaseVersion(version);
  if (parsedVersion.channel !== channel) {
    throw new Error(`Release tag channel mismatch: expected ${channel}`);
  }
  return Object.freeze({ tag: value, ...parsedVersion });
}

export function parseAnyReleaseTag(value) {
  if (typeof value !== "string") {
    throw new Error("Release tag is invalid");
  }
  if (BETA_TAG_PATTERN.test(value)) {
    return parseReleaseTag(value, RELEASE_CHANNELS.beta);
  }
  return parseReleaseTag(value, RELEASE_CHANNELS.stable);
}

export function compareReleaseVersions(left, right) {
  const leftVersion = typeof left === "string" ? parseReleaseVersion(left) : left;
  const rightVersion = typeof right === "string" ? parseReleaseVersion(right) : right;
  for (const field of ["major", "minor", "patch"]) {
    if (leftVersion[field] !== rightVersion[field]) {
      return leftVersion[field] < rightVersion[field] ? -1 : 1;
    }
  }
  if (leftVersion.betaNumber === rightVersion.betaNumber) return 0;
  if (leftVersion.betaNumber === null) return 1;
  if (rightVersion.betaNumber === null) return -1;
  return leftVersion.betaNumber < rightVersion.betaNumber ? -1 : 1;
}

export function expectedPrerelease(channel) {
  return requireReleaseChannel(channel) === RELEASE_CHANNELS.beta;
}

export function assertReleaseState({
  release,
  expectedReleaseId,
  expectedTag,
  expectedChannel,
  expectedSourceSha,
  state,
  requireEmptyAssets = false,
  requireExactTarget = false,
}) {
  const target = requirePlainObject(release, "GitHub Release");
  const identity = parseReleaseTag(expectedTag, expectedChannel);
  if (expectedReleaseId !== undefined) {
    requirePositiveInteger(expectedReleaseId, "Expected release ID");
    if (target.id !== expectedReleaseId) {
      throw new Error(
        `GitHub Release ID mismatch: expected ${expectedReleaseId}, received ${target.id}`
      );
    }
  } else {
    requirePositiveInteger(target.id, "GitHub Release ID");
  }
  if (target.tag_name !== identity.tag) {
    throw new Error(
      `GitHub Release tag mismatch: expected ${identity.tag}, received ${target.tag_name}`
    );
  }
  if (state !== "draft" && state !== "published") {
    throw new Error("Expected release state must be draft or published");
  }
  const expectedDraft = state === "draft";
  if (
    target.draft !== expectedDraft ||
    target.prerelease !== expectedPrerelease(identity.channel)
  ) {
    throw new Error(
      `GitHub Release must be a ${identity.channel} ${expectedDraft ? "draft" : "publication"}`
    );
  }
  if (state === "published") {
    if (typeof target.published_at !== "string" || Number.isNaN(Date.parse(target.published_at))) {
      throw new Error("Published GitHub Release must have a valid published_at timestamp");
    }
  }
  if (requireEmptyAssets) {
    if (!Array.isArray(target.assets)) {
      throw new Error("GitHub Release assets must be an array");
    }
    if (target.assets.length !== 0) {
      throw new Error("GitHub Release must be empty before candidate promotion");
    }
  }
  if (expectedSourceSha !== undefined) {
    const sourceSha = requireSourceSha(expectedSourceSha);
    if (
      (requireExactTarget || identity.channel === RELEASE_CHANNELS.beta) &&
      target.target_commitish !== sourceSha
    ) {
      throw new Error(
        `GitHub Release target mismatch: expected ${sourceSha}, received ${target.target_commitish}`
      );
    }
  }
  return { release: target, identity };
}

function parseCliArgs(values) {
  const args = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith("--") || value === undefined || args.has(key.slice(2))) {
      throw new Error("Release contract arguments are invalid");
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
  const identity = parseReleaseTag(tag, channel);
  requireSourceSha(sourceSha);

  if (command === "describe") {
    assertOnlyArgs(args, new Set(["channel", "tag", "source-sha"]), command);
    process.stdout.write(`${JSON.stringify({ ...identity, sourceSha })}\n`);
    return;
  }
  if (command === "assert-empty-draft") {
    assertOnlyArgs(args, new Set(["channel", "tag", "source-sha", "release-json"]), command);
    const release = JSON.parse(readFileSync(resolve(cliValue(args, "release-json")), "utf8"));
    assertReleaseState({
      release,
      expectedTag: tag,
      expectedChannel: channel,
      expectedSourceSha: sourceSha,
      state: "draft",
      requireEmptyAssets: true,
      requireExactTarget: true,
    });
    return;
  }
  throw new Error(
    "Usage: node scripts/release-contract.mjs <describe|assert-empty-draft> --channel <stable|beta> --tag <tag> --source-sha <sha> [--release-json <path>]"
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
