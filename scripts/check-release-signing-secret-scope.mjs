import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const modulePath = fileURLToPath(import.meta.url);
const repoRoot = dirname(dirname(modulePath));
const contractCommand =
  "node scripts/check-release-signing-secret-scope.selftest.mjs && node scripts/check-release-signing-secret-scope.mjs";
const crossStepCommandFiles = [
  "GITHUB_ENV",
  "GITHUB_OUTPUT",
  "GITHUB_PATH",
  "GITHUB_STATE",
  "GITHUB_STEP_SUMMARY",
];

function jobBlock(source, jobName) {
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex((line) => line === `  ${jobName}:`);
  if (start === -1) return "";

  let end = start + 1;
  while (end < lines.length && !/^  [A-Za-z0-9_-]+:\s*$/.test(lines[end])) end += 1;
  return lines.slice(start, end).join("\n");
}

function executableLines(source) {
  return source
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

function stepBlocks(job) {
  const lines = job.split(/\r?\n/);
  const starts = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (/^      - /.test(lines[index])) starts.push(index);
  }
  return starts.map((start, index) => {
    const end = starts[index + 1] ?? lines.length;
    return lines.slice(start, end).join("\n");
  });
}

function stepName(step) {
  return step.match(/^\s+(?:-\s+)?name:\s*(.+)$/m)?.[1]?.trim() ?? null;
}

function namedStepIndex(steps, name) {
  return steps.findIndex((step) => stepName(step) === name);
}

function requireLine(lines, expected, message, failures) {
  if (!lines.includes(expected)) failures.push(message);
}

function hasRunCommand(source, command) {
  return executableLines(source).some((line) => {
    const run = line.match(/^(?:-\s+)?run:\s*(.+)$/);
    return run?.[1]?.trim() === command;
  });
}

function jobLevelSigningEnvironment(job) {
  const lines = job.split(/\r?\n/);
  const start = lines.findIndex((line) => line === "    env:");
  if (start === -1) return [];
  const result = [];
  for (let index = start + 1; index < lines.length; index += 1) {
    if (lines[index].trim() === "") continue;
    if (!/^      /.test(lines[index])) break;
    result.push(lines[index].trim());
  }
  return result.filter((line) => line.includes("TAURI_SIGNING_"));
}

export function validateReleaseSigningSecretScope({ release }) {
  const failures = [];
  const buildJob = jobBlock(release, "build");
  const releasePleaseJob = jobBlock(release, "release-please");
  if (!buildJob) failures.push("build job is required");

  const steps = stepBlocks(buildJob);
  const validateIndex = namedStepIndex(steps, "Validate updater signing secrets");
  const buildIndex = namedStepIndex(steps, "Build signed Tauri candidate");
  const cleanupIndex = namedStepIndex(steps, "Delete updater signing key");

  if (validateIndex === -1) failures.push("signing secret validation step is required");
  if (buildIndex === -1) failures.push("signed Tauri build step is required");
  if (cleanupIndex === -1) failures.push("signing key cleanup step is required");

  const validate = executableLines(steps[validateIndex] ?? "");
  const build = executableLines(steps[buildIndex] ?? "");
  const cleanup = executableLines(steps[cleanupIndex] ?? "");
  const candidate = executableLines(buildJob);

  const jobSigningEnvironment = jobLevelSigningEnvironment(buildJob);
  if (jobSigningEnvironment.length > 0) {
    failures.push("release build signing secrets must not use job-level environment variables");
  }
  for (const commandFile of crossStepCommandFiles) {
    if (candidate.some((line) => line.includes(commandFile))) {
      failures.push(`release build must not promote signing data through ${commandFile}`);
    }
  }

  requireLine(
    validate,
    "umask 077",
    "validation must restrict permissions before writing the signing key",
    failures
  );
  requireLine(
    validate,
    'key_path="$RUNNER_TEMP/tauri-updater.key"',
    "validation must use the fixed runner-temp signing key path",
    failures
  );
  requireLine(
    validate,
    'rm -f "$key_path"',
    "validation must remove a stale runner-temp signing key",
    failures
  );
  requireLine(
    validate,
    'printf \'%s\' "$normalized_key" > "$key_path"',
    "validation must write the normalized key only to the runner-temp file",
    failures
  );
  requireLine(
    validate,
    'chmod 600 "$key_path"',
    "runner-temp signing key must be mode 600",
    failures
  );
  if (!validate.some((line) => line.startsWith('-f "$key_path"'))) {
    failures.push("signer probe must read the runner-temp signing key file");
  }

  requireLine(
    build,
    "TAURI_SIGNING_PRIVATE_KEY: ${{ runner.temp }}/tauri-updater.key",
    "signed build must receive only the runner-temp signing key path",
    failures
  );
  requireLine(
    build,
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}",
    "signed build password must remain step-scoped",
    failures
  );

  const privateKeySecret = "${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}";
  const passwordSecret = "${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}";
  requireLine(
    validate,
    "TAURI_SIGNING_PRIVATE_KEY_SECRET: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}",
    "validation must receive the private key through its step-scoped environment",
    failures
  );
  requireLine(
    validate,
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD_SECRET: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}",
    "validation must receive the password through its step-scoped environment",
    failures
  );
  if (
    candidate.filter((line) => line.includes(privateKeySecret)).length !== 1 ||
    validate.filter((line) => line.includes(privateKeySecret)).length !== 1
  ) {
    failures.push("private key secret must be referenced only by the validation step");
  }
  if (
    candidate.filter((line) => line.includes(passwordSecret)).length !== 2 ||
    validate.filter((line) => line.includes(passwordSecret)).length !== 1 ||
    build.filter((line) => line.includes(passwordSecret)).length !== 1
  ) {
    failures.push("signing password secret must be referenced only by validation and signed build");
  }

  if (validateIndex === -1 || buildIndex === -1 || validateIndex >= buildIndex) {
    failures.push("signing secret validation must run before the signed build");
  }
  if (buildIndex === -1 || cleanupIndex !== buildIndex + 1) {
    failures.push("signing key cleanup must immediately follow the signed build");
  }
  requireLine(cleanup, "if: always()", "signing key cleanup must run with always()", failures);
  requireLine(
    cleanup,
    "shell: bash",
    "signing key cleanup must use bash on every runner",
    failures
  );
  requireLine(
    cleanup,
    'run: rm -f "$RUNNER_TEMP/tauri-updater.key"',
    "signing key cleanup must execute rm on the fixed runner-temp file",
    failures
  );

  const later = executableLines(steps.slice(cleanupIndex + 1).join("\n"));
  if (
    later.some(
      (line) => line.includes("TAURI_SIGNING_PRIVATE_KEY") || line.includes("tauri-updater.key")
    )
  ) {
    failures.push("steps after signing key cleanup must not reference the private key");
  }

  if (!hasRunCommand(releasePleaseJob, contractCommand)) {
    failures.push("release-please must execute the release signing secret scope contract");
  }
  return failures;
}

export function assertReleaseSigningSecretScope(files) {
  const failures = validateReleaseSigningSecretScope(files);
  if (failures.length > 0) {
    throw new Error(`Release signing secret scope contract failed:\n- ${failures.join("\n- ")}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === modulePath) {
  assertReleaseSigningSecretScope({
    release: readFileSync(resolve(repoRoot, ".github/workflows/release.yml"), "utf8"),
  });
  console.log("[release-signing-secret-scope] workflow contract passed");
}
