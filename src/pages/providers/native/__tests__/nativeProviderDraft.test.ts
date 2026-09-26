import { describe, expect, it } from "vitest";
import {
  nativeFailureMessage,
  nativeNodePatches,
  parseNativeNode,
  setNativeField,
} from "../nativeProviderDraft";

describe("native provider drafts", () => {
  it("patches only changed fields and preserves unknown/model overrides", () => {
    const original = {
      api: "custom-native-api",
      headers: { "x-custom": "value" },
      models: [{ id: "model", contextWindow: 12345, future: { kept: true } }],
      future: [1, 2],
    };
    const next = setNativeField(original, "api", "anthropic-messages");
    expect(nativeNodePatches(original, next)).toEqual([
      { path: ["api"], value: "anthropic-messages" },
    ]);
    expect(next.models).toEqual(original.models);
    expect(next.future).toEqual([1, 2]);
    expect(nativeNodePatches(original, setNativeField(original, "headers", undefined))).toEqual([
      { path: ["headers"], value: null },
    ]);
  });

  it("does not evaluate credential expressions or invent capabilities", () => {
    const node = parseNativeNode('{"apiKey":"!echo private","models":[{"id":"gpt-future"}]}');
    expect(node.apiKey).toBe("!echo private");
    expect(node.models).toEqual([{ id: "gpt-future" }]);
  });

  it("rejects damaged/array/null nodes without converting them into empty configuration", () => {
    for (const raw of ["{broken", "[]", "null", '"secret"'])
      expect(() => parseNativeNode(raw)).toThrow();
    expect(nativeNodePatches({ name: "same" }, { name: "same" })).toEqual([]);
  });

  it("presents conflict, recovery, and unknown outcomes without leaking backend text", () => {
    expect(nativeFailureMessage(new Error("NATIVE_REVISION_CONFLICT secret-value"))).toContain(
      "重新核对"
    );
    expect(nativeFailureMessage(new Error("NATIVE_RECOVERY_REQUIRED secret-value"))).toContain(
      "需要恢复"
    );
    expect(nativeFailureMessage(new Error("PRIVATE_TOKEN secret-value"))).not.toContain(
      "secret-value"
    );
    expect(nativeFailureMessage(null)).toContain("状态尚未确认");
  });
});
