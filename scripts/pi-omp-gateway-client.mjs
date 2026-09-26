#!/usr/bin/env node
// A real fixed-version CLI client for the Rust W6 gateway integration test.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile, realpath } from "node:fs/promises";
import { join, relative, resolve, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { repoRoot, runtimeRoot } from "./pi-omp-bootstrap.mjs";
import {
  cliFlags,
  makeSandbox,
  nativeConfig,
  probeCli,
  protocols,
  runCli,
} from "./pi-omp-wire-capture.mjs";
import { assistantMessages, parseEvents } from "./pi-omp-wire-extended.mjs";

function argumentsMap(args) {
  const result = {};
  for (let i = 0; i < args.length; i += 2) {
    assert(
      args[i].startsWith("--") && args[i + 1] && !args[i + 1].startsWith("--"),
      "Expected --name value pairs"
    );
    const key = args[i].slice(2);
    assert(
      [
        "client",
        "protocol",
        "base-origin",
        "model",
        "target",
        "expect-text",
        "entry-json",
      ].includes(key),
      "Unsupported option: " + key
    );
    assert(!(key in result), "Duplicate option: " + key);
    result[key] = args[i + 1];
  }
  return result;
}
async function runtimePath(path, label) {
  const absolute = resolve(repoRoot, path);
  const within = relative(runtimeRoot, absolute);
  assert(
    !within.startsWith("..") && !isAbsolute(within),
    label + " must remain inside .trellis/.runtime/research/omp-pi"
  );
  const canonicalRelative = relative(await realpath(runtimeRoot), await realpath(absolute));
  assert(
    !canonicalRelative.startsWith("..") && !isAbsolute(canonicalRelative),
    label + " symlink leaves runtime root"
  );
  return absolute;
}
function validateEntryTransport(value, origin, key = "") {
  if (Array.isArray(value)) {
    value.forEach((item) => validateEntryTransport(item, origin, key));
    return;
  }
  if (value && typeof value === "object") {
    for (const [name, item] of Object.entries(value)) validateEntryTransport(item, origin, name);
    return;
  }
  // Native config supports command-based secrets; generated fixtures must be inert.
  if (typeof value === "string")
    assert(
      !value.trimStart().startsWith("!"),
      "Generated entry cannot contain command-backed values"
    );
  if (key === "baseUrl") {
    const parsed = new URL(value);
    assert(
      parsed.origin === origin && !parsed.username && !parsed.password,
      "Generated entry baseUrl must use --base-origin"
    );
  }
  if (key === "auth") assert(value === "apiKey", "Generated entry must use API-key authentication");
}
export async function gatewayClient(options) {
  const cli = options.client;
  assert(cli === "pi" || cli === "omp", "--client must be pi or omp");
  assert(protocols.includes(options.protocol), "Unsupported protocol");
  const origin = new URL(options["base-origin"]);
  assert(
    origin.protocol === "http:" &&
      origin.hostname === "127.0.0.1" &&
      origin.port &&
      origin.pathname === "/" &&
      !origin.username &&
      !origin.password &&
      !origin.search &&
      !origin.hash,
    "--base-origin must be a local 127.0.0.1 HTTP origin with an explicit port"
  );
  const target = resolve(repoRoot, options.target || join(runtimeRoot, "gateway"));
  const within = relative(runtimeRoot, target);
  assert(
    !within.startsWith("..") && !isAbsolute(within),
    "--target must remain inside .trellis/.runtime/research/omp-pi"
  );
  await mkdir(target, { recursive: true });
  const canonicalTarget = await realpath(target);
  const canonicalRuntime = await realpath(runtimeRoot);
  const canonicalRelative = relative(canonicalRuntime, canonicalTarget);
  assert(
    !canonicalRelative.startsWith("..") && !isAbsolute(canonicalRelative),
    "Target symlink leaves runtime root"
  );
  const runDir = join(target, Date.now().toString(36) + "-" + process.pid);
  const sandbox = await makeSandbox(runDir, cli + "-" + options.protocol);
  const info = await probeCli(cli, sandbox.env, sandbox.cwd);
  sandbox.env.PI_OMP_FIXTURE_ORIGIN = origin.origin;
  let provider = "w0-local";
  let entryEvidence;
  let config;
  if (options["entry-json"]) {
    const entryPath = await runtimePath(options["entry-json"], "--entry-json");
    const raw = await readFile(entryPath, "utf8");
    assert(Buffer.byteLength(raw) <= 1024 * 1024, "Generated entry exceeds 1 MiB");
    const entry = JSON.parse(raw);
    assert.equal(entry.protocol, options.protocol, "Generated entry protocol mismatch");
    assert(
      typeof entry.nativeKey === "string" && /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(entry.nativeKey),
      "Invalid generated nativeKey"
    );
    assert(
      entry.node && typeof entry.node === "object" && !Array.isArray(entry.node),
      "Generated entry must contain a provider node"
    );
    validateEntryTransport(entry.node, origin.origin);
    provider = entry.nativeKey;
    // Preserve the production node exactly: no api/auth/baseUrl/model overrides.
    config = { providers: { [provider]: entry.node } };
    entryEvidence = {
      path: entryPath,
      sha256: createHash("sha256").update(raw).digest("hex"),
      node: entry.node,
    };
  } else {
    config = nativeConfig(options.protocol, origin.origin, { id: options.model || "w0-model" });
    config.providers[provider].baseUrl = config.providers[provider].baseUrl.replace(
      "/fixture/",
      "/" + cli + "/_protocol/"
    );
    if (cli === "omp") config.providers[provider].auth = "apiKey";
  }
  await writeFile(
    join(sandbox.env.PI_CODING_AGENT_DIR, cli === "pi" ? "models.json" : "models.yml"),
    JSON.stringify(config, null, 2)
  );
  const settings =
    cli === "pi"
      ? { retry: { enabled: false }, compaction: { enabled: false } }
      : { "retry.enabled": false, "compaction.enabled": false };
  await writeFile(
    join(sandbox.env.PI_CODING_AGENT_DIR, cli === "pi" ? "settings.json" : "config.yml"),
    JSON.stringify(settings)
  );
  const result = await runCli(
    cli,
    [
      ...cliFlags(cli),
      "--no-tools",
      "--provider",
      provider,
      "--model",
      options.model || "w0-model",
      "--thinking",
      "off",
      "--mode",
      "json",
      "-p",
      "Return the local gateway fixture response.",
    ],
    sandbox.env,
    sandbox.cwd
  );
  const events = parseEvents(result.stdout);
  const last = assistantMessages(events).at(-1);
  const text =
    last?.content
      ?.filter((p) => p.type === "text")
      .map((p) => p.text)
      .join("") || "";
  const passed =
    result.code === 0 &&
    !result.timedOut &&
    last?.stopReason === "stop" &&
    Boolean(text) &&
    events.some((e) => e.type === "agent_end") &&
    (!options["expect-text"] || text.includes(options["expect-text"]));
  const evidencePath = join(sandbox.root, "evidence.json");
  await writeFile(
    evidencePath,
    JSON.stringify(
      {
        client: cli,
        version: info.version,
        versionProbe: info,
        protocol: options.protocol,
        provider,
        model: options.model || "w0-model",
        entry: entryEvidence,
        passed,
        process: result,
        finalAssistant: last,
      },
      null,
      2
    )
  );
  return {
    passed,
    client: cli,
    version: info.version,
    runtime: result.runtime,
    entrySha256: info.entrySha256,
    protocol: options.protocol,
    provider,
    cliExit: result.code,
    timedOut: result.timedOut,
    stopReason: last?.stopReason,
    text,
    evidencePath,
    ...(passed
      ? {}
      : { error: last?.errorMessage || result.spawnError || result.stderr.slice(-1200) }),
  };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  gatewayClient(argumentsMap(process.argv.slice(2)))
    .then((result) => {
      console.log(JSON.stringify(result));
      if (!result.passed) process.exitCode = 1;
    })
    .catch((error) => {
      console.error(JSON.stringify({ passed: false, error: String(error) }));
      process.exitCode = 1;
    });
}
