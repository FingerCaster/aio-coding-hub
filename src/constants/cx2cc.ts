import type { Cx2ccReasoningEffortMapping } from "../generated/bindings";

export type { Cx2ccReasoningEffortMapping } from "../generated/bindings";

export const CX2CC_PROVIDER_DEFAULT_MODEL = "gpt-5.6-sol";

export const CX2CC_REASONING_EFFORT_MAPPING_MAX_RULES = 32;
export const CX2CC_REASONING_EFFORT_MAPPING_MAX_BYTES = 64;

export const DEFAULT_CX2CC_REASONING_EFFORT_MAPPINGS = [
  { source: "low", target: "low" },
  { source: "medium", target: "medium" },
  { source: "high", target: "high" },
  { source: "xhigh", target: "xhigh" },
  { source: "max", target: "max" },
  { source: "ultra", target: "max" },
] as const satisfies readonly Cx2ccReasoningEffortMapping[];

export function createDefaultCx2ccReasoningEffortMappings(): Cx2ccReasoningEffortMapping[] {
  return DEFAULT_CX2CC_REASONING_EFFORT_MAPPINGS.map((mapping) => ({ ...mapping }));
}

export const CX2CC_CONTEXT_WINDOW_MIN = 1_024;
export const CX2CC_CONTEXT_WINDOW_MAX = 10_000_000;

export const CX2CC_RESPONSES_MODEL_PRESETS = [
  CX2CC_PROVIDER_DEFAULT_MODEL,
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4",
] as const;
