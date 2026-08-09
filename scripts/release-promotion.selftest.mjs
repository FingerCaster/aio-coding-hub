import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  RELEASE_CANDIDATE_MANIFEST,
  EXPECTED_RELEASE_ASSET_NAMES,
  assertReleasePublicationTarget,
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

assert.equal(EXPECTED_RELEASE_ASSET_NAMES.length, 14, "the current release must keep 14 assets");
assert.deepEqual(
  [...EXPECTED_RELEASE_ASSET_NAMES].sort(),
  [
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
  ].sort(),
  "candidate verification must pin the current release and Homebrew asset matrix"
);

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
  assert.deepEqual(
    parseReleaseCandidateManifest(JSON.stringify(fixture.manifest)),
    fixture.manifest
  );
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

  const wrongAttemptFixture = await createFixture("wrong-attempt-argument");
  await assert.rejects(
    () =>
      stageReleaseCandidate({
        directory: wrongAttemptFixture.directory,
        stagingDirectory: join(root, "wrong-attempt-argument-staged"),
        tag: releaseTag,
        sourceSha,
        runId,
        runAttempt: runAttempt + 1,
      }),
    /runAttempt mismatch/
  );
  assert.equal(readdirSync(root).includes("wrong-attempt-argument-staged"), false);

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

  const publishedAssets = fixture.manifest.assets.map((asset, index) => ({
    id: 1000 + index,
    name: asset.name,
    state: "uploaded",
    digest: `sha256:${asset.sha256}`,
  }));
  const uploadedAssets = publishedAssets.map(({ id, name }) => ({ id, name }));
  const completeDraft = { ...emptyDraft, assets: publishedAssets };
  assert.doesNotThrow(() =>
    assertReleasePublicationTarget({
      release: completeDraft,
      expectedReleaseId: 77,
      expectedTag: releaseTag,
      candidateManifest: fixture.manifest,
      uploadedReleaseId: 77,
      uploadedAssets,
    })
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: completeDraft,
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
        uploadedReleaseId: 78,
        uploadedAssets,
      }),
    /Uploaded release ID mismatch/
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: completeDraft,
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
        uploadedReleaseId: 77,
        uploadedAssets: uploadedAssets.map((asset, index) =>
          index === 0 ? { ...asset, id: asset.id + 100 } : asset
        ),
      }),
    /do not match the current upload IDs/
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: completeDraft,
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
        uploadedAssets: uploadedAssets.slice(1),
      }),
    /Uploaded release assets does not match the current release asset matrix/
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: {
          ...completeDraft,
          assets: publishedAssets.map((asset, index) =>
            index === 0 ? { ...asset, digest: `sha256:${"0".repeat(64)}` } : asset
          ),
        },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
      }),
    /asset digest mismatch/
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: { ...completeDraft, assets: publishedAssets.slice(1) },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
      }),
    /GitHub Release assets does not match the current release asset matrix/
  );
  assert.throws(
    () =>
      assertReleasePublicationTarget({
        release: { ...completeDraft, draft: false },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        candidateManifest: fixture.manifest,
      }),
    /must be a non-prerelease draft/
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
  'node "$RUNNER_TEMP/release-promotion.mjs" create-manifest \\',
  "candidate manifest generation"
);
requireWorkflowText(
  "name: release-candidate-${{ needs.release-please.outputs.checkout_ref }}-${{ github.run_id }}-${{ github.run_attempt }}",
  "immutable candidate artifact identity"
);
requireWorkflowText(
  "candidate_artifact_name: ${{ format('release-candidate-{0}-{1}-{2}', needs.release-please.outputs.checkout_ref, github.run_id, github.run_attempt) }}",
  "candidate-producing attempt output"
);
requireWorkflowText("promote-release:", "release promotion job");
requireWorkflowText(
  "needs.assemble-release-candidate.result == 'success'",
  "successful candidate conclusion gate"
);
requireWorkflowText(
  'node "$RUNNER_TEMP/release-promotion.mjs" verify-and-stage \\',
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
requireWorkflowText("draft: true", "candidate uploads must leave the release as a draft");
requireWorkflowText(
  "UPLOADED_RELEASE_ID: ${{ steps.upload_assets.outputs.id }}",
  "exact upload target verification"
);
requireWorkflowText(
  "UPLOADED_ASSETS: ${{ steps.upload_assets.outputs.assets }}",
  "exact current-attempt upload result verification"
);
requireWorkflowText("assertReleasePublicationTarget({", "publication asset digest verification");
requireWorkflowText(
  "name: Download exact publication candidate",
  "publication must reload the exact run-attempt candidate"
);
assert.equal(
  releaseWorkflow.match(
    /name: \$\{\{ needs\.assemble-release-candidate\.outputs\.candidate_artifact_name \}\}/g
  )?.length,
  2,
  "promotion and publication must download the candidate-producing attempt artifact"
);
assert.equal(
  releaseWorkflow.match(
    /RUN_ATTEMPT: \$\{\{ needs\.assemble-release-candidate\.outputs\.candidate_run_attempt \}\}/g
  )?.length,
  2,
  "promotion and publication must verify the candidate-producing attempt"
);
requireWorkflowText(
  "name: Reverify release tag before publication",
  "publication-time exact tag verification"
);
requireWorkflowText(
  "needs: [release-please, assemble-release-candidate, promote-release]",
  "publication must wait for verified promotion"
);
assert.equal(
  releaseWorkflow.includes("Number('${{ needs.release-please.outputs.release_id }}')"),
  false,
  "release IDs must enter scripts through the environment, not inline expressions"
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
const immutablePromotionCheckoutIndex = releaseWorkflow.indexOf(
  "Checkout immutable release source"
);
const candidateGuardCheckoutIndex = releaseWorkflow.indexOf("Checkout candidate manifest helper");
const latestJsonIndex = releaseWorkflow.indexOf("Generate latest.json");
const candidateGuardStageIndex = releaseWorkflow.indexOf("Stage release candidate manifest helper");
const candidateManifestIndex = releaseWorkflow.indexOf(
  "Create immutable release candidate manifest"
);
const preflightIndex = releaseWorkflow.indexOf("Verify empty draft release target");
const uploadIndex = releaseWorkflow.indexOf("Upload verified candidate assets");
const uploadVerificationIndex = releaseWorkflow.indexOf("Verify uploaded draft assets");
const publicationVerificationIndex = releaseWorkflow.indexOf("Verify and publish GitHub Release");
assert.ok(
  guardStageIndex !== -1 &&
    immutablePromotionCheckoutIndex !== -1 &&
    guardStageIndex < immutablePromotionCheckoutIndex,
  "promotion guard must be staged before the release-source checkout"
);
assert.ok(
  candidateGuardCheckoutIndex !== -1 &&
    candidateGuardStageIndex > candidateGuardCheckoutIndex &&
    latestJsonIndex > candidateGuardStageIndex &&
    candidateManifestIndex > latestJsonIndex,
  "candidate assembly must use helpers staged from the workflow guard"
);
requireWorkflowText(
  'run: cp workflow-guard/scripts/release-promotion.mjs "$RUNNER_TEMP/release-promotion.mjs"',
  "candidate manifest helper provenance"
);
requireWorkflowText(
  "node workflow-guard/scripts/support-matrix.mjs generate-latest-json \\",
  "candidate support-matrix helper provenance"
);
assert.equal(
  releaseWorkflow.includes("Checkout immutable candidate source"),
  false,
  "candidate assembly must not execute helpers from the release source checkout"
);
assert.ok(
  preflightIndex !== -1 && uploadIndex !== -1 && preflightIndex < uploadIndex,
  "release target preflight must finish before the only upload step"
);
assert.ok(
  uploadIndex < uploadVerificationIndex && uploadVerificationIndex < publicationVerificationIndex,
  "the exact upload result must be verified before final publication"
);

console.log("[release-promotion:selftest] all assertions passed");
