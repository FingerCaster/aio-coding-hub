#!/usr/bin/env node
import { createServer } from "node:http";
import { mkdir, writeFile, readdir, stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runtimeRoot } from "./pi-omp-bootstrap.mjs";
import {
  cliFlags,
  fakeKey,
  makeSandbox,
  marker,
  modelId,
  nativeConfig,
  protocols,
  runCli,
  sse,
} from "./pi-omp-wire-capture.mjs";
import {
  errorMarker,
  errorSse,
  selectedTool,
  thinkingSse,
  thought,
  toolResult,
  toolSse,
} from "./pi-omp-wire-fixtures.mjs";

const png =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAE0lEQVR4nGP4z8DwnwGM/zMwAAAf7gP9NRsAMwAAAABJRU5ErkJggg==";
export function parseEvents(stdout) {
  return stdout.split(/\r?\n/).flatMap((line) => {
    try {
      return [JSON.parse(line)];
    } catch {
      return [];
    }
  });
}
export function assistantMessages(events) {
  return events
    .filter((e) => e.type === "message_end" && e.message?.role === "assistant")
    .map((e) => e.message);
}
async function snapshots(root) {
  const result = {};
  async function walk(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) await walk(path);
      else {
        const info = await stat(path);
        result[path.slice(root.length + 1).replaceAll("\\", "/")] = {
          size: info.size,
          mtimeMs: info.mtimeMs,
        };
      }
    }
  }
  await walk(root);
  return result;
}

export async function runScenario(cli, protocol, scenario, runDir) {
  const sandbox = await makeSandbox(runDir, `${cli}-${protocol}-${scenario}`);
  const captures = [];
  const serverErrors = [];
  let child;
  let abortSent = false;
  let connectionClosed = false;
  const toolPath = join(sandbox.cwd, "fixture.txt");
  await writeFile(toolPath, toolResult + "\n");
  await writeFile(join(sandbox.cwd, "pixel.png"), Buffer.from(png, "base64"));
  const server = createServer(async (req, res) => {
    try {
      let raw = "";
      for await (const chunk of req) raw += chunk;
      const body = raw ? JSON.parse(raw) : null;
      const capture = {
        at: Date.now(),
        method: req.method,
        url: req.url,
        headers: req.headers,
        body,
      };
      captures.push(capture);
      if (scenario === "http400" || (scenario.startsWith("retry429") && captures.length <= 2)) {
        capture.responseStatus = scenario === "http400" ? 400 : 429;
        res.writeHead(capture.responseStatus, {
          "content-type": "application/json",
          "retry-after": "0.01",
        });
        capture.responseBody = JSON.stringify({
          error:
            protocol === "google-generative-ai"
              ? {
                  code: capture.responseStatus,
                  status:
                    capture.responseStatus === 429 ? "RESOURCE_EXHAUSTED" : "INVALID_ARGUMENT",
                  message: errorMarker,
                }
              : {
                  type:
                    capture.responseStatus === 429 ? "rate_limit_error" : "invalid_request_error",
                  code: "w0_fixture_error",
                  message: errorMarker,
                },
        });
        res.end(capture.responseBody);
        return;
      }
      capture.responseStatus = 200;
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "close",
      });
      if (scenario === "cancel") {
        res.write(": fixture waiting for native RPC abort\n\n");
        res.on("close", () => {
          connectionClosed = true;
        });
        setTimeout(() => {
          abortSent = true;
          child.stdin.write(JSON.stringify({ id: "w0-abort", type: "abort" }) + "\n");
        }, 150);
        return;
      }
      if (scenario === "sse-error") capture.responseBody = errorSse(protocol);
      else if (scenario === "midstream-error") capture.responseBody = errorSse(protocol, true);
      else if (scenario === "thinking") capture.responseBody = thinkingSse(protocol);
      else if ((scenario === "tool" || scenario.startsWith("auth-")) && captures.length === 1) {
        const name = selectedTool(protocol, body);
        if (!name) throw new Error(`No read tool declared: ${JSON.stringify(body.tools)}`);
        capture.responseBody = toolSse(protocol, name, toolPath);
      } else capture.responseBody = sse(protocol);
      res.end(capture.responseBody);
    } catch (error) {
      serverErrors.push(String(error));
      if (!res.headersSent) res.writeHead(500);
      res.end(String(error));
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  sandbox.env.PI_OMP_FIXTURE_ORIGIN = origin;
  const changes = scenario === "thinking" ? { reasoning: true, maxTokens: 8192 } : {};
  if (scenario === "thinking" && cli === "omp") {
    changes.thinking = {
      mode:
        protocol === "anthropic-messages" || protocol === "google-generative-ai"
          ? "budget"
          : "effort",
      efforts: ["low", "medium", "high"],
    };
  }
  const config = nativeConfig(protocol, origin, changes);
  if (cli === "omp") config.providers["w0-local"].auth = "apiKey";
  if (scenario === "model-override") {
    const provider = config.providers["w0-local"];
    provider.models[0].api = protocol;
    provider.models[0].baseUrl = provider.baseUrl;
    provider.models[0].headers = { "X-W0-Source": "model" };
    provider.headers = { "X-W0-Source": "provider", "X-W0-Provider": "retained" };
    provider.api = "openai-completions";
    provider.baseUrl = origin + "/wrong-provider-base/v1";
  }
  if (scenario.startsWith("auth-")) {
    config.providers["w0-local"].auth = "apiKey";
    config.providers["w0-local"].apiKey = scenario.includes("generic")
      ? "w0-fake-not-a-real-key"
      : fakeKey;
    if (scenario.includes("omitted")) delete config.providers["w0-local"].auth;
  }
  await writeFile(
    join(sandbox.env.PI_CODING_AGENT_DIR, cli === "pi" ? "models.json" : "models.yml"),
    JSON.stringify(config, null, 2)
  );
  const settings =
    cli === "pi"
      ? { retry: { enabled: false }, compaction: { enabled: false } }
      : { "retry.enabled": false, "compaction.enabled": false };
  if (scenario === "retry429-agent") {
    if (cli === "pi") settings.retry = { enabled: true, maxRetries: 2, baseDelayMs: 1 };
    else
      Object.assign(settings, {
        "retry.enabled": true,
        "retry.maxRetries": 2,
        "retry.baseDelayMs": 1,
      });
  }
  await writeFile(
    join(sandbox.env.PI_CODING_AGENT_DIR, cli === "pi" ? "settings.json" : "config.yml"),
    JSON.stringify(settings)
  );
  const before = await snapshots(sandbox.env.HOME);
  const args = [
    ...cliFlags(cli),
    ...(scenario === "tool" || scenario.startsWith("auth-") ? ["--tools", "read"] : ["--no-tools"]),
    "--provider",
    "w0-local",
    "--model",
    modelId,
    "--thinking",
    scenario === "thinking" ? "low" : "off",
  ];
  let output = "";
  let endScheduled = false;
  const launchOptions =
    scenario === "cancel"
      ? {
          keepStdin: true,
          onChild(c) {
            child = c;
            c.stdin.write(
              JSON.stringify({
                id: "w0-prompt",
                type: "prompt",
                message: "Wait for the fixture.",
              }) + "\n"
            );
          },
          onStdout(chunk, c) {
            output += chunk;
            if (!endScheduled && output.includes("w0-abort") && output.includes("agent_end")) {
              endScheduled = true;
              setTimeout(() => c.stdin.end(), 250);
            }
          },
        }
      : {};
  if (scenario === "cancel") args.push("--mode", "rpc");
  else
    args.push(
      "--mode",
      "json",
      "-p",
      "Use only the local fixture.",
      ...(scenario === "image" ? [`@${join(sandbox.cwd, "pixel.png")}`] : [])
    );
  let result;
  try {
    result = await runCli(cli, args, sandbox.env, sandbox.cwd, launchOptions);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
  const events = parseEvents(result.stdout);
  const messages = assistantMessages(events);
  const last = messages.at(-1);
  const checks = [];
  const check = (name, passed) => checks.push({ name, passed: Boolean(passed) });
  check("bounded-process", !result.timedOut && !result.spawnError);
  check("fixture-server-success", serverErrors.length === 0);
  check("request-captured", captures.length > 0);
  if (["http400", "sse-error", "midstream-error"].includes(scenario)) {
    check("error-visible", last?.stopReason === "error" && Boolean(last.errorMessage));
    check("no-replay-after-error", captures.length === 1);
  } else if (scenario === "cancel") {
    check(
      "native-abort-command",
      abortSent && events.some((e) => e.type === "response" && e.command === "abort" && e.success)
    );
    check("request-disconnected", connectionClosed);
    check(
      "aborted-event",
      messages.some((m) => m.stopReason === "aborted")
    );
    check("no-replay-after-cancel", captures.length === 1);
  } else if (scenario.startsWith("retry429")) {
    const expectedAttempts = cli === "pi" && scenario === "retry429" ? 1 : 3;
    check(
      "bounded-retry-outcome",
      captures.length === expectedAttempts &&
        last?.stopReason === (expectedAttempts === 1 ? "error" : "stop")
    );
  } else {
    check("process-success", result.code === 0);
    check(
      "assistant-completed",
      last?.stopReason === "stop" && JSON.stringify(last).includes(marker)
    );
    if (scenario === "model-override") {
      check(
        "model-api-baseurl-override",
        captures.length === 1 && captures[0].url.startsWith("/fixture/" + protocol + "/")
      );
      check(
        "model-header-override",
        captures[0]?.headers["x-w0-source"] === "model" &&
          captures[0]?.headers["x-w0-provider"] === "retained"
      );
    }
    if (scenario === "tool" || scenario.startsWith("auth-")) {
      check("two-turn-tool-loop", captures.length === 2);
      check(
        "local-tool-result-forwarded",
        JSON.stringify(captures[1]?.body ?? {}).includes(toolResult)
      );
      check(
        "tool-execution-success",
        events.some(
          (e) =>
            e.type === "tool_execution_end" && !e.isError && JSON.stringify(e).includes(toolResult)
        )
      );
    }
    if (scenario === "thinking") {
      check(
        "thinking-received",
        messages.some((m) =>
          m.content?.some((part) => part.type === "thinking" && part.thinking?.includes(thought))
        )
      );
      const body = captures[0]?.body;
      check(
        "thinking-requested",
        Boolean(
          body?.thinking ||
          body?.reasoning_effort ||
          body?.reasoning ||
          body?.generationConfig?.thinkingConfig
        )
      );
    }
    if (scenario === "image") {
      // Both CLIs may resize/re-encode the real input PNG before transmitting it.
      const serialized = JSON.stringify(captures[0]?.body ?? {});
      check(
        "image-transmitted",
        /image\/(png|jpeg|webp)/.test(serialized) && serialized.length > 100
      );
    }
    if (scenario.startsWith("auth-")) {
      check(
        "custom-endpoint-bearer",
        captures.every(
          (c) =>
            c.headers.authorization === `Bearer ${config.providers["w0-local"].apiKey}` &&
            !c.headers["x-api-key"]
        )
      );
      check(
        "auth-mode-tool-name",
        selectedTool(protocol, captures[0]?.body) ===
          (scenario.includes("omitted") ? "_read" : "read")
      );
      check(
        "auth-mode-url",
        captures[0]?.url.endsWith(
          scenario.includes("omitted") ? "/v1/messages?beta=true" : "/v1/messages"
        )
      );
      check(
        "auth-mode-user-agent",
        scenario.includes("omitted")
          ? captures[0]?.headers["user-agent"].startsWith("claude-cli/")
          : captures[0]?.headers["user-agent"] === "omp/18.3.2"
      );
    }
  }
  const after = await snapshots(sandbox.env.HOME);
  const evidence = {
    cli,
    protocol,
    scenario,
    captures,
    process: result,
    checks,
    passed: checks.every((c) => c.passed),
    observations: {
      lastStopReason: last?.stopReason,
      errorMessage: last?.errorMessage,
      serverErrors,
      abortSent,
      connectionClosed,
      filesBefore: before,
      filesAfter: after,
    },
  };
  await writeFile(join(sandbox.root, "evidence.json"), JSON.stringify(evidence, null, 2));
  return evidence;
}

async function main() {
  const runDir = join(
    runtimeRoot,
    "runs",
    "extended-" + new Date().toISOString().replace(/[:.]/g, "-")
  );
  await mkdir(runDir, { recursive: true });
  const scenarios = process.argv
    .find((a) => a.startsWith("--scenarios="))
    ?.split("=")[1]
    .split(",") || [
    "tool",
    "thinking",
    "image",
    "http400",
    "sse-error",
    "midstream-error",
    "retry429",
    "cancel",
    "model-override",
    "retry429-agent",
  ];
  const selectedClients = process.argv.includes("--pi-only")
    ? ["pi"]
    : process.argv.includes("--omp-only")
      ? ["omp"]
      : ["pi", "omp"];
  const selectedProtocols = process.argv.includes("--anthropic-only")
    ? ["anthropic-messages"]
    : protocols;
  const results = [];
  for (const cli of selectedClients)
    for (const protocol of selectedProtocols)
      for (const scenario of scenarios) {
        const result = await runScenario(cli, protocol, scenario, runDir);
        results.push(result);
        console.log(
          `${result.passed ? "PASS" : "FAIL"} ${cli} ${protocol} ${scenario}: requests=${result.captures.length} exit=${result.process.code} stop=${result.observations.lastStopReason}`
        );
        if (!result.passed)
          console.log(
            JSON.stringify({
              checks: result.checks,
              stderr: result.process.stderr.slice(-1200),
              output: result.process.stdout.slice(-1400),
            })
          );
        await writeFile(join(runDir, "summary.json"), JSON.stringify(results, null, 2));
      }
  console.log(`Evidence: ${runDir}`);
  if (results.some((r) => !r.passed)) process.exitCode = 1;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
