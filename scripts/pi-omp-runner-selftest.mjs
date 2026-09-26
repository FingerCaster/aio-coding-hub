#!/usr/bin/env node
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { isolatedEnv, repoRoot, runtimeRoot } from "./pi-omp-bootstrap.mjs";
import { fakeKey, marker, nativeConfig, protocols, sse } from "./pi-omp-wire-capture.mjs";

async function run(command, args, options = {}) {
  const child = spawn(command, args, {
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    ...options,
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const timer = setTimeout(() => child.kill(), 40000);
  const code = await new Promise((res, rej) => {
    child.on("error", rej);
    child.on("close", res);
  });
  clearTimeout(timer);
  return { code, stdout, stderr };
}
const target = join(runtimeRoot, "selftest", Date.now().toString(36));
await mkdir(target, { recursive: true });
const env = await isolatedEnv(join(target, "controller"));
for (const key of ["PI_OMP_TEST_PI_ENTRY", "PI_OMP_TEST_OMP_BINARY"])
  if (process.env[key]) env[key] = process.env[key];
for (const forbidden of [
  "ANTHROPIC_API_KEY",
  "OPENAI_API_KEY",
  "GOOGLE_API_KEY",
  "GEMINI_API_KEY",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "NODE_OPTIONS",
  "BUN_OPTIONS",
  "OMP_PROFILE",
  "PI_PROFILE",
])
  assert(!(forbidden in env), "Sensitive inherited env: " + forbidden);
const results = [];
let fixture = { cli: "pi", protocol: protocols[0], fail: false };
let captures = [];
const server = createServer(async (req, res) => {
  let body = "";
  for await (const chunk of req) body += chunk;
  captures.push({ method: req.method, url: req.url, headers: req.headers, body: JSON.parse(body) });
  if (fixture.fail) {
    res.writeHead(400, { "content-type": "application/json" });
    res.end(
      JSON.stringify({ error: { message: "W0_RUNNER_FAILURE", type: "invalid_request_error" } })
    );
  } else {
    res.writeHead(200, { "content-type": "text/event-stream" });
    res.end(sse(fixture.protocol));
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = "http://127.0.0.1:" + server.address().port;
try {
  for (const cli of ["pi", "omp"])
    for (const protocol of protocols) {
      fixture = { cli, protocol, fail: false };
      captures = [];
      const args = [
        join(repoRoot, "scripts/pi-omp-gateway-client.mjs"),
        "--client",
        cli,
        "--protocol",
        protocol,
        "--base-origin",
        origin,
        "--model",
        "w0-model",
        "--target",
        target,
        "--expect-text",
        marker,
      ];
      const result = await run(process.execPath, args, { env, cwd: repoRoot });
      const output = JSON.parse(result.stdout);
      assert.equal(result.code, 0, JSON.stringify(result));
      assert.equal(output.passed, true);
      assert.equal(captures.length, 1);
      assert(captures[0].url.startsWith("/" + cli + "/_protocol/" + protocol + "/"));
      assert(JSON.stringify(captures[0].headers).includes(fakeKey));
      results.push({ cli, protocol, kind: "success", result: output });
      console.log("PASS runner " + cli + " " + protocol);
    }
  for (const cli of ["pi", "omp"])
    for (const protocol of protocols) {
      fixture = { cli, protocol, fail: false };
      captures = [];
      const nativeKey = "aio-coding-hub-" + protocol;
      const node = nativeConfig(protocol, origin).providers["w0-local"];
      node.baseUrl = node.baseUrl.replace("/fixture/", "/" + cli + "/_protocol/");
      node.headers = { "X-W0-Generated-Node": nativeKey };
      if (cli === "omp") node.auth = "apiKey";
      const entryPath = join(target, "entry-" + cli + "-" + protocol + ".json");
      const entryText = JSON.stringify({
        protocol,
        nativeKey,
        baseUrl: node.baseUrl,
        models: [],
        node,
      });
      await writeFile(entryPath, entryText);
      const args = [
        join(repoRoot, "scripts/pi-omp-gateway-client.mjs"),
        "--client",
        cli,
        "--protocol",
        protocol,
        "--base-origin",
        origin,
        "--model",
        "w0-model",
        "--target",
        target,
        "--expect-text",
        marker,
        "--entry-json",
        entryPath,
      ];
      const result = await run(process.execPath, args, { env, cwd: repoRoot });
      const output = JSON.parse(result.stdout);
      assert.equal(result.code, 0, JSON.stringify(result));
      assert.equal(output.passed, true);
      assert.equal(output.provider, nativeKey);
      assert.equal(captures.length, 1);
      assert.equal(captures[0].headers["x-w0-generated-node"], nativeKey);
      const evidence = JSON.parse(await readFile(output.evidencePath, "utf8"));
      assert.deepEqual(evidence.entry.node, node);
      assert.equal(await readFile(entryPath, "utf8"), entryText);
      assert(evidence.process.args.includes(nativeKey));
      results.push({ cli, protocol, kind: "entry-json-success", result: output });
      console.log("PASS entry-json " + cli + " " + protocol);
    }
  for (const kind of [
    "outside-runtime",
    "nonlocal-endpoint",
    "command-secret",
    "protocol-mismatch",
  ]) {
    captures = [];
    const node = nativeConfig("openai-completions", origin).providers["w0-local"];
    if (kind === "nonlocal-endpoint") node.baseUrl = "https://example.invalid/v1";
    if (kind === "command-secret") node.apiKey = "!should-never-execute";
    const entryPath =
      kind === "outside-runtime"
        ? join(repoRoot, "scripts/pi-omp-gateway-client.mjs")
        : join(target, "invalid-" + kind + ".json");
    if (kind !== "outside-runtime")
      await writeFile(
        entryPath,
        JSON.stringify({
          protocol: kind === "protocol-mismatch" ? "openai-responses" : "openai-completions",
          nativeKey: "aio-invalid",
          node,
        })
      );
    const result = await run(
      process.execPath,
      [
        join(repoRoot, "scripts/pi-omp-gateway-client.mjs"),
        "--client",
        "pi",
        "--protocol",
        "openai-completions",
        "--base-origin",
        origin,
        "--target",
        target,
        "--entry-json",
        entryPath,
      ],
      { env, cwd: repoRoot }
    );
    assert.equal(result.code, 1);
    assert.equal(captures.length, 0);
    assert.equal(JSON.parse(result.stderr).passed, false);
    results.push({ kind: "entry-json-rejected", reason: kind });
    console.log("PASS entry-json rejects " + kind);
  }
  for (const cli of ["pi", "omp"]) {
    fixture = { cli, protocol: "openai-completions", fail: true };
    captures = [];
    const result = await run(
      process.execPath,
      [
        join(repoRoot, "scripts/pi-omp-gateway-client.mjs"),
        "--client",
        cli,
        "--protocol",
        fixture.protocol,
        "--base-origin",
        origin,
        "--target",
        target,
      ],
      { env, cwd: repoRoot }
    );
    const output = JSON.parse(result.stdout);
    assert.equal(result.code, 1);
    assert.equal(output.passed, false);
    assert.equal(output.stopReason, "error");
    assert.equal(captures.length, 1);
    if (cli === "pi")
      assert.equal(output.cliExit, 0, "Native Pi JSON error exit is observed separately");
    results.push({ cli, kind: "native-error-propagation", result: output });
    console.log(
      "PASS runner detects native error " +
        cli +
        " (CLI exit=" +
        output.cliExit +
        ", runner exit=1)"
    );
  }
} finally {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
for (const [name, command, preload] of [
  ["node", process.execPath, "--import"],
  ["bun", join(runtimeRoot, "tools/node_modules/@oven/bun-windows-x64/bin/bun.exe"), "--preload"],
]) {
  const guard = join(repoRoot, "scripts/pi-omp-network-guard.mjs");
  const result = await run(
    command,
    [
      preload,
      name === "node" ? pathToFileURL(guard).href : guard,
      "-e",
      'fetch("https://example.invalid/should-not-send").then(()=>process.exit(8)).catch(()=>process.exit(0))',
    ],
    { env }
  );
  // Guard throws synchronously before either runtime opens a connection.
  assert.notEqual(result.code, 8);
  assert(result.stderr.includes("W0 blocked non-fixture network"), JSON.stringify(result));
  results.push({ kind: "network-guard", runtime: name, blocked: true });
  console.log("PASS network guard " + name);
}
await writeFile(join(target, "summary.json"), JSON.stringify(results, null, 2));
console.log("Evidence: " + target);
