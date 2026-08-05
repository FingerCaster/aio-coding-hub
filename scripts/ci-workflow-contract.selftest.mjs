import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { assertCiGate } from "./ci-gate.mjs";

const workflowPath = fileURLToPath(new URL("../.github/workflows/ci.yml", import.meta.url));
const workflow = readFileSync(workflowPath, "utf8").replace(/\r\n/g, "\n");
const EXPECTED_JOBS = [
  "change-scope",
  "pr-title",
  "docs-contract",
  "support-contract",
  "desktop-support-contract",
  "frontend",
  "rust",
  "ci-gate",
];
const FULL_JOBS = ["support-contract", "desktop-support-contract", "frontend", "rust"];
const GATE_NEEDS = ["change-scope", "pr-title", "docs-contract", ...FULL_JOBS];

function indentation(line) {
  return line.match(/^ */)[0].length;
}

function extractBlock(lines, headerIndex, headerIndent) {
  let end = headerIndex + 1;
  while (
    end < lines.length &&
    (lines[end].trim() === "" || indentation(lines[end]) > headerIndent)
  ) {
    end += 1;
  }
  return lines.slice(headerIndex + 1, end);
}

function extractJobs(source) {
  const lines = source.split("\n");
  const jobsIndex = lines.findIndex((line) => line === "jobs:");
  if (jobsIndex === -1) {
    throw new Error("workflow is missing top-level jobs");
  }
  const jobsBlock = extractBlock(lines, jobsIndex, 0);
  const jobs = new Map();
  for (let index = 0; index < jobsBlock.length; index += 1) {
    const match = /^  ([a-z0-9-]+):\s*$/.exec(jobsBlock[index]);
    if (!match) continue;
    jobs.set(match[1], extractBlock(jobsBlock, index, 2));
  }
  return jobs;
}

function property(job, key) {
  const matches = job
    .map((line) => new RegExp(`^    ${key}:\\s*(.*?)\\s*$`).exec(line))
    .filter(Boolean);
  if (matches.length !== 1 || matches[0][1] === "") {
    throw new Error(`job property ${key} must appear exactly once with a scalar value`);
  }
  return matches[0][1];
}

function needs(job) {
  const index = job.findIndex((line) => /^    needs:\s*/.test(line));
  if (index === -1) return [];
  const inline = job[index].replace(/^    needs:\s*/, "").trim();
  if (inline !== "") {
    if (inline.startsWith("[") && inline.endsWith("]")) {
      return inline
        .slice(1, -1)
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean);
    }
    return [inline];
  }

  const result = [];
  for (let cursor = index + 1; cursor < job.length; cursor += 1) {
    if (job[cursor].trim() === "") continue;
    if (indentation(job[cursor]) <= 4) break;
    const match = /^      - ([a-z0-9-]+)\s*$/.exec(job[cursor]);
    if (!match) throw new Error("needs block contains a non-job entry");
    result.push(match[1]);
  }
  return result;
}

function mapping(job, key) {
  const index = job.findIndex((line) => line === `    ${key}:`);
  if (index === -1) throw new Error(`job is missing ${key} mapping`);
  const result = new Map();
  for (let cursor = index + 1; cursor < job.length; cursor += 1) {
    if (job[cursor].trim() === "") continue;
    if (indentation(job[cursor]) <= 4) break;
    const match = /^      ([a-z0-9_]+):\s+(.+?)\s*$/.exec(job[cursor]);
    if (!match) throw new Error(`${key} contains an invalid entry`);
    result.set(match[1], match[2]);
  }
  return result;
}

function assertSetEqual(actual, expected, label) {
  assert.deepEqual([...actual].sort(), [...expected].sort(), label);
}

function requireText(text, expected, label) {
  assert.ok(text.includes(expected), `${label}: missing ${expected}`);
}

function requireLine(lines, expected, label) {
  assert.ok(lines.includes(expected), label + ": missing exact line " + expected);
}

function runLines(job) {
  const result = [];
  for (let index = 0; index < job.length; index += 1) {
    const match = /^        run:\s*(.*?)\s*$/.exec(job[index]);
    if (!match) continue;
    if (match[1] !== "|" && match[1] !== ">-" && match[1] !== ">") {
      result.push(match[1]);
      continue;
    }
    for (let cursor = index + 1; cursor < job.length; cursor += 1) {
      if (job[cursor].trim() === "") continue;
      if (indentation(job[cursor]) <= 8) break;
      const line = job[cursor].trim();
      if (!line.startsWith("#")) {
        result.push(line);
      }
    }
  }
  return result;
}

function requireRunLine(job, expected, label) {
  requireLine(runLines(job), expected, label);
}

export function assertWorkflowContract(source) {
  assert.equal(source.includes("\t"), false, "workflow must not contain tabs");
  requireText(
    source,
    "on:\n  push:\n    branches: [dev, main]\n  pull_request:\n    branches: [dev, main]",
    "workflow push/pull_request triggers"
  );
  requireText(source, "permissions:\n  contents: read", "workflow permissions");
  requireText(
    source,
    "group: ci-${{ github.event.pull_request.number || github.ref }}",
    "workflow concurrency group"
  );
  requireText(source, "cancel-in-progress: true", "workflow cancellation");
  const jobs = extractJobs(source);
  assertSetEqual(jobs.keys(), EXPECTED_JOBS, "workflow job IDs");

  const changeScope = jobs.get("change-scope");
  assertSetEqual(needs(changeScope), [], "change-scope needs");
  const outputs = mapping(changeScope, "outputs");
  assert.deepEqual(Object.fromEntries(outputs), {
    scope: "${{ steps.scope.outputs.scope }}",
    full_ci: "${{ steps.scope.outputs.full_ci }}",
    docs_checks: "${{ steps.scope.outputs.docs_checks }}",
    reason: "${{ steps.scope.outputs.reason }}",
  });
  const changeScopeText = changeScope.join("\n");
  requireRunLine(
    changeScope,
    "node scripts/ci-change-scope.selftest.mjs",
    "change-scope classifier self-test"
  );
  requireRunLine(
    changeScope,
    "node scripts/ci-workflow-contract.selftest.mjs",
    "change-scope workflow self-test"
  );
  requireRunLine(changeScope, "node scripts/ci-change-scope.mjs", "change-scope classifier");
  requireText(changeScopeText, "fetch-depth: 0", "change-scope history");
  requireText(changeScopeText, "persist-credentials: false", "change-scope credentials");

  const prTitle = jobs.get("pr-title");
  assert.equal(property(prTitle, "if"), "github.event_name == 'pull_request'");
  assertSetEqual(needs(prTitle), [], "pr-title needs");
  requireText(
    prTitle.join("\n"),
    "PR_TITLE: ${{ github.event.pull_request.title }}",
    "pr-title environment"
  );
  requireText(prTitle.join("\n"), 'title="$PR_TITLE"', "pr-title shell input");

  const docsContract = jobs.get("docs-contract");
  assertSetEqual(needs(docsContract), ["change-scope"], "docs-contract needs");
  assert.equal(property(docsContract, "if"), "needs.change-scope.outputs.docs_checks == 'true'");
  for (const command of [
    "node scripts/check-plugin-system-docs.mjs",
    "node scripts/check-plugin-api-contract.mjs",
    "node scripts/check-spec-links.mjs",
  ]) {
    requireRunLine(docsContract, command, "docs-contract command");
  }
  for (const forbidden of ["pnpm ", "cargo ", "check-tui", "candidate-plan"]) {
    assert.equal(
      docsContract.join("\n").includes(forbidden),
      false,
      `docs-contract must not include ${forbidden}`
    );
  }

  for (const jobId of FULL_JOBS) {
    const job = jobs.get(jobId);
    const expectedNeeds =
      jobId === "support-contract" ? ["change-scope"] : ["change-scope", "support-contract"];
    assertSetEqual(needs(job), expectedNeeds, `${jobId} needs`);
    assert.equal(property(job, "if"), "needs.change-scope.outputs.full_ci == 'true'", jobId);
  }

  const supportContract = jobs.get("support-contract");
  requireRunLine(
    supportContract,
    'echo "desktop_matrix=$(node scripts/support-matrix.mjs ci-matrix)" >> "$GITHUB_OUTPUT"',
    "support-contract matrix command"
  );
  for (const command of [
    "node scripts/support-matrix.mjs check",
    "node scripts/support-matrix.homebrew-cask.selftest.mjs",
    "node scripts/check-sync-upstream-policy.selftest.mjs",
    "node scripts/check-sync-upstream-policy.mjs",
  ]) {
    requireRunLine(supportContract, command, "support-contract command");
  }

  const desktopContract = jobs.get("desktop-support-contract");
  const desktopText = desktopContract.join("\n");
  requireText(
    desktopText,
    "include: ${{ fromJson(needs.support-contract.outputs.desktop_matrix) }}",
    "desktop matrix source"
  );
  requireLine(desktopContract, "      fail-fast: false", "desktop matrix fail-fast");
  requireRunLine(desktopContract, "pnpm check:support-matrix", "desktop matrix command");

  const frontend = jobs.get("frontend");
  for (const systemCommand of [
    "sudo apt-get update",
    "sudo apt-get install -y libasound2-dev librsvg2-dev patchelf",
    "if ! sudo apt-get install -y libayatana-appindicator3-dev; then",
    "sudo apt-get install -y libappindicator3-dev",
    "if ! sudo apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev; then",
    "sudo apt-get install -y libwebkit2gtk-4.0-dev libsoup2.4-dev",
  ]) {
    requireRunLine(frontend, systemCommand, "frontend system dependency");
  }
  for (const command of [
    "pnpm install --frozen-lockfile",
    "pnpm check:support-matrix",
    "pnpm audit:deps",
    "pnpm lint",
    "pnpm check:gateway-error-codes",
    "pnpm check:plugin-system-docs",
    "pnpm check:plugin-api-contract",
    "pnpm check:generated-bindings",
    "pnpm plugin-sdk:typecheck",
    "pnpm plugin-sdk:test",
    "pnpm --filter create-aio-plugin test",
    "pnpm test:e2e",
    "pnpm test:unit:coverage",
    "pnpm build",
  ]) {
    requireRunLine(frontend, command, "frontend command");
  }

  const rust = jobs.get("rust");
  assert.equal(property(rust, "runs-on"), "ubuntu-22.04", "rust runner");
  requireLine(rust, "      - name: Install Rust 1.90.0", "rust toolchain step");
  requireLine(rust, "          toolchain: 1.90.0", "rust toolchain pin");
  requireLine(rust, "          components: rustfmt, clippy", "rust toolchain components");
  for (const systemCommand of [
    "sudo apt-get update",
    "sudo apt-get install -y libasound2-dev librsvg2-dev patchelf",
    "if ! sudo apt-get install -y libayatana-appindicator3-dev; then",
    "sudo apt-get install -y libappindicator3-dev",
    "if ! sudo apt-get install -y libwebkit2gtk-4.1-dev libsoup-3.0-dev; then",
    "sudo apt-get install -y libwebkit2gtk-4.0-dev libsoup2.4-dev",
  ]) {
    requireRunLine(rust, systemCommand, "rust system dependency");
  }
  for (const command of [
    "cargo fmt -- --check",
    "cargo update --workspace",
    "if git diff --quiet -- src-tauri/Cargo.lock; then",
    "cargo clippy --all-targets --locked -- -D warnings",
    "cargo test --locked -- --test-threads=1",
    "cargo install cargo-audit --locked",
    "cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195",
  ]) {
    requireRunLine(rust, command, "rust command");
  }

  const gate = jobs.get("ci-gate");
  assert.equal(property(gate, "name"), "ci-gate");
  assert.equal(property(gate, "if"), "always()");
  assertSetEqual(needs(gate), GATE_NEEDS, "ci-gate needs");
  const gateText = gate.join("\n");
  for (const binding of [
    "EVENT_NAME: ${{ github.event_name }}",
    "CHANGE_SCOPE_RESULT: ${{ needs.change-scope.result }}",
    "SCOPE: ${{ needs.change-scope.outputs.scope }}",
    "FULL_CI: ${{ needs.change-scope.outputs.full_ci }}",
    "DOCS_CHECKS: ${{ needs.change-scope.outputs.docs_checks }}",
    "SCOPE_REASON: ${{ needs.change-scope.outputs.reason }}",
    "PR_TITLE_RESULT: ${{ needs.pr-title.result }}",
    "DOCS_RESULT: ${{ needs.docs-contract.result }}",
    "SUPPORT_RESULT: ${{ needs.support-contract.result }}",
    "DESKTOP_RESULT: ${{ needs.desktop-support-contract.result }}",
    "FRONTEND_RESULT: ${{ needs.frontend.result }}",
    "RUST_RESULT: ${{ needs.rust.result }}",
  ]) {
    requireText(gateText, binding, "ci-gate environment");
  }
  requireRunLine(gate, "node scripts/ci-gate.mjs", "ci-gate command");
}

assert.doesNotThrow(() => assertWorkflowContract(workflow));

for (const [name, from, to] of [
  [
    "scope output",
    "scope: ${{ steps.scope.outputs.scope }}",
    "scope: ${{ steps.scope.outputs.reason }}",
  ],
  [
    "docs condition",
    "needs.change-scope.outputs.docs_checks == 'true'",
    "needs.change-scope.outputs.full_ci == 'true'",
  ],
  [
    "full condition",
    "if: needs.change-scope.outputs.full_ci == 'true'",
    "if: needs.change-scope.outputs.full_ci != 'true'",
  ],
  ["gate always", "    if: always()", "    if: success()"],
  ["gate dependency", "      - desktop-support-contract", "      - missing-desktop"],
  [
    "gate result binding",
    "DESKTOP_RESULT: ${{ needs.desktop-support-contract.result }}",
    "DESKTOP_RESULT: ${{ needs.support-contract.result }}",
  ],
  [
    "workflow self-test",
    "node scripts/ci-workflow-contract.selftest.mjs",
    "node scripts/missing-workflow-selftest.mjs",
  ],
  [
    "fork sync policy",
    "node scripts/check-sync-upstream-policy.selftest.mjs",
    "node scripts/missing-sync-policy-selftest.mjs",
  ],
  [
    "comment-spoofed frontend command",
    "        run: pnpm lint",
    "        run: true # run: pnpm lint",
  ],
  ["desktop fail-fast", "      fail-fast: false", "      fail-fast: true"],
  ["rust toolchain pin", "          toolchain: 1.90.0", "          toolchain: nightly"],
  [
    "frontend system dependencies",
    "          sudo apt-get install -y libasound2-dev librsvg2-dev patchelf",
    "          true # sudo apt-get install -y libasound2-dev librsvg2-dev patchelf",
  ],
]) {
  assert.ok(workflow.includes(from), `${name}: fixture source must exist`);
  assert.throws(() => assertWorkflowContract(workflow.replace(from, to)), undefined, name);
}

function expectedInput({ eventName, scope, docsChecks }) {
  const fullCi = scope === "full";
  const fullResult = fullCi ? "success" : "skipped";
  return {
    eventName,
    changeScopeResult: "success",
    scope,
    fullCi: String(fullCi),
    docsChecks: String(docsChecks),
    scopeReason: "self-test",
    prTitleResult: eventName === "pull_request" ? "success" : "skipped",
    docsResult: docsChecks ? "success" : "skipped",
    supportResult: fullResult,
    desktopResult: fullResult,
    frontendResult: fullResult,
    rustResult: fullResult,
  };
}

const validScenarios = [];
for (const eventName of ["pull_request", "push"]) {
  validScenarios.push(
    expectedInput({ eventName, scope: "process-docs", docsChecks: false }),
    expectedInput({ eventName, scope: "checked-docs", docsChecks: true }),
    expectedInput({ eventName, scope: "full", docsChecks: false }),
    expectedInput({ eventName, scope: "full", docsChecks: true })
  );
}
for (const scenario of validScenarios) {
  assert.doesNotThrow(() => assertCiGate(scenario), JSON.stringify(scenario));

  const expectedPrTitle = scenario.eventName === "pull_request" ? "success" : "skipped";
  assert.throws(() =>
    assertCiGate({
      ...scenario,
      prTitleResult: expectedPrTitle === "success" ? "skipped" : "success",
    })
  );
  assert.throws(() => assertCiGate({ ...scenario, prTitleResult: "failure" }));
  assert.throws(() => assertCiGate({ ...scenario, prTitleResult: undefined }));

  const expectedDocs = scenario.docsChecks === "true" ? "success" : "skipped";
  assert.throws(() =>
    assertCiGate({
      ...scenario,
      docsResult: expectedDocs === "success" ? "skipped" : "success",
    })
  );
  assert.throws(() => assertCiGate({ ...scenario, docsResult: "failure" }));
  assert.throws(() => assertCiGate({ ...scenario, docsResult: undefined }));

  const expectedFull = scenario.fullCi === "true" ? "success" : "skipped";
  for (const field of ["supportResult", "desktopResult", "frontendResult", "rustResult"]) {
    assert.throws(() =>
      assertCiGate({
        ...scenario,
        [field]: expectedFull === "success" ? "skipped" : "success",
      })
    );
    assert.throws(() => assertCiGate({ ...scenario, [field]: "failure" }));
    assert.throws(() => assertCiGate({ ...scenario, [field]: "cancelled" }));
    assert.throws(() => assertCiGate({ ...scenario, [field]: undefined }));
  }
}

const processPush = expectedInput({ eventName: "push", scope: "process-docs", docsChecks: false });
for (const mutation of [
  { changeScopeResult: "failure" },
  { changeScopeResult: undefined },
  { scopeReason: "" },
  { scopeReason: "bad\nreason" },
  { eventName: "workflow_dispatch" },
  { scope: "unknown" },
  { scope: "full", fullCi: "false" },
  { docsChecks: "true" },
]) {
  assert.throws(() => assertCiGate({ ...processPush, ...mutation }));
}

console.log("CI workflow contract self-test passed.");
