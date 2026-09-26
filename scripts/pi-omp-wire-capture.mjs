#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { writeFile, mkdir } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { isolatedEnv, repoRoot, runtimeRoot } from "./pi-omp-bootstrap.mjs";

export const protocols = [
  "anthropic-messages",
  "openai-completions",
  "openai-responses",
  "google-generative-ai",
];
export const fakeKey = "sk-ant-api03-w0-fake-not-a-real-key";
export const modelId = "w0-model";
export const marker = "W0_LOCAL_FIXTURE_OK";
const guard = join(repoRoot, "scripts/pi-omp-network-guard.mjs");
const clients = {
  pi: {
    version: "0.87.1",
    command: process.execPath,
    entry: join(runtimeRoot, "pi/package/dist/bundle/cli.js"),
  },
  omp: {
    version: "18.3.2",
    command: join(runtimeRoot, "tools/node_modules/@oven/bun-windows-x64/bin/bun.exe"),
    entry: join(runtimeRoot, "omp/package/dist/cli.js"),
  },
};

// Explicit controller-only overrides exercise actual installations. Child environments
// remain isolated; a compiled Bun executable cannot load our JavaScript preload.
export function clientRuntime(cli) {
  assert(cli in clients, "Unsupported CLI: " + cli);
  const spec = { ...clients[cli], source: "fixture-package", networkGuard: "javascript-preload" };
  const override = process.env[cli === "pi" ? "PI_OMP_TEST_PI_ENTRY" : "PI_OMP_TEST_OMP_BINARY"];
  if (!override) return spec;
  assert(isAbsolute(override), "Installed CLI override must be an absolute path");
  spec.source = "installed";
  if (cli === "pi") spec.entry = resolve(override);
  else {
    spec.command = resolve(override);
    spec.entry = undefined;
    spec.networkGuard = "unavailable-standalone";
  }
  return spec;
}

export async function probeCli(cli, env, cwd) {
  const spec = clientRuntime(cli);
  const result = await runCli(cli, ["--version"], env, cwd);
  assert(result.code === 0 && !result.timedOut, `${cli} version probe failed: ${result.stderr}`);
  const version = result.stdout.trim().replace(/^omp\//, "");
  assert.equal(
    version,
    spec.version,
    "Unexpected installed CLI version; update the wire contract first"
  );
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(spec.entry || spec.command)) hash.update(chunk);
  return { version, entrySha256: hash.digest("hex"), result };
}

export function sse(protocol, text = marker) {
  const data = (value) => `data: ${JSON.stringify(value)}\n\n`;
  const event = (value) => `event: ${value.type}\n${data(value)}`;
  if (protocol === "anthropic-messages")
    return [
      {
        type: "message_start",
        message: {
          id: "msg_w0",
          type: "message",
          role: "assistant",
          model: modelId,
          content: [],
          stop_reason: null,
          stop_sequence: null,
          usage: { input_tokens: 7, output_tokens: 0 },
        },
      },
      { type: "content_block_start", index: 0, content_block: { type: "text", text: "" } },
      { type: "content_block_delta", index: 0, delta: { type: "text_delta", text } },
      { type: "content_block_stop", index: 0 },
      {
        type: "message_delta",
        delta: { stop_reason: "end_turn", stop_sequence: null },
        usage: { output_tokens: 4 },
      },
      { type: "message_stop" },
    ]
      .map(event)
      .join("");
  if (protocol === "openai-completions")
    return (
      [
        {
          id: "chatcmpl_w0",
          object: "chat.completion.chunk",
          created: 1,
          model: modelId,
          choices: [{ index: 0, delta: { role: "assistant", content: text }, finish_reason: null }],
        },
        {
          id: "chatcmpl_w0",
          object: "chat.completion.chunk",
          created: 1,
          model: modelId,
          choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
          usage: { prompt_tokens: 7, completion_tokens: 4, total_tokens: 11 },
        },
      ]
        .map(data)
        .join("") + "data: [DONE]\n\n"
    );
  if (protocol === "openai-responses") {
    const message = {
      id: "msg_w0",
      type: "message",
      status: "completed",
      role: "assistant",
      content: [{ type: "output_text", text, annotations: [] }],
    };
    const response = {
      id: "resp_w0",
      object: "response",
      created_at: 1,
      status: "completed",
      model: modelId,
      output: [message],
      usage: {
        input_tokens: 7,
        output_tokens: 4,
        total_tokens: 11,
        input_tokens_details: { cached_tokens: 0 },
        output_tokens_details: { reasoning_tokens: 0 },
      },
    };
    return [
      { type: "response.created", response: { ...response, status: "in_progress", output: [] } },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: { ...message, status: "in_progress", content: [] },
      },
      {
        type: "response.content_part.added",
        item_id: message.id,
        output_index: 0,
        content_index: 0,
        part: { type: "output_text", text: "", annotations: [] },
      },
      {
        type: "response.output_text.delta",
        item_id: message.id,
        output_index: 0,
        content_index: 0,
        delta: text,
      },
      {
        type: "response.output_text.done",
        item_id: message.id,
        output_index: 0,
        content_index: 0,
        text,
      },
      {
        type: "response.content_part.done",
        item_id: message.id,
        output_index: 0,
        content_index: 0,
        part: message.content[0],
      },
      { type: "response.output_item.done", output_index: 0, item: message },
      { type: "response.completed", response },
    ]
      .map((value, sequence_number) => event({ ...value, sequence_number }))
      .join("");
  }
  if (protocol === "google-generative-ai")
    return data({
      candidates: [
        { index: 0, content: { role: "model", parts: [{ text }] }, finishReason: "STOP" },
      ],
      modelVersion: modelId,
      usageMetadata: { promptTokenCount: 7, candidatesTokenCount: 4, totalTokenCount: 11 },
    });
  throw new Error(`Unknown protocol: ${protocol}`);
}

export function nativeConfig(protocol, origin, changes = {}) {
  const suffix =
    protocol === "anthropic-messages"
      ? ""
      : protocol === "google-generative-ai"
        ? "/v1beta"
        : "/v1";
  return {
    providers: {
      "w0-local": {
        api: protocol,
        baseUrl: `${origin}/fixture/${protocol}${suffix}`,
        apiKey: fakeKey,
        models: [
          {
            id: modelId,
            name: "W0 local mock",
            reasoning: false,
            input: ["text", "image"],
            contextWindow: 32768,
            maxTokens: 1024,
            cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
            ...changes,
          },
        ],
      },
    },
  };
}

export async function runCli(
  cli,
  args,
  env,
  cwd,
  { timeoutMs = 30000, input, onChild, keepStdin = false, onStdout } = {}
) {
  const spec = clientRuntime(cli);
  const preload = !spec.entry
    ? []
    : cli === "pi"
      ? ["--import", pathToFileURL(guard).href]
      : ["--preload", guard];
  const start = Date.now();
  const child = spawn(spec.command, [...preload, ...(spec.entry ? [spec.entry] : []), ...args], {
    env,
    cwd,
    stdio: ["pipe", "pipe", "pipe"],
    windowsHide: true,
  });
  let stdout = "";
  let stderr = "";
  let timedOut = false;
  child.stdout.on("data", (data) => {
    stdout += data;
    onStdout?.(String(data), child);
  });
  child.stderr.on("data", (data) => {
    stderr += data;
  });
  const timer = setTimeout(() => {
    timedOut = true;
    child.kill();
  }, timeoutMs);
  onChild?.(child);
  child.stdin.on("error", () => {});
  if (input !== undefined) child.stdin.end(input);
  else if (!keepStdin) child.stdin.end();
  const result = await new Promise((resolve) => {
    child.on("error", (error) => resolve({ code: null, signal: null, spawnError: String(error) }));
    child.on("close", (code, signal) => resolve({ code, signal }));
  });
  clearTimeout(timer);
  return {
    ...result,
    timedOut,
    durationMs: Date.now() - start,
    stdout,
    stderr,
    args,
    runtime: spec,
  };
}

export function cliFlags(cli) {
  return [
    "--no-session",
    "--no-extensions",
    "--no-skills",
    ...(cli === "pi"
      ? ["--offline", "--no-prompt-templates", "--no-themes", "--no-context-files", "--no-approve"]
      : ["--no-rules", "--no-lsp", "--no-pty", "--no-prewalk", "--no-title"]),
  ];
}

export async function makeSandbox(runDir, name) {
  // OMP native helper paths exceed Windows MAX_PATH if verbose case names are used.
  const root = join(
    runDir,
    name.slice(0, 3) + "-" + createHash("sha256").update(name).digest("hex").slice(0, 10)
  );
  const env = await isolatedEnv(root);
  const cwd = join(root, "cwd");
  await mkdir(join(cwd, ".git"), { recursive: true });
  await mkdir(join(cwd, ".pi"), { recursive: true });
  await mkdir(join(cwd, ".omp"), { recursive: true });
  env.PI_OMP_GUARD_LOG = join(root, "blocked-network.jsonl");
  return { root, env, cwd };
}

export async function runBasic(cli, protocol, runDir) {
  const sandbox = await makeSandbox(runDir, `${cli}-${protocol}`);
  const captures = [];
  const server = createServer(async (req, res) => {
    let raw = "";
    for await (const chunk of req) raw += chunk;
    const body = raw ? JSON.parse(raw) : null;
    captures.push({ method: req.method, url: req.url, headers: req.headers, body });
    res.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "close",
    });
    res.end(sse(protocol));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  sandbox.env.PI_OMP_FIXTURE_ORIGIN = origin;
  const configFile = join(
    sandbox.env.PI_CODING_AGENT_DIR,
    cli === "pi" ? "models.json" : "models.yml"
  );
  const native = nativeConfig(protocol, origin);
  if (cli === "omp") native.providers["w0-local"].auth = "apiKey";
  await writeFile(configFile, JSON.stringify(native, null, 2));
  const config =
    cli === "pi"
      ? { retry: { enabled: false }, compaction: { enabled: false } }
      : { "retry.enabled": false, "compaction.enabled": false };
  await writeFile(
    join(sandbox.env.PI_CODING_AGENT_DIR, cli === "pi" ? "settings.json" : "config.yml"),
    JSON.stringify(config)
  );
  let result;
  try {
    result = await runCli(
      cli,
      [
        ...cliFlags(cli),
        "--no-tools",
        "--provider",
        "w0-local",
        "--model",
        modelId,
        "--thinking",
        "off",
        "--mode",
        "json",
        "-p",
        "Reply with the local fixture marker.",
      ],
      sandbox.env,
      sandbox.cwd
    );
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
  const checks = [];
  const check = (name, passed) => checks.push({ name, passed: Boolean(passed) });
  check("process-success", result.code === 0 && !result.timedOut);
  check("single-request", captures.length === 1);
  const captured = captures[0];
  const expectedSuffix = {
    "anthropic-messages": cli === "omp" ? "/v1/messages" : "/v1/messages?beta=true",
    "openai-completions": "/v1/chat/completions",
    "openai-responses": "/v1/responses",
    "google-generative-ai": `/v1beta/models/${modelId}:streamGenerateContent?alt=sse`,
  }[protocol];
  check("url", captured?.url === `/fixture/${protocol}${expectedSuffix}`);
  check("POST", captured?.method === "POST");
  check(
    "model",
    protocol === "google-generative-ai"
      ? captured?.url.includes(`/models/${modelId}:`)
      : captured?.body?.model === modelId
  );
  check(
    "auth",
    protocol === "anthropic-messages" && cli === "pi"
      ? captured?.headers["x-api-key"] === fakeKey
      : protocol === "google-generative-ai"
        ? captured?.headers["x-goog-api-key"] === fakeKey
        : captured?.headers.authorization === `Bearer ${fakeKey}`
  );
  check("assistant-output", result.stdout.includes(marker));
  check("completed", result.stdout.includes("agent_end"));
  const evidence = {
    cli,
    version: clients[cli].version,
    protocol,
    origin,
    captures,
    process: result,
    checks,
    passed: checks.every((c) => c.passed),
  };
  await writeFile(join(sandbox.root, "evidence.json"), JSON.stringify(evidence, null, 2));
  return evidence;
}

async function main() {
  const runDir = join(runtimeRoot, "runs", new Date().toISOString().replace(/[:.]/g, "-"));
  await mkdir(runDir, { recursive: true });
  const selected = process.argv.includes("--pi-only")
    ? ["pi"]
    : process.argv.includes("--omp-only")
      ? ["omp"]
      : ["pi", "omp"];
  const versions = {};
  for (const cli of selected) {
    const sandbox = await makeSandbox(runDir, `${cli}-version`);
    versions[cli] = await probeCli(cli, sandbox.env, sandbox.cwd);
    await writeFile(join(sandbox.root, "evidence.json"), JSON.stringify(versions[cli], null, 2));
    console.log(`${cli}: ${versions[cli].version} (${versions[cli].result.runtime.source})`);
  }
  if (process.argv.includes("--probe")) return;
  const results = [];
  for (const cli of selected)
    for (const protocol of protocols) {
      const result = await runBasic(cli, protocol, runDir);
      results.push(result);
      console.log(
        `${result.passed ? "PASS" : "FAIL"} ${cli} ${protocol}: ${result.captures[0]?.url || "NO REQUEST"} exit=${result.process.code}`
      );
      if (!result.passed)
        console.log(
          JSON.stringify({
            checks: result.checks,
            stderr: result.process.stderr.slice(-2500),
            stdout: result.process.stdout.slice(-1500),
          })
        );
    }
  await writeFile(join(runDir, "summary.json"), JSON.stringify({ versions, results }, null, 2));
  console.log(`Evidence: ${runDir}`);
  if (results.some((r) => !r.passed)) process.exitCode = 1;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
