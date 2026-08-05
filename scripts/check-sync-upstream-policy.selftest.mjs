import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { assertSyncUpstreamPolicy } from "./check-sync-upstream-policy.mjs";

const workflowPath = fileURLToPath(
  new URL("../.github/workflows/sync-upstream.yml", import.meta.url)
);
const workflow = readFileSync(workflowPath, "utf8");

assert.doesNotThrow(() => assertSyncUpstreamPolicy(workflow));

function expectRejected(name, from, to, expected) {
  assert.ok(workflow.includes(from), `${name}: fixture source must exist`);
  const mutant = workflow.replace(from, to);
  assert.notEqual(mutant, workflow, `${name}: fixture must mutate the workflow`);
  assert.throws(() => assertSyncUpstreamPolicy(mutant), expected, name);
}

expectRejected("content write permission", "  contents: read", "  contents: write", /read-only/);
expectRejected(
  "direct target push",
  'create_or_update_upstream_pr "target branch can be fast-forwarded but requires fork semantic review."',
  ':; git -c http.extraheader=redacted push origin "HEAD:${TARGET_BRANCH}"',
  /must not push/
);
expectRejected(
  "automatic CLI merge",
  'echo "Manual review and merge required."',
  ':; gh pr merge "${pr_number}" --repo "${GITHUB_REPOSITORY}" --merge\n              echo "Manual review and merge required."',
  /must not merge/
);
expectRejected(
  "automatic API merge",
  'echo "Manual review and merge required."',
  'gh api -X PUT "repos/${GITHUB_REPOSITORY}/pulls/${pr_number}/merge"\n              echo "Manual review and merge required."',
  /GitHub API/
);
expectRejected(
  "missing PR creation",
  "gh pr create \\",
  "gh pr view \\",
  /retain pull request creation/
);
expectRejected(
  "missing manual review body",
  "Please review and merge manually.",
  "This pull request is ready.",
  /body must require manual review/
);
expectRejected(
  "missing manual review summary",
  'echo "Manual review and merge required."',
  'echo "Pull request ready."',
  /step summary/
);
expectRejected(
  "fast-forward bypass",
  'create_or_update_upstream_pr "target branch can be fast-forwarded but requires fork semantic review."',
  'echo "Fast-forward path skipped."',
  /fast-forward topology must create/
);
expectRejected(
  "local conflict merge",
  'create_or_update_upstream_pr "target branch has local commits and cannot be safely fast-forwarded."',
  ':; git merge "${UPSTREAM}"',
  /must not merge upstream commits locally/
);
expectRejected(
  "missing conflict failure",
  '[ "${merge_state}" = "DIRTY" ]',
  '[ "${merge_state}" = "CLEAN" ]',
  /fail closed when the open PR has conflicts/
);
expectRejected(
  "review-blocked PR remains open",
  '[ "${merge_state}" = "UNKNOWN" ]; then',
  '[ "${merge_state}" = "UNKNOWN" ] || [ "${merge_state}" = "BLOCKED" ]; then',
  /leave review-blocked pull requests open/
);
expectRejected(
  "missing no-op branch",
  "Already up to date. Nothing to sync.",
  "No-op branch removed.",
  /no-op branch must remain explicit/
);
expectRejected("missing schedule", "  schedule:", "  disabled-schedule:", /scheduled trigger/);
expectRejected(
  "missing manual trigger",
  "  workflow_dispatch:",
  "  disabled-workflow-dispatch:",
  /manual trigger/
);
expectRejected(
  "changed upstream repository",
  "UPSTREAM_REPO: dyndynjyxa/aio-coding-hub",
  "UPSTREAM_REPO: example/other",
  /configured upstream repository/
);
expectRejected(
  "missing upstream fetch",
  'git fetch upstream "+refs/heads/${TARGET_BRANCH}:refs/remotes/upstream/${TARGET_BRANCH}"',
  'echo "upstream fetch removed"',
  /fetch the target branch from upstream/
);

console.log("Sync upstream manual-review policy self-test passed.");
