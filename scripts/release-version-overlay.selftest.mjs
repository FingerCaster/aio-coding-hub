import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  RELEASE_VERSION_FILES,
  applyReleaseVersionOverlay,
  verifyReleaseVersionAttestations,
} from "./release-version-overlay.mjs";

const sourceSha = "b".repeat(40);
const platforms = ["windows-x86_64", "darwin-x86_64", "darwin-aarch64", "linux-x86_64"];
const root = mkdtempSync(join(tmpdir(), "aio-release-version-overlay-"));

function run(cwd, command, args) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed (${result.status}): ${result.stderr || result.stdout}`
    );
  }
  return result.stdout;
}

function createFixture(name, version) {
  const directory = join(root, name);
  mkdirSync(join(directory, "src-tauri/src"), { recursive: true });
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({ name: "aio-coding-hub", private: true, version }, null, 2)}\n`
  );
  writeFileSync(
    join(directory, "src-tauri/tauri.conf.json"),
    `${JSON.stringify({ productName: "AIO Coding Hub", version, identifier: "test" }, null, 2)}\n`
  );
  writeFileSync(
    join(directory, "src-tauri/Cargo.toml"),
    `[package]\nname = "aio-coding-hub"\nversion = "${version}"\nedition = "2021"\n\n[dependencies]\n`
  );
  writeFileSync(join(directory, "src-tauri/src/lib.rs"), "pub fn fixture() {}\n");
  writeFileSync(join(directory, "README.md"), "fixture\n");
  run(directory, "cargo", [
    "generate-lockfile",
    "--manifest-path",
    join(directory, "src-tauri/Cargo.toml"),
  ]);
  run(directory, "git", ["init", "--initial-branch=main"]);
  run(directory, "git", ["config", "user.name", "Version Overlay Selftest"]);
  run(directory, "git", ["config", "user.email", "version-overlay@example.invalid"]);
  run(directory, "git", ["add", "."]);
  run(directory, "git", ["commit", "-m", "fixture"]);
  return directory;
}

try {
  const betaOne = createFixture("beta-one", "0.60.40");
  const betaOneOutput = join(root, "beta-one-attestation.json");
  const betaOneAttestation = applyReleaseVersionOverlay({
    repoRoot: betaOne,
    channel: "beta",
    tag: "aio-coding-hub-v0.60.41-beta.1",
    sourceSha,
    outputPath: betaOneOutput,
  });
  assert.equal(betaOneAttestation.version, "0.60.41-beta.1");
  assert.equal(
    JSON.parse(readFileSync(join(betaOne, "package.json"), "utf8")).version,
    betaOneAttestation.version
  );
  assert.equal(
    JSON.parse(readFileSync(join(betaOne, "src-tauri/tauri.conf.json"), "utf8")).version,
    betaOneAttestation.version
  );
  assert.match(
    readFileSync(join(betaOne, "src-tauri/Cargo.toml"), "utf8"),
    /version = "0\.60\.41-beta\.1"/
  );
  assert.match(
    readFileSync(join(betaOne, "src-tauri/Cargo.lock"), "utf8"),
    /name = "aio-coding-hub"\nversion = "0\.60\.41-beta\.1"/
  );
  const changed = run(betaOne, "git", ["diff", "--name-only"]).trim().split(/\r?\n/).sort();
  assert.deepEqual(changed, [...RELEASE_VERSION_FILES].sort());

  const betaTwo = createFixture("beta-two", "0.60.40");
  const betaTwoOutput = join(root, "beta-two-attestation.json");
  applyReleaseVersionOverlay({
    repoRoot: betaTwo,
    channel: "beta",
    tag: "aio-coding-hub-v0.60.41-beta.1",
    sourceSha,
    outputPath: betaTwoOutput,
  });
  assert.equal(
    readFileSync(betaOneOutput, "utf8"),
    readFileSync(betaTwoOutput, "utf8"),
    "independent platform checkouts must emit identical attestations"
  );

  const attestationDirectory = join(root, "platform-attestations");
  mkdirSync(attestationDirectory);
  for (const platform of platforms) {
    writeFileSync(
      join(attestationDirectory, `release-version-attestation-${platform}.json`),
      readFileSync(betaOneOutput)
    );
  }
  assert.equal(
    verifyReleaseVersionAttestations({
      directory: attestationDirectory,
      platforms,
      channel: "beta",
      tag: "aio-coding-hub-v0.60.41-beta.1",
      sourceSha,
    }).overlayDigest,
    betaOneAttestation.overlayDigest
  );
  writeFileSync(
    join(attestationDirectory, "release-version-attestation-linux-x86_64.json"),
    readFileSync(betaOneOutput, "utf8").replace(sourceSha, "c".repeat(40))
  );
  assert.throws(
    () =>
      verifyReleaseVersionAttestations({
        directory: attestationDirectory,
        platforms,
        channel: "beta",
        tag: "aio-coding-hub-v0.60.41-beta.1",
        sourceSha,
      }),
    /overlay digest|Cross-platform/
  );

  const stable = createFixture("stable", "0.60.41");
  const stableOutput = join(root, "stable-attestation.json");
  const stableAttestation = applyReleaseVersionOverlay({
    repoRoot: stable,
    channel: "stable",
    tag: "aio-coding-hub-v0.60.41",
    sourceSha,
    outputPath: stableOutput,
  });
  assert.equal(stableAttestation.overlayMode, "none");
  assert.equal(run(stable, "git", ["status", "--porcelain"]), "");

  const dirty = createFixture("dirty", "0.60.40");
  writeFileSync(join(dirty, "README.md"), "unexpected diff\n");
  assert.throws(
    () =>
      applyReleaseVersionOverlay({
        repoRoot: dirty,
        channel: "beta",
        tag: "aio-coding-hub-v0.60.41-beta.1",
        sourceSha,
        outputPath: join(root, "dirty-attestation.json"),
      }),
    /requires a clean checkout/
  );

  const inconsistent = createFixture("inconsistent", "0.60.40");
  const tauriPath = join(inconsistent, "src-tauri/tauri.conf.json");
  const tauri = JSON.parse(readFileSync(tauriPath, "utf8"));
  tauri.version = "0.60.39";
  writeFileSync(tauriPath, `${JSON.stringify(tauri, null, 2)}\n`);
  run(inconsistent, "git", ["add", tauriPath]);
  run(inconsistent, "git", ["commit", "-m", "inconsistent version"]);
  assert.throws(
    () =>
      applyReleaseVersionOverlay({
        repoRoot: inconsistent,
        channel: "beta",
        tag: "aio-coding-hub-v0.60.41-beta.1",
        sourceSha,
        outputPath: join(root, "inconsistent-attestation.json"),
      }),
    /do not agree/
  );
} finally {
  rmSync(root, { recursive: true, force: true });
}

console.log("[release-version-overlay:selftest] deterministic four-file overlay passed");
