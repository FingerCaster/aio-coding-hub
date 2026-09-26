#!/usr/bin/env node
// Probe the real native CLI RPC parser; do not substitute a model-registry import.
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile, readdir } from "node:fs/promises";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { runtimeRoot } from "./pi-omp-bootstrap.mjs";
import { cliFlags, makeSandbox, nativeConfig, runCli } from "./pi-omp-wire-capture.mjs";
import { parseEvents } from "./pi-omp-wire-extended.mjs";

function config(id = "w0-selected") {
  return nativeConfig("openai-completions", "http://127.0.0.1:9", { id });
}
function yaml(id = "w0-selected") {
  return (
    "# W0 native YAML fixture\nproviders:\n  w0-local:\n    api: openai-completions\n    auth: apiKey\n    apiKey: w0-fake-key\n    baseUrl: http://127.0.0.1:9/v1\n    models:\n      - id: " +
    id +
    "\n        contextWindow: 32768\n        maxTokens: 1024\n"
  );
}
async function snapshots(root) {
  const result = {};
  async function visit(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) await visit(path);
      else if (/models\.(json|jsonc|yaml|yml)(\.bak)?$/.test(entry.name))
        result[path.slice(root.length + 1)] = createHash("sha256")
          .update(await readFile(path))
          .digest("hex");
    }
  }
  await visit(root);
  return result;
}
const cases = [
  ...[
    "json",
    "jsonc-bom-comments",
    "block-comment",
    "trailing-comma",
    "single-quotes",
    "malformed",
    "duplicate-key",
    "default-dir",
  ].map((name) => ({ cli: "pi", name })),
  ...[
    "yaml",
    "yaml-fallback",
    "yml-priority",
    "malformed-yml-no-fallback",
    "duplicate-key",
    "alias-merge",
    "legacy-json",
    "default-dir",
    "config-dir",
    "named-profile",
    "profile-env-precedence",
    "profile-cli-precedence",
    "profile-default-override",
    "profile-empty-env",
    "invalid-profile",
  ].map((name) => ({ cli: "omp", name })),
];

async function runCase({ cli, name }, runDir) {
  const sandbox = await makeSandbox(runDir, cli + "-" + name);
  const env = sandbox.env;
  let agent = env.PI_CODING_AGENT_DIR;
  let expected = ["w0-selected"];
  let expectedMutation = false;
  const extraArgs = [];
  const file = cli === "pi" ? "models.json" : "models.yml";
  const writes = [];
  const add = (path, content) => writes.push([path, content]);
  const addConfig = (dir, id, filename = file) =>
    add(join(dir, filename), JSON.stringify(config(id)));
  if (name === "default-dir") {
    delete env.PI_CODING_AGENT_DIR;
    agent = join(env.HOME, cli === "pi" ? ".pi/agent" : ".omp/agent");
  }
  if (name === "config-dir") {
    delete env.PI_CODING_AGENT_DIR;
    env.PI_CONFIG_DIR = ".w0-omp";
    agent = join(env.HOME, ".w0-omp/agent");
  }
  if (name.includes("profile")) {
    for (const profile of ["blue", "green", "red"])
      addConfig(join(env.HOME, ".omp/profiles", profile, "agent"), "w0-" + profile);
    addConfig(agent, "w0-override");
    addConfig(join(env.HOME, ".omp/agent"), "w0-default");
    env.OMP_PROFILE = "blue";
    expected = ["w0-blue"];
    if (name === "named-profile") extraArgs.push("--profile", "blue");
    if (name === "profile-env-precedence") env.PI_PROFILE = "red";
    if (name === "profile-cli-precedence") {
      extraArgs.push("--profile", "green");
      expected = ["w0-green"];
    }
    if (name === "profile-default-override") {
      extraArgs.push("--profile", "default");
      expected = ["w0-override"];
    }
    if (name === "profile-empty-env") {
      env.OMP_PROFILE = "";
      env.PI_PROFILE = "blue";
      expected = ["w0-override"];
    }
    if (name === "invalid-profile") {
      extraArgs.push("--profile", "../escape");
      expected = [];
    }
  } else if (cli === "pi") {
    let text = JSON.stringify(config(), null, 2);
    if (name === "jsonc-bom-comments")
      text =
        "\ufeff// W0 comment\n" + text.replace('"providers":', '// inline comment\n"providers":');
    if (name === "block-comment") {
      text = "/* W0 block comment */" + text;
      expected = [];
    }
    if (name === "trailing-comma") text = text.slice(0, -1) + ",}";
    if (name === "single-quotes") {
      text = text.replaceAll('"', "'");
      expected = [];
    }
    if (name === "malformed") {
      text = "{ providers: [";
      expected = [];
    }
    if (name === "duplicate-key")
      text = '{"providers":{},"providers":' + JSON.stringify(config().providers) + "}";
    add(join(agent, file), text);
  } else {
    if (name === "yaml-fallback") add(join(agent, "models.yaml"), yaml());
    else if (name === "yml-priority" || name === "malformed-yml-no-fallback") {
      add(join(agent, "models.yml"), name === "yml-priority" ? yaml() : "providers: [");
      add(join(agent, "models.yaml"), yaml("w0-shadowed"));
      if (name === "malformed-yml-no-fallback") expected = [];
    } else if (name === "duplicate-key") add(join(agent, file), "providers: {}\n" + yaml());
    else if (name === "alias-merge")
      add(
        join(agent, file),
        "defaults: &defaults\n  api: openai-completions\n  apiKey: w0-fake-key\n  baseUrl: http://127.0.0.1:9/v1\nproviders:\n  w0-local:\n    <<: *defaults\n    models:\n      - id: w0-selected\n"
      );
    else if (name === "legacy-json") {
      add(join(agent, "models.json"), JSON.stringify(config()));
      expectedMutation = true;
    } else add(join(agent, file), yaml());
  }
  for (const [path, content] of writes) {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
  const before = await snapshots(env.HOME);
  let buffered = "";
  let closed = false;
  const result = await runCli(
    cli,
    [...cliFlags(cli), "--no-tools", ...extraArgs, "--mode", "rpc"],
    env,
    sandbox.cwd,
    {
      keepStdin: true,
      onChild(child) {
        child.stdin.write(JSON.stringify({ id: "w0-models", type: "get_available_models" }) + "\n");
      },
      onStdout(chunk, child) {
        buffered += chunk;
        if (
          !closed &&
          parseEvents(buffered).some((e) => e.id === "w0-models" && e.type === "response")
        ) {
          closed = true;
          child.stdin.end();
        }
      },
    }
  );
  const response = parseEvents(result.stdout).find(
    (e) => e.id === "w0-models" && e.type === "response"
  );
  const observed = (response?.data?.models || [])
    .filter((m) => m.provider === "w0-local")
    .map((m) => m.id)
    .sort();
  const after = await snapshots(env.HOME);
  const unchanged = JSON.stringify(before) === JSON.stringify(after);
  const diagnosticVisible = /error|invalid|failed|parse|Unexpected|Duplicate/i.test(
    result.stderr + result.stdout
  );
  const checks = [
    { name: "bounded-process", passed: !result.timedOut && !result.spawnError },
    {
      name: "selected-models",
      passed: JSON.stringify(observed) === JSON.stringify(expected.sort()),
    },
    { name: "configuration-mutation", passed: expectedMutation ? !unchanged : unchanged },
    // Pi's native RPC currently hides invalid-file errors; lock the observation, do not call it valid empty config.
    {
      name: "native-diagnostic-contract",
      passed: expected.length
        ? response?.success === true
        : cli === "pi"
          ? response?.success === true && !diagnosticVisible
          : diagnosticVisible,
    },
  ];
  const evidence = {
    cli,
    name,
    expected,
    observed,
    before,
    after,
    diagnosticVisible,
    knownNativeSilentEmpty: cli === "pi" && expected.length === 0,
    process: result,
    checks,
    passed: checks.every((c) => c.passed),
  };
  await writeFile(join(sandbox.root, "evidence.json"), JSON.stringify(evidence, null, 2));
  return evidence;
}
async function main() {
  const runDir = join(
    runtimeRoot,
    "runs",
    "config-" + new Date().toISOString().replace(/[:.]/g, "-")
  );
  await mkdir(runDir, { recursive: true });
  const results = [];
  for (const item of cases) {
    const result = await runCase(item, runDir);
    results.push(result);
    console.log(
      (result.passed ? "PASS" : "FAIL") +
        " " +
        item.cli +
        " " +
        item.name +
        ": observed=" +
        result.observed.join(",") +
        " exit=" +
        result.process.code
    );
    if (!result.passed)
      console.log(
        JSON.stringify({
          checks: result.checks,
          stderr: result.process.stderr.slice(-1800),
          output: result.process.stdout.slice(-700),
        })
      );
    await writeFile(join(runDir, "summary.json"), JSON.stringify(results, null, 2));
  }
  console.log("Evidence: " + runDir);
  if (results.some((r) => !r.passed)) process.exitCode = 1;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
