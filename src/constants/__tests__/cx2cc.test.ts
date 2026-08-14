import { describe, expect, it } from "vitest";
import { CX2CC_PROVIDER_DEFAULT_MODEL, CX2CC_RESPONSES_MODEL_PRESETS } from "../cx2cc";

describe("CX2CC model presets", () => {
  it("uses the Sol model by default without exposing the invalid bare GPT-5.6 preset", () => {
    expect(CX2CC_PROVIDER_DEFAULT_MODEL).toBe("gpt-5.6-sol");
    expect(CX2CC_RESPONSES_MODEL_PRESETS).toEqual([
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.4",
    ]);
    expect(CX2CC_RESPONSES_MODEL_PRESETS).not.toContain("gpt-5.6");
    expect(new Set(CX2CC_RESPONSES_MODEL_PRESETS).size).toBe(CX2CC_RESPONSES_MODEL_PRESETS.length);
  });
});
