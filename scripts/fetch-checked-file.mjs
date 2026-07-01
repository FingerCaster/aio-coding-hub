#!/usr/bin/env node
import { createHash } from "node:crypto";
import { open, rm } from "node:fs/promises";

const args = parseArgs(process.argv.slice(2));
const url = requireArg(args, "url");
const expectedSha256 = requireArg(args, "sha256").toLowerCase();
const output = requireArg(args, "output");

if (!/^[a-f0-9]{64}$/.test(expectedSha256)) {
  throw new Error("Expected --sha256 to be a lowercase or uppercase SHA-256 hex digest.");
}

await fetchCheckedFile(url, expectedSha256, output);

function parseArgs(argv) {
  const parsed = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index];
    if (!name.startsWith("--")) {
      throw new Error(`Unexpected argument: ${name}`);
    }
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for ${name}`);
    }
    parsed.set(name.slice(2), value);
    index += 1;
  }
  return parsed;
}

function requireArg(args, name) {
  const value = args.get(name);
  if (!value) {
    throw new Error(`Missing required argument --${name}`);
  }
  return value;
}

async function fetchCheckedFile(rawUrl, expectedDigest, outputPath) {
  const url = new URL(rawUrl);
  if (url.protocol !== "https:") {
    throw new Error(`Refusing non-HTTPS URL: ${rawUrl}`);
  }

  const response = await fetch(url, {
    redirect: "follow",
    headers: {
      "User-Agent": "aio-coding-hub-release",
    },
  });
  if (!response.ok) {
    throw new Error(`Fetch failed with HTTP ${response.status}: ${rawUrl}`);
  }
  if (!response.body) {
    throw new Error(`Fetch returned an empty body: ${rawUrl}`);
  }

  const hash = createHash("sha256");
  const file = await open(outputPath, "w", 0o600);
  let ok = false;
  try {
    for await (const chunk of response.body) {
      const buffer = Buffer.from(chunk);
      hash.update(buffer);
      await file.write(buffer);
    }

    const actualDigest = hash.digest("hex");
    if (actualDigest !== expectedDigest) {
      throw new Error(
        `SHA-256 mismatch for ${rawUrl}: expected ${expectedDigest}, got ${actualDigest}`
      );
    }
    ok = true;
  } finally {
    await file.close();
    if (!ok) {
      await rm(outputPath, { force: true });
    }
  }
}
