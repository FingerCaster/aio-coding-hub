// Usage: node scripts/generate-native-model-defaults.mjs <pi-ai package> <OMP checkout> <Pi checkout>
// Regenerate from reviewed local sources only; never fetch or run a CLI during discovery.
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
const [piRoot, ompRoot, piSourceRoot] = process.argv.slice(2);
if (!piRoot || !ompRoot || !piSourceRoot)
  throw Error("Provide pi-ai package, OMP source and Pi source directories");
const read = async (path) => JSON.parse(await readFile(path, "utf8"));
const piVersion = (await read(resolve(piRoot, "package.json"))).version;
const ompVersion = (await read(resolve(ompRoot, "packages/ai/package.json"))).version;
const ompBytes = await readFile(resolve(ompRoot, "packages/catalog/src/models.json"));
const ompModels = JSON.parse(ompBytes.toString("utf8"));
const levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
const providers = ["openai", "openai-codex", "anthropic", "google", "xai", "xai-oauth"];
const modes = ["effort", "budget", "google-level", "anthropic-adaptive", "anthropic-budget-effort"];
const rows = [];
const files = {};
for (const client of ["pi", "omp"]) {
  for (const provider of providers) {
    let models;
    if (client === "pi") {
      // Pi no longer bundles an OAuth xAI provider; do not borrow a different auth catalog.
      if (provider === "xai-oauth") continue;
      const path = resolve(piRoot, "dist/providers/data", provider + ".json");
      const bytes = await readFile(path);
      files[provider] = createHash("sha256").update(bytes).digest("hex");
      models = Object.assign({}, ...Object.values(JSON.parse(bytes.toString("utf8"))));
    } else models = ompModels[provider];
    for (const model of Object.values(models)) {
      const protocol = model.api === "openai-codex-responses" ? "openai-responses" : model.api;
      if (
        ![
          "openai-responses",
          "openai-completions",
          "anthropic-messages",
          "google-generative-ai",
        ].includes(protocol) ||
        !model.input.includes("text") ||
        !Number.isInteger(model.contextWindow) ||
        !Number.isInteger(model.maxTokens) ||
        model.maxTokens < 1 ||
        model.maxTokens > model.contextWindow
      )
        continue;
      let nativeThinking = null;
      let thinkingMode = null;
      if (model.reasoning && client === "pi") {
        // Mirrors Pi getSupportedThinkingLevels: absent ordinary levels are supported;
        // xhigh/max must be explicitly mapped. Preserve non-identity and uppercase mappings.
        const map = model.thinkingLevelMap ?? {};
        nativeThinking = {
          client,
          levelMap: Object.fromEntries(
            levels.map((level) => [
              level,
              map[level] === null || (["xhigh", "max"].includes(level) && map[level] === undefined)
                ? null
                : (map[level] ?? level),
            ])
          ),
        };
      } else if (
        model.reasoning &&
        client === "omp" &&
        modes.includes(model.thinking?.mode) &&
        model.thinking.efforts?.length
      ) {
        const t = model.thinking;
        const efforts = t.efforts.filter((v) => levels.includes(v) && v !== "off");
        if (!efforts.length) continue;
        thinkingMode = t.mode;
        nativeThinking = {
          client,
          mode: t.mode,
          efforts,
          defaultLevel: efforts.includes(t.defaultLevel)
            ? t.defaultLevel
            : efforts.includes("medium")
              ? "medium"
              : efforts.includes("low")
                ? "low"
                : efforts[0],
          effortMap: Object.fromEntries(
            efforts.map((level) => [level, t.effortMap?.[level] ?? level])
          ),
          supportsDisplay: t.supportsDisplay ?? null,
          requiresEffort: t.requiresEffort ?? null,
        };
      }
      const mapped =
        nativeThinking?.client === "pi"
          ? Object.values(nativeThinking.levelMap)
          : nativeThinking?.client === "omp"
            ? Object.values(nativeThinking.effortMap)
            : [];
      const toolsModel = client === "omp" ? model : ompModels[provider]?.[model.id];
      const suggestion = {
        modelId: model.id,
        displayName: model.name,
        input: model.input.includes("image") ? ["text", "image"] : ["text"],
        contextWindow: model.contextWindow,
        maxTokens: model.maxTokens,
        // OMP catalog types explicitly define absent supportsTools as native tool support.
        // This rule applies only to this versioned catalog, never an arbitrary /models row.
        supportsTools: toolsModel ? toolsModel.supportsTools !== false : null,
        reasoning: model.reasoning,
        reasoningEfforts: mapped.length ? [...new Set(mapped.filter((v) => v != null))] : null,
        defaultReasoningEffort:
          nativeThinking?.client === "omp"
            ? nativeThinking.effortMap[nativeThinking.defaultLevel]
            : null,
        thinkingMode,
        supportsDisplay: nativeThinking?.supportsDisplay ?? null,
        requiresEffort: nativeThinking?.requiresEffort ?? null,
        nativeThinking,
        sources: [
          client + "_catalog:" + (client === "pi" ? piVersion : ompVersion) + ":" + provider,
        ],
      };
      rows.push({ client, provider, protocol, suggestion });
    }
  }
}
rows.sort((a, b) =>
  JSON.stringify([a.client, a.provider, a.protocol, a.suggestion.modelId]).localeCompare(
    JSON.stringify([b.client, b.provider, b.protocol, b.suggestion.modelId])
  )
);
const result = {
  provenance: {
    piVersion,
    ompVersion,
    ompCommit: execFileSync("git", ["-C", ompRoot, "rev-parse", "HEAD"], {
      encoding: "utf8",
    }).trim(),
    piFilesSha256: files,
    ompCatalogSha256: createHash("sha256").update(ompBytes).digest("hex"),
    piSource: execFileSync("git", ["-C", piSourceRoot, "remote", "get-url", "origin"], {
      encoding: "utf8",
    }).trim(),
    ompSource: "https://github.com/can1357/oh-my-pi",
    note: "Reviewed catalog defaults, not an upstream availability guarantee. Exact ID matching only; user and upstream overrides take precedence.",
  },
  rows,
};
const destination = resolve("src-tauri/src/app/native_model_defaults.json");
await writeFile(destination, JSON.stringify(result, null, 2) + "\n");
await writeFile(
  resolve("src-tauri/resources/native-model-defaults.LICENSE.txt"),
  "Pi model data and thinking-level semantics:\n" +
    (await readFile(resolve(piSourceRoot, "LICENSE"), "utf8")) +
    "\nOMP model data and capability semantics:\n" +
    (await readFile(resolve(ompRoot, "LICENSE"), "utf8"))
);
console.log(JSON.stringify({ destination, rows: rows.length, piVersion, ompVersion }));
