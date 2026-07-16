import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const script = resolve("scripts/check-release-bundle-inventory.mjs");
const root = mkdtempSync(join(tmpdir(), "aio-bundle-inventory-"));
try {
  const safe = join(root, "safe");
  mkdirSync(join(safe, "resources", "plugins"), { recursive: true });
  writeFileSync(join(safe, "AIO Coding Hub.exe"), "app");
  writeFileSync(join(safe, "resources", "plugins", "manifest.json"), "{}");
  const safeResult = spawnSync(process.execPath, [script, "--root", safe], { encoding: "utf8" });
  if (safeResult.status !== 0) {
    throw new Error(`safe inventory failed:\n${safeResult.stderr}`);
  }

  for (const forbidden of ["gateway.mjs", "node.exe", "codex-retry-gateway/source.js"]) {
    const candidate = join(root, forbidden.replaceAll("/", "-"));
    mkdirSync(candidate, { recursive: true });
    const target = join(candidate, forbidden);
    mkdirSync(resolve(target, ".."), { recursive: true });
    if (!forbidden.endsWith("source.js")) writeFileSync(target, "forbidden");
    else writeFileSync(target, "forbidden");
    const result = spawnSync(process.execPath, [script, "--root", candidate], { encoding: "utf8" });
    if (result.status === 0 || !result.stderr.includes("forbidden external gateway material")) {
      throw new Error(`expected rejection for ${forbidden}:\n${result.stdout}\n${result.stderr}`);
    }
  }
  console.log("[bundle-inventory] selftest passed");
} finally {
  rmSync(root, { recursive: true, force: true });
}
