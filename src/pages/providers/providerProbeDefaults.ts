// Usage: Probe suggestions use concrete remote model names owned by the fork.
import type { ProviderSummary } from "../../services/providers/providers";
export const DEFAULT_PROBE_PROMPT = "hi";
export function defaultProbeModel(provider: ProviderSummary): string {
  return provider.availability_test_model?.trim() ?? "";
}
export function probeModelCandidates(
  provider: ProviderSummary,
  catalogModels: string[] = []
): string[] {
  const configured = defaultProbeModel(provider);
  const claude = Object.values(provider.claude_models ?? {}).filter(
    (value): value is string => typeof value === "string"
  );
  return [
    ...new Set(
      [configured, ...catalogModels, ...claude]
        .map((value) => value.trim())
        .filter((value) => value && !value.includes("*") && !value.startsWith("aio/"))
    ),
  ];
}
