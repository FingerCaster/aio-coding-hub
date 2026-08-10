import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  RELEASE_CHANNELS,
  assertReleaseState,
  compareReleaseVersions,
  parseAnyReleaseTag,
  parseReleaseTag,
  requireSourceSha,
} from "./release-contract.mjs";

const scriptPath = fileURLToPath(new URL("./release-contract.mjs", import.meta.url));
const sourceSha = "a".repeat(40);
const stableTag = "aio-coding-hub-v0.60.41";
const betaTag = "aio-coding-hub-v0.60.41-beta.1";

assert.equal(parseReleaseTag(stableTag, RELEASE_CHANNELS.stable).version, "0.60.41");
assert.deepEqual(parseReleaseTag(betaTag, RELEASE_CHANNELS.beta), parseAnyReleaseTag(betaTag));
assert.throws(() => parseReleaseTag(betaTag, RELEASE_CHANNELS.stable), /invalid for stable/);
assert.throws(
  () => parseReleaseTag("aio-coding-hub-v0.60.41-beta.0", RELEASE_CHANNELS.beta),
  /invalid for beta/
);
assert.throws(
  () => parseReleaseTag("aio-coding-hub-v0.60.41-beta.01", RELEASE_CHANNELS.beta),
  /invalid for beta/
);
assert.throws(() => parseReleaseTag("aio-coding-hub-v01.60.41", RELEASE_CHANNELS.stable));
assert.throws(() => requireSourceSha("A".repeat(40)), /lowercase full commit SHA/);
assert.throws(() => requireSourceSha("a".repeat(39)), /lowercase full commit SHA/);

assert.ok(compareReleaseVersions("0.60.41-beta.1", "0.60.40") > 0);
assert.ok(compareReleaseVersions("0.60.41-beta.2", "0.60.41-beta.1") > 0);
assert.ok(compareReleaseVersions("0.60.41", "0.60.41-beta.99") > 0);
assert.ok(compareReleaseVersions("0.60.42-beta.1", "0.60.41") > 0);
assert.equal(compareReleaseVersions("0.60.41", "0.60.41"), 0);

const betaDraft = {
  id: 71,
  tag_name: betaTag,
  target_commitish: sourceSha,
  draft: true,
  prerelease: true,
  assets: [],
};
assert.doesNotThrow(() =>
  assertReleaseState({
    release: betaDraft,
    expectedReleaseId: 71,
    expectedTag: betaTag,
    expectedChannel: RELEASE_CHANNELS.beta,
    expectedSourceSha: sourceSha,
    state: "draft",
    requireEmptyAssets: true,
  })
);
assert.throws(
  () =>
    assertReleaseState({
      release: { ...betaDraft, prerelease: false },
      expectedTag: betaTag,
      expectedChannel: RELEASE_CHANNELS.beta,
      state: "draft",
    }),
  /beta draft/
);
assert.throws(
  () =>
    assertReleaseState({
      release: { ...betaDraft, target_commitish: "b".repeat(40) },
      expectedTag: betaTag,
      expectedChannel: RELEASE_CHANNELS.beta,
      expectedSourceSha: sourceSha,
      state: "draft",
    }),
  /target mismatch/
);
assert.throws(
  () =>
    assertReleaseState({
      release: { ...betaDraft, assets: [{ name: "existing" }] },
      expectedTag: betaTag,
      expectedChannel: RELEASE_CHANNELS.beta,
      state: "draft",
      requireEmptyAssets: true,
    }),
  /must be empty/
);

const published = {
  ...betaDraft,
  draft: false,
  published_at: "2026-08-10T00:00:00Z",
};
assert.doesNotThrow(() =>
  assertReleaseState({
    release: published,
    expectedTag: betaTag,
    expectedChannel: RELEASE_CHANNELS.beta,
    state: "published",
  })
);
assert.throws(
  () =>
    assertReleaseState({
      release: { ...published, published_at: null },
      expectedTag: betaTag,
      expectedChannel: RELEASE_CHANNELS.beta,
      state: "published",
    }),
  /published_at/
);

const root = mkdtempSync(join(tmpdir(), "aio-release-contract-"));
try {
  const releasePath = join(root, "release.json");
  writeFileSync(releasePath, `${JSON.stringify(betaDraft)}\n`);
  const result = spawnSync(
    process.execPath,
    [
      scriptPath,
      "assert-empty-draft",
      "--channel",
      RELEASE_CHANNELS.beta,
      "--tag",
      betaTag,
      "--source-sha",
      sourceSha,
      "--release-json",
      releasePath,
    ],
    { encoding: "utf8" }
  );
  assert.equal(result.status, 0, result.stderr);
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log("[release-contract:selftest] stable and beta contracts passed");
