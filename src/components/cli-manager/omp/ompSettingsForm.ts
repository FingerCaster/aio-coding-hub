import type { OmpSettingPatch, OmpSettingsSnapshot } from "../../../services/ompSettings";

export type SettingsValues = OmpSettingsSnapshot["values"];
export const RECORD_KEYS = [
  "modelRoles",
  "task.agentModelOverrides",
  "task.agentServiceTierOverrides",
  "task.agentPrewalk",
  "task.agentAdvisor",
];
export const ROLE_LABELS: Record<string, string> = {
  default: "默认模型",
  smol: "快速模型",
  slow: "深度思考",
  vision: "视觉",
  plan: "规划",
  commit: "提交信息",
  tiny: "轻量任务",
  memory: "记忆",
  task: "子任务",
  advisor: "顾问",
  image: "图像生成",
  web: "网页搜索",
  speech: "语音合成",
  dictation: "语音识别",
  judge: "评判",
};
export const EFFORTS = ["minimal", "low", "medium", "high", "xhigh", "max"];
export function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
export function selectorText(value: unknown): string {
  return typeof value === "string"
    ? value
    : Array.isArray(value)
      ? value.filter((v) => typeof v === "string").join(", ")
      : "";
}
export function settingsPatches(before: SettingsValues, after: SettingsValues): OmpSettingPatch[] {
  const patches: OmpSettingPatch[] = [];
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (RECORD_KEYS.includes(key)) {
      const oldMap = record(before[key]);
      const newMap = record(after[key]);
      for (const member of new Set([...Object.keys(oldMap), ...Object.keys(newMap)])) {
        if (JSON.stringify(oldMap[member]) !== JSON.stringify(newMap[member])) {
          patches.push({
            path: [...key.split("."), member],
            value: (newMap[member] as OmpSettingPatch["value"]) ?? null,
          });
        }
      }
    } else if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
      patches.push({ path: key.split("."), value: after[key] ?? null });
    }
  }
  return patches;
}
export function validateDraft(
  values: SettingsValues,
  snapshot: OmpSettingsSnapshot
): string | null {
  for (const field of snapshot.fields) {
    const value = values[field.key];
    if (value == null) continue;
    if (JSON.stringify(value) === JSON.stringify(snapshot.values[field.key])) continue;
    if (
      field.kind === "number" &&
      (typeof value !== "number" ||
        !Number.isInteger(value) ||
        (field.min != null && value < field.min) ||
        (field.max != null && value > field.max))
    )
      return field.label + " 必须为 " + field.min + "～" + field.max + " 之间的整数";
  }
  return null;
}
