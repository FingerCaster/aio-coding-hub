import { describe, expect, it } from "vitest";
import {
  cloneModelRoutingPolicy,
  DEFAULT_MODEL_ROUTING_POLICY,
  emptyModelRoutingRule,
  normalizeModelRoutingPolicy,
  validateModelRoutingPolicy,
} from "../modelRoutingPolicy";

describe("modelRoutingPolicy", () => {
  it("clones policies without sharing mutable rules", () => {
    const cloned = cloneModelRoutingPolicy({
      enabled: true,
      rules: [{ source_model: "source", target_model: "target", reasoning_effort: null }],
    });

    cloned.rules[0].target_model = "changed";

    expect(cloned).not.toEqual(DEFAULT_MODEL_ROUTING_POLICY);
    expect(DEFAULT_MODEL_ROUTING_POLICY).toEqual({ enabled: false, rules: [] });
    expect(emptyModelRoutingRule()).toEqual({
      source_model: "",
      target_model: null,
      reasoning_effort: null,
    });
  });

  it("normalizes exact-match fields and blank optional outputs", () => {
    expect(
      normalizeModelRoutingPolicy({
        enabled: true,
        rules: [
          {
            source_model: "  source  ",
            target_model: " target ",
            reasoning_effort: "   ",
          },
        ],
      })
    ).toEqual({
      enabled: true,
      rules: [{ source_model: "source", target_model: "target", reasoning_effort: null }],
    });
  });

  it("accepts model-only and effort-only rules", () => {
    expect(
      validateModelRoutingPolicy({
        enabled: true,
        rules: [
          { source_model: "source", target_model: "target", reasoning_effort: null },
          { source_model: "other", target_model: null, reasoning_effort: "low" },
        ],
      })
    ).toBeNull();
  });

  it("rejects enabled-empty, incomplete, duplicate, unsafe, and oversized rules", () => {
    expect(validateModelRoutingPolicy({ enabled: true, rules: [] })).toContain("至少需要一条规则");
    expect(
      validateModelRoutingPolicy({
        enabled: true,
        rules: [{ source_model: "source", target_model: null, reasoning_effort: null }],
      })
    ).toContain("至少填写目标模型或思考强度");
    expect(
      validateModelRoutingPolicy({
        enabled: true,
        rules: [
          { source_model: "source", target_model: "one", reasoning_effort: null },
          { source_model: " source ", target_model: "two", reasoning_effort: null },
        ],
      })
    ).toContain("与已有来源模型重复");
    expect(
      validateModelRoutingPolicy({
        enabled: true,
        rules: [{ source_model: "source", target_model: "bad\nmodel", reasoning_effort: null }],
      })
    ).toContain("控制字符");
    expect(
      validateModelRoutingPolicy({
        enabled: true,
        rules: [{ source_model: "模".repeat(86), target_model: "target", reasoning_effort: null }],
      })
    ).toContain("不能超过 256 字节");
  });
});
