import { describe, expect, it } from "vitest";
import { CX2CC_PROVIDER_DEFAULT_MODEL, CX2CC_RESPONSES_MODEL_PRESETS } from "../cx2cc";

describe("CX2CC model presets", () => {
  it("keeps the provider default stable while exposing GPT-5.6 variants", () => {
    expect(CX2CC_PROVIDER_DEFAULT_MODEL).toBe("gpt-5.5");
    expect(CX2CC_RESPONSES_MODEL_PRESETS).toEqual([
      "gpt-5.6",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.4",
    ]);
    expect(new Set(CX2CC_RESPONSES_MODEL_PRESETS).size).toBe(CX2CC_RESPONSES_MODEL_PRESETS.length);
  });
});
