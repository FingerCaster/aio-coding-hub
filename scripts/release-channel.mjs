import { createHash } from "node:crypto";

import {
  RELEASE_CHANNELS,
  compareReleaseVersions,
  parseReleaseTag,
  requirePlainObject,
  requirePositiveInteger,
  requireSha256,
  requireSourceSha,
} from "./release-contract.mjs";
import {
  assertPublishedReleaseAssetMatrix,
  assertPublishedReleaseTarget,
} from "./release-promotion.mjs";
import { OFFICIAL_RELEASE_TARGETS } from "./support-matrix.mjs";

export const RELEASE_CHANNEL_BRANCH = "release-channels";
export const RELEASE_CHANNEL_MANIFEST = "latest-beta.json";
export const RELEASE_CHANNEL_STATE = "beta-channel-state.json";
export const RELEASE_CHANNEL_STATE_SCHEMA = 1;

const MAX_MANIFEST_BYTES = 1024 * 1024;
const REPOSITORY_SEGMENT_PATTERN = /^[A-Za-z0-9_.-]{1,100}$/;
const OPERATOR_PATTERN = /^[^\u0000-\u001f\u007f]{1,100}$/;
const STATE_KEYS = Object.freeze(
  [
    "schema_version",
    "action",
    "selected_channel",
    "selected_tag",
    "selected_version",
    "release_id",
    "release_url",
    "source_sha",
    "manifest_sha256",
    "previous_ref_sha",
    "previous_selected_tag",
    "withdrawn_tag",
    "workflow_run_id",
    "workflow_run_attempt",
    "operator",
    "updated_at",
  ].sort()
);

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function requireExactKeys(value, expectedKeys, label) {
  const actual = Object.keys(value).sort();
  if (actual.join("\n") !== [...expectedKeys].sort().join("\n")) {
    throw new Error(`${label} fields are invalid`);
  }
}

function requireRepositorySegment(value, label) {
  if (typeof value !== "string" || !REPOSITORY_SEGMENT_PATTERN.test(value)) {
    throw new Error(`${label} is invalid`);
  }
  return value;
}

function requireOperator(value) {
  if (typeof value !== "string" || !OPERATOR_PATTERN.test(value) || value !== value.trim()) {
    throw new Error("Release channel operator is invalid");
  }
  return value;
}

function requireUtcTimestamp(value, label) {
  if (
    typeof value !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(value)
  ) {
    throw new Error(`${label} must be a UTC timestamp`);
  }
  const parsed = new Date(value);
  const canonicalValue = value.endsWith(".000Z") ? value : value.replace(/Z$/, ".000Z");
  if (Number.isNaN(parsed.getTime()) || parsed.toISOString() !== canonicalValue) {
    throw new Error(`${label} must use canonical UTC ISO format`);
  }
  return value;
}

function requireNullableSha(value, label) {
  return value === null ? null : requireSourceSha(value, label);
}

function requireNullableTag(value, label) {
  if (value === null) return null;
  return parseReleaseTag(value, value.includes("-beta.") ? "beta" : "stable").tag;
}

function releaseUrl(owner, repo, tag) {
  return `https://github.com/${owner}/${repo}/releases/tag/${tag}`;
}

function releaseAssetUrl(owner, repo, tag, assetName) {
  return `https://github.com/${owner}/${repo}/releases/download/${tag}/${assetName}`;
}

export function parseUpdaterManifest(text, { owner, repo, tag, expectedVersion } = {}) {
  if (typeof text !== "string" || Buffer.byteLength(text) === 0) {
    throw new Error("Updater manifest must be non-empty text");
  }
  if (Buffer.byteLength(text) > MAX_MANIFEST_BYTES) {
    throw new Error("Updater manifest exceeds the size limit");
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `Updater manifest is invalid JSON: ${error instanceof Error ? error.message : error}`
    );
  }
  const manifest = requirePlainObject(parsed, "Updater manifest");
  requireExactKeys(manifest, ["version", "notes", "pub_date", "platforms"], "Updater manifest");
  const identity = parseReleaseTag(tag, tag.includes("-beta.") ? "beta" : "stable");
  if (manifest.version !== identity.version || manifest.version !== expectedVersion) {
    throw new Error("Updater manifest version does not match the selected release tag");
  }
  if (typeof manifest.notes !== "string") {
    throw new Error("Updater manifest notes must be text");
  }
  requireUtcTimestamp(manifest.pub_date, "Updater manifest pub_date");
  const platforms = requirePlainObject(manifest.platforms, "Updater manifest platforms");
  const expectedPlatforms = OFFICIAL_RELEASE_TARGETS.map((target) => target.updaterPlatform).sort();
  if (Object.keys(platforms).sort().join("\n") !== expectedPlatforms.join("\n")) {
    throw new Error("Updater manifest platform set does not match the official support matrix");
  }
  for (const target of OFFICIAL_RELEASE_TARGETS) {
    const platform = requirePlainObject(
      platforms[target.updaterPlatform],
      `Updater manifest platform ${target.updaterPlatform}`
    );
    requireExactKeys(platform, ["signature", "url"], `Updater manifest ${target.updaterPlatform}`);
    if (typeof platform.signature !== "string" || platform.signature.trim().length === 0) {
      throw new Error(`Updater manifest signature is missing for ${target.updaterPlatform}`);
    }
    const expectedUrl = releaseAssetUrl(owner, repo, identity.tag, target.latestAssetName);
    if (platform.url !== expectedUrl) {
      throw new Error(`Updater manifest URL is invalid for ${target.updaterPlatform}`);
    }
  }
  return manifest;
}

export function parseReleaseChannelState(text, { owner, repo, manifestText }) {
  if (typeof text !== "string") {
    throw new Error("Release channel state must be text");
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `Release channel state is invalid JSON: ${error instanceof Error ? error.message : error}`
    );
  }
  const state = requirePlainObject(parsed, "Release channel state");
  requireExactKeys(state, STATE_KEYS, "Release channel state");
  if (state.schema_version !== RELEASE_CHANNEL_STATE_SCHEMA) {
    throw new Error(`Unsupported release channel state schema: ${state.schema_version}`);
  }
  if (state.action !== "promote" && state.action !== "pause") {
    throw new Error("Release channel state action must be promote or pause");
  }
  const identity = parseReleaseTag(state.selected_tag, state.selected_channel);
  if (state.selected_version !== identity.version) {
    throw new Error("Release channel state version does not match its tag");
  }
  requirePositiveInteger(state.release_id, "Release channel state release ID");
  if (state.release_url !== releaseUrl(owner, repo, identity.tag)) {
    throw new Error("Release channel state release URL is invalid");
  }
  requireSourceSha(state.source_sha, "Release channel state source SHA");
  const manifestDigest = requireSha256(
    state.manifest_sha256,
    "Release channel state manifest digest"
  );
  if (sha256(manifestText) !== manifestDigest) {
    throw new Error("Release channel state manifest digest does not match latest-beta.json");
  }
  const previousRefSha = requireNullableSha(
    state.previous_ref_sha,
    "Release channel state previous ref SHA"
  );
  const previousSelectedTag = requireNullableTag(
    state.previous_selected_tag,
    "Release channel state previous selected tag"
  );
  const withdrawnTag = requireNullableTag(state.withdrawn_tag, "Release channel withdrawn tag");
  if ((previousRefSha === null) !== (previousSelectedTag === null)) {
    throw new Error("Release channel previous ref and selected tag must both be present or absent");
  }
  if (state.action === "promote" && withdrawnTag !== null) {
    throw new Error("Promote state cannot record a withdrawn tag");
  }
  if (state.action === "pause" && withdrawnTag === null) {
    throw new Error("Pause state must record a withdrawn tag");
  }
  if (state.action === "pause" && withdrawnTag !== previousSelectedTag) {
    throw new Error("Pause state withdrawn tag must match the previous selected tag");
  }
  requirePositiveInteger(state.workflow_run_id, "Release channel workflow run ID");
  requirePositiveInteger(state.workflow_run_attempt, "Release channel workflow run attempt");
  requireOperator(state.operator);
  requireUtcTimestamp(state.updated_at, "Release channel updated_at");
  parseUpdaterManifest(manifestText, {
    owner,
    repo,
    tag: identity.tag,
    expectedVersion: identity.version,
  });
  return {
    ...state,
    previous_ref_sha: previousRefSha,
    previous_selected_tag: previousSelectedTag,
    withdrawn_tag: withdrawnTag,
  };
}

function decodeBlob(blob, label) {
  const data = requirePlainObject(blob, label);
  if (data.encoding !== "base64" || typeof data.content !== "string") {
    throw new Error(`${label} must be base64 encoded`);
  }
  const encoded = data.content.replace(/\s/g, "");
  if (encoded.length === 0 || encoded.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(encoded)) {
    throw new Error(`${label} has invalid base64 content`);
  }
  const bytes = Buffer.from(encoded, "base64");
  if (bytes.toString("base64") !== encoded) {
    throw new Error(`${label} has non-canonical base64 content`);
  }
  if (bytes.length === 0 || bytes.length > MAX_MANIFEST_BYTES) {
    throw new Error(`${label} has an invalid size`);
  }
  const text = bytes.toString("utf8");
  if (!Buffer.from(text, "utf8").equals(bytes)) {
    throw new Error(`${label} is not valid UTF-8`);
  }
  return text;
}

function errorStatus(error) {
  return typeof error === "object" && error !== null ? error.status : undefined;
}

export async function readReleaseChannelSnapshot({ github, owner, repo }) {
  requireRepositorySegment(owner, "Repository owner");
  requireRepositorySegment(repo, "Repository name");
  let refResponse;
  try {
    refResponse = await github.rest.git.getRef({
      owner,
      repo,
      ref: `heads/${RELEASE_CHANNEL_BRANCH}`,
    });
  } catch (error) {
    if (errorStatus(error) === 404) return null;
    throw error;
  }
  const ref = requirePlainObject(refResponse.data, "Release channel ref");
  const refObject = requirePlainObject(ref.object, "Release channel ref object");
  if (refObject.type !== "commit") {
    throw new Error("Release channel ref must point to a commit");
  }
  const refSha = requireSourceSha(refObject.sha, "Release channel ref SHA");
  const commitResponse = await github.rest.git.getCommit({ owner, repo, commit_sha: refSha });
  const commit = requirePlainObject(commitResponse.data, "Release channel commit");
  if (!Array.isArray(commit.parents)) {
    throw new Error("Release channel commit parents must be an array");
  }
  const parentShas = commit.parents.map((parent, index) =>
    requireSourceSha(
      requirePlainObject(parent, `Release channel commit parent ${index}`).sha,
      `Release channel commit parent ${index} SHA`
    )
  );
  if (parentShas.length > 1) {
    throw new Error("Release channel commit must not be a merge commit");
  }
  const treeSha = requireSourceSha(
    requirePlainObject(commit.tree, "Release channel commit tree").sha,
    "Release channel tree SHA"
  );
  const treeResponse = await github.rest.git.getTree({ owner, repo, tree_sha: treeSha });
  const tree = requirePlainObject(treeResponse.data, "Release channel tree");
  if (tree.truncated === true || !Array.isArray(tree.tree)) {
    throw new Error("Release channel tree is truncated or invalid");
  }
  const entries = [...tree.tree].sort((left, right) =>
    String(left?.path).localeCompare(String(right?.path), "en")
  );
  const expectedPaths = [RELEASE_CHANNEL_STATE, RELEASE_CHANNEL_MANIFEST].sort();
  if (
    entries.length !== expectedPaths.length ||
    entries.some(
      (entry, index) =>
        entry?.path !== expectedPaths[index] || entry.type !== "blob" || entry.mode !== "100644"
    )
  ) {
    throw new Error("Release channel branch must contain exactly the manifest and state blobs");
  }
  const contentByPath = new Map();
  for (const entry of entries) {
    const blobSha = requireSourceSha(entry.sha, `Release channel blob SHA for ${entry.path}`);
    const blob = await github.rest.git.getBlob({ owner, repo, file_sha: blobSha });
    contentByPath.set(entry.path, decodeBlob(blob.data, `Release channel blob ${entry.path}`));
  }
  const manifestText = contentByPath.get(RELEASE_CHANNEL_MANIFEST);
  const stateText = contentByPath.get(RELEASE_CHANNEL_STATE);
  const state = parseReleaseChannelState(stateText, { owner, repo, manifestText });
  if (
    (state.previous_ref_sha === null && parentShas.length !== 0) ||
    (state.previous_ref_sha !== null &&
      (parentShas.length !== 1 || parentShas[0] !== state.previous_ref_sha))
  ) {
    throw new Error("Release channel state previous ref does not match the commit parent");
  }
  return { refSha, treeSha, manifestText, stateText, state };
}

async function downloadVerifiedReleaseAsset({
  fetchImpl,
  owner,
  repo,
  tag,
  assetName,
  asset,
  rawAsset,
  label,
}) {
  const expectedUrl = releaseAssetUrl(owner, repo, tag, assetName);
  if (!asset || rawAsset?.browser_download_url !== expectedUrl) {
    throw new Error(`Published Release ${label} URL is invalid`);
  }
  if (asset.size > MAX_MANIFEST_BYTES) {
    throw new Error(`Published ${label} exceeds the size limit`);
  }
  const response = await fetchImpl(expectedUrl, { redirect: "follow" });
  if (!response?.ok || typeof response.arrayBuffer !== "function") {
    throw new Error(`Failed to download published ${label}: HTTP ${response?.status ?? "?"}`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  if (bytes.length !== asset.size || bytes.length === 0 || bytes.length > MAX_MANIFEST_BYTES) {
    throw new Error(`Published ${label} size does not match its Release asset`);
  }
  if (sha256(bytes) !== asset.sha256) {
    throw new Error(`Published ${label} digest does not match its Release asset`);
  }
  return bytes;
}

async function downloadVerifiedManifest({ fetchImpl, owner, repo, release, assets, tag, version }) {
  const latestAsset = assets.find((asset) => asset.name === "latest.json");
  const rawAsset = release.assets.find((asset) => asset?.name === "latest.json");
  const bytes = await downloadVerifiedReleaseAsset({
    fetchImpl,
    owner,
    repo,
    tag,
    assetName: "latest.json",
    asset: latestAsset,
    rawAsset,
    label: "updater manifest",
  });
  const manifestText = bytes.toString("utf8");
  const manifest = parseUpdaterManifest(manifestText, {
    owner,
    repo,
    tag,
    expectedVersion: version,
  });
  for (const target of OFFICIAL_RELEASE_TARGETS) {
    const signatureAsset = assets.find((asset) => asset.name === target.latestSignatureName);
    const rawSignatureAsset = release.assets.find(
      (asset) => asset?.name === target.latestSignatureName
    );
    const signatureBytes = await downloadVerifiedReleaseAsset({
      fetchImpl,
      owner,
      repo,
      tag,
      assetName: target.latestSignatureName,
      asset: signatureAsset,
      rawAsset: rawSignatureAsset,
      label: `updater signature ${target.updaterPlatform}`,
    });
    const signature = signatureBytes.toString("utf8").replace(/[\r\n]+/g, "");
    if (signature !== manifest.platforms[target.updaterPlatform].signature) {
      throw new Error(
        `Published updater signature does not match latest.json for ${target.updaterPlatform}`
      );
    }
  }
  return { manifestText, manifest, manifestDigest: latestAsset.sha256 };
}

export async function loadVerifiedReleaseChannelTarget({
  github,
  fetchImpl = fetch,
  owner,
  repo,
  tag,
  sourceSha,
  candidateManifest,
}) {
  requireRepositorySegment(owner, "Repository owner");
  requireRepositorySegment(repo, "Repository name");
  const identity = parseReleaseTag(tag, tag.includes("-beta.") ? "beta" : "stable");
  requireSourceSha(sourceSha);
  const response = await github.rest.repos.getReleaseByTag({ owner, repo, tag: identity.tag });
  const release = requirePlainObject(response.data, "Selected GitHub Release");
  const verified = candidateManifest
    ? assertPublishedReleaseTarget({
        release,
        expectedReleaseId: release.id,
        expectedTag: identity.tag,
        expectedChannel: identity.channel,
        expectedSourceSha: sourceSha,
        candidateManifest,
      })
    : assertPublishedReleaseAssetMatrix({
        release,
        expectedReleaseId: release.id,
        expectedTag: identity.tag,
        expectedChannel: identity.channel,
        expectedSourceSha: sourceSha,
      });
  const manifest = await downloadVerifiedManifest({
    fetchImpl,
    owner,
    repo,
    release: verified.release,
    assets: verified.assets,
    tag: identity.tag,
    version: identity.version,
  });
  return { identity, release: verified.release, assets: verified.assets, ...manifest };
}

function normalizeAction(value) {
  if (value !== "promote" && value !== "pause") {
    throw new Error("Release channel action must be promote or pause");
  }
  return value;
}

function validateTransition({ action, current, target, withdrawnTag }) {
  if (action === "promote") {
    if (withdrawnTag !== null && withdrawnTag !== undefined && withdrawnTag !== "") {
      throw new Error("Promote must not include a withdrawn tag");
    }
    if (
      current &&
      compareReleaseVersions(target.identity.version, current.state.selected_version) <= 0
    ) {
      throw new Error("Release channel promotion must select a strictly higher version");
    }
    return null;
  }
  if (!current) {
    throw new Error("Release channel cannot pause before its first promotion");
  }
  const normalizedWithdrawnTag = requireNullableTag(withdrawnTag, "Withdrawn release tag");
  if (normalizedWithdrawnTag !== current.state.selected_tag) {
    throw new Error("Pause withdrawn tag must match the current channel target");
  }
  if (target.identity.tag === normalizedWithdrawnTag) {
    throw new Error("Pause safe target must differ from the withdrawn release");
  }
  return normalizedWithdrawnTag;
}

function buildState({
  action,
  current,
  target,
  sourceSha,
  manifestDigest,
  withdrawnTag,
  workflowRunId,
  workflowRunAttempt,
  operator,
  updatedAt,
  owner,
  repo,
}) {
  return {
    schema_version: RELEASE_CHANNEL_STATE_SCHEMA,
    action,
    selected_channel: target.identity.channel,
    selected_tag: target.identity.tag,
    selected_version: target.identity.version,
    release_id: target.release.id,
    release_url: releaseUrl(owner, repo, target.identity.tag),
    source_sha: sourceSha,
    manifest_sha256: manifestDigest,
    previous_ref_sha: current?.refSha ?? null,
    previous_selected_tag: current?.state.selected_tag ?? null,
    withdrawn_tag: withdrawnTag,
    workflow_run_id: requirePositiveInteger(workflowRunId, "Workflow run ID"),
    workflow_run_attempt: requirePositiveInteger(workflowRunAttempt, "Workflow run attempt"),
    operator: requireOperator(operator),
    updated_at: requireUtcTimestamp(updatedAt, "Release channel update timestamp"),
  };
}

export async function publishReleaseChannel({
  github,
  fetchImpl = fetch,
  owner,
  repo,
  action,
  tag,
  sourceSha,
  expectedRefSha,
  withdrawnTag,
  candidateManifest,
  workflowRunId,
  workflowRunAttempt,
  operator,
  updatedAt,
}) {
  const normalizedAction = normalizeAction(action);
  const expected = requireNullableSha(expectedRefSha, "Expected release channel ref SHA");
  const current = await readReleaseChannelSnapshot({ github, owner, repo });
  if ((current?.refSha ?? null) !== expected) {
    throw new Error(
      `Release channel ref changed: expected ${expected ?? "<absent>"}, received ${current?.refSha ?? "<absent>"}`
    );
  }
  const target = await loadVerifiedReleaseChannelTarget({
    github,
    fetchImpl,
    owner,
    repo,
    tag,
    sourceSha,
    candidateManifest,
  });
  const normalizedWithdrawnTag = validateTransition({
    action: normalizedAction,
    current,
    target,
    withdrawnTag,
  });
  const state = buildState({
    action: normalizedAction,
    current,
    target,
    sourceSha,
    manifestDigest: target.manifestDigest,
    withdrawnTag: normalizedWithdrawnTag,
    workflowRunId,
    workflowRunAttempt,
    operator,
    updatedAt,
    owner,
    repo,
  });
  const stateText = `${JSON.stringify(state, null, 2)}\n`;
  parseReleaseChannelState(stateText, {
    owner,
    repo,
    manifestText: target.manifestText,
  });

  const [manifestBlob, stateBlob] = await Promise.all([
    github.rest.git.createBlob({
      owner,
      repo,
      content: Buffer.from(target.manifestText).toString("base64"),
      encoding: "base64",
    }),
    github.rest.git.createBlob({
      owner,
      repo,
      content: Buffer.from(stateText).toString("base64"),
      encoding: "base64",
    }),
  ]);
  const treeRequest = {
    owner,
    repo,
    tree: [
      {
        path: RELEASE_CHANNEL_MANIFEST,
        mode: "100644",
        type: "blob",
        sha: requireSourceSha(manifestBlob.data.sha, "Created manifest blob SHA"),
      },
      {
        path: RELEASE_CHANNEL_STATE,
        mode: "100644",
        type: "blob",
        sha: requireSourceSha(stateBlob.data.sha, "Created state blob SHA"),
      },
    ],
  };
  if (current) treeRequest.base_tree = current.treeSha;
  const tree = await github.rest.git.createTree(treeRequest);
  const commit = await github.rest.git.createCommit({
    owner,
    repo,
    message: `chore(release-channel): ${normalizedAction} ${target.identity.tag}`,
    tree: requireSourceSha(tree.data.sha, "Created release channel tree SHA"),
    parents: current ? [current.refSha] : [],
  });
  // The commit cannot contain its own SHA. Persist the parent in state and expose the new SHA only
  // after GitHub creates the commit so callers can confirm and log the CAS result.
  const newRefSha = requireSourceSha(commit.data.sha, "Created release channel commit SHA");
  if (current) {
    await github.rest.git.updateRef({
      owner,
      repo,
      ref: `heads/${RELEASE_CHANNEL_BRANCH}`,
      sha: newRefSha,
      force: false,
    });
  } else {
    await github.rest.git.createRef({
      owner,
      repo,
      ref: `refs/heads/${RELEASE_CHANNEL_BRANCH}`,
      sha: newRefSha,
    });
  }
  return { previousRefSha: current?.refSha ?? null, newRefSha, state };
}
