import { describe, expect, it } from "vitest";
import {
  MAX_PROVIDER_BASE_URLS,
  MAX_PROVIDER_BASE_URL_CHARS,
  normalizeBaseUrlRows,
  providerBaseUrlSummary,
  providerPrimaryBaseUrl,
} from "../baseUrl";
import { validateProviderClaudeModels } from "../../../schemas/providerEditorDialog";
import { CX2CC_PROVIDER_DEFAULT_MODEL } from "../../../constants/cx2cc";
import { resolveCx2ccDefaultModelSelectValue, withCx2ccDefaultModel } from "../providerEditorUtils";

describe("pages/providers/baseUrl helpers", () => {
  it("summarizes provider base urls", () => {
    expect(providerPrimaryBaseUrl(null)).toBe("—");
    expect(providerPrimaryBaseUrl({ base_urls: ["https://a"] } as any)).toBe("https://a");
    expect(providerBaseUrlSummary({ base_urls: ["https://a"] } as any)).toBe("https://a");
    expect(providerBaseUrlSummary({ base_urls: ["https://a", "https://b"] } as any)).toBe(
      "https://a · https://b"
    );
    expect(
      providerBaseUrlSummary({ base_urls: ["https://a", "https://b", "https://c"] } as any)
    ).toBe("https://a · https://b (+1)");
  });

  it("normalizes base url rows with validation", () => {
    expect(normalizeBaseUrlRows([] as any).ok).toBe(false);
    expect(
      normalizeBaseUrlRows([{ id: "1", url: "   ", ping: { status: "idle" } }] as any).ok
    ).toBe(false);
    expect(
      normalizeBaseUrlRows([{ id: "1", url: "ftp://x", ping: { status: "idle" } }] as any).ok
    ).toBe(false);
    expect(
      normalizeBaseUrlRows([{ id: "1", url: "not-a-url", ping: { status: "idle" } }] as any).ok
    ).toBe(false);
    expect(
      normalizeBaseUrlRows([
        { id: "1", url: "https://a", ping: { status: "idle" } },
        { id: "2", url: "https://a", ping: { status: "idle" } },
      ] as any).ok
    ).toBe(false);

    const ok = normalizeBaseUrlRows([
      { id: "1", url: "https://a", ping: { status: "idle" } },
      { id: "2", url: " https://b ", ping: { status: "idle" } },
    ] as any);
    expect(ok.ok).toBe(true);
    if (ok.ok) expect(ok.baseUrls).toEqual(["https://a", "https://b"]);
  });

  it("rejects base url boundary overflows before submit", () => {
    const tooManyRows = Array.from({ length: MAX_PROVIDER_BASE_URLS + 1 }, (_, index) => ({
      id: String(index),
      url: `https://api-${index}.example.com`,
      ping: { status: "idle" },
    }));
    const tooMany = normalizeBaseUrlRows(tooManyRows as any);
    expect(tooMany.ok).toBe(false);
    if (!tooMany.ok) expect(tooMany.message).toMatch(/最多支持/);

    const tooLong = normalizeBaseUrlRows([
      {
        id: "long",
        url: `https://example.com/${"a".repeat(MAX_PROVIDER_BASE_URL_CHARS)}`,
        ping: { status: "idle" },
      },
    ] as any);
    expect(tooLong.ok).toBe(false);
    if (!tooLong.ok) expect(tooLong.message).toMatch(/不能超过/);
  });
});

describe("validateProviderClaudeModels", () => {
  it("validates Claude model mapping length", () => {
    expect(validateProviderClaudeModels({ main_model: "x".repeat(201) })).toMatch(/过长/);
    expect(validateProviderClaudeModels({ main_model: "ok" })).toBeNull();
  });

  it("validates cx2cc context windows, model pairing, and provider scope", () => {
    expect(
      validateProviderClaudeModels(
        {
          main_model: "main",
          main_context_window: 1_024,
          opus_model: "opus",
          opus_context_window: 10_000_000,
        },
        { allowCx2ccContextWindows: true }
      )
    ).toBeNull();

    for (const value of [1_023, 10_000_001, 1_024.5]) {
      expect(
        validateProviderClaudeModels(
          { main_model: "main", main_context_window: value },
          { allowCx2ccContextWindows: true }
        )
      ).toMatch(/1024-10000000 的整数/);
    }

    expect(
      validateProviderClaudeModels(
        { main_model: "", main_context_window: 1_024 },
        { allowCx2ccContextWindows: true }
      )
    ).toMatch(/必须先配置模型/);
    expect(validateProviderClaudeModels({ main_model: "main", main_context_window: 1_024 })).toBe(
      "上下文窗口仅支持 CX2CC Provider"
    );
  });
});

describe("CX2CC provider model defaults", () => {
  it("defaults only the four runtime mapper slots to the Sol model", () => {
    expect(withCx2ccDefaultModel({})).toEqual({
      main_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      haiku_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      sonnet_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      opus_model: CX2CC_PROVIDER_DEFAULT_MODEL,
    });

    expect(withCx2ccDefaultModel({ reasoning_model: "legacy-thinking" })).toEqual({
      reasoning_model: "legacy-thinking",
      main_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      haiku_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      sonnet_model: CX2CC_PROVIDER_DEFAULT_MODEL,
      opus_model: CX2CC_PROVIDER_DEFAULT_MODEL,
    });
  });

  it("ignores the invalid reasoning slot when resolving the default dropdown", () => {
    expect(
      resolveCx2ccDefaultModelSelectValue({
        main_model: "gpt-5.6-sol",
        reasoning_model: "legacy-thinking",
        haiku_model: "gpt-5.6-sol",
        sonnet_model: "gpt-5.6-sol",
        opus_model: "gpt-5.6-sol",
      })
    ).toBe("gpt-5.6-sol");
  });
});
