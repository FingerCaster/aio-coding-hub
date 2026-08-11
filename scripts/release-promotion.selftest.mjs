import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { compareReleaseVersions } from "./release-contract.mjs";
import {
  RELEASE_CANDIDATE_MANIFEST,
  EXPECTED_RELEASE_ASSET_NAMES,
  assertPublishedReleaseTarget,
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
const releasePleaseConfigPath = fileURLToPath(
  new URL("../release-please-config.json", import.meta.url)
);
const releasePromotionPath = fileURLToPath(new URL("./release-promotion.mjs", import.meta.url));
const releaseWorkflow = readFileSync(releaseWorkflowPath, "utf8").replace(/\r\n/g, "\n");
const releasePleaseConfig = JSON.parse(readFileSync(releasePleaseConfigPath, "utf8"));
const sourceSha = "d".repeat(40);
const otherSha = "e".repeat(40);
const releaseTag = "aio-coding-hub-v0.60.40";
const releaseVersion = "0.60.40";
const overlayDigest = "f".repeat(64);
const runId = 123456;
const runAttempt = 2;
const candidateIdentity = Object.freeze({
  channel: "stable",
  tag: releaseTag,
  sourceSha,
  releaseVersion,
  overlayDigest,
  runId,
  runAttempt,
});
const root = mkdtempSync(join(tmpdir(), "aio-release-promotion-"));

assert.equal(EXPECTED_RELEASE_ASSET_NAMES.length, 14, "the current release must keep 14 assets");
assert.deepEqual(
  EXPECTED_RELEASE_ASSET_NAMES.filter((name) => name.endsWith(".sig")).sort(),
  [
    "aio-coding-hub-linux-amd64.AppImage.sig",
    "aio-coding-hub-macos-arm.tar.gz.sig",
    "aio-coding-hub-macos-intel.tar.gz.sig",
    "aio-coding-hub-win64.msi.sig",
  ].sort(),
  "the exact standard updater signature asset matrix must remain published"
);
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

async function createFixture(name, identity = candidateIdentity) {
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
    ...identity,
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
        ...candidateIdentity,
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
      ...candidateIdentity,
    }),
    fixture.manifest
  );

  const stagingDirectory = join(root, "valid-staged");
  await stageReleaseCandidate({
    directory: fixture.directory,
    stagingDirectory,
    ...candidateIdentity,
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
      "--channel",
      "stable",
      "--tag",
      releaseTag,
      "--source-sha",
      sourceSha,
      "--release-version",
      releaseVersion,
      "--overlay-digest",
      overlayDigest,
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
      "--channel",
      "stable",
      "--tag",
      releaseTag,
      "--source-sha",
      sourceSha,
      "--release-version",
      releaseVersion,
      "--overlay-digest",
      overlayDigest,
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
        ...candidateIdentity,
        sourceSha: otherSha,
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
        ...candidateIdentity,
        runId: runId + 1,
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
        ...candidateIdentity,
        runAttempt: runAttempt + 1,
      }),
    /runAttempt mismatch/
  );
  assert.equal(readdirSync(root).includes("wrong-attempt-argument-staged"), false);

  await expectStageRejected(
    "tampered-asset",
    async ({ directory }) => {
      const path = join(directory, "aio-coding-hub-win64.msi");
      const original = readFileSync(path, "utf8");
      writeFileSync(path, `${original[0] === "x" ? "y" : "x"}${original.slice(1)}`);
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
    /release version mismatch/
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
            { name: "Asset.bin", size: 1, sha256: "a".repeat(64) },
            { name: "asset.bin", size: 1, sha256: "b".repeat(64) },
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
        ...candidateIdentity,
      }),
    /has no assets/
  );

  const candidateAssetNames = fixture.manifest.assets.map((asset) => asset.name);
  const emptyDraft = {
    id: 77,
    tag_name: releaseTag,
    target_commitish: sourceSha,
    draft: true,
    prerelease: false,
    assets: [],
  };
  assert.doesNotThrow(() =>
    assertReleasePromotionTarget({
      release: emptyDraft,
      expectedReleaseId: 77,
      expectedTag: releaseTag,
      expectedChannel: "stable",
      expectedSourceSha: sourceSha,
      candidateAssetNames,
    })
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: { ...emptyDraft, assets: [{ name: candidateAssetNames[0] }] },
        expectedReleaseId: 77,
        expectedTag: releaseTag,
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
        candidateAssetNames,
      }),
    /stable draft/
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: emptyDraft,
        expectedReleaseId: 78,
        expectedTag: releaseTag,
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
        candidateAssetNames,
      }),
    /Release ID mismatch/
  );

  const publishedAssets = fixture.manifest.assets.map((asset, index) => ({
    id: 1000 + index,
    name: asset.name,
    state: "uploaded",
    size: asset.size,
    digest: `sha256:${asset.sha256}`,
  }));
  const uploadedAssets = publishedAssets.map(({ id, name }) => ({ id, name }));
  const completeDraft = { ...emptyDraft, assets: publishedAssets };
  assert.doesNotThrow(() =>
    assertReleasePublicationTarget({
      release: completeDraft,
      expectedReleaseId: 77,
      expectedTag: releaseTag,
      expectedChannel: "stable",
      expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
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
        expectedChannel: "stable",
        expectedSourceSha: sourceSha,
        candidateManifest: fixture.manifest,
      }),
    /stable draft/
  );

  const publishedStable = {
    ...completeDraft,
    draft: false,
    published_at: "2026-08-10T00:00:00Z",
  };
  assert.doesNotThrow(() =>
    assertPublishedReleaseTarget({
      release: publishedStable,
      expectedReleaseId: 77,
      expectedTag: releaseTag,
      expectedChannel: "stable",
      expectedSourceSha: sourceSha,
      candidateManifest: fixture.manifest,
    })
  );

  const betaIdentity = Object.freeze({
    ...candidateIdentity,
    channel: "beta",
    tag: "aio-coding-hub-v0.60.41-beta.1",
    releaseVersion: "0.60.41-beta.1",
  });
  const betaFixture = await createFixture("valid-beta", betaIdentity);
  const betaDraft = {
    ...emptyDraft,
    id: 88,
    tag_name: betaIdentity.tag,
    target_commitish: sourceSha,
    prerelease: true,
  };
  assert.doesNotThrow(() =>
    assertReleasePromotionTarget({
      release: betaDraft,
      expectedReleaseId: 88,
      expectedTag: betaIdentity.tag,
      expectedChannel: "beta",
      expectedSourceSha: sourceSha,
      candidateAssetNames: betaFixture.manifest.assets.map((asset) => asset.name),
    })
  );
  assert.throws(
    () =>
      assertReleasePromotionTarget({
        release: { ...betaDraft, prerelease: false },
        expectedReleaseId: 88,
        expectedTag: betaIdentity.tag,
        expectedChannel: "beta",
        expectedSourceSha: sourceSha,
        candidateAssetNames: betaFixture.manifest.assets.map((asset) => asset.name),
      }),
    /beta draft/
  );

  const betaPublishedAssets = betaFixture.manifest.assets.map((asset, index) => ({
    id: 2000 + index,
    name: asset.name,
    state: "uploaded",
    size: asset.size,
    digest: `sha256:${asset.sha256}`,
  }));
  const completeBetaDraft = { ...betaDraft, assets: betaPublishedAssets };
  assert.doesNotThrow(() =>
    assertReleasePublicationTarget({
      release: completeBetaDraft,
      expectedReleaseId: 88,
      expectedTag: betaIdentity.tag,
      expectedChannel: "beta",
      expectedSourceSha: sourceSha,
      candidateManifest: betaFixture.manifest,
    })
  );
  const publishedBeta = {
    ...completeBetaDraft,
    draft: false,
    published_at: "2026-08-10T00:00:00Z",
  };
  assert.doesNotThrow(() =>
    assertPublishedReleaseTarget({
      release: publishedBeta,
      expectedReleaseId: 88,
      expectedTag: betaIdentity.tag,
      expectedChannel: "beta",
      expectedSourceSha: sourceSha,
      candidateManifest: betaFixture.manifest,
    })
  );
  assert.throws(
    () =>
      assertPublishedReleaseTarget({
        release: { ...publishedBeta, prerelease: false },
        expectedReleaseId: 88,
        expectedTag: betaIdentity.tag,
        expectedChannel: "beta",
        expectedSourceSha: sourceSha,
        candidateManifest: betaFixture.manifest,
      }),
    /beta publication/
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
assert.match(
  releaseWorkflow,
  /release_channel:\n\s+description: "Release channel"\n\s+required: false\n\s+default: stable\n\s+type: choice/,
  "stable must remain the default release channel"
);
assert.equal(
  /^  push:/m.test(releaseWorkflow),
  false,
  "release publication must remain manual-only under the current local-validation contract"
);
requireWorkflowText(
  "if: ${{ inputs.release_channel == 'stable' && inputs.release_tag == '' }}",
  "stable default release-please first stage"
);
requireWorkflowText(
  "uses: googleapis/release-please-action@8b8fd2cc23b2e18957157a9d923d75aa0c6f6ad5",
  "stable release-please action"
);
assert.equal(
  releasePleaseConfig.packages?.["."]?.draft,
  true,
  "stable release-please must create a draft for verified promotion"
);
requireWorkflowText(
  "if: steps.release.outputs.prs_created == 'true' && steps.release.outputs.release_created != 'true'",
  "stable release-please PR phase"
);
requireWorkflowText(
  "if: ${{ inputs.repair_beta_pointer == true || steps.manual_release.outputs.release_created == 'true' || steps.release.outputs.release_created == 'true' }}",
  "release-please second-stage immutable source resolution"
);
requireWorkflowText(
  "repair_beta_pointer:",
  "Beta pointer recovery must be an explicit workflow input"
);
requireWorkflowText(
  "repair-beta-release-channel:",
  "Beta pointer recovery must have a dedicated pointer-only job"
);
requireWorkflowText(
  "Verify public Beta and repair release channel pointer with CAS",
  "pointer-only recovery must verify the public Release before CAS"
);
requireWorkflowText(
  "inputs.repair_beta_pointer == true &&",
  "pointer-only recovery must be explicitly gated"
);
requireWorkflowText(
  'if [[ "$channel" == "beta" ]]; then prerelease=true; fi',
  "manual Beta draft prerelease state"
);
requireWorkflowText("make_latest: 'false'", "manual drafts must not affect GitHub latest");
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
  "prerelease: ${{ needs.release-please.outputs.release_channel == 'beta' }}",
  "Beta candidate draft prerelease state"
);
requireWorkflowText("make_latest: false", "draft asset upload latest-release guard");
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
  3,
  "promotion, publication, and channel publication must download the candidate-producing attempt artifact"
);
assert.equal(
  releaseWorkflow.match(
    /RUN_ATTEMPT: \$\{\{ needs\.assemble-release-candidate\.outputs\.candidate_run_attempt \}\}/g
  )?.length,
  3,
  "promotion, publication, and channel publication must verify the candidate-producing attempt"
);
requireWorkflowText(
  "name: Reverify release tag before publication",
  "publication-time exact tag verification"
);
assert.match(
  releaseWorkflow,
  /name: Checkout publication guard[\s\S]*?fetch-depth: 0[\s\S]*?persist-credentials: false[\s\S]*?name: Stage release publication guard/,
  "publication ancestry verification requires a full non-credentialed checkout"
);
requireWorkflowText(
  "needs: [release-please, assemble-release-candidate, promote-release]",
  "publication must wait for verified promotion"
);
requireWorkflowText("prerelease: beta,", "published Beta must remain a prerelease");
requireWorkflowText(
  "make_latest: beta ? 'false' : 'true',",
  "Beta must not become GitHub latest while stable keeps the existing latest behavior"
);
requireWorkflowText("assertPublishedReleaseTarget({", "public release state verification");
requireWorkflowText("publish-release-channel:", "release channel pointer publication job");
requireWorkflowText(
  "needs: [release-please, assemble-release-candidate, publish]",
  "release channel pointer must wait for public release verification"
);
requireWorkflowText(
  "needs.publish.result == 'success'",
  "release channel pointer must require successful public release verification"
);
requireWorkflowText(
  "const result = await publishReleaseChannel({",
  "release channel Git Data CAS publication"
);
requireWorkflowText(
  "expectedRefSha: snapshot?.refSha ?? null,",
  "release channel expected-ref CAS input"
);
requireWorkflowText(
  "if (confirmed?.refSha !== result.newRefSha)",
  "release channel post-CAS ref confirmation"
);
const repairJobStart = releaseWorkflow.indexOf("  repair-beta-release-channel:");
const repairJobEnd = releaseWorkflow.indexOf("\n  publish-release-channel:", repairJobStart);
assert.ok(
  repairJobStart !== -1 && repairJobEnd > repairJobStart,
  "pointer recovery job block is bounded"
);
const repairJob = releaseWorkflow.slice(repairJobStart, repairJobEnd);
assert.doesNotMatch(
  repairJob,
  /softprops\/action-gh-release|Upload verified candidate assets|Build signed Tauri candidate/,
  "pointer-only recovery must not build or mutate Release assets"
);
assert.match(
  repairJob,
  /publishReleaseChannel\([\s\S]*?expectedRefSha: snapshot\?\.refSha \?\? null/,
  "pointer-only recovery must use the same CAS publisher"
);
requireWorkflowText(
  "needs.release-please.outputs.release_channel == 'stable'",
  "Homebrew publication must remain stable-only"
);
const channelJobStart = releaseWorkflow.indexOf("  publish-release-channel:");
const channelJobEnd = releaseWorkflow.indexOf("\n  publish-homebrew-cask:", channelJobStart);
assert.ok(
  channelJobStart !== -1 && channelJobEnd > channelJobStart,
  "pointer job block is bounded"
);
const channelJob = releaseWorkflow.slice(channelJobStart, channelJobEnd);
const stablePreflightSource = channelJob.match(
  /function shouldSkipStablePointerPromotion\([\s\S]*?\n {12}\}(?=\n)/
)?.[0];
assert.ok(stablePreflightSource, "stable pointer preflight function must be extractable");
const shouldSkipStablePointerPromotion = Function(
  "compareReleaseVersions",
  `"use strict";\n${stablePreflightSource}\nreturn shouldSkipStablePointerPromotion;`
)(compareReleaseVersions);
for (const snapshot of [
  {
    state: {
      selected_version: "0.60.40",
      promotion_high_water_version: "0.60.42-beta.2",
    },
  },
  {
    state: {
      selected_version: "0.60.39",
      promotion_high_water_version: "0.60.42-beta.2",
    },
  },
]) {
  assert.equal(
    shouldSkipStablePointerPromotion({
      releaseChannel: "stable",
      releaseVersion: "0.60.41",
      snapshot,
    }),
    true,
    "successive pauses must skip a public stable release that cannot advance promotion high-water"
  );
}
assert.equal(
  shouldSkipStablePointerPromotion({
    releaseChannel: "stable",
    releaseVersion: "0.60.42",
    snapshot: {
      state: {
        selected_version: "0.60.40",
        promotion_high_water_version: "0.60.42-beta.2",
      },
    },
  }),
  false,
  "a stable release above promotion high-water must still publish the pointer"
);
assert.equal(
  shouldSkipStablePointerPromotion({
    releaseChannel: "beta",
    releaseVersion: "0.60.41-beta.1",
    snapshot: {
      state: {
        selected_version: "0.60.40",
        promotion_high_water_version: "0.60.42-beta.2",
      },
    },
  }),
  false,
  "the stable preflight must not alter Beta promotion behavior"
);
assert.match(
  channelJob,
  /if \(\s*shouldSkipStablePointerPromotion\([\s\S]*?\)\s*\) \{[\s\S]*?core\.notice\([\s\S]*?return;\s*\}/,
  "a true stable preflight decision must return before channel CAS"
);
assert.match(
  channelJob,
  /RELEASE_CHANNEL: \$\{\{ needs\.release-please\.outputs\.release_channel \}\}/,
  "pointer job must receive both stable and Beta channel identities"
);
assert.match(
  channelJob,
  /action: 'promote'/,
  "published stable and Beta releases use promotion CAS"
);
assert.match(
  channelJob,
  /compareReleaseVersions\(\s*releaseVersion,\s*snapshot\.state\.promotion_high_water_version,\s*\) <= 0/,
  "stable publication preflight must compare against the normalized promotion high-water version"
);
assert.doesNotMatch(
  channelJob,
  /snapshot\.state\.selected_version/,
  "stable publication preflight must not regress to the pause-selected version"
);
assert.doesNotMatch(channelJob, /release_channel == 'beta'/, "pointer job must not be Beta-only");
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
const channelPublicationIndex = releaseWorkflow.indexOf(
  "Publish verified release channel pointer with CAS"
);
const homebrewIndex = releaseWorkflow.indexOf("publish-homebrew-cask:");
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
  'cp workflow-guard/scripts/release-promotion.mjs "$RUNNER_TEMP/release-promotion.mjs"',
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
assert.ok(
  publicationVerificationIndex < channelPublicationIndex,
  "release channel pointer publication must occur after public release verification"
);
assert.ok(
  publicationVerificationIndex < homebrewIndex,
  "stable Homebrew publication must remain downstream of GitHub publication"
);

console.log("[release-promotion:selftest] all assertions passed");
