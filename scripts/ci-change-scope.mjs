import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { TextDecoder } from "node:util";
import { fileURLToPath } from "node:url";

const DEFAULT_POLICY_PATH = ".github/ci-scope.json";
const POLICY_KEYS = ["checkedDocumentation", "processDocumentation", "version"];
const RULE_SET_KEYS = ["exactPaths", "prefixRules"];
const PREFIX_RULE_KEYS = ["extensions", "prefix"];
const SHA_PATTERN = /^[0-9a-f]{40,64}$/i;
const UNSAFE_PATH_PATTERN = /[\\\u0000-\u001f\u007f]/;
const CONTROL_PLANE_EXACT_PATHS = new Set([
  "scripts/ci-change-scope.mjs",
  "scripts/ci-change-scope.selftest.mjs",
  "scripts/ci-gate.mjs",
  "scripts/ci-workflow-contract.selftest.mjs",
]);
const CONTROL_PLANE_PREFIXES = [".github/"];
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

function assertObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
}

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} contains unsupported or missing fields`);
  }
}

export function isSafeRepositoryPath(path) {
  if (
    typeof path !== "string" ||
    path.length === 0 ||
    path.startsWith("/") ||
    /^[A-Za-z]:/.test(path)
  ) {
    return false;
  }
  if (UNSAFE_PATH_PATTERN.test(path)) {
    return false;
  }
  return path.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

function validateRuleSet(ruleSet, label) {
  assertObject(ruleSet, label);
  assertExactKeys(ruleSet, RULE_SET_KEYS, label);
  if (!Array.isArray(ruleSet.exactPaths) || !Array.isArray(ruleSet.prefixRules)) {
    throw new Error(`${label} exactPaths and prefixRules must be arrays`);
  }

  const exactPaths = new Set();
  for (const path of ruleSet.exactPaths) {
    if (!isSafeRepositoryPath(path) || exactPaths.has(path)) {
      throw new Error(`${label} contains an invalid or duplicate exact path`);
    }
    exactPaths.add(path);
  }

  const prefixes = new Set();
  for (const rule of ruleSet.prefixRules) {
    assertObject(rule, `${label} prefix rule`);
    assertExactKeys(rule, PREFIX_RULE_KEYS, `${label} prefix rule`);
    if (
      typeof rule.prefix !== "string" ||
      !rule.prefix.endsWith("/") ||
      !isSafeRepositoryPath(rule.prefix.slice(0, -1)) ||
      prefixes.has(rule.prefix)
    ) {
      throw new Error(`${label} contains an invalid or duplicate prefix`);
    }
    if (!Array.isArray(rule.extensions) || rule.extensions.length === 0) {
      throw new Error(`${label} prefix extensions must be a non-empty array`);
    }

    const extensions = new Set();
    for (const extension of rule.extensions) {
      if (
        typeof extension !== "string" ||
        !/^\.[a-z0-9]+$/.test(extension) ||
        extensions.has(extension)
      ) {
        throw new Error(`${label} contains an invalid or duplicate extension`);
      }
      extensions.add(extension);
    }
    prefixes.add(rule.prefix);
  }
}

export function validatePolicy(policy) {
  assertObject(policy, "CI scope policy");
  assertExactKeys(policy, POLICY_KEYS, "CI scope policy");
  if (policy.version !== 1) {
    throw new Error("CI scope policy version must be 1");
  }
  validateRuleSet(policy.processDocumentation, "processDocumentation");
  validateRuleSet(policy.checkedDocumentation, "checkedDocumentation");
  return policy;
}

export function loadPolicy(policyPath = DEFAULT_POLICY_PATH) {
  return validatePolicy(JSON.parse(readFileSync(policyPath, "utf8")));
}

function matchesRuleSet(path, ruleSet) {
  return (
    ruleSet.exactPaths.includes(path) ||
    ruleSet.prefixRules.some(
      (rule) =>
        path.startsWith(rule.prefix) && rule.extensions.some((extension) => path.endsWith(extension))
    )
  );
}

export function classifyPath(path, policy) {
  if (!isSafeRepositoryPath(path)) {
    return { path, tier: "full", reason: "unsafe-path" };
  }
  if (
    CONTROL_PLANE_EXACT_PATHS.has(path) ||
    CONTROL_PLANE_PREFIXES.some((prefix) => path.startsWith(prefix))
  ) {
    return { path, tier: "full", reason: "ci-control-plane" };
  }

  const processDocumentation = matchesRuleSet(path, policy.processDocumentation);
  const checkedDocumentation = matchesRuleSet(path, policy.checkedDocumentation);
  if (processDocumentation && checkedDocumentation) {
    return { path, tier: "full", reason: "ambiguous-policy" };
  }
  if (checkedDocumentation) {
    return { path, tier: "checked-docs", reason: "checked-documentation" };
  }
  if (processDocumentation) {
    return { path, tier: "process-docs", reason: "process-documentation" };
  }
  return { path, tier: "full", reason: "unclassified-path" };
}

export function fullCiResult(reason, error = undefined) {
  return {
    scope: "full",
    fullCi: true,
    docsChecks: false,
    reason,
    classifications: [],
    ...(error ? { error } : {}),
  };
}

export function classifyPaths(paths, policy) {
  const uniquePaths = [...new Set(paths)];
  if (uniquePaths.length === 0) {
    return fullCiResult("empty-diff");
  }

  const classifications = uniquePaths.map((path) => classifyPath(path, policy));
  const tiers = new Set(classifications.map(({ tier }) => tier));
  const mixedTiers = tiers.size > 1;
  const fullCi = mixedTiers || tiers.has("full");
  const docsChecks = classifications.some(({ tier }) => tier === "checked-docs");
  return {
    scope: fullCi ? "full" : docsChecks ? "checked-docs" : "process-docs",
    fullCi,
    docsChecks,
    reason: mixedTiers
      ? "mixed-tiers"
      : fullCi
        ? "full-ci-path"
      : docsChecks
        ? "checked-documentation"
        : "process-documentation",
    classifications,
  };
}

function decodeUtf8(value, label) {
  if (Buffer.isBuffer(value) || value instanceof Uint8Array) {
    try {
      return UTF8_DECODER.decode(value);
    } catch {
      throw new Error(`${label} is not valid UTF-8`);
    }
  }
  if (typeof value !== "string") {
    throw new Error(`${label} must be text or bytes`);
  }
  return value;
}

export function parseNameStatus(output) {
  if (
    (typeof output === "string" && output === "") ||
    ((Buffer.isBuffer(output) || output instanceof Uint8Array) && output.length === 0)
  ) {
    return [];
  }

  const text = decodeUtf8(output, "git diff name-status output");
  if (!text.endsWith("\0")) {
    throw new Error("git diff name-status output is not NUL terminated");
  }

  const fields = text.split("\0");
  fields.pop();
  const records = [];
  for (let index = 0; index < fields.length; ) {
    const status = fields[index++];
    const renameOrCopy = /^([RC])(\d{1,3})$/.exec(status);
    if (renameOrCopy) {
      if (Number(renameOrCopy[2]) > 100 || index + 1 >= fields.length) {
        throw new Error(`invalid ${status} name-status record`);
      }
      records.push({ status, paths: [fields[index++], fields[index++]] });
      continue;
    }
    if (!/^[ADMTU]$/.test(status) || index >= fields.length) {
      throw new Error(`invalid ${status || "empty"} name-status record`);
    }
    records.push({ status, paths: [fields[index++]] });
  }
  return records;
}

export function classifyNameStatus(output, policy) {
  return classifyPaths(
    parseNameStatus(output).flatMap(({ paths }) => paths),
    policy
  );
}

function assertUsableSha(sha, label) {
  if (!SHA_PATTERN.test(sha || "") || /^0+$/.test(sha)) {
    throw new Error(`${label} is not a usable Git object ID`);
  }
}

function runGitCommand(args) {
  return execFileSync("git", args, {
    maxBuffer: 32 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

export function collectChangedPaths(
  { eventName, baseSha = "", headSha = "", beforeSha = "" },
  runGit = runGitCommand
) {
  if (eventName === "workflow_dispatch") {
    return { forceFull: true, reason: "manual-dispatch", paths: [] };
  }

  let fromSha;
  if (eventName === "pull_request") {
    assertUsableSha(baseSha, "pull request base SHA");
    assertUsableSha(headSha, "pull request head SHA");
    fromSha = decodeUtf8(runGit(["merge-base", baseSha, headSha]), "git merge-base output").trim();
    assertUsableSha(fromSha, "pull request merge base");
  } else if (eventName === "push") {
    assertUsableSha(beforeSha, "push before SHA");
    assertUsableSha(headSha, "push head SHA");
    fromSha = beforeSha;
  } else {
    return { forceFull: true, reason: "unsupported-event", paths: [] };
  }

  const output = runGit([
    "diff",
    "--name-status",
    "-z",
    "--find-renames",
    "--find-copies-harder",
    fromSha,
    headSha,
    "--",
  ]);
  return {
    forceFull: false,
    reason: "classified-diff",
    paths: parseNameStatus(output).flatMap(({ paths }) => paths),
  };
}

function conciseError(error) {
  const message = error instanceof Error ? error.message : String(error);
  return message.split("\n", 1)[0].slice(0, 300);
}

export function runClassifier(options, runGit = runGitCommand) {
  try {
    const policy = loadPolicy(options.policyPath || DEFAULT_POLICY_PATH);
    const changed = collectChangedPaths(options, runGit);
    if (changed.forceFull) {
      return fullCiResult(changed.reason);
    }
    return classifyPaths(changed.paths, policy);
  } catch (error) {
    return fullCiResult("classification-error", conciseError(error));
  }
}

function parseCliOptions(argv) {
  const options = {
    eventName: process.env.GITHUB_EVENT_NAME || "",
    baseSha: process.env.CI_BASE_SHA || "",
    headSha: process.env.CI_HEAD_SHA || "",
    beforeSha: process.env.CI_BEFORE_SHA || "",
    policyPath: process.env.CI_SCOPE_POLICY || DEFAULT_POLICY_PATH,
  };
  const fields = new Map([
    ["--event", "eventName"],
    ["--base", "baseSha"],
    ["--head", "headSha"],
    ["--before", "beforeSha"],
    ["--policy", "policyPath"],
  ]);
  for (let index = 0; index < argv.length; index += 2) {
    const field = fields.get(argv[index]);
    if (!field || index + 1 >= argv.length) {
      throw new Error(`unsupported or incomplete argument: ${argv[index] || "empty"}`);
    }
    options[field] = argv[index + 1];
  }
  return options;
}

function writeGitHubOutputs(result) {
  if (!process.env.GITHUB_OUTPUT) {
    return;
  }
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    [
      `scope=${result.scope}`,
      `full_ci=${String(result.fullCi)}`,
      `docs_checks=${String(result.docsChecks)}`,
      `reason=${result.reason}`,
      "",
    ].join("\n")
  );
}

function main() {
  let result;
  try {
    result = runClassifier(parseCliOptions(process.argv.slice(2)));
  } catch (error) {
    result = fullCiResult("classification-error", conciseError(error));
  }
  writeGitHubOutputs(result);
  if (result.error) {
    console.error(`CI scope classification failed closed: ${result.error}`);
  }
  console.log(JSON.stringify(result, null, 2));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
