import { describe, expect, it } from "vitest";
import { nativeModel } from "../../../../test/fixtures/native";
import { parseGatewayModels } from "../nativeGatewayModelDraft";
describe("explicit gateway model declarations", () => {
  it("retains exact user capacities and does not infer capability from names", () => {
    const model = nativeModel({
      requestModelId: "super-thinking-vision",
      contextWindow: 16384,
      maxTokens: 512,
      reasoning: false,
    });
    expect(parseGatewayModels(JSON.stringify([model]), "pi")).toEqual([model]);
  });
  it("rejects omitted capacity, duplicate ids and incompatible output limits", () => {
    expect(() =>
      parseGatewayModels(JSON.stringify([{ ...nativeModel(), contextWindow: undefined }]), "pi")
    ).toThrow();
    expect(() => parseGatewayModels(JSON.stringify([nativeModel(), nativeModel()]), "pi")).toThrow(
      /不能重复/
    );
    expect(() =>
      parseGatewayModels(JSON.stringify([nativeModel({ maxTokens: 9000 })]), "pi")
    ).toThrow(/上下文/);
  });
  it("requires explicit complete Pi thinking maps and keeps unsupported levels null", () => {
    const model = nativeModel({
      reasoning: true,
      thinking: {
        client: "pi",
        levelMap: {
          off: null,
          minimal: null,
          low: "low",
          medium: "medium",
          high: "high",
          xhigh: null,
          max: null,
        },
      },
    });
    expect(parseGatewayModels(JSON.stringify([model]), "pi")).toEqual([model]);
    expect(() =>
      parseGatewayModels(
        JSON.stringify([
          nativeModel({ reasoning: true, thinking: { client: "pi", levelMap: { high: "high" } } }),
        ]),
        "pi"
      )
    ).toThrow(/七|档位/);
    expect(() => parseGatewayModels(JSON.stringify([model]), "omp")).toThrow(/不能混用/);
  });
  it("retains OMP tool flags and thinking semantics", () => {
    const model = nativeModel({
      supportsTools: false,
      reasoning: true,
      thinking: {
        client: "omp",
        mode: "effort",
        efforts: ["low", "high"],
        defaultLevel: "low",
        effortMap: { low: "low", high: "high" },
        supportsDisplay: false,
        requiresEffort: true,
      },
    });
    expect(parseGatewayModels(JSON.stringify([model]), "omp")).toEqual([model]);
    expect(() =>
      parseGatewayModels(JSON.stringify([nativeModel({ supportsTools: false })]), "pi")
    ).toThrow(/禁用工具/);
  });
});
