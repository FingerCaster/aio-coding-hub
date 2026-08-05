import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const VALID_SCOPE_OUTPUTS = new Set([
  "full:true:false",
  "full:true:true",
  "checked-docs:false:true",
  "process-docs:false:false",
]);

function requireResult(actual, expected, job) {
  if (actual !== expected) {
    throw new Error(`Expected ${job} to be ${expected}, got ${actual || "empty"}`);
  }
}

export function assertCiGate(input) {
  requireResult(input.changeScopeResult, "success", "change-scope");
  if (
    typeof input.scopeReason !== "string" ||
    input.scopeReason.length === 0 ||
    /[\r\n]/.test(input.scopeReason)
  ) {
    throw new Error("change-scope did not provide a single-line reason");
  }

  const scopeOutputs = `${input.scope}:${input.fullCi}:${input.docsChecks}`;
  if (!VALID_SCOPE_OUTPUTS.has(scopeOutputs)) {
    throw new Error(
      `Invalid change-scope outputs: scope=${input.scope || "empty"} ` +
        `full_ci=${input.fullCi || "empty"} docs_checks=${input.docsChecks || "empty"}`
    );
  }

  if (input.eventName === "pull_request") {
    requireResult(input.prTitleResult, "success", "pr-title");
  } else if (input.eventName === "push") {
    requireResult(input.prTitleResult, "skipped", "pr-title");
  } else {
    throw new Error(`Unsupported CI event reached ci-gate: ${input.eventName || "empty"}`);
  }

  requireResult(
    input.docsResult,
    input.docsChecks === "true" ? "success" : "skipped",
    "docs-contract"
  );

  const fullExpected = input.fullCi === "true" ? "success" : "skipped";
  requireResult(input.supportResult, fullExpected, "support-contract");
  requireResult(input.desktopResult, fullExpected, "desktop-support-contract");
  requireResult(input.frontendResult, fullExpected, "frontend");
  requireResult(input.rustResult, fullExpected, "rust");
}

function inputFromEnvironment() {
  return {
    eventName: process.env.EVENT_NAME,
    changeScopeResult: process.env.CHANGE_SCOPE_RESULT,
    scope: process.env.SCOPE,
    fullCi: process.env.FULL_CI,
    docsChecks: process.env.DOCS_CHECKS,
    scopeReason: process.env.SCOPE_REASON,
    prTitleResult: process.env.PR_TITLE_RESULT,
    docsResult: process.env.DOCS_RESULT,
    supportResult: process.env.SUPPORT_RESULT,
    desktopResult: process.env.DESKTOP_RESULT,
    frontendResult: process.env.FRONTEND_RESULT,
    rustResult: process.env.RUST_RESULT,
  };
}

function main() {
  try {
    assertCiGate(inputFromEnvironment());
    console.log("CI gate result contract passed.");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`::error::${message}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
