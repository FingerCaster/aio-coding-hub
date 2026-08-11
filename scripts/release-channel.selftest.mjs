import assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  RELEASE_CHANNEL_BRANCH,
  RELEASE_CHANNEL_MANIFEST,
  RELEASE_CHANNEL_STATE,
  parseReleaseChannelState,
  publishReleaseChannel,
  readReleaseChannelSnapshot,
  waitForReleaseChannelSnapshot,
} from "./release-channel.mjs";
import { EXPECTED_RELEASE_ASSET_NAMES } from "./release-promotion.mjs";
import { OFFICIAL_RELEASE_TARGETS } from "./support-matrix.mjs";

const owner = "FingerCaster";
const repo = "aio-coding-hub";
const sourceSha = "1".repeat(40);
const workflow = {
  workflowRunId: 98765,
  workflowRunAttempt: 2,
  operator: "release-operator",
};

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function sha1(value) {
  return createHash("sha1").update(value).digest("hex");
}

function apiError(status, message) {
  const error = new Error(message);
  error.status = status;
  return error;
}

class GitDataFixture {
  constructor() {
    this.blobs = new Map();
    this.trees = new Map();
    this.commits = new Map();
    this.releases = new Map();
    this.downloads = new Map();
    this.refSha = null;
    this.counter = 0;
    this.raceOnNextUpdate = false;
    this.calls = [];
    this.github = {
      rest: {
        repos: {
          getReleaseByTag: async ({ tag }) => {
            this.calls.push(["getReleaseByTag", tag]);
            const release = this.releases.get(tag);
            if (!release) throw apiError(404, `missing release ${tag}`);
            return { data: structuredClone(release) };
          },
        },
        git: {
          getRef: async ({ ref }) => {
            this.calls.push(["getRef", ref]);
            if (this.refSha === null) throw apiError(404, "missing ref");
            return {
              data: {
                ref: `refs/${ref}`,
                object: { type: "commit", sha: this.refSha },
              },
            };
          },
          getCommit: async ({ commit_sha }) => {
            const commit = this.commits.get(commit_sha);
            if (!commit) throw apiError(404, "missing commit");
            return { data: structuredClone(commit) };
          },
          getTree: async ({ tree_sha }) => {
            const tree = this.trees.get(tree_sha);
            if (!tree) throw apiError(404, "missing tree");
            return { data: { sha: tree_sha, tree: structuredClone(tree), truncated: false } };
          },
          getBlob: async ({ file_sha }) => {
            const blob = this.blobs.get(file_sha);
            if (!blob) throw apiError(404, "missing blob");
            return {
              data: { sha: file_sha, encoding: "base64", content: blob.toString("base64") },
            };
          },
          createBlob: async ({ content, encoding }) => {
            assert.equal(encoding, "base64");
            const bytes = Buffer.from(content, "base64");
            const id = this.objectSha("blob", bytes);
            this.blobs.set(id, bytes);
            return { data: { sha: id } };
          },
          createTree: async ({ base_tree, tree }) => {
            const entries = base_tree ? structuredClone(this.trees.get(base_tree) ?? []) : [];
            const byPath = new Map(entries.map((entry) => [entry.path, entry]));
            for (const entry of tree) byPath.set(entry.path, structuredClone(entry));
            const next = [...byPath.values()].sort((left, right) =>
              left.path.localeCompare(right.path)
            );
            const id = this.objectSha("tree", JSON.stringify(next));
            this.trees.set(id, next);
            return { data: { sha: id } };
          },
          createCommit: async ({ message, tree, parents }) => {
            const id = this.objectSha("commit", JSON.stringify({ message, tree, parents }));
            this.commits.set(id, {
              sha: id,
              message,
              tree: { sha: tree },
              parents: parents.map((sha) => ({ sha })),
            });
            return { data: { sha: id } };
          },
          createRef: async ({ ref, sha }) => {
            this.calls.push(["createRef", ref, sha]);
            if (this.refSha !== null) throw apiError(422, "ref already exists");
            this.refSha = sha;
            return { data: { ref, object: { type: "commit", sha } } };
          },
          updateRef: async ({ ref, sha, force }) => {
            this.calls.push(["updateRef", ref, sha, force]);
            assert.equal(force, false, "release channel updates must use force=false");
            if (this.raceOnNextUpdate) {
              this.raceOnNextUpdate = false;
              const current = this.commits.get(this.refSha);
              const currentEntries = structuredClone(this.trees.get(current.tree.sha));
              const stateEntry = currentEntries.find(
                (entry) => entry.path === RELEASE_CHANNEL_STATE
              );
              const currentState = JSON.parse(this.blobs.get(stateEntry.sha).toString("utf8"));
              currentState.previous_ref_sha = this.refSha;
              currentState.previous_selected_tag = currentState.selected_tag;
              currentState.withdrawn_tag =
                currentState.action === "pause" ? currentState.selected_tag : null;
              currentState.updated_at = "2026-08-10T00:08:00.000Z";
              const stateBytes = Buffer.from(`${JSON.stringify(currentState, null, 2)}\n`);
              const stateSha = this.objectSha("blob", stateBytes);
              this.blobs.set(stateSha, stateBytes);
              stateEntry.sha = stateSha;
              const treeSha = this.objectSha("tree", JSON.stringify(currentEntries));
              this.trees.set(treeSha, currentEntries);
              const sibling = this.objectSha("race", this.refSha);
              this.commits.set(sibling, {
                sha: sibling,
                message: "concurrent release channel update",
                tree: { sha: treeSha },
                parents: [{ sha: this.refSha }],
              });
              this.refSha = sibling;
            }
            if (!this.isAncestor(this.refSha, sha)) {
              throw apiError(422, "non-fast-forward ref update");
            }
            this.refSha = sha;
            return { data: { ref, object: { type: "commit", sha } } };
          },
        },
      },
    };
    this.fetch = async (url) => {
      this.calls.push(["fetch", url]);
      const bytes = this.downloads.get(url);
      if (!bytes) {
        return { ok: false, status: 404, arrayBuffer: async () => new ArrayBuffer(0) };
      }
      return {
        ok: true,
        status: 200,
        arrayBuffer: async () =>
          bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
      };
    };
  }

  objectSha(kind, value) {
    this.counter += 1;
    return sha1(Buffer.concat([Buffer.from(`${kind}:${this.counter}:`), Buffer.from(value)]));
  }

  isAncestor(ancestor, descendant) {
    const pending = [descendant];
    const visited = new Set();
    while (pending.length > 0) {
      const candidate = pending.pop();
      if (candidate === ancestor) return true;
      if (visited.has(candidate)) continue;
      visited.add(candidate);
      const commit = this.commits.get(candidate);
      for (const parent of commit?.parents ?? []) pending.push(parent.sha);
    }
    return false;
  }

  addRelease({ tag, id, prerelease, manifestOverride, releaseOverride, assetOverride }) {
    const version = tag.slice("aio-coding-hub-v".length);
    const platforms = {};
    const signaturesByAssetName = new Map();
    for (const target of OFFICIAL_RELEASE_TARGETS) {
      const signature = `signed-${target.updaterPlatform}`;
      platforms[target.updaterPlatform] = {
        signature,
        url: `https://github.com/${owner}/${repo}/releases/download/${tag}/${target.latestAssetName}`,
      };
      signaturesByAssetName.set(target.latestSignatureName, signature);
    }
    const manifestText = `${JSON.stringify(
      manifestOverride ?? {
        version,
        notes: `Release ${version}`,
        pub_date: "2026-08-10T00:00:00Z",
        platforms,
      },
      null,
      2
    )}\n`;
    const assets = EXPECTED_RELEASE_ASSET_NAMES.map((name, index) => {
      const bytes = Buffer.from(
        name === "latest.json"
          ? manifestText
          : (signaturesByAssetName.get(name) ?? `${tag}:${name}:bytes`)
      );
      const url = `https://github.com/${owner}/${repo}/releases/download/${tag}/${name}`;
      this.downloads.set(url, bytes);
      return {
        id: id * 100 + index,
        name,
        state: "uploaded",
        size: bytes.length,
        digest: `sha256:${sha256(bytes)}`,
        browser_download_url: url,
      };
    });
    const release = {
      id,
      tag_name: tag,
      target_commitish: sourceSha,
      draft: false,
      prerelease,
      published_at: "2026-08-10T00:00:00Z",
      assets,
      ...releaseOverride,
    };
    if (assetOverride) release.assets = assetOverride(release.assets);
    this.releases.set(tag, release);
    return { release, manifestText };
  }
}

const fixture = new GitDataFixture();
const betaOne = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.41-beta.1",
  id: 101,
  prerelease: true,
});
fixture.addRelease({
  tag: "aio-coding-hub-v0.60.41-beta.2",
  id: 102,
  prerelease: true,
});
fixture.addRelease({
  tag: "aio-coding-hub-v0.60.41-beta.3",
  id: 103,
  prerelease: true,
});
fixture.addRelease({
  tag: "aio-coding-hub-v0.60.40",
  id: 100,
  prerelease: false,
});

const first = await publishReleaseChannel({
  github: fixture.github,
  fetchImpl: fixture.fetch,
  owner,
  repo,
  action: "promote",
  tag: "aio-coding-hub-v0.60.41-beta.1",
  sourceSha,
  expectedRefSha: null,
  ...workflow,
  updatedAt: new Date("2026-08-10T00:00:00.789Z").toISOString(),
});
assert.equal(first.previousRefSha, null);
assert.equal(first.state.selected_channel, "beta");
assert.equal(first.state.previous_ref_sha, null);
assert.equal(first.state.withdrawn_tag, null);
assert.equal(first.state.promotion_high_water_version, "0.60.41-beta.1");
assert.equal(first.state.manifest_sha256, sha256(betaOne.manifestText));
assert.equal(
  Object.hasOwn(first.state, "new_ref_sha"),
  false,
  "state cannot contain the SHA of the commit that contains the state itself"
);
assert.ok(
  fixture.calls.some(
    (call) => call[0] === "createRef" && call[1] === `refs/heads/${RELEASE_CHANNEL_BRANCH}`
  ),
  "first publication must create release-channels"
);

let snapshot = await readReleaseChannelSnapshot({ github: fixture.github, owner, repo });
assert.equal(snapshot.refSha, first.newRefSha);
assert.equal(snapshot.manifestText, betaOne.manifestText, "pointer must preserve Release bytes");
const idempotentFirst = await publishReleaseChannel({
  github: fixture.github,
  fetchImpl: fixture.fetch,
  owner,
  repo,
  action: "promote",
  tag: "aio-coding-hub-v0.60.41-beta.1",
  sourceSha,
  expectedRefSha: first.newRefSha,
  ...workflow,
  updatedAt: "2026-08-10T00:00:01.000Z",
});
assert.equal(idempotentFirst.idempotent, true, "repeating an exact promotion must be a no-op");
assert.equal(idempotentFirst.newRefSha, first.newRefSha);
let staleReads = 2;
const eventuallyConsistentGithub = {
  ...fixture.github,
  rest: {
    ...fixture.github.rest,
    git: {
      ...fixture.github.rest.git,
      getRef: async (args) => {
        if (staleReads > 0) {
          staleReads -= 1;
          throw apiError(404, "stale ref read");
        }
        return fixture.github.rest.git.getRef(args);
      },
    },
  },
};
const confirmedAfterRetry = await waitForReleaseChannelSnapshot({
  github: eventuallyConsistentGithub,
  owner,
  repo,
  expectedRefSha: first.newRefSha,
  attempts: 3,
  delayMs: 0,
});
assert.equal(confirmedAfterRetry.refSha, first.newRefSha);
const legacyState = JSON.parse(snapshot.stateText);
legacyState.schema_version = 1;
delete legacyState.promotion_high_water_version;
assert.equal(
  parseReleaseChannelState(`${JSON.stringify(legacyState)}\n`, {
    owner,
    repo,
    manifestText: snapshot.manifestText,
  }).promotion_high_water_version,
  "0.60.41-beta.1",
  "schema v1 state must derive a safe promotion high-water version"
);
const regressedHighWaterState = JSON.parse(snapshot.stateText);
regressedHighWaterState.promotion_high_water_version = "0.60.40";
assert.throws(
  () =>
    parseReleaseChannelState(`${JSON.stringify(regressedHighWaterState)}\n`, {
      owner,
      repo,
      manifestText: snapshot.manifestText,
    }),
  /high-water version regressed/
);
assert.equal(
  Object.hasOwn(JSON.parse(snapshot.stateText), "new_ref_sha"),
  false,
  "newRefSha must remain a publication result/log value, not committed state"
);
assert.deepEqual(
  fixture.trees
    .get(snapshot.treeSha)
    .map((entry) => entry.path)
    .sort(),
  [RELEASE_CHANNEL_MANIFEST, RELEASE_CHANNEL_STATE].sort()
);

const firstCommit = fixture.commits.get(snapshot.refSha);
const firstParents = structuredClone(firstCommit.parents);
firstCommit.parents = [{ sha: "f".repeat(40) }];
await assert.rejects(
  () => readReleaseChannelSnapshot({ github: fixture.github, owner, repo }),
  /previous ref does not match the commit parent/
);
firstCommit.parents = firstParents;
const malformedTimestampState = JSON.parse(snapshot.stateText);
malformedTimestampState.updated_at = "2026-02-31T00:00:00Z";
assert.throws(
  () =>
    parseReleaseChannelState(`${JSON.stringify(malformedTimestampState)}\n`, {
      owner,
      repo,
      manifestText: snapshot.manifestText,
    }),
  /canonical UTC ISO format/
);

const second = await publishReleaseChannel({
  github: fixture.github,
  fetchImpl: fixture.fetch,
  owner,
  repo,
  action: "promote",
  tag: "aio-coding-hub-v0.60.41-beta.2",
  sourceSha,
  expectedRefSha: first.newRefSha,
  ...workflow,
  updatedAt: "2026-08-10T00:01:00.000Z",
});
assert.equal(second.state.previous_ref_sha, first.newRefSha);
assert.equal(second.state.previous_selected_tag, "aio-coding-hub-v0.60.41-beta.1");
assert.equal(Object.hasOwn(second.state, "new_ref_sha"), false);
assert.notEqual(
  second.newRefSha,
  second.state.previous_ref_sha,
  "newRefSha and the committed previous_ref_sha have distinct CAS meanings"
);
assert.ok(
  fixture.calls.some((call) => call[0] === "updateRef" && call[3] === false),
  "pointer updates must use force=false"
);

await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: "aio-coding-hub-v0.60.41-beta.1",
      sourceSha,
      expectedRefSha: second.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:02:00.000Z",
    }),
  /strictly higher version/
);

const paused = await publishReleaseChannel({
  github: fixture.github,
  fetchImpl: fixture.fetch,
  owner,
  repo,
  action: "pause",
  tag: "aio-coding-hub-v0.60.40",
  sourceSha,
  expectedRefSha: second.newRefSha,
  withdrawnTag: "aio-coding-hub-v0.60.41-beta.2",
  ...workflow,
  updatedAt: "2026-08-10T00:03:00.000Z",
});
assert.equal(paused.state.action, "pause");
assert.equal(paused.state.withdrawn_tag, "aio-coding-hub-v0.60.41-beta.2");
assert.equal(paused.state.selected_channel, "stable");

const withdrawalFixture = new GitDataFixture();
for (const [tag, id, prerelease] of [
  ["aio-coding-hub-v0.60.39", 109, false],
  ["aio-coding-hub-v0.60.40", 110, false],
  ["aio-coding-hub-v0.60.41-beta.1", 111, true],
  ["aio-coding-hub-v0.60.41-beta.2", 112, true],
  ["aio-coding-hub-v0.60.41-beta.3", 113, true],
  ["aio-coding-hub-v0.60.42", 114, false],
]) {
  withdrawalFixture.addRelease({ tag, id, prerelease });
}
const publishWithdrawalTarget = ({
  action = "promote",
  tag,
  expectedRefSha,
  withdrawnTag,
  updatedAt,
}) =>
  publishReleaseChannel({
    github: withdrawalFixture.github,
    fetchImpl: withdrawalFixture.fetch,
    owner,
    repo,
    action,
    tag,
    sourceSha,
    expectedRefSha,
    withdrawnTag,
    ...workflow,
    updatedAt,
  });
const withdrawalBetaOne = await publishWithdrawalTarget({
  tag: "aio-coding-hub-v0.60.41-beta.1",
  expectedRefSha: null,
  updatedAt: "2026-08-10T00:03:10.000Z",
});
const withdrawalBetaTwo = await publishWithdrawalTarget({
  tag: "aio-coding-hub-v0.60.41-beta.2",
  expectedRefSha: withdrawalBetaOne.newRefSha,
  updatedAt: "2026-08-10T00:03:20.000Z",
});
const withdrawalPause = await publishWithdrawalTarget({
  action: "pause",
  tag: "aio-coding-hub-v0.60.40",
  expectedRefSha: withdrawalBetaTwo.newRefSha,
  withdrawnTag: "aio-coding-hub-v0.60.41-beta.2",
  updatedAt: "2026-08-10T00:03:30.000Z",
});
assert.equal(withdrawalPause.state.promotion_high_water_version, "0.60.41-beta.2");
for (const [tag, updatedAt] of [
  ["aio-coding-hub-v0.60.41-beta.2", "2026-08-10T00:03:40.000Z"],
  ["aio-coding-hub-v0.60.41-beta.1", "2026-08-10T00:03:50.000Z"],
]) {
  await assert.rejects(
    () =>
      publishWithdrawalTarget({
        tag,
        expectedRefSha: withdrawalPause.newRefSha,
        updatedAt,
      }),
    /strictly higher version/
  );
}
const withdrawalPauseAgain = await publishWithdrawalTarget({
  action: "pause",
  tag: "aio-coding-hub-v0.60.39",
  expectedRefSha: withdrawalPause.newRefSha,
  withdrawnTag: "aio-coding-hub-v0.60.40",
  updatedAt: "2026-08-10T00:03:55.000Z",
});
assert.equal(withdrawalPauseAgain.state.promotion_high_water_version, "0.60.41-beta.2");
for (const [tag, updatedAt] of [
  ["aio-coding-hub-v0.60.41-beta.2", "2026-08-10T00:03:56.000Z"],
  ["aio-coding-hub-v0.60.41-beta.1", "2026-08-10T00:03:57.000Z"],
]) {
  await assert.rejects(
    () =>
      publishWithdrawalTarget({
        tag,
        expectedRefSha: withdrawalPauseAgain.newRefSha,
        updatedAt,
      }),
    /strictly higher version/
  );
}
const withdrawalBetaThree = await publishWithdrawalTarget({
  tag: "aio-coding-hub-v0.60.41-beta.3",
  expectedRefSha: withdrawalPauseAgain.newRefSha,
  updatedAt: "2026-08-10T00:04:00.000Z",
});
assert.equal(withdrawalBetaThree.state.selected_tag, "aio-coding-hub-v0.60.41-beta.3");
assert.equal(withdrawalBetaThree.state.previous_ref_sha, withdrawalPauseAgain.newRefSha);
const withdrawalStable = await publishWithdrawalTarget({
  tag: "aio-coding-hub-v0.60.42",
  expectedRefSha: withdrawalBetaThree.newRefSha,
  updatedAt: "2026-08-10T00:04:05.000Z",
});
assert.equal(withdrawalStable.state.selected_channel, "stable");
assert.equal(withdrawalStable.state.promotion_high_water_version, "0.60.42");

await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "pause",
      tag: "aio-coding-hub-v0.60.41-beta.1",
      sourceSha,
      expectedRefSha: paused.newRefSha,
      withdrawnTag: "aio-coding-hub-v0.60.41-beta.2",
      ...workflow,
      updatedAt: "2026-08-10T00:04:00.000Z",
    }),
  /must match the current channel target/
);

const unsafeDraft = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.1",
  id: 201,
  prerelease: true,
  releaseOverride: { draft: true, published_at: null },
});
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: unsafeDraft.release.tag_name,
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:05:00.000Z",
    }),
  /beta publication/
);

fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.2",
  id: 202,
  prerelease: true,
  assetOverride: (assets) => assets.slice(1),
});
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: "aio-coding-hub-v0.60.42-beta.2",
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:06:00.000Z",
    }),
  /asset matrix/
);

fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.3",
  id: 203,
  prerelease: true,
  manifestOverride: {
    version: "0.60.42-beta.3",
    notes: "unsafe",
    pub_date: "2026-08-10T00:00:00Z",
    platforms: {},
  },
});
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: "aio-coding-hub-v0.60.42-beta.3",
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:07:00.000Z",
    }),
  /platform set/
);

const badManifestDigest = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.4",
  id: 204,
  prerelease: true,
});
const latestAsset = badManifestDigest.release.assets.find((asset) => asset.name === "latest.json");
const tamperedManifestBytes = Buffer.from(badManifestDigest.manifestText);
tamperedManifestBytes[0] ^= 1;
fixture.downloads.set(latestAsset.browser_download_url, tamperedManifestBytes);
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: badManifestDigest.release.tag_name,
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:07:30.000Z",
    }),
  /manifest digest does not match/
);

const mismatchedSignature = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.5",
  id: 205,
  prerelease: true,
});
const mismatchedSignatureAsset = mismatchedSignature.release.assets.find(
  (asset) => asset.name === "aio-coding-hub-win64.msi.sig"
);
const mismatchedSignatureBytes = Buffer.from("signed-but-not-the-manifest-value");
fixture.downloads.set(mismatchedSignatureAsset.browser_download_url, mismatchedSignatureBytes);
mismatchedSignatureAsset.size = mismatchedSignatureBytes.length;
mismatchedSignatureAsset.digest = `sha256:${sha256(mismatchedSignatureBytes)}`;
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: mismatchedSignature.release.tag_name,
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:07:45.000Z",
    }),
  /signature does not match latest\.json/
);

const invalidUtf8Manifest = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.6",
  id: 206,
  prerelease: true,
});
const invalidUtf8ManifestAsset = invalidUtf8Manifest.release.assets.find(
  (asset) => asset.name === "latest.json"
);
const invalidUtf8ManifestBytes = Buffer.from([0x7b, 0x22, 0xff, 0x22, 0x7d]);
fixture.downloads.set(invalidUtf8ManifestAsset.browser_download_url, invalidUtf8ManifestBytes);
invalidUtf8ManifestAsset.size = invalidUtf8ManifestBytes.length;
invalidUtf8ManifestAsset.digest = `sha256:${sha256(invalidUtf8ManifestBytes)}`;
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: invalidUtf8Manifest.release.tag_name,
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:07:50.000Z",
    }),
  /Published updater manifest is not valid UTF-8/
);

const invalidUtf8Signature = fixture.addRelease({
  tag: "aio-coding-hub-v0.60.42-beta.7",
  id: 207,
  prerelease: true,
});
const invalidUtf8SignatureAsset = invalidUtf8Signature.release.assets.find(
  (asset) => asset.name === "aio-coding-hub-win64.msi.sig"
);
const invalidUtf8SignatureBytes = Buffer.from([0x73, 0x69, 0x67, 0xff]);
fixture.downloads.set(invalidUtf8SignatureAsset.browser_download_url, invalidUtf8SignatureBytes);
invalidUtf8SignatureAsset.size = invalidUtf8SignatureBytes.length;
invalidUtf8SignatureAsset.digest = `sha256:${sha256(invalidUtf8SignatureBytes)}`;
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: invalidUtf8Signature.release.tag_name,
      sourceSha,
      expectedRefSha: paused.newRefSha,
      ...workflow,
      updatedAt: "2026-08-10T00:07:55.000Z",
    }),
  /Published updater signature windows-x86_64 is not valid UTF-8/
);

snapshot = await readReleaseChannelSnapshot({ github: fixture.github, owner, repo });
fixture.raceOnNextUpdate = true;
await assert.rejects(
  () =>
    publishReleaseChannel({
      github: fixture.github,
      fetchImpl: fixture.fetch,
      owner,
      repo,
      action: "promote",
      tag: "aio-coding-hub-v0.60.41-beta.3",
      sourceSha,
      expectedRefSha: snapshot.refSha,
      ...workflow,
      updatedAt: "2026-08-10T00:08:00.000Z",
    }),
  /non-fast-forward/
);
const afterRace = await readReleaseChannelSnapshot({ github: fixture.github, owner, repo });
assert.equal(
  afterRace.state.selected_tag,
  paused.state.selected_tag,
  "CAS race must not publish the losing pointer bytes"
);

const stableCatchupFixture = new GitDataFixture();
const betaBeforeStable = stableCatchupFixture.addRelease({
  tag: "aio-coding-hub-v0.60.50-beta.1",
  id: 301,
  prerelease: true,
});
const stableCatchup = stableCatchupFixture.addRelease({
  tag: "aio-coding-hub-v0.60.50",
  id: 302,
  prerelease: false,
});
const betaPointer = await publishReleaseChannel({
  github: stableCatchupFixture.github,
  fetchImpl: stableCatchupFixture.fetch,
  owner,
  repo,
  action: "promote",
  tag: betaBeforeStable.release.tag_name,
  sourceSha,
  expectedRefSha: null,
  ...workflow,
  updatedAt: "2026-08-10T00:09:00.000Z",
});
const stablePointer = await publishReleaseChannel({
  github: stableCatchupFixture.github,
  fetchImpl: stableCatchupFixture.fetch,
  owner,
  repo,
  action: "promote",
  tag: stableCatchup.release.tag_name,
  sourceSha,
  expectedRefSha: betaPointer.newRefSha,
  ...workflow,
  updatedAt: "2026-08-10T00:10:00.000Z",
});
assert.equal(stablePointer.state.selected_channel, "stable");
assert.equal(stablePointer.state.selected_tag, stableCatchup.release.tag_name);
assert.equal(stablePointer.state.previous_ref_sha, betaPointer.newRefSha);
assert.equal(stablePointer.state.previous_selected_tag, betaBeforeStable.release.tag_name);
assert.equal(
  (
    await readReleaseChannelSnapshot({
      github: stableCatchupFixture.github,
      owner,
      repo,
    })
  ).state.selected_channel,
  "stable",
  "stable publication must advance the same Beta pointer line"
);

console.log("[release-channel:selftest] promote, pause, safety, and CAS fixtures passed");
