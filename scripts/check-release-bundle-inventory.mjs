import { lstatSync, readlinkSync, readdirSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";

const MAX_ENTRIES = 200_000;
const roots = [];
for (let index = 2; index < process.argv.length; index += 1) {
  if (process.argv[index] !== "--root" || !process.argv[index + 1]) {
    throw new Error(
      "Usage: node scripts/check-release-bundle-inventory.mjs --root <dir> [--root <dir>]"
    );
  }
  roots.push(resolve(process.argv[++index]));
}
if (roots.length === 0) {
  throw new Error("At least one extracted bundle root is required.");
}

function forbiddenReason(relativePath) {
  const normalized = relativePath.split(sep).join("/").toLowerCase();
  const segments = normalized.split("/").filter(Boolean);
  if (normalized.includes("codex-retry-gateway") || normalized.includes("codex_retry_gateway")) {
    return "external gateway source path";
  }
  if (segments.includes("node_modules")) return "bundled Node dependency tree";
  const name = segments.at(-1) ?? "";
  if (name === "gateway.mjs") return "external gateway entrypoint";
  if (name === "node" || name === "node.exe" || name === "nodejs") {
    return "bundled Node runtime";
  }
  if (name === "package-lock.json" || name === "npm-shrinkwrap.json") {
    return "external npm dependency lock";
  }
  return null;
}

let entryCount = 0;
const violations = [];
for (const root of roots) {
  if (!isAbsolute(root) || !lstatSync(root).isDirectory()) {
    throw new Error(`Extracted bundle root is not a directory: ${root}`);
  }
  const canonicalRoot = realpathSync(root);
  const pending = [canonicalRoot];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      entryCount += 1;
      if (entryCount > MAX_ENTRIES) {
        throw new Error(`Extracted bundle inventory exceeds ${MAX_ENTRIES} entries.`);
      }
      const path = resolve(current, entry.name);
      const rel = relative(canonicalRoot, path);
      const reason = forbiddenReason(rel);
      if (reason) violations.push(`${rel}: ${reason}`);
      if (entry.isSymbolicLink()) {
        const target = resolve(dirname(path), readlinkSync(path));
        const targetRel = relative(canonicalRoot, target);
        if (targetRel === ".." || targetRel.startsWith(`..${sep}`) || isAbsolute(targetRel)) {
          violations.push(`${rel}: bundle symlink escapes the extracted root`);
        } else {
          const targetReason = forbiddenReason(targetRel);
          if (targetReason) violations.push(`${rel} -> ${targetRel}: ${targetReason}`);
        }
      }
      if (entry.isDirectory() && !entry.isSymbolicLink()) pending.push(path);
    }
  }
}

if (violations.length > 0) {
  throw new Error(
    `Release bundle contains forbidden external gateway material:\n${violations.join("\n")}`
  );
}
console.log(
  `[bundle-inventory] checked ${entryCount} entries across ${roots.length} extracted root(s)`
);
