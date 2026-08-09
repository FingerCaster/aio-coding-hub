import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const releaseWorkflowPath = fileURLToPath(
  new URL("../.github/workflows/release.yml", import.meta.url)
);
const releaseWorkflow = readFileSync(releaseWorkflowPath, "utf8").replace(/\r\n/g, "\n");
const resolveStepIndex = releaseWorkflow.indexOf("name: Resolve release checkout ref");
const tagFetchCommand = 'git fetch --force --no-tags origin "refs/tags/$tag"';
const sourceResolveCommand =
  'fetched_tag_sha="$(git rev-parse --verify "FETCH_HEAD^{commit}")"';
const tagFetchIndex = releaseWorkflow.indexOf(tagFetchCommand, resolveStepIndex);
const sourceResolveIndex = releaseWorkflow.indexOf(sourceResolveCommand, resolveStepIndex);
const laterFetchIndex = releaseWorkflow.indexOf("git fetch --no-tags --depth=1", tagFetchIndex + 1);

assert.notEqual(resolveStepIndex, -1, "release workflow must resolve or create its release tag");
assert.notEqual(tagFetchIndex, -1, "release workflow must fetch the remote tag into FETCH_HEAD");
assert.notEqual(sourceResolveIndex, -1, "release workflow must peel FETCH_HEAD to a commit");
assert.ok(
  tagFetchIndex < sourceResolveIndex &&
    (laterFetchIndex === -1 || sourceResolveIndex < laterFetchIndex),
  "release workflow must peel FETCH_HEAD before another fetch can replace it"
);
assert.equal(
  releaseWorkflow.includes('refs/tags/$tag:refs/tags/$tag'),
  false,
  "release workflow must not replace checkout-created local tag refs"
);
assert.ok(
  releaseWorkflow.includes('echo "checkout_ref=$checkout_ref" >> "$GITHUB_OUTPUT"'),
  "release workflow must pass the resolved immutable SHA to downstream jobs"
);
assert.ok(
  releaseWorkflow.includes("[[ \"$checkout_ref\" =~ ^[0-9a-f]{40}$ ]]"),
  "release workflow must reject a non-SHA checkout ref"
);
assert.ok(
  releaseWorkflow.includes(
    '[[ ! "$tag" =~ ^aio-coding-hub-v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$ ]]'
  ),
  "release workflow must reject non-canonical release tags before mutation"
);

function runGit(cwd, args, { allowFailure = false } = {}) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (!allowFailure && result.status !== 0) {
    throw new Error(
      `git ${args.join(" ")} failed (${result.status}): ${result.stderr || result.stdout}`
    );
  }
  return result;
}

function createConsumer(root, name, origin) {
  const consumer = join(root, name);
  runGit(root, ["init", "--initial-branch=main", consumer]);
  runGit(consumer, ["remote", "add", "origin", origin]);
  runGit(consumer, ["fetch", "--no-tags", "origin", "refs/heads/main"]);
  runGit(consumer, ["checkout", "--detach", "FETCH_HEAD"]);
  return consumer;
}

function resolveRemoteTag(cwd, tagName) {
  if (!/^aio-coding-hub-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(tagName)) {
    throw new Error(`Invalid release tag: ${tagName}`);
  }
  runGit(cwd, ["fetch", "--force", "--no-tags", "origin", `refs/tags/${tagName}`]);
  return runGit(cwd, ["rev-parse", "--verify", "FETCH_HEAD^{commit}"]).stdout.trim();
}

const root = mkdtempSync(join(tmpdir(), "aio-release-source-"));

try {
  const origin = join(root, "origin.git");
  const source = join(root, "source");
  const annotatedTag = "aio-coding-hub-v0.60.40";
  const lightweightTag = "aio-coding-hub-v0.60.41";
  const missingDraftTag = "aio-coding-hub-v0.60.42";

  runGit(root, ["init", "--bare", "--initial-branch=main", origin]);
  runGit(root, ["init", "--initial-branch=main", source]);
  runGit(source, ["config", "user.name", "Release Source Selftest"]);
  runGit(source, ["config", "user.email", "release-source@example.invalid"]);
  runGit(source, ["commit", "--allow-empty", "-m", "base source"]);
  const baseSha = runGit(source, ["rev-parse", "HEAD"]).stdout.trim();
  runGit(source, ["commit", "--allow-empty", "-m", "release source"]);
  const releaseSha = runGit(source, ["rev-parse", "HEAD"]).stdout.trim();
  runGit(source, ["remote", "add", "origin", origin]);
  runGit(source, ["push", "--set-upstream", "origin", "main"]);
  runGit(source, ["tag", "-a", annotatedTag, "-m", annotatedTag, releaseSha]);
  runGit(source, ["tag", lightweightTag, releaseSha]);
  runGit(source, ["push", "origin", `refs/tags/${annotatedTag}`]);
  runGit(source, ["push", "origin", `refs/tags/${lightweightTag}`]);

  const tagCheckout = createConsumer(root, "tag-checkout", origin);
  runGit(tagCheckout, ["tag", annotatedTag, baseSha]);
  assert.equal(resolveRemoteTag(tagCheckout, annotatedTag), releaseSha);
  assert.equal(
    runGit(tagCheckout, ["rev-parse", `refs/tags/${annotatedTag}^{commit}`]).stdout.trim(),
    baseSha,
    "FETCH_HEAD resolution must not replace a same-name local lightweight tag"
  );

  const manualCheckout = createConsumer(root, "manual-checkout", origin);
  assert.equal(resolveRemoteTag(manualCheckout, annotatedTag), releaseSha);
  assert.equal(resolveRemoteTag(manualCheckout, lightweightTag), releaseSha);
  assert.notEqual(
    runGit(manualCheckout, ["show-ref", "--verify", "--quiet", `refs/tags/${annotatedTag}`], {
      allowFailure: true,
    }).status,
    0,
    "remote tag resolution must not create a local tag"
  );

  assert.throws(() => resolveRemoteTag(manualCheckout, "invalid-tag"), /Invalid release tag/);
  assert.throws(() => resolveRemoteTag(manualCheckout, missingDraftTag), /git fetch/);
  runGit(origin, ["update-ref", `refs/tags/${missingDraftTag}`, releaseSha]);
  assert.equal(
    resolveRemoteTag(manualCheckout, missingDraftTag),
    releaseSha,
    "a tag created for an existing draft release must resolve to its immutable target"
  );
  assert.notEqual(
    runGit(manualCheckout, ["show-ref", "--verify", "--quiet", `refs/tags/${missingDraftTag}`], {
      allowFailure: true,
    }).status,
    0,
    "draft tag recovery must leave checkout refs untouched"
  );
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log("[release-source:selftest] annotated, lightweight, and missing draft tags passed");
