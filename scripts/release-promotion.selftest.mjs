import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  RELEASE_CANDIDATE_MANIFEST,
  EXPECTED_RELEASE_ASSET_NAMES,
  assertReleasePromotionTarget,
  createReleaseCandidateManifest,
  parseReleaseCandidateManifest,
  releaseConcurrencyGroup,
  stageReleaseCandidate,
  verifyReleaseCandidate,
} from "./release-promotion.mjs";

const releaseWorkflowPath = fileURLToPath(
  new URL("../.github/workflows/release.yml", import.meta.url)
);
const releasePromotionPath = fileURLToPath(new URL("./release-promotion.mjs", import.meta.url));
const releaseWorkflow = readFileSync(releaseWorkflowPath, "utf8").replace(/\r\n/g, "\n");
const sourceSha = "d".repeat(40);
const otherSha = "e".repeat(40);
const releaseTag = "aio-coding-hub-v0.60.40";
const runId = 123456;
const runAttempt = 2;
const root = mkdtempSync(join(tmpdir(), "aio-release-promotion-"));

async function createFixture(name) {
  const directory = join(root, name);
  mkdirSync(directory);
  for (const assetName of EXPECTED_RELEASE_ASSET_NAMES) {
    writeFileSync(
      join(directory, assetName),
      assetName === "latest.json" ? '{"version":"0.60.40"}\n' : `${assetName}-candidate`
    );
  }
  const manifest = await createReleaseCandidateManifest({
    directory,
    tag: releaseTag,
    sourceSha,
    runId,
    runAttempt,
  });
  writeFileSync(
    join(directory, RELEASE_CANDIDATE_MANIFEST),
    `${JSON.stringify(manifest, null, 2)}\n`
  );
  return { directory, manifest };
}

async function expectStageRejected(name, mutate, expected) {
  const fixture = await createFixture(name);
  await mutate(fixture);
  const stagingDirectory = join(root, `${name}-staged`);
  await assert.rejects(
    () =>
      stageReleaseCandidate({
        directory: fixture.directory,
        stagingDirectory,
        tag: releaseTag,
        sourceSha,
        runId,
        runAttempt,
      }),
    expected,
    name
  );
  assert.equal(
    readdirSync(root).includes(`${name}-staged`),
    false,
    `${name} must not leave uploadable staging output`
  );
}

try {
  const fixture = await createFixture("valid");
  assert.deepEqual(parseReleaseCandidateManifest(JSON.stringify(fixture.manifest)), fixture.manifest);
  assert.deepEqual(
    await verifyReleaseCandidate({
      directory: fixture.directory,
      tag: releaseTag,
      sourceSha,
      runId,
      runAttempt,
    }),
    fixture.manifest
  );

  const stagingDirectory = join(root, "valid-staged");
  await stageReleaseCandidate({
    directory: fixture.directory,
    stagingDirectory,
    tag: releaseTag,
    sourceSha,
    runId,
    runAttempt,
  });
  assert.deepEqual(
    readdirSync(stagingDirectory).sort(),
    fixture.manifest.assets.map((asset) => asset.name),
    "only manifest-listed release assets may reach promotion staging"
  );
  assert.equal(
    readdirSync(stagingDirectory).includes(RELEASE_CANDIDATE_MANIFEST),
    false,
    "internal candidate identity must not become a public release asset"
  );

  const cliDirectory = join(root, "cli-candidate");
  const cliStagingDirectory = join(root, "cli-staged");
  mkdirSync(cliDirectory);
  for (const assetName of EXPECTED_RELEASE_ASSET_NAMES) {
    writeFileSync(join(cliDirectory, assetName), assetName === "latest.json" ? "{}\n" : assetName);
  }
  let cliResult = spawnSync(
    process.execPath,
    [
      releasePromotionPath,
      "create-manifest",
      "--directory",
      cliDirectory,
      "--tag",
      releaseTag,
      "--source-sha",
      sourceSha,
      "--run-id",
      String(runId),
      "--run-attempt",
      String(runAttempt),
    ],
    { encoding: "utf8" }
  );
  assert.equal(cliResult.status, 0, cliResult.stderr);
  cliResult = spawnSync(
    process.execPath,
    [
      releasePromotionPath,
      "verify-and-stage",
      "--directory",
      cliDirectory,
      "--staging-directory",
      cliStagingDirectory,
      "--tag",
      releaseTag,
      "--source-sha",
      sourceSha,
      "--run-id",
      String(runId),
      "--run-attempt",
      String(runAttempt),
    ],
    { encoding: "utf8" }
  );
  assert.equal(cliResult.status, 0, cliResult.stderr);
  assert.deepEqual(
    readdirSync(cliStagingDirectory).sort(),
    [...EXPECTED_RELEASE_ASSET_NAMES].sort()
  );

  const wrongSourceFixture = await createFixture("wrong-source-argument");
  await assert.rejects(
    () =>
      stageReleaseCandidate({
        directory: wrongSourceFixture.directory,
        stagingDirectory: join(root, "wrong-source-argument-staged"),
        tag: releaseTag,
        sourceSha: otherSha,
        runId,
        runAttempt,
      }),
    /sourceSha mismatch/
  );
  assert.equal(readdirSync(root).includes("wrong-source-argument-staged"), false);

  const wrongRunFixture = await createFixture("wrong-run-argument");
  await assert.rejects(
    () =>
      stageReleaseCandidate({
        directory: wrongRunFixture.directory,
        stagingDirectory: join(root, "wrong-run-argument-staged"),
        tag: releaseTag,
        sourceSha,
        runId: runId + 1,
        runAttempt,
      }),
    /runId mismatch/
  );
  assert.equal(readdirSync(root).includes("wrong-run-argument-staged"), false);

  await expectStageRejected(
    "tampered-asset",
    async ({ directory }) => {
      writeFileSync(join(directory, "aio-coding-hub-win64.msi"), "different-bytes");
    },
    /asset digest mismatch/
  );
  await expectStageRejected(
    "missing-asset",
    async ({ directory }) => {
      rmSync(join(directory, "aio-coding-hub-macos-arm.zip"));
    },
    /does not match/
  );
  await expectStageRejected(
    "extra-asset",
    async ({ directory }) => {
      writeFileSync(join(directory, "unlisted.bin"), "unlisted");
    },
    /does not match/
  );
  await expectStageRejected(
    "manifest-tag-mismatch",
    async ({ directory, manifest }) => {
      writeFileSync(
        join(directory, RELEASE_CANDIDATE_MANIFEST),
        `${JSON.stringify({ ...manifest, tag: "aio-coding-hub-v0.60.41" }, null, 2)}\n`
      );
    },
    /tag mismatch/
  );
  await expectStageRejected(
    "manifest-identity-field",
    async ({ directory, manifest }) => {
      writeFileSync(
        join(directory, RELEASE_CANDIDATE_MANIFEST),
        `${JSON.stringify({ ...manifest, mutableBranch: "main" }, null, 2)}\n`
      );
    },
    /fields are invalid/
  );
  assert.throws(
    () =>
      parseReleaseCandidateManifest(
        JSON.stringify({
          ...fixture.manifest,
          assets: [
            { name: "Asset.bin", sha256: "a".repeat(64) },
            { name: "asset.bin", sha256: "b".repeat(64) },
          ],
        })
      ),
    /duplicate asset/
  );

  const emptyDirectory = join(root, "empty");
  mkdirSync(emptyDirectory);
  await assert.rejects(
    () =>
      createReleaseCandidateManifest({
        directory: emptyDirectory,
        tag: releaseTag,
        sourceSha,
        runId,
        runAttempt,
      }),
    /has no assets/
  );

  const candidateAssetNames = fixture.manifest.assets.map((asset) => asset.name);
  const emptyDraft = {
    id: 77,
    tag_name: releaseTag,
    draft: true,
    prerelease: false,
    assets: [],
  };
  assert.doesNotThrow(() =>
    assertReleasePromotionTarget({
      release: emptyDraft,
      expectedReleaseId: 77,
      expectedTag: releaseTag,
      candidateAssetNames,
    })
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: { ...emptyDraft, assets: [{ name: candidateAssetNames[0] }] },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateAssetNames,
      }),
    /already contains assets/
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: { ...emptyDraft, draft: false },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateAssetNames,
      }),
    /must be a non-prerelease draft/
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: emptyDraft,
        expectedReleaseId: 78,
        expectedTag: releaseTag,
        candidateAssetNames,
      }),
    /Release ID mismatch/
  );

  assert.equal(
    releaseConcurrencyGroup({ releaseTag: "", refName: releaseTag }),
    releaseConcurrencyGroup({ releaseTag, refName: "main" }),
    "tag events and explicit dispatches for the same tag must share a concurrency key"
  );
  assert.notEqual(
    releaseConcurrencyGroup({ releaseTag: "aio-coding-hub-v0.60.41", refName: "main" }),
    releaseConcurrencyGroup({ releaseTag, refName: "main" }),
    "different release tags must not block each other"
  );
} finally {
  rmSync(root, { recursive: true, force: true });
}

function requireWorkflowText(expected, message) {
  assert.ok(releaseWorkflow.includes(expected), `${message}: missing ${expected}`);
}

requireWorkflowText(
  "group: release-${{ inputs.release_tag || github.ref_name }}",
  "release concurrency identity"
);
requireWorkflowText("cancel-in-progress: false", "same-tag releases must queue");
assert.equal(
  /^  push:/m.test(releaseWorkflow),
  false,
  "release publication must remain manual-only under the current local-validation contract"
);
requireWorkflowText(
  "include: ${{ fromJson(needs.release-please.outputs.build_matrix) }}",
  "current release asset matrix"
);
requireWorkflowText(
  "ref: ${{ needs.release-please.outputs.checkout_ref }}",
  "immutable release checkout"
);
requireWorkflowText(
  "release-platform-${{ needs.release-please.outputs.checkout_ref }}-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.updater_platform }}",
  "immutable platform artifact identity"
);
requireWorkflowText("assemble-release-candidate:", "candidate assembly job");
requireWorkflowText(
  "pattern: release-platform-${{ needs.release-please.outputs.checkout_ref }}-${{ github.run_id }}-${{ github.run_attempt }}-*",
  "exact platform artifact download"
);
requireWorkflowText(
  "node \"$RUNNER_TEMP/release-promotion.mjs\" create-manifest \\",
  "candidate manifest generation"
);
requireWorkflowText(
  "name: release-candidate-${{ needs.release-please.outputs.checkout_ref }}-${{ github.run_id }}-${{ github.run_attempt }}",
  "immutable candidate artifact identity"
);
requireWorkflowText("promote-release:", "release promotion job");
requireWorkflowText(
  "needs.assemble-release-candidate.result == 'success'",
  "successful candidate conclusion gate"
);
requireWorkflowText(
  "node \"$RUNNER_TEMP/release-promotion.mjs\" verify-and-stage \\",
  "candidate verification before promotion"
);
requireWorkflowText(
  'git fetch --force --no-tags origin "refs/tags/$RELEASE_TAG"',
  "promotion-time remote tag verification"
);
requireWorkflowText(
  'tag_sha="$(git rev-parse --verify "FETCH_HEAD^{commit}")"',
  "promotion-time immutable tag peel"
);
requireWorkflowText("assertReleasePromotionTarget({", "empty draft preflight");
requireWorkflowText("files: promotion-assets/*", "single staged publication set");
requireWorkflowText("overwrite_files: false", "release asset no-overwrite policy");
requireWorkflowText(
  "needs: [release-please, promote-release]",
  "publication must wait for verified promotion"
);
assert.equal(
  /^\s+releaseId:/m.test(releaseWorkflow),
  false,
  "matrix builds must not upload directly to a GitHub Release"
);
assert.equal(
  releaseWorkflow.includes("Delete existing latest.json"),
  false,
  "promotion must never delete an existing release asset"
);
assert.equal(
  releaseWorkflow.includes("overwrite_files: true"),
  false,
  "release assets must never be overwritten"
);
assert.equal(
  releaseWorkflow.match(/uses: softprops\/action-gh-release@/g)?.length,
  1,
  "release assets must be uploaded by one promotion step"
);

const guardStageIndex = releaseWorkflow.indexOf("Stage release promotion guard");
const immutablePromotionCheckoutIndex = releaseWorkflow.indexOf("Checkout immutable release source");
const preflightIndex = releaseWorkflow.indexOf("Verify empty draft release target");
const uploadIndex = releaseWorkflow.indexOf("Upload verified candidate assets");
assert.ok(
  guardStageIndex !== -1 &&
    immutablePromotionCheckoutIndex !== -1 &&
    guardStageIndex < immutablePromotionCheckoutIndex,
  "promotion guard must be staged before the release-source checkout"
);
assert.ok(
  preflightIndex !== -1 && uploadIndex !== -1 && preflightIndex < uploadIndex,
  "release target preflight must finish before the only upload step"
);

console.log("[release-promotion:selftest] all assertions passed");
