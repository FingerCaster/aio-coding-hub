import type { NativeCliKey } from "../../../constants/clis";
import type {
  GatewayProtocol,
  ModelCapabilitySuggestion,
  ThinkingSpec,
} from "../../../services/nativeChannels";

export const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"] as const;
export const OMP_THINKING_MODES = {
  effort: "思考等级（effort）",
  budget: "Token 预算（budget）",
  "google-level": "Google 思考等级",
  "anthropic-adaptive": "Anthropic 自适应思考",
  "anthropic-budget-effort": "Anthropic 预算与等级",
} as const;

export function emptyThinking(client: NativeCliKey): ThinkingSpec {
  return client === "pi"
    ? { client, levelMap: Object.fromEntries(THINKING_LEVELS.map((level) => [level, null])) }
    : {
        client,
        mode: "",
        efforts: [],
        defaultLevel: null,
        effortMap: {},
        supportsDisplay: null,
        requiresEffort: null,
      };
}

export function suggestedThinking(
  client: NativeCliKey,
  protocol: GatewayProtocol,
  suggestion: ModelCapabilitySuggestion
): ThinkingSpec | null {
  if (suggestion.reasoning === true && suggestion.nativeThinking?.client === client)
    return structuredClone(suggestion.nativeThinking);
  if (suggestion.reasoning !== true || !suggestion.reasoningEfforts?.length) return null;
  const supported = suggestion.reasoningEfforts;
  if (client === "pi") {
    const levelMap = Object.fromEntries(
      THINKING_LEVELS.map((level) => [
        level,
        supported.includes(level)
          ? level
          : level === "off" && supported.includes("none")
            ? "none"
            : null,
      ])
    );
    return THINKING_LEVELS.slice(1).some((level) => levelMap[level]) ? { client, levelMap } : null;
  }
  const efforts = THINKING_LEVELS.filter((level) => level !== "off" && supported.includes(level));
  if (!efforts.length) return null;
  const mode =
    suggestion.thinkingMode ??
    (protocol === "openai-responses" || protocol === "openai-completions"
      ? "effort"
      : protocol === "google-generative-ai"
        ? "google-level"
        : "");
  return {
    client,
    mode,
    efforts,
    effortMap: Object.fromEntries(efforts.map((level) => [level, level])),
    defaultLevel:
      efforts.find((level) => level === suggestion.defaultReasoningEffort) ??
      efforts.find((level) => level === "medium") ??
      efforts.find((level) => level === "low") ??
      efforts[0],
    supportsDisplay: suggestion.supportsDisplay,
    requiresEffort: suggestion.requiresEffort,
  };
}

export interface EditableModelItem {
  id: string;
  requestModelId: string;
  displayName: string;
  input: ("text" | "image")[];
  contextWindow: number | "";
  maxTokens: number | "";
  supportsTools: boolean | null;
  reasoning: boolean | null;
  thinking: ThinkingSpec | null;
  edited: Partial<
    Record<
      | "displayName"
      | "input"
      | "contextWindow"
      | "maxTokens"
      | "supportsTools"
      | "reasoning"
      | "thinking",
      boolean
    >
  >;
}

export function fillModelSuggestion(
  item: EditableModelItem,
  suggestion: ModelCapabilitySuggestion | undefined,
  client: NativeCliKey,
  protocol: GatewayProtocol
): EditableModelItem {
  if (!suggestion || item.requestModelId.trim() !== suggestion.modelId) return item;
  const next = { ...item };
  const canFill = (field: keyof EditableModelItem["edited"]) => !item.edited[field];
  if (
    suggestion.sources.some(
      (source) => source === "routing_confirmation" || source === "upstream_conflict"
    )
  ) {
    if (canFill("contextWindow")) next.contextWindow = "";
    if (canFill("maxTokens")) next.maxTokens = "";
    if (canFill("supportsTools")) next.supportsTools = null;
    if (canFill("reasoning")) next.reasoning = null;
    if (canFill("thinking")) next.thinking = null;
    if (canFill("input")) next.input = ["text"];
    return next;
  }
  if (canFill("displayName") && suggestion.displayName) next.displayName = suggestion.displayName;
  if (canFill("input") && suggestion.input?.includes("text"))
    next.input = suggestion.input.includes("image") ? ["text", "image"] : ["text"];
  if (canFill("contextWindow") && suggestion.contextWindow !== null)
    next.contextWindow = suggestion.contextWindow;
  if (canFill("maxTokens") && suggestion.maxTokens !== null) next.maxTokens = suggestion.maxTokens;
  if (canFill("supportsTools") && suggestion.supportsTools !== null)
    next.supportsTools = suggestion.supportsTools;
  if (canFill("reasoning") && suggestion.reasoning !== null) next.reasoning = suggestion.reasoning;
  if (canFill("thinking")) {
    if (next.reasoning === false) next.thinking = null;
    else if (next.reasoning === true)
      next.thinking = suggestedThinking(client, protocol, suggestion) ?? item.thinking;
  }
  return next;
}
