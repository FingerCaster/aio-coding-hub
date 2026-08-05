import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const modulePath = fileURLToPath(import.meta.url);
const repoRoot = dirname(dirname(modulePath));
const workflowPath = resolve(repoRoot, ".github/workflows/sync-upstream.yml");

function executableLines(source) {
  return source
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

function countMatchingLines(source, pattern) {
  return executableLines(source).filter((line) => pattern.test(line)).length;
}

function topLevelBlock(source, name) {
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `${name}:`);
  if (start === -1) return "";

  let end = start + 1;
  while (end < lines.length && (lines[end].trim() === "" || /^\s/.test(lines[end]))) end += 1;
  return lines.slice(start, end).join("\n");
}

export function validateSyncUpstreamPolicy(source) {
  const failures = [];
  const commands = executableLines(source);
  const commandText = commands.join("\n").replace(/\\\s*\n/g, " ");
  const permissions = topLevelBlock(source, "permissions");
  const requireText = (text, message) => {
    if (!source.includes(text)) failures.push(message);
  };

  if (!/^  contents: read\s*$/m.test(permissions)) {
    failures.push("top-level contents permission must be read-only");
  }
  if (!/^  pull-requests: write\s*$/m.test(permissions)) {
    failures.push("pull-requests permission must remain writable");
  }

  if (/\bgit\b[^\n]*\bpush(?:\s|$)/m.test(commandText)) {
    failures.push("workflow must not push a target branch directly");
  }
  if (/\bgit\s+merge(?:\s|$)/m.test(commandText)) {
    failures.push("workflow must not merge upstream commits locally");
  }
  if (/\bgh\s+pr\s+merge(?:\s|$)/m.test(commandText)) {
    failures.push("workflow must not merge a pull request automatically");
  }
  if (
    /\bgh\s+api\b[^\n]*(?:\/pulls\/[^\s]+\/merge(?=["'\s]|$)|mergePullRequest)/m.test(
      commandText
    ) ||
    source.includes("mergePullRequest")
  ) {
    failures.push("workflow must not merge a pull request through the GitHub API");
  }

  if (!commands.some((line) => /^gh\s+pr\s+create(?:\s|$)/.test(line))) {
    failures.push("workflow must retain pull request creation");
  }
  if (!commands.some((line) => /^gh\s+pr\s+edit(?:\s|$)/.test(line))) {
    failures.push("workflow must retain existing pull request updates");
  }
  requireText('--base "${TARGET_BRANCH}"', "sync pull requests must target TARGET_BRANCH");
  requireText('--head "${UPSTREAM_HEAD}"', "sync pull requests must use the upstream head");
  requireText("Please review and merge manually.", "pull request body must require manual review");
  requireText(
    "Manual review and merge required.",
    "step summary must state that manual review and merge are required"
  );
  requireText("  schedule:", "workflow must retain the scheduled trigger");
  requireText('- cron: "0 0 * * *"', "workflow must retain the daily sync schedule");
  requireText("  workflow_dispatch:", "workflow must retain the manual trigger");
  requireText(
    "UPSTREAM_REPO: dyndynjyxa/aio-coding-hub",
    "workflow must retain the configured upstream repository"
  );
  requireText(
    "TARGET_BRANCH: ${{ github.event.inputs.target_branch || 'main' }}",
    "workflow must retain the configured target branch"
  );
  requireText(
    'git fetch origin "+refs/heads/${TARGET_BRANCH}:refs/remotes/origin/${TARGET_BRANCH}"',
    "workflow must fetch the target branch from origin"
  );
  requireText(
    'git fetch upstream "+refs/heads/${TARGET_BRANCH}:refs/remotes/upstream/${TARGET_BRANCH}"',
    "workflow must fetch the target branch from upstream"
  );
  requireText("--json mergeStateStatus", "workflow must inspect the open PR merge state once");
  requireText(
    '[ "${merge_state}" = "DIRTY" ]',
    "workflow must fail closed when the open PR has conflicts"
  );
  requireText(
    '[ "${merge_state}" = "UNKNOWN" ]',
    "workflow must fail closed when the open PR merge state is unknown"
  );
  if (source.includes('[ "${merge_state}" = "BLOCKED" ]')) {
    failures.push("workflow must leave review-blocked pull requests open for manual review");
  }
  requireText(
    "Manual conflict resolution required",
    "conflicted sync pull requests must report manual resolution"
  );

  const noOpMarker = 'if git merge-base --is-ancestor "${UPSTREAM}" "${LOCAL}"; then';
  const fastForwardMarker = 'if git merge-base --is-ancestor "${LOCAL}" "${UPSTREAM}"; then';
  const divergedMarker =
    'echo "Branches have diverged. Creating cross-repository sync PR instead."';
  requireText(noOpMarker, "workflow must retain the already-synchronized no-op branch");
  requireText("Already up to date. Nothing to sync.", "no-op branch must remain explicit");
  requireText(fastForwardMarker, "workflow must distinguish the fast-forward topology");
  requireText(divergedMarker, "workflow must retain the diverged topology path");

  const fastForwardStart = source.indexOf(fastForwardMarker);
  const divergedStart = source.indexOf(divergedMarker);
  if (fastForwardStart !== -1 && divergedStart > fastForwardStart) {
    const fastForwardBlock = source.slice(fastForwardStart, divergedStart);
    if (countMatchingLines(fastForwardBlock, /^if(?:\s|$)/) !== 1) {
      failures.push("fast-forward topology must not conditionally bypass pull request creation");
    }
    if (countMatchingLines(fastForwardBlock, /^create_or_update_upstream_pr(?:\s|$)/) !== 1) {
      failures.push("fast-forward topology must create or update exactly one pull request");
    }
  }

  if (divergedStart !== -1) {
    const divergedBlock = source.slice(divergedStart);
    if (countMatchingLines(divergedBlock, /^create_or_update_upstream_pr(?:\s|$)/) !== 1) {
      failures.push("diverged topology must create or update exactly one pull request");
    }
  }

  return failures;
}

export function assertSyncUpstreamPolicy(source) {
  const failures = validateSyncUpstreamPolicy(source);
  if (failures.length > 0) {
    throw new Error(`Sync upstream policy check failed:\n- ${failures.join("\n- ")}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === modulePath) {
  assertSyncUpstreamPolicy(readFileSync(workflowPath, "utf8"));
  console.log("Sync upstream manual-review policy check passed.");
}
