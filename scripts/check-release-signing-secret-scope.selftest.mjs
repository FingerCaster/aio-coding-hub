import assert from "node:assert/strict";

import { assertReleaseSigningSecretScope } from "./check-release-signing-secret-scope.mjs";

const workflow = `
jobs:
  release-please:
    steps:
      - name: Validate release signing secret scope
        run: node scripts/check-release-signing-secret-scope.selftest.mjs && node scripts/check-release-signing-secret-scope.mjs

  build:
    strategy:
      matrix:
        include: []
    steps:
      - name: Validate updater signing secrets
        shell: bash
        env:
          TAURI_SIGNING_PRIVATE_KEY_SECRET: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD_SECRET: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: |
          set -euo pipefail
          normalized_key="$(printf '%s' "$TAURI_SIGNING_PRIVATE_KEY_SECRET" | tr -d '\\r\\n\\t ')"
          umask 077
          key_path="$RUNNER_TEMP/tauri-updater.key"
          rm -f "$key_path"
          temp_dir="$(mktemp -d)"
          trap 'rm -rf "$temp_dir"' EXIT
          test_path="$temp_dir/signing-probe.txt"
          printf '%s' "$normalized_key" > "$key_path"
          chmod 600 "$key_path"
          pnpm exec tauri signer sign \\
            -f "$key_path" \\
            -p "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD_SECRET" \\
            "$test_path" >/dev/null 2>&1

      - id: tauri
        name: Build signed Tauri candidate
        uses: tauri-apps/tauri-action@pinned
        env:
          TAURI_SIGNING_PRIVATE_KEY: \${{ runner.temp }}/tauri-updater.key
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}

      - name: Delete updater signing key
        if: always()
        shell: bash
        run: rm -f "$RUNNER_TEMP/tauri-updater.key"

      - name: Prepare stable updater assets
        run: node scripts/support-matrix.mjs prepare-stable-assets

  publish:
`;

function expectRejected(name, release, expected) {
  assert.throws(() => assertReleaseSigningSecretScope({ release }), expected, name);
}

assert.doesNotThrow(() => assertReleaseSigningSecretScope({ release: workflow }));

for (const commandFile of [
  "GITHUB_ENV",
  "GITHUB_OUTPUT",
  "GITHUB_PATH",
  "GITHUB_STATE",
  "GITHUB_STEP_SUMMARY",
]) {
  expectRejected(
    `command-file signing data (${commandFile})`,
    workflow.replace(
      'printf \'%s\' "$normalized_key" > "$key_path"',
      `printf 'TAURI_SIGNING_PRIVATE_KEY=%s\\n' "$normalized_key" >> "$${commandFile}"`
    ),
    new RegExp(`must not promote signing data through ${commandFile}`)
  );
}

expectRejected(
  "direct private key secret in build",
  workflow.replace(
    "TAURI_SIGNING_PRIVATE_KEY: \${{ runner.temp }}/tauri-updater.key",
    "TAURI_SIGNING_PRIVATE_KEY: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}"
  ),
  /must receive only the runner-temp signing key path/
);
expectRejected(
  "job-level signing secret",
  workflow.replace(
    "  build:\n    strategy:",
    `  build:
    env:
      TAURI_SIGNING_PRIVATE_KEY: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    strategy:`
  ),
  /must not use job-level environment variables/
);
expectRejected(
  "workspace key path",
  workflow.replaceAll("$RUNNER_TEMP/tauri-updater.key", "$GITHUB_WORKSPACE/tauri-updater.key"),
  /fixed runner-temp signing key path/
);
expectRejected(
  "loose signing key permissions",
  workflow.replace("          umask 077\n", ""),
  /restrict permissions before writing/
);
expectRejected(
  "missing stale-key removal",
  workflow.replace('          rm -f "$key_path"\n', ""),
  /remove a stale runner-temp signing key/
);
expectRejected(
  "password outside signed build",
  workflow.replace(
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}",
    "TAURI_SIGNING_PRIVATE_KEY_PASSWORD: inherited-password"
  ),
  /signed build password must remain step-scoped/
);
expectRejected(
  "missing cleanup",
  workflow.replace(
    `      - name: Delete updater signing key
        if: always()
        shell: bash
        run: rm -f "$RUNNER_TEMP/tauri-updater.key"

`,
    ""
  ),
  /cleanup step is required/
);
expectRejected(
  "cleanup not adjacent",
  workflow.replace(
    "      - name: Delete updater signing key",
    "      - name: Unrelated action\n        run: true\n\n      - name: Delete updater signing key"
  ),
  /cleanup must immediately follow/
);
expectRejected(
  "cleanup only echoed",
  workflow.replace(
    'run: rm -f "$RUNNER_TEMP/tauri-updater.key"',
    'run: echo \'rm -f "$RUNNER_TEMP/tauri-updater.key"\''
  ),
  /cleanup must execute rm/
);
expectRejected(
  "later key reference",
  workflow.replace(
    "      - name: Prepare stable updater assets\n        run:",
    "      - name: Prepare stable updater assets\n        env:\n          TAURI_SIGNING_PRIVATE_KEY: still-visible\n        run:"
  ),
  /steps after signing key cleanup must not reference/
);
expectRejected(
  "release contract disconnected",
  workflow.replace(
    "node scripts/check-release-signing-secret-scope.selftest.mjs && node scripts/check-release-signing-secret-scope.mjs",
    "node scripts/missing-release-signing-contract.mjs"
  ),
  /release-please must execute/
);

console.log("[release-signing-secret-scope:selftest] all assertions passed");
