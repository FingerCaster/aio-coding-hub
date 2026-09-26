import { describe, expect, it } from "vitest";
import { resolveNativeChannelRoute } from "../nativeChannelRoute";

const marker = {
  type: "channel_binding",
  scope: "request",
  consumerCli: "pi",
  sourceChannel: "codex",
  bindingId: "a".repeat(64),
};
describe("native channel request attribution", () => {
  it("shows source separately while retaining the actual consumer", () => {
    expect(resolveNativeChannelRoute("pi", JSON.stringify([marker]))).toEqual({
      consumerLabel: "Pi",
      sourceLabel: "Codex",
      bindingId: marker.bindingId,
    });
    expect(
      resolveNativeChannelRoute(
        "omp",
        JSON.stringify([{ ...marker, consumerCli: "omp", sourceChannel: "gemini" }])
      )?.sourceLabel
    ).toBe("Gemini");
  });
  it("ignores missing, malformed, unsupported or mismatched evidence", () => {
    for (const raw of [
      null,
      "bad-json",
      "[]",
      JSON.stringify([{ ...marker, sourceChannel: "antigravity" }]),
      JSON.stringify([{ ...marker, bindingId: "untrusted" }]),
      JSON.stringify([{ ...marker, scope: "attempt" }]),
    ]) {
      expect(resolveNativeChannelRoute("pi", raw)).toBeNull();
    }
    expect(resolveNativeChannelRoute("omp", JSON.stringify([marker]))).toBeNull();
    expect(resolveNativeChannelRoute("codex", JSON.stringify([marker]))).toBeNull();
  });
});
