import type { OmpAgentSummary, OmpSettingsSnapshot } from "../../../services/ompSettings";
import { record, ROLE_LABELS, selectorText, type SettingsValues } from "./ompSettingsForm";

/** Configuration preview only: never treats the catalog as an authenticated model pool. */
export type OmpModelPreview = {
  kind: "model" | "chain" | "runtime" | "unresolved";
  label: string;
  selectors: string[];
  source: string;
  description: string;
  thinking: string;
};
type Resolution = {
  selectors: string[];
  trail: string[];
  explanation?: string;
  runtime?: boolean;
};
type Context = { values: SettingsValues; snapshot: OmpSettingsSnapshot };
const hasOwn = (value: object, key: string) => Object.prototype.hasOwnProperty.call(value, key);

export function splitOmpModelSelector(
  value: string,
  snapshot: Pick<OmpSettingsSnapshot, "models">
) {
  // A catalog ID may itself end in a token such as :high or :auto.
  if (snapshot.models.some((model) => model.selector === value)) return { base: value, effort: "" };
  const match = !value.includes(",")
    ? /^(.*):(off|minimal|low|medium|high|xhigh|max|auto)$/.exec(value)
    : null;
  return { base: match?.[1] ?? value, effort: match?.[2] ?? "" };
}

function configuredRole(role: string, context: Context): string {
  const roles = record(context.values.modelRoles);
  return hasOwn(roles, role) ? selectorText(roles[role]).trim() : "";
}

function resolveRole(role: string, context: Context, visited: string[] = []): Resolution {
  const trail = [...visited, "@" + role];
  if (visited.includes("@" + role) || visited.length >= 24) {
    return {
      selectors: [],
      trail,
      explanation: "角色存在循环引用，无法确定模型；请检查角色配置。",
    };
  }
  const configured = configuredRole(role, context);
  if (configured) return resolvePatterns(configured, context, trail);
  const fallback =
    role === "smol" || role === "slow"
      ? configuredRole("default", context)
        ? "default"
        : null
      : role === "tiny"
        ? "smol"
        : role === "memory"
          ? "tiny"
          : role === "advisor" && configuredRole("slow", context)
            ? "slow"
            : null;
  if (fallback) return resolveRole(fallback, context, trail);
  const reasons: Record<string, string> = {
    default:
      "未配置固定默认模型。OMP 启动时根据可用供应商和认证状态选择；恢复会话还可能使用会话模型。",
    smol: "未配置快速模型或固定默认模型。OMP 按内置快速模型优先链与实际可用模型选择。",
    slow: "未配置深度思考模型或固定默认模型。OMP 按内置深度思考模型优先链与实际可用模型选择。",
    advisor: "未配置顾问模型或 slow 角色。OMP 使用内置深度思考模型优先链，不直接继承默认模型。",
    vision: "OMP 根据默认模型、当前会话及模型的图像能力选择；本页未探测运行中会话和可用模型。",
    plan: "未配置规划模型，具体模型由进入规划模式时的会话状态决定。",
    commit: "未配置提交模型。OMP 依次检查其他已配置角色和可用模型，最终结果在执行提交任务时确定。",
    task: "未配置子任务角色，使用任务启动时的父会话模型；不一定等于全局默认模型。",
    image: "OMP 按内置图像生成模型优先链与实际可用模型选择。",
    web: "OMP 按内置网页搜索模型优先链与实际可用模型选择。",
    speech: "OMP 按内置语音合成模型优先链与实际可用模型选择。",
    dictation: "OMP 按内置语音识别模型优先链与实际可用模型选择。",
    judge: "OMP 按内置评判模型优先链和角色回退选择。",
  };
  return {
    selectors: [],
    trail,
    runtime: hasOwn(ROLE_LABELS, role),
    explanation: hasOwn(reasons, role) ? reasons[role] : "角色 @" + role + " 未配置模型。",
  };
}

function resolvePatterns(value: string, context: Context, visited: string[] = []): Resolution {
  const results = value.split(",").map((part): Resolution => {
    const selector = part.trim();
    if (!selector) return { selectors: [], trail: visited };
    const { base, effort } = splitOmpModelSelector(selector, context.snapshot);
    const role =
      base === "*"
        ? "default"
        : base.startsWith("@")
          ? base.slice(1)
          : base.startsWith("pi/") &&
              (hasOwn(ROLE_LABELS, base.slice(3)) || configuredRole(base.slice(3), context))
            ? base.slice(3)
            : null;
    if (!role) return { selectors: [selector], trail: visited };
    const result = resolveRole(role, context, visited);
    return effort
      ? {
          ...result,
          selectors: result.selectors.map(
            (item) => splitOmpModelSelector(item, context.snapshot).base + ":" + effort
          ),
        }
      : result;
  });
  return {
    selectors: results.flatMap((result) => result.selectors),
    trail: [...new Set(results.flatMap((result) => result.trail))],
    explanation: [
      ...new Set(results.flatMap((result) => (result.explanation ? [result.explanation] : []))),
    ].join(" "),
    runtime: results.some((result) => result.runtime),
  };
}

function formatPreview(
  result: Resolution,
  context: Context,
  thinking?: string | null
): OmpModelPreview {
  const fixed = result.selectors.length === 1 && !result.runtime && !result.explanation;
  const selector = fixed ? result.selectors[0] : "";
  const { base, effort } = splitOmpModelSelector(selector, context.snapshot);
  const model = context.snapshot.models.find((item) => item.selector === base);
  const pattern = base && (!base.includes("/") || /[*?]/.test(base));
  const kind =
    fixed && !pattern
      ? "model"
      : result.selectors.length
        ? "chain"
        : result.runtime
          ? "runtime"
          : "unresolved";
  const defaultThinking =
    selectorText(context.values.defaultThinkingLevel) ||
    selectorText(
      context.snapshot.fields.find((field) => field.key === "defaultThinkingLevel")?.defaultValue
    );
  return {
    kind,
    label:
      kind === "model"
        ? model?.label || base
        : kind === "chain"
          ? "按候选模型选择"
          : kind === "runtime"
            ? "运行时自动选择（未固定模型）"
            : "无法确定模型",
    selectors: result.selectors,
    source: result.trail.join(" → "),
    description:
      result.explanation ||
      (kind === "chain"
        ? "按顺序匹配实际可用模型；目录收录不代表已登录，不能据此确定最终命中项。"
        : "根据当前配置解析；尚未检查认证和上游可用性。"),
    thinking:
      effort || thinking || (kind === "model" ? defaultThinking || "OMP 原生默认" : "运行时确定"),
  };
}

export function ompRoleModelPreview(
  role: string,
  values: SettingsValues,
  snapshot: OmpSettingsSnapshot,
  inherit = false
): OmpModelPreview {
  if (inherit) {
    const roles = { ...record(values.modelRoles) };
    delete roles[role];
    values = { ...values, modelRoles: roles as SettingsValues[string] };
  }
  const context = { values, snapshot };
  return formatPreview(resolveRole(role, context), context);
}

export function ompSelectorModelPreview(
  value: string,
  values: SettingsValues,
  snapshot: OmpSettingsSnapshot
) {
  const context = { values, snapshot };
  return formatPreview(resolvePatterns(value, context), context);
}

export function ompAgentModelPreview(
  agent: OmpAgentSummary,
  values: SettingsValues,
  snapshot: OmpSettingsSnapshot,
  inherit = false
): OmpModelPreview {
  const context = { values, snapshot };
  const override = inherit
    ? ""
    : selectorText(record(values["task.agentModelOverrides"])[agent.name]).trim();
  if (override)
    return formatPreview(
      resolvePatterns(override, context, [agent.name + " 覆盖"]),
      context,
      agent.thinking
    );
  if (agent.source === "configured")
    return {
      kind: "unresolved",
      label: "定义尚未加载",
      selectors: [],
      source: agent.name + " 项目 / 插件定义",
      description: "本页未加载该 Agent 的项目或插件定义，无法确定其原生模型。可设置显式覆盖。",
      thinking: "由 Agent 定义决定",
    };
  const definition = agent.model
    .flatMap((item) => item.split(","))
    .map((item) => item.trim())
    .filter(Boolean)
    .join(", ");
  const base = splitOmpModelSelector(definition, snapshot).base;
  const parent =
    !definition ||
    // Native inheritance markers are exact: @default:high is a model-role
    // selector when default is configured, not the plain @default session marker.
    ["default", "@default", "pi/default", "*"].includes(definition) ||
    (["@default", "pi/default", "*"].includes(base) && !configuredRole("default", context)) ||
    (["@task", "pi/task"].includes(base) && !configuredRole("task", context));
  if (!parent)
    return formatPreview(
      resolvePatterns(definition, context, [agent.name + " 定义"]),
      context,
      agent.thinking
    );
  const reference = ompRoleModelPreview("default", values, snapshot);
  return {
    kind: "runtime",
    label: "继承父会话模型",
    selectors: [],
    source: agent.name + " 定义 → 父会话",
    description:
      "由任务启动时的父会话决定。" +
      (reference.kind === "model"
        ? "新会话默认参考：" + reference.label + "（" + reference.selectors.join(", ") + "）。"
        : "当前也未配置可确定的新会话默认模型。"),
    thinking: agent.thinking || "父会话决定",
  };
}
