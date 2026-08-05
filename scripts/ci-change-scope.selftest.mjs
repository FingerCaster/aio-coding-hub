import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  classifyNameStatus,
  classifyPath,
  classifyPaths,
  collectChangedPaths,
  loadPolicy,
  parseNameStatus,
  runClassifier,
  validatePolicy,
} from "./ci-change-scope.mjs";

const classifierPath = fileURLToPath(new URL("./ci-change-scope.mjs", import.meta.url));
const policyPath = fileURLToPath(new URL("../.github/ci-scope.json", import.meta.url));
const policy = loadPolicy(policyPath);
const sha = (character) => character.repeat(40);

function expectScope(paths, expected) {
  const result = classifyPaths(paths, policy);
  assert.equal(result.scope, expected.scope, JSON.stringify(paths));
  assert.equal(result.fullCi, expected.fullCi, JSON.stringify(paths));
  assert.equal(result.docsChecks, expected.docsChecks, JSON.stringify(paths));
  if (expected.reason) {
    assert.equal(result.reason, expected.reason, JSON.stringify(paths));
  }
  return result;
}

for (const path of [
  "AGENTS.md",
  ".trellis/tasks/08-05-task/prd.md",
  ".trellis/tasks/08-05-task/task.json",
  ".trellis/tasks/08-05-task/implement.jsonl",
  ".trellis/workspace/developer/journal-1.md",
]) {
  assert.equal(classifyPath(path, policy).tier, "process-docs", path);
}

for (const path of [
  "README.md",
  "README_EN.md",
  "docs/plugins/authoring.md",
  ".trellis/spec/aio-coding-hub/cross-layer/index.md",
]) {
  assert.equal(classifyPath(path, policy).tier, "checked-docs", path);
}

for (const path of [
  "CHANGELOG.md",
  ".trellis/workflow.md",
  ".trellis/agents/trellis-check.md",
  ".trellis/tasks/08-05-task/data.txt",
  ".trellis/workspace/developer/state.json",
  "docs/plugins/plugin-api-v1-contract.json",
  "docs/image.png",
  ".trellis/spec/index.yml",
  "package.json",
  "pnpm-lock.yaml",
  "scripts/check-spec-links.mjs",
  "src/main.tsx",
]) {
  assert.equal(classifyPath(path, policy).tier, "full", path);
}

expectScope(["AGENTS.md", ".trellis/tasks/08-05-task/task.json"], {
  scope: "process-docs",
  fullCi: false,
  docsChecks: false,
  reason: "process-documentation",
});
expectScope(["AGENTS.md", "docs/plugins/authoring.md"], {
  scope: "full",
  fullCi: true,
  docsChecks: true,
  reason: "mixed-tiers",
});
expectScope(["src/main.tsx", "docs/plugins/authoring.md"], {
  scope: "full",
  fullCi: true,
  docsChecks: true,
  reason: "mixed-tiers",
});
expectScope(["src/main.tsx", "AGENTS.md"], {
  scope: "full",
  fullCi: true,
  docsChecks: false,
  reason: "mixed-tiers",
});
expectScope([], {
  scope: "full",
  fullCi: true,
  docsChecks: false,
  reason: "empty-diff",
});

for (const path of [
  "",
  "../README.md",
  "/README.md",
  "C:/README.md",
  "docs\\guide.md",
  "docs//guide.md",
  "docs/./guide.md",
  "docs/../README.md",
  "docs/guide\n.md",
]) {
  assert.equal(classifyPath(path, policy).reason, "unsafe-path", JSON.stringify(path));
}

const permissivePolicy = structuredClone(policy);
permissivePolicy.processDocumentation.exactPaths.push(
  ".github/ci-scope.json",
  ".github/workflows/ci.yml",
  "scripts/ci-change-scope.mjs",
  "scripts/ci-change-scope.selftest.mjs",
  "scripts/ci-gate.mjs",
  "scripts/ci-workflow-contract.selftest.mjs"
);
validatePolicy(permissivePolicy);
for (const path of [
  ".github/ci-scope.json",
  ".github/workflows/ci.yml",
  ".github/ISSUE_TEMPLATE/config.yml",
  "scripts/ci-change-scope.mjs",
  "scripts/ci-change-scope.selftest.mjs",
  "scripts/ci-gate.mjs",
  "scripts/ci-workflow-contract.selftest.mjs",
]) {
  assert.deepEqual(classifyPath(path, permissivePolicy), {
    path,
    tier: "full",
    reason: "ci-control-plane",
  });
}

const ambiguousPolicy = structuredClone(policy);
ambiguousPolicy.checkedDocumentation.exactPaths.push("AGENTS.md");
validatePolicy(ambiguousPolicy);
assert.equal(classifyPath("AGENTS.md", ambiguousPolicy).reason, "ambiguous-policy");

const nameStatus =
  "M\0AGENTS.md\0D\0docs/old.md\0R100\0AGENTS.md\0src/agent.ts\0C090\0README.md\0docs/copy.md\0";
assert.deepEqual(parseNameStatus(nameStatus), [
  { status: "M", paths: ["AGENTS.md"] },
  { status: "D", paths: ["docs/old.md"] },
  { status: "R100", paths: ["AGENTS.md", "src/agent.ts"] },
  { status: "C090", paths: ["README.md", "docs/copy.md"] },
]);
assert.equal(classifyNameStatus("D\0.trellis/tasks/old/prd.md\0", policy).scope, "process-docs");
assert.equal(classifyNameStatus("D\0docs/old.md\0", policy).scope, "checked-docs");
assert.equal(
  classifyNameStatus("R100\0.trellis/tasks/old/prd.md\0.trellis/tasks/new/prd.md\0", policy)
    .scope,
  "process-docs"
);
assert.equal(classifyNameStatus("R100\0AGENTS.md\0src/agent.ts\0", policy).scope, "full");
assert.equal(
  classifyNameStatus("R100\0AGENTS.md\0docs/agent.md\0", policy).reason,
  "mixed-tiers"
);
assert.equal(classifyNameStatus("C090\0README.md\0docs/copy.md\0", policy).scope, "checked-docs");
const copyIntoSource = classifyNameStatus("C100\0README.md\0src/readme.md\0", policy);
assert.equal(copyIntoSource.scope, "full");
assert.equal(copyIntoSource.docsChecks, true);

assert.throws(() => parseNameStatus("M\0README.md"), /NUL terminated/);
assert.throws(() => parseNameStatus("M\0"), /invalid M/);
assert.throws(() => parseNameStatus("R100\0README.md\0"), /invalid R100/);
assert.throws(() => parseNameStatus("R101\0README.md\0docs/readme.md\0"), /invalid R101/);
assert.throws(() => parseNameStatus("C\0README.md\0docs/readme.md\0"), /invalid C/);
assert.throws(() => parseNameStatus("Q\0README.md\0"), /invalid Q/);
assert.throws(() => parseNameStatus("X\0README.md\0"), /invalid X/);
assert.throws(() => parseNameStatus("B\0README.md\0"), /invalid B/);
assert.throws(
  () => parseNameStatus(Buffer.from([0x4d, 0x00, 0xff, 0x00])),
  /not valid UTF-8/
);

const baseSha = sha("a");
const headSha = sha("b");
const mergeBaseSha = sha("c");
const pullCalls = [];
const pull = collectChangedPaths({ eventName: "pull_request", baseSha, headSha }, (args) => {
  pullCalls.push(args);
  return args[0] === "merge-base" ? Buffer.from(`${mergeBaseSha}\n`) : Buffer.from("M\0README.md\0");
});
assert.deepEqual(pull.paths, ["README.md"]);
assert.deepEqual(pullCalls, [
  ["merge-base", baseSha, headSha],
  [
    "diff",
    "--name-status",
    "-z",
    "--find-renames",
    "--find-copies-harder",
    mergeBaseSha,
    headSha,
    "--",
  ],
]);

const pushCalls = [];
const push = collectChangedPaths({ eventName: "push", beforeSha: baseSha, headSha }, (args) => {
  pushCalls.push(args);
  return Buffer.from("D\0AGENTS.md\0");
});
assert.deepEqual(push.paths, ["AGENTS.md"]);
assert.deepEqual(pushCalls, [
  [
    "diff",
    "--name-status",
    "-z",
    "--find-renames",
    "--find-copies-harder",
    baseSha,
    headSha,
    "--",
  ],
]);

assert.deepEqual(collectChangedPaths({ eventName: "workflow_dispatch" }), {
  forceFull: true,
  reason: "manual-dispatch",
  paths: [],
});
assert.deepEqual(collectChangedPaths({ eventName: "schedule" }), {
  forceFull: true,
  reason: "unsupported-event",
  paths: [],
});
assert.throws(
  () => collectChangedPaths({ eventName: "push", beforeSha: sha("0"), headSha }),
  /before SHA/
);
assert.throws(
  () => collectChangedPaths({ eventName: "push", beforeSha: baseSha, headSha: "not-a-sha" }),
  /head SHA/
);
assert.throws(
  () =>
    collectChangedPaths({ eventName: "pull_request", baseSha, headSha }, () => Buffer.from("bad\n")),
  /merge base/
);

for (const [options, runGit, expectedReason] of [
  [{ eventName: "workflow_dispatch", policyPath }, undefined, "manual-dispatch"],
  [{ eventName: "schedule", policyPath }, undefined, "unsupported-event"],
  [
    { eventName: "push", beforeSha: baseSha, headSha, policyPath },
    () => Buffer.alloc(0),
    "empty-diff",
  ],
]) {
  const result = runClassifier(options, runGit);
  assert.equal(result.scope, "full");
  assert.equal(result.fullCi, true);
  assert.equal(result.docsChecks, false);
  assert.equal(result.reason, expectedReason);
}

for (const [options, runGit] of [
  [{ eventName: "push", beforeSha: sha("0"), headSha, policyPath }, undefined],
  [
    { eventName: "push", beforeSha: baseSha, headSha, policyPath },
    () => {
      throw new Error("injected git failure");
    },
  ],
  [
    { eventName: "push", beforeSha: baseSha, headSha, policyPath },
    () => Buffer.from("M\0README.md"),
  ],
  [{ eventName: "push", beforeSha: baseSha, headSha, policyPath: `${policyPath}.missing` }, undefined],
]) {
  const result = runClassifier(options, runGit);
  assert.equal(result.scope, "full");
  assert.equal(result.fullCi, true);
  assert.equal(result.reason, "classification-error");
  assert.equal(typeof result.error, "string");
  assert.ok(result.error.length > 0);
}

assert.throws(() => validatePolicy({ ...policy, version: 2 }), /version must be 1/);
assert.throws(() => validatePolicy({ ...policy, extra: true }), /unsupported or missing fields/);
const duplicateExact = structuredClone(policy);
duplicateExact.processDocumentation.exactPaths.push("AGENTS.md");
assert.throws(() => validatePolicy(duplicateExact), /invalid or duplicate exact path/);
const invalidPrefix = structuredClone(policy);
invalidPrefix.checkedDocumentation.prefixRules[0].prefix = "docs";
assert.throws(() => validatePolicy(invalidPrefix), /invalid or duplicate prefix/);
const duplicateExtension = structuredClone(policy);
duplicateExtension.checkedDocumentation.prefixRules[0].extensions.push(".md");
assert.throws(() => validatePolicy(duplicateExtension), /invalid or duplicate extension/);
const unknownRuleField = structuredClone(policy);
unknownRuleField.checkedDocumentation.extra = [];
assert.throws(() => validatePolicy(unknownRuleField), /unsupported or missing fields/);

const tempDirectory = mkdtempSync(join(tmpdir(), "ci-change-scope-"));
try {
  const outputPath = join(tempDirectory, "github-output.txt");
  const cli = spawnSync(
    process.execPath,
    [classifierPath, "--event", "workflow_dispatch", "--policy", policyPath],
    {
      encoding: "utf8",
      env: { ...process.env, GITHUB_OUTPUT: outputPath },
    }
  );
  assert.equal(cli.status, 0, cli.stderr);
  assert.match(cli.stdout, /"scope": "full"/);
  assert.equal(
    readFileSync(outputPath, "utf8"),
    "scope=full\nfull_ci=true\ndocs_checks=false\nreason=manual-dispatch\n"
  );

  const failClosedOutputPath = join(tempDirectory, "github-output-fail-closed.txt");
  const failClosedCli = spawnSync(process.execPath, [classifierPath, "--unknown", "value"], {
    encoding: "utf8",
    env: { ...process.env, GITHUB_OUTPUT: failClosedOutputPath },
  });
  assert.equal(failClosedCli.status, 0, failClosedCli.stderr);
  assert.equal(
    readFileSync(failClosedOutputPath, "utf8"),
    "scope=full\nfull_ci=true\ndocs_checks=false\nreason=classification-error\n"
  );
} finally {
  rmSync(tempDirectory, { recursive: true, force: true });
}

console.log("CI change-scope self-test passed.");
