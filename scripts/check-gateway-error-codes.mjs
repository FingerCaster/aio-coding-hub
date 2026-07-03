// Validates that gateway error codes in Rust (error_code.rs) and TypeScript
// (gatewayErrorCodes.ts) are in sync. Exits non-zero on drift.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

const rustPath = join(repoRoot, "src-tauri/src/gateway/proxy/error_code.rs");
const tsPath = join(repoRoot, "src/constants/gatewayErrorCodes.ts");

function parseRustCodes(source) {
  const asStrBlock = source.match(/pub.*const fn as_str\(self\)[^{]*\{([\s\S]*?)\n    \}/);
  if (!asStrBlock) {
    throw new Error("Cannot parse as_str() in Rust error_code.rs.");
  }
  const codes = [];
  for (const match of asStrBlock[1].matchAll(/"(GW_[A-Z0-9_]+)"/g)) {
    codes.push(match[1]);
  }
  return [...new Set(codes)].sort();
}

function parseTsCodes(source) {
  const block = source.match(/export const GatewayErrorCodes\s*=\s*\{([\s\S]*?)\}\s*as\s*const/);
  if (!block) {
    throw new Error("Cannot parse GatewayErrorCodes object in TS file.");
  }
  const codes = [];
  for (const match of block[1].matchAll(/"(GW_[A-Z0-9_]+)"/g)) {
    codes.push(match[1]);
  }
  return [...new Set(codes)].sort();
}

function parseTsShortLabelCodes(source) {
  const block = source.match(
    /(?:export\s+)?const GatewayErrorShortLabels\s*=\s*\{([\s\S]*?)\}\s*satisfies/
  );
  if (!block) {
    throw new Error("Cannot parse GatewayErrorShortLabels object in TS file.");
  }
  const codes = [];
  for (const match of block[1].matchAll(/\[GatewayErrorCodes\.([A-Z0-9_]+)\]/g)) {
    codes.push(match[1]);
  }
  return [...new Set(codes)].sort();
}

function parseTsDescriptionCodes(source) {
  const block = source.match(
    /export const GatewayErrorDescriptions\s*=\s*\{([\s\S]*?)\}\s*satisfies/
  );
  if (!block) {
    throw new Error("Cannot parse GatewayErrorDescriptions object in TS file.");
  }
  const codes = [];
  for (const match of block[1].matchAll(/^\s*(GW_[A-Z0-9_]+)\s*:/gm)) {
    codes.push(match[1]);
  }
  return [...new Set(codes)].sort();
}

const rustSource = readFileSync(rustPath, "utf8");
const tsSource = readFileSync(tsPath, "utf8");

const rustCodes = parseRustCodes(rustSource);
const tsCodes = parseTsCodes(tsSource);
const tsCodeNames = Object.fromEntries(
  Array.from(tsSource.matchAll(/^\s*([A-Z0-9_]+):\s*"(GW_[A-Z0-9_]+)"/gm), (match) => [
    match[1],
    match[2],
  ])
);
const tsShortLabelCodeNames = parseTsShortLabelCodes(tsSource);
const tsShortLabelCodes = tsShortLabelCodeNames.map((name) => tsCodeNames[name] ?? name).sort();
const tsDescCodes = parseTsDescriptionCodes(tsSource);

let failed = false;

const missingInTs = rustCodes.filter((c) => !tsCodes.includes(c));
const extraInTs = tsCodes.filter((c) => !rustCodes.includes(c));

if (missingInTs.length > 0) {
  console.error(
    `ERROR: ${missingInTs.length} Rust error code(s) missing from GatewayErrorCodes in TS:`
  );
  for (const c of missingInTs) console.error(`  - ${c}`);
  failed = true;
}

if (extraInTs.length > 0) {
  console.error(`ERROR: ${extraInTs.length} TS error code(s) not found in Rust:`);
  for (const c of extraInTs) console.error(`  - ${c}`);
  failed = true;
}

const missingDesc = tsCodes.filter((c) => !tsDescCodes.includes(c));
const extraDesc = tsDescCodes.filter((c) => !tsCodes.includes(c));
if (missingDesc.length > 0) {
  console.error(
    `ERROR: ${missingDesc.length} code(s) in GatewayErrorCodes missing from GatewayErrorDescriptions:`
  );
  for (const c of missingDesc) console.error(`  - ${c}`);
  failed = true;
}

if (extraDesc.length > 0) {
  console.error(
    `ERROR: ${extraDesc.length} GatewayErrorDescriptions code(s) not found in GatewayErrorCodes:`
  );
  for (const c of extraDesc) console.error(`  - ${c}`);
  failed = true;
}

const missingShortLabels = tsCodes.filter((c) => !tsShortLabelCodes.includes(c));
const extraShortLabels = tsShortLabelCodes.filter((c) => !tsCodes.includes(c));
if (missingShortLabels.length > 0) {
  console.error(
    `ERROR: ${missingShortLabels.length} code(s) in GatewayErrorCodes missing from GatewayErrorShortLabels:`
  );
  for (const c of missingShortLabels) console.error(`  - ${c}`);
  failed = true;
}

if (extraShortLabels.length > 0) {
  console.error(
    `ERROR: ${extraShortLabels.length} GatewayErrorShortLabels code(s) not found in GatewayErrorCodes:`
  );
  for (const c of extraShortLabels) console.error(`  - ${c}`);
  failed = true;
}

const unknownShortLabelNames = tsShortLabelCodeNames.filter((name) => !tsCodeNames[name]);
if (unknownShortLabelNames.length > 0) {
  console.error(
    `ERROR: ${unknownShortLabelNames.length} GatewayErrorShortLabels key(s) not found in GatewayErrorCodes:`
  );
  for (const name of unknownShortLabelNames) console.error(`  - ${name}`);
  failed = true;
}

if (failed) {
  console.error(
    "\nFix: update src/constants/gatewayErrorCodes.ts to match src-tauri/src/gateway/proxy/error_code.rs"
  );
  process.exit(1);
}

console.log(
  `OK: ${rustCodes.length} gateway error codes in sync (Rust ↔ TS codes ↔ TS short labels ↔ TS descriptions).`
);
