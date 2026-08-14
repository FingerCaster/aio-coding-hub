export const CX2CC_PROVIDER_DEFAULT_MODEL = "gpt-5.6-sol";

export const CX2CC_CONTEXT_WINDOW_MIN = 1_024;
export const CX2CC_CONTEXT_WINDOW_MAX = 10_000_000;

export const CX2CC_RESPONSES_MODEL_PRESETS = [
  CX2CC_PROVIDER_DEFAULT_MODEL,
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4",
] as const;
