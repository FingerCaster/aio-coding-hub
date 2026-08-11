import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { parseAnyReleaseTag } from "./release-contract.mjs";

const releaseWorkflowPath = fileURLToPath(
  new URL("../.github/workflows/release.yml", import.meta.url)
);
const releaseWorkflow = readFileSync(releaseWorkflowPath, "utf8").replace(/\r\n/g, "\n");
const resolveStepIndex = releaseWorkflow.indexOf("name: Resolve release checkout ref");
const tagFetchCommand = 'git fetch --force --no-tags origin "refs/tags/$tag"';
const sourceResolveCommand = 'fetched_tag_sha="$(git rev-parse --verify "FETCH_HEAD^{commit}")"';
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
  releaseWorkflow.includes("refs/tags/$tag:refs/tags/$tag"),
  false,
  "release workflow must not replace checkout-created local tag refs"
);
assert.ok(
  releaseWorkflow.includes('echo "checkout_ref=$checkout_ref"'),
  "release workflow must pass the resolved immutable SHA to downstream jobs"
);
assert.ok(
  releaseWorkflow.includes('[[ "$checkout_ref" =~ ^[0-9a-f]{40}$ ]]'),
  "release workflow must reject a non-SHA checkout ref"
);
assert.ok(
  releaseWorkflow.includes("node scripts/release-contract.mjs describe"),
  "release workflow must use the shared stable/Beta tag and SHA parser"
);
assert.ok(
  releaseWorkflow.includes("default: stable") && releaseWorkflow.includes("- beta"),
  "release workflow must preserve stable default while exposing explicit Beta"
);
assert.ok(
  releaseWorkflow.includes("Beta releases require release_tag and target_commitish."),
  "Beta dispatch must require an explicit tag and full target SHA"
);
assert.ok(
  releaseWorkflow.includes(
    "Release target $target_sha is not reachable from origin/main; refusing to create a tag."
  ),
  "stable fallback tag creation must verify origin/main ancestry first"
);
assert.equal(
  releaseWorkflow.includes("ref: ${{ needs.release-please.outputs.tag_name }}"),
  false,
  "downstream jobs must never checkout a release tag"
);

function workflowJobBlock(jobName) {
  const lines = releaseWorkflow.split("\n");
  const start = lines.findIndex((line) => line === `  ${jobName}:`);
  assert.notEqual(start, -1, `release workflow must contain ${jobName}`);
  let end = start + 1;
  while (end < lines.length && !/^  [A-Za-z0-9_-]+:\s*$/.test(lines[end])) end += 1;
  return lines.slice(start, end).join("\n");
}

function assertImmediateFetchHeadPeel(jobName) {
  const job = workflowJobBlock(jobName);
  const fetch = 'git fetch --force --no-tags origin "refs/tags/$RELEASE_TAG"';
  const peel = 'tag_sha="$(git rev-parse --verify "FETCH_HEAD^{commit}")"';
  const fetchIndex = job.indexOf(fetch);
  const peelIndex = job.indexOf(peel, fetchIndex + 1);
  const nextFetchIndex = job.indexOf("git fetch", fetchIndex + fetch.length);
  assert.notEqual(fetchIndex, -1, `${jobName} must fetch the exact remote release tag`);
  assert.ok(
    peelIndex > fetchIndex && (nextFetchIndex === -1 || peelIndex < nextFetchIndex),
    `${jobName} must peel FETCH_HEAD before another fetch can replace it`
  );
}

assertImmediateFetchHeadPeel("promote-release");
assertImmediateFetchHeadPeel("publish");
assertImmediateFetchHeadPeel("repair-beta-release-channel");
assertImmediateFetchHeadPeel("publish-release-channel");

const releaseChannelJob = workflowJobBlock("publish-release-channel");
assert.ok(
  releaseChannelJob.includes("core.notice(") &&
    releaseChannelJob.includes("does not advance Beta pointer"),
  "stable releases that do not advance the Beta pointer must emit an Actions notice"
);

const manualJob = workflowJobBlock("release-please");
const tagMutationIndex = manualJob.indexOf('"repos/$GITHUB_REPOSITORY/git/refs"');
const releaseMutationIndex = manualJob.indexOf('"repos/$GITHUB_REPOSITORY/releases"');
assert.ok(
  tagMutationIndex !== -1 && releaseMutationIndex !== -1 && tagMutationIndex < releaseMutationIndex,
  "manual releases must resolve or create the immutable tag before creating the draft Release"
);
for (const jobName of ["release-please", "publish", "publish-release-channel"]) {
  assert.ok(
    workflowJobBlock(jobName).includes(
      'git merge-base --is-ancestor "$SOURCE_SHA" refs/remotes/origin/main'
    ) || workflowJobBlock(jobName).includes("git merge-base --is-ancestor"),
    `${jobName} must verify origin/main ancestry`
  );
}

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
  parseAnyReleaseTag(tagName);
  runGit(cwd, ["fetch", "--force", "--no-tags", "origin", `refs/tags/${tagName}`]);
  return runGit(cwd, ["rev-parse", "--verify", "FETCH_HEAD^{commit}"]).stdout.trim();
}

function isOriginMainAncestor(cwd, sourceSha) {
  runGit(cwd, [
    "fetch",
    "--force",
    "--no-tags",
    "origin",
    "+refs/heads/main:refs/remotes/origin/main",
  ]);
  return (
    runGit(cwd, ["merge-base", "--is-ancestor", sourceSha, "refs/remotes/origin/main"], {
      allowFailure: true,
    }).status === 0
  );
}

const root = mkdtempSync(join(tmpdir(), "aio-release-source-"));

try {
  const origin = join(root, "origin.git");
  const source = join(root, "source");
  const annotatedTag = "aio-coding-hub-v0.60.40";
  const lightweightTag = "aio-coding-hub-v0.60.41";
  const missingDraftTag = "aio-coding-hub-v0.60.42";
  const betaTag = "aio-coding-hub-v0.60.43-beta.1";
  const nonMainBetaTag = "aio-coding-hub-v0.60.43-beta.2";

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
  runGit(source, ["tag", betaTag, releaseSha]);
  runGit(source, ["push", "origin", `refs/tags/${annotatedTag}`]);
  runGit(source, ["push", "origin", `refs/tags/${lightweightTag}`]);
  runGit(source, ["push", "origin", `refs/tags/${betaTag}`]);
  runGit(source, ["checkout", "--detach", baseSha]);
  runGit(source, ["commit", "--allow-empty", "-m", "non-main source"]);
  const nonMainSha = runGit(source, ["rev-parse", "HEAD"]).stdout.trim();
  runGit(source, ["tag", nonMainBetaTag, nonMainSha]);
  runGit(source, ["push", "origin", `refs/tags/${nonMainBetaTag}`]);
  runGit(source, ["checkout", "main"]);

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
  assert.equal(resolveRemoteTag(manualCheckout, betaTag), releaseSha);
  assert.equal(resolveRemoteTag(manualCheckout, nonMainBetaTag), nonMainSha);
  assert.equal(isOriginMainAncestor(manualCheckout, releaseSha), true);
  assert.equal(
    isOriginMainAncestor(manualCheckout, nonMainSha),
    false,
    "a canonical Beta tag must still be rejected when its SHA is not reachable from origin/main"
  );
  assert.notEqual(
    runGit(manualCheckout, ["show-ref", "--verify", "--quiet", `refs/tags/${annotatedTag}`], {
      allowFailure: true,
    }).status,
    0,
    "remote tag resolution must not create a local tag"
  );

  assert.throws(() => resolveRemoteTag(manualCheckout, "invalid-tag"), /Release tag is invalid/);
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

console.log("[release-source:selftest] stable/Beta tags and origin/main ancestry passed");
