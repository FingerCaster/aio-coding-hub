import { describe, expect, it } from "vitest";
import {
  cloneUpstreamErrorResponseRules,
  createUpstreamErrorResponseRule,
  type UpstreamErrorResponseRule,
  validateUpstreamErrorResponseRules,
} from "../upstreamErrorResponseRules";

function validRule(): UpstreamErrorResponseRule {
  return {
    id: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51",
    name: "限额响应",
    description: "",
    enabled: true,
    priority: 10,
    status_codes: [429],
    keywords: ["quota"],
    match_mode: "all",
    cli_keys: ["codex"],
    provider_ids: [7],
    status_behavior: { mode: "override", status_code: 503 },
    message_behavior: { mode: "override", message: "上游当前不可用" },
  };
}

describe("services/gateway/upstreamErrorResponseRules", () => {
  it("creates an enabled rule after the current maximum priority", () => {
    const created = createUpstreamErrorResponseRule([validRule()]);
    expect(created.id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u
    );
    expect(created.priority).toBe(20);
    expect(created.enabled).toBe(true);
    expect(created.match_mode).toBe("any");
  });

  it("validates condition, scope, and response boundaries", () => {
    expect(validateUpstreamErrorResponseRules([validRule()])).toBeNull();
    expect(
      validateUpstreamErrorResponseRules([{ ...validRule(), status_codes: [], keywords: [] }])
    ).toContain("至少需要");
    expect(
      validateUpstreamErrorResponseRules([
        { ...validRule(), status_behavior: { mode: "override", status_code: 200 } },
      ])
    ).toContain("400-599");
    expect(
      validateUpstreamErrorResponseRules([{ ...validRule(), cli_keys: ["future"] }])
    ).toContain("未知 CLI");
  });

  it("drops malformed runtime entries instead of throwing while cloning", () => {
    expect(
      cloneUpstreamErrorResponseRules([
        validRule(),
        { id: "broken", status_codes: null } as unknown as UpstreamErrorResponseRule,
      ])
    ).toEqual([validRule()]);
    expect(cloneUpstreamErrorResponseRules(null)).toEqual([]);
  });
});
