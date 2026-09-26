import { z } from "zod";
import type { NativeCliKey } from "../../../constants/clis";
import type { NativeModelSpec } from "../../../services/nativeGateway";
import { OMP_THINKING_MODES, THINKING_LEVELS } from "./nativeChannelModelSuggestions";

const text = z.string().trim().min(1).max(256);
const thinking = z.discriminatedUnion("client", [
  z
    .object({ client: z.literal("pi"), levelMap: z.record(z.string(), z.string().nullable()) })
    .strict(),
  z
    .object({
      client: z.literal("omp"),
      mode: text,
      efforts: z.array(text).min(1),
      defaultLevel: text.nullable(),
      effortMap: z.record(z.string(), z.string()),
      supportsDisplay: z.boolean().nullable(),
      requiresEffort: z.boolean().nullable(),
    })
    .strict(),
]);
const schema = z
  .array(
    z
      .object({
        requestModelId: text,
        displayName: text,
        input: z
          .array(z.enum(["text", "image"]))
          .min(1)
          .max(2),
        contextWindow: z.number().int().min(1).max(10_000_000),
        maxTokens: z.number().int().min(1),
        reasoning: z.boolean(),
        thinking: thinking.nullable(),
        supportsTools: z.boolean().nullable(),
      })
      .strict()
  )
  .max(512);

export function parseGatewayModels(raw: string, client: NativeCliKey): NativeModelSpec[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("模型声明必须是有效 JSON 数组。");
  }
  const result = schema.safeParse(parsed);
  if (!result.success)
    throw new Error(
      "请明确填写模型 ID、显示名称、输入类型、正整数上下文与输出容量，以及思考和工具能力。"
    );
  const ids = new Set<string>();
  for (const model of result.data) {
    if (ids.has(model.requestModelId)) throw new Error("模型 ID 不能重复。");
    ids.add(model.requestModelId);
    if (
      !model.input.includes("text") ||
      new Set(model.input).size !== model.input.length ||
      model.maxTokens > model.contextWindow
    )
      throw new Error("输入必须包含 text 且不重复；最大输出不能超过上下文窗口。");
    if (
      model.reasoning !== Boolean(model.thinking) ||
      (model.thinking && model.thinking.client !== client)
    )
      throw new Error("思考能力必须使用当前 CLI 的完整声明；Pi 与 OMP 字段不能混用。");
    if (client === "pi" && model.supportsTools === false)
      throw new Error("Pi 无法发布明确禁用工具的模型。");
    if (model.thinking?.client === "pi") {
      const levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
      const map = model.thinking.levelMap;
      if (
        Object.keys(map).length !== levels.length ||
        levels.some((level) => !Object.prototype.hasOwnProperty.call(map, level)) ||
        Object.values(map).some(
          (value) =>
            value !== null && (!value.trim() || value.length > 64 || /[\x00-\x1f\x7f]/.test(value))
        ) ||
        !levels.slice(1).some((level) => Boolean(map[level]))
      )
        throw new Error(
          "Pi 思考映射必须明确列出 off/minimal/low/medium/high/xhigh/max；不支持的档位填 null。"
        );
    }
    if (model.thinking?.client === "omp") {
      const spec = model.thinking;
      if (
        !Object.prototype.hasOwnProperty.call(OMP_THINKING_MODES, spec.mode) ||
        spec.efforts.length > 6 ||
        new Set(spec.efforts).size !== spec.efforts.length ||
        spec.efforts.some(
          (level) => !THINKING_LEVELS.slice(1).includes(level as (typeof THINKING_LEVELS)[number])
        ) ||
        (spec.defaultLevel !== null && !spec.efforts.includes(spec.defaultLevel)) ||
        Object.entries(spec.effortMap).some(
          ([key, value]) =>
            !spec.efforts.includes(key) ||
            !value.trim() ||
            value.length > 64 ||
            /[\x00-\x1f\x7f]/.test(value)
        )
      ) {
        throw new Error("OMP 思考模式、支持等级、默认等级或映射无效，请从下拉选项中补齐。");
      }
    }
  }
  return result.data;
}
