#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const DEFAULT_GATEWAY_URL = "http://127.0.0.1:37123";
const EXPECTED_ANSWER = 21;
const STRATEGY_VALUES = new Map([
  ["stable", "continuation_repair"],
  ["experimental", "continuation_repair_experimental"],
]);

const PROMPT = `不使用任何外部工具回答以下问题：在一个黑色的袋子里放有三种口味的糖果，每种糖果有两种不同的形状（圆形和五角星形，不同的形状靠手感可以分辨）。现已知不同口味的糖和不同形状的数量统计如下表。参赛者需要在活动前决定摸出的糖果数目，那么，最少取出多少个糖果才能保证手中同时拥有不同形状的苹果味和桃子味的糖？（同时手中有圆形苹果味匹配五角星桃子味糖果，或者有圆形桃子味匹配五角星苹果味糖果都满足要求）

        苹果味  桃子味  西瓜味
圆形       7      9      8
五角星形   7      6      4

注意：因为形状靠手感可以分辨，参赛者可以在摸取过程中按形状控制拿到的圆形/五角星形数量，但不能通过手感分辨口味。请按这个规则求活动前需要决定的最小总数。

请给出最终数字答案，并简要说明。`;

function usage() {
  console.log(`Usage: node scripts/probe-codex-continuation.mjs --strategy current|stable|experimental [--repeat N]

Environment:
  AIO_GATEWAY_URL      Gateway origin, default discovered from settings or ${DEFAULT_GATEWAY_URL}
  AIO_SETTINGS_PATH    Settings JSON path, default ~/.aio-coding-hub/settings.json
  AIO_PROBE_MODEL      Responses model, default gpt-5.5
  AIO_PROBE_TIMEOUT_MS Request timeout, default 120000
`);
}

function parseArgs(argv) {
  const args = {
    strategy: "current",
    repeat: 1,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      usage();
      process.exit(0);
    }
    if (arg === "--strategy") {
      args.strategy = argv[++index];
      continue;
    }
    if (arg === "--repeat") {
      args.repeat = Number(argv[++index]);
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  if (!["current", "stable", "experimental"].includes(args.strategy)) {
    throw new Error("--strategy must be one of current, stable, experimental");
  }
  if (!Number.isInteger(args.repeat) || args.repeat < 1 || args.repeat > 20) {
    throw new Error("--repeat must be an integer from 1 to 20");
  }
  return args;
}

function settingsPath() {
  if (process.env.AIO_SETTINGS_PATH) {
    return process.env.AIO_SETTINGS_PATH;
  }
  const dotdir = process.env.AIO_CODING_HUB_DOTDIR_NAME || ".aio-coding-hub";
  return join(homedir(), dotdir, "settings.json");
}

function readSettings() {
  const path = settingsPath();
  if (!existsSync(path)) {
    return { path, settings: null, warning: "settings file not found" };
  }
  try {
    return { path, settings: JSON.parse(readFileSync(path, "utf8")), warning: null };
  } catch (error) {
    return { path, settings: null, warning: `settings parse failed: ${error.message}` };
  }
}

function gatewayUrl(settings) {
  if (process.env.AIO_GATEWAY_URL) {
    return process.env.AIO_GATEWAY_URL.replace(/\/+$/, "");
  }
  const preferredPort = Number(settings?.preferred_port);
  if (Number.isInteger(preferredPort) && preferredPort > 0) {
    return `http://127.0.0.1:${preferredPort}`;
  }
  return DEFAULT_GATEWAY_URL;
}

function checkStrategyPrecondition(requestedStrategy, settingsInfo) {
  const expected = STRATEGY_VALUES.get(requestedStrategy);
  const current = settingsInfo.settings?.codex_reasoning_guard_post_match_strategy ?? null;
  const guardEnabled = settingsInfo.settings?.codex_reasoning_guard_enabled ?? null;
  if (!expected) {
    return { current, expected: null, guardEnabled };
  }
  if (!current) {
    throw new Error(
      `Cannot verify ${requestedStrategy} precondition: ${settingsInfo.warning ?? "strategy setting missing"} at ${settingsInfo.path}`
    );
  }
  if (current !== expected) {
    throw new Error(
      `Strategy precondition failed: requested ${requestedStrategy} requires ${expected}, current setting is ${current}. Change the app setting first, wait for settings cache to expire, then rerun.`
    );
  }
  if (guardEnabled !== true) {
    throw new Error(
      `Strategy precondition failed: requested ${requestedStrategy} requires codex_reasoning_guard_enabled=true, current setting is ${guardEnabled}. Enable the Codex reasoning guard first, wait for settings cache to expire, then rerun.`
    );
  }
  return { current, expected, guardEnabled };
}

function requestBody() {
  return {
    model: process.env.AIO_PROBE_MODEL || "gpt-5.5",
    stream: true,
    reasoning: { effort: process.env.AIO_PROBE_REASONING_EFFORT || "high" },
    input: PROMPT,
  };
}

function parseSseFrames(raw) {
  const frames = [];
  for (const block of raw.split(/\r?\n\r?\n/)) {
    const lines = block.split(/\r?\n/);
    const eventLines = [];
    const dataLines = [];
    for (const line of lines) {
      if (line.startsWith("event:")) {
        eventLines.push(line.slice("event:".length).trim());
      } else if (line.startsWith("data:")) {
        dataLines.push(line.slice("data:".length).trimStart());
      }
    }
    if (dataLines.length === 0) {
      continue;
    }
    const data = dataLines.join("\n");
    if (data === "[DONE]") {
      frames.push({ event: eventLines.at(-1) ?? "message", data: "[DONE]" });
      continue;
    }
    try {
      frames.push({ event: eventLines.at(-1) ?? "message", data: JSON.parse(data) });
    } catch {
      frames.push({ event: eventLines.at(-1) ?? "message", data });
    }
  }
  return frames;
}

function extractTextFromContent(content) {
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .map((part) => {
      if (typeof part?.text === "string") return part.text;
      if (typeof part?.output_text === "string") return part.output_text;
      return "";
    })
    .join("");
}

function extractTextFromResponseJson(value) {
  const response = value?.response ?? value;
  if (typeof response?.output_text === "string") {
    return response.output_text;
  }
  if (!Array.isArray(response?.output)) {
    return "";
  }
  return response.output
    .map((item) => {
      if (typeof item?.text === "string") return item.text;
      if (Array.isArray(item?.content)) return extractTextFromContent(item.content);
      return "";
    })
    .join("");
}

function extractTextFromSse(raw) {
  let deltaText = "";
  let completedText = "";
  let guardCode = null;
  for (const frame of parseSseFrames(raw)) {
    const data = frame.data;
    if (!data || typeof data !== "object") {
      continue;
    }
    guardCode ??= data?.error?.code ?? data?.code ?? data?.error_code ?? null;
    if (typeof data.delta === "string" && data.type === "response.output_text.delta") {
      deltaText += data.delta;
    }
    if (data.type === "response.output_item.done") {
      completedText += extractTextFromContent(data.item?.content);
    }
    if (data.type === "response.completed") {
      completedText = extractTextFromResponseJson(data) || completedText;
    }
  }
  return { text: completedText || deltaText, guardCode };
}

function parseResponseText(contentType, raw) {
  if (contentType.includes("text/event-stream") || raw.includes("event: response.")) {
    return extractTextFromSse(raw);
  }
  try {
    const json = JSON.parse(raw);
    return {
      text: extractTextFromResponseJson(json),
      guardCode: json?.error?.code ?? json?.code ?? json?.error_code ?? null,
    };
  } catch {
    return {
      text: raw.trim(),
      guardCode: raw.includes("GW_CODEX_REASONING_GUARD") ? "GW_CODEX_REASONING_GUARD" : null,
    };
  }
}

function extractFinalNumber(text) {
  const candidates = [];
  const patterns = [
    /最终(?:数字)?答案[^\d]{0,20}(\d+)/gi,
    /答案[^\d]{0,20}(\d+)/gi,
    /最少(?:需要|取出)?[^\d]{0,20}(\d+)\s*(?:个|颗)?/gi,
    /至少[^\d]{0,20}(\d+)\s*(?:个|颗)?/gi,
  ];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      candidates.push(Number(match[1]));
    }
    const unique = [...new Set(candidates)];
    if (unique.length === 1) {
      return { value: unique[0], source: "final-answer-pattern" };
    }
    if (unique.length > 1) {
      return { value: null, source: "ambiguous-final-answer-pattern", candidates: unique };
    }
  }

  const paragraphs = text
    .trim()
    .split(/\n{1,}/)
    .filter(Boolean);
  const lastParagraph = paragraphs.at(-1) ?? text;
  const trailingNumbers = [...lastParagraph.matchAll(/\d+/g)].map((match) => Number(match[0]));
  const uniqueTrailing = [...new Set(trailingNumbers)];
  if (uniqueTrailing.length === 1) {
    return { value: uniqueTrailing[0], source: "last-paragraph" };
  }
  return { value: null, source: "ambiguous-or-missing", candidates: uniqueTrailing };
}

async function runProbe({ url, iteration }) {
  const controller = new AbortController();
  const timeoutMs = Number(process.env.AIO_PROBE_TIMEOUT_MS || 120000);
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${url}/v1/responses`, {
      method: "POST",
      signal: controller.signal,
      headers: {
        "content-type": "application/json",
        accept: "text/event-stream, application/json",
        authorization: `Bearer ${
          process.env.AIO_GATEWAY_API_KEY || process.env.OPENAI_API_KEY || "aio-coding-hub"
        }`,
      },
      body: JSON.stringify(requestBody()),
    });
    const contentType = response.headers.get("content-type") ?? "";
    const raw = await response.text();
    const parsed = parseResponseText(contentType, raw);
    if (!response.ok) {
      const excerpt = raw.slice(0, 1000).replace(/\s+/g, " ");
      throw new Error(
        `iteration ${iteration}: HTTP ${response.status}; guard=${parsed.guardCode ?? "none"}; body=${excerpt}`
      );
    }
    if (
      parsed.guardCode === "GW_CODEX_REASONING_GUARD" ||
      raw.includes("GW_CODEX_REASONING_GUARD")
    ) {
      throw new Error(`iteration ${iteration}: local guard returned GW_CODEX_REASONING_GUARD`);
    }
    if (!parsed.text.trim()) {
      throw new Error(`iteration ${iteration}: parsed final text is empty`);
    }
    const answer = extractFinalNumber(parsed.text);
    if (answer.value !== EXPECTED_ANSWER) {
      throw new Error(
        `iteration ${iteration}: expected final numeric answer ${EXPECTED_ANSWER}, got ${
          answer.value ?? "none"
        } via ${answer.source}; candidates=${JSON.stringify(
          answer.candidates ?? []
        )}; text=${parsed.text.slice(0, 1000)}`
      );
    }
    console.log(
      `[probe] iteration ${iteration}: PASS answer=${answer.value} source=${answer.source} text=${JSON.stringify(parsed.text.slice(0, 180))}`
    );
  } finally {
    clearTimeout(timer);
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const settingsInfo = readSettings();
  const strategy = checkStrategyPrecondition(args.strategy, settingsInfo);
  const url = gatewayUrl(settingsInfo.settings);
  console.log(
    `[probe] gateway=${url} strategy_requested=${args.strategy} strategy_current=${
      strategy.current ?? "unknown"
    } guard_enabled=${strategy.guardEnabled ?? "unknown"} settings=${settingsInfo.path}`
  );
  if (settingsInfo.warning) {
    console.log(`[probe] settings_note=${settingsInfo.warning}`);
  }
  for (let iteration = 1; iteration <= args.repeat; iteration += 1) {
    await runProbe({ url, iteration });
  }
}

main().catch((error) => {
  console.error(`[probe] FAIL ${error.message}`);
  process.exitCode = 1;
});
