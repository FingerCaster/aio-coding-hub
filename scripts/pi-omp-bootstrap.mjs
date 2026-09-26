#!/usr/bin/env node
// All installations, npm state, and credentials are isolated under the W0 runtime.
import { mkdir, writeFile, readFile, symlink } from "node:fs/promises";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const runtimeRoot = join(repoRoot, ".trellis/.runtime/research/omp-pi");

export async function isolatedEnv(root) {
  const env = {};
  for (const key of ["SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"]) {
    if (process.env[key]) env[key] = process.env[key];
  }
  const home = join(root, "home");
  const temp = join(root, "tmp");
  const agent = join(home, "agent");
  for (const dir of [home, temp, agent, join(root, "cwd")]) await mkdir(dir, { recursive: true });
  return Object.assign(env, {
    PATH: [
      dirname(process.execPath),
      ...(env.SystemRoot ? [join(env.SystemRoot, "System32")] : ["/usr/bin", "/bin"]),
    ].join(process.platform === "win32" ? ";" : ":"),
    HOME: home,
    USERPROFILE: home,
    HOMEDRIVE: home.slice(0, 2),
    HOMEPATH: home.slice(2),
    APPDATA: join(home, "AppData/Roaming"),
    LOCALAPPDATA: join(home, "AppData/Local"),
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CACHE_HOME: join(home, ".cache"),
    XDG_STATE_HOME: join(home, ".local/state"),
    TEMP: temp,
    TMP: temp,
    TMPDIR: temp,
    PI_CODING_AGENT_DIR: agent,
    PI_CONFIG_DIR: ".omp",
    PI_OFFLINE: "1",
    CI: "1",
    NO_COLOR: "1",
    TERM: "dumb",
    DO_NOT_TRACK: "1",
    OTEL_SDK_DISABLED: "true",
    BUN_INSTALL: join(root, "bun-home"),
    BUN_RUNTIME_TRANSPILER_CACHE_PATH: join(root, "bun-cache"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: join(home, ".gitconfig"),
  });
}

async function main() {
  if (process.platform !== "win32" || process.arch !== "x64")
    throw new Error("This captured runtime currently supports Windows x64 only.");
  const env = await isolatedEnv(join(runtimeRoot, "installer"));
  const archives = [
    [
      "pi",
      "0.87.1",
      "@earendil-works",
      "1423ee3c61e7c96464e1cbf3c8dc24d3056cb3410995c3671a98c3ecc527540f",
    ],
    [
      "omp",
      "18.3.2",
      "@oh-my-pi",
      "a8784d8f9f2524685c505ce66cf5d748d3060d15228dd9aadc3f151309623d59",
    ],
  ];
  for (const [name, version, scope, hash] of archives) {
    const archive = join(runtimeRoot, name + "-" + version + ".tgz");
    if (!existsSync(archive)) {
      const response = await fetch(
        "https://registry.npmjs.org/" +
          scope +
          "/pi-coding-agent/-/pi-coding-agent-" +
          version +
          ".tgz",
        { redirect: "error" }
      );
      if (!response.ok) throw new Error("Pinned archive download failed: " + response.status);
      const bytes = Buffer.from(await response.arrayBuffer());
      if (createHash("sha256").update(bytes).digest("hex") !== hash)
        throw new Error("Pinned archive digest mismatch: " + name);
      await writeFile(archive, bytes);
    }
    if (
      createHash("sha256")
        .update(await readFile(archive))
        .digest("hex") !== hash
    )
      throw new Error("Local archive digest mismatch: " + name);
    const destination = join(runtimeRoot, name);
    await mkdir(destination, { recursive: true });
    const extraction = spawn("tar", ["-xf", archive, "-C", destination], {
      env,
      stdio: "inherit",
      windowsHide: true,
    });
    const code = await new Promise((res, rej) => {
      extraction.on("error", rej);
      extraction.on("close", res);
    });
    if (code !== 0) throw new Error("Pinned archive extraction failed: " + name);
  }
  const npmCli =
    process.argv[2] || join(dirname(process.execPath), "node_modules/npm/bin/npm-cli.js");
  if (!existsSync(npmCli)) throw new Error("Pass the local npm-cli.js path as argument 1.");
  const prefix = join(runtimeRoot, "tools");
  await mkdir(prefix, { recursive: true });
  const emptyConfig = join(runtimeRoot, "installer/empty.npmrc");
  await writeFile(emptyConfig, "");
  await writeFile(
    join(prefix, "package.json"),
    JSON.stringify({ name: "pi-omp-wire-research", private: true, version: "0.0.0" })
  );
  const packages = process.argv.slice(3);
  if (!packages.length)
    packages.push(
      "@oven/bun-windows-x64@1.3.14",
      "@earendil-works/chord@0.87.1",
      "@silvia-odwyer/photon-node@0.3.4",
      "undici@8.10.2",
      "typebox@1.3.27",
      "@oh-my-pi/pi-natives@18.3.2",
      "@oh-my-pi/omptype@18.3.2",
      "@babel/parser@7.29.7"
    );
  const args = [
    npmCli,
    "install",
    "--prefix",
    prefix,
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "--save-exact",
    "--userconfig",
    emptyConfig,
    "--globalconfig",
    emptyConfig + ".global",
    "--cache",
    join(runtimeRoot, "npm-cache"),
    "--registry",
    "https://registry.npmjs.org",
    ...packages,
  ];
  await writeFile(emptyConfig + ".global", "");
  const child = spawn(process.execPath, args, {
    cwd: prefix,
    env,
    stdio: "inherit",
    windowsHide: true,
  });
  const result = await new Promise((res, rej) => {
    child.on("error", rej);
    child.on("close", (code, signal) => res({ code, signal }));
  });
  if (result.code !== 0) throw new Error(`Local install failed: ${JSON.stringify(result)}`);
  for (const cli of ["pi", "omp"]) {
    const link = join(runtimeRoot, cli, "package/node_modules");
    if (!existsSync(link)) await symlink(join(prefix, "node_modules"), link, "junction");
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
