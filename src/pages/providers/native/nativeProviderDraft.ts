import type { JsonValue } from "../../../generated/bindings";

export type NativeNode = Record<string, JsonValue>;

export function isNativeNode(value: unknown): value is NativeNode {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function parseNativeNode(text: string): NativeNode {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("原生节点必须是有效 JSON 对象；未保存任何更改。");
  }
  if (!isNativeNode(value)) throw new Error("原生节点必须是 JSON 对象。");
  return value;
}

export function nativeString(node: NativeNode, key: string): string {
  return typeof node[key] === "string" ? node[key] : "";
}

/** Only edited fields are sent. Unknown fields and untouched nested values survive. */
export function nativeNodePatches(original: NativeNode, draft: NativeNode) {
  return [...new Set([...Object.keys(original), ...Object.keys(draft)])]
    .filter((key) => JSON.stringify(original[key]) !== JSON.stringify(draft[key]))
    .map((key) => ({ path: [key], value: draft[key] ?? null }));
}

export function setNativeField(node: NativeNode, key: string, value: JsonValue | undefined) {
  const result = { ...node };
  if (value === undefined) delete result[key];
  else result[key] = value;
  return result;
}

export function nativeFailureMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  if (/COMPENSATION|RECOVERY|PARTIAL/.test(message)) {
    return "操作未完整完成，需要恢复。请刷新检查实际文件状态，再决定下一步。";
  }
  if (/CONFLICT|STALE|REVISION|DIGEST|CONCURRENT/.test(message)) {
    return "配置或预览已变化，请刷新并重新核对后操作；未覆盖外部修改。";
  }
  if (/READ_ONLY|READONLY|PARSE|FORMAT|AMBIGUOUS/.test(message)) {
    return "配置不可安全编辑。请先在原生 CLI 修复格式或完成迁移，然后刷新。";
  }
  if (/MANAGED|OWNERSHIP/.test(message)) {
    return "该节点由 AIO 入口管理，请使用 AIO 网关视图处理。";
  }
  return "操作失败，状态尚未确认。请刷新后重试；不会将失败视为成功。";
}
