import { describe, expect, it } from "vitest";
import {
  bodyContainsFromTextarea,
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
  validateUpstreamRetryPolicy,
} from "../upstreamRetryPolicy";

describe("upstreamRetryPolicy", () => {
  it("keeps only behavior-bearing new-user defaults", () => {
    const cloned = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    expect(cloned.http_rules).toEqual([
      {
        enabled: true,
        status_code: 400,
        body_contains: ["selected model is at capacity"],
        description: "Codex model capacity",
      },
    ]);
    expect(cloned.transport_errors).toEqual(["connect", "timeout", "read"]);
    expect(cloned.stream_internal_errors).toEqual({
      enabled: true,
      passthrough_keywords: [],
      legacy_retry_keywords: [],
    });
    cloned.http_rules[0].body_contains.push("changed");
    expect(DEFAULT_UPSTREAM_RETRY_POLICY.http_rules[0].body_contains).toEqual([
      "selected model is at capacity",
    ]);
  });

  it("parses one literal body matcher per non-empty line", () => {
    expect(bodyContainsFromTextarea(" quota,limit \n\n *.json \n[a-z]+ ")).toEqual([
      "quota,limit",
      "*.json",
      "[a-z]+",
    ]);
  });

  it("validates rule boundaries and enabled matcher requirements", () => {
    const base = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    base.http_rules.push({
      enabled: true,
      status_code: 599,
      body_contains: [],
      description: "",
    });
    expect(validateUpstreamRetryPolicy(base)).toBeNull();

    base.http_rules[0].status_code = Number.NaN;
    expect(validateUpstreamRetryPolicy(base)).toContain("必须是整数");
    base.http_rules[0].status_code = 399;
    expect(validateUpstreamRetryPolicy(base)).toContain("400-599");

    const inactive = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    inactive.http_rules = inactive.http_rules.map((rule) => ({ ...rule, enabled: false }));
    inactive.transport_errors = [];
    inactive.stream_internal_errors.enabled = false;
    expect(validateUpstreamRetryPolicy(inactive)).toContain("至少需要一条已启用 HTTP 规则");
    inactive.enabled = false;
    expect(validateUpstreamRetryPolicy(inactive)).toBeNull();
  });

  it("rejects empty, oversized, and unsafe rule text", () => {
    const policy = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    policy.http_rules[0].body_contains = [" "];
    expect(validateUpstreamRetryPolicy(policy)).toContain("匹配内容不能为空");
    policy.http_rules[0].body_contains = ["字".repeat(513)];
    expect(validateUpstreamRetryPolicy(policy)).toContain("最多 512 个字符");
    policy.http_rules[0].body_contains = [];
    policy.http_rules[0].description = "unsafe\nline";
    expect(validateUpstreamRetryPolicy(policy)).toContain("控制字符");
  });

  it("matches backend bounds after Unicode normalization", () => {
    const policy = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    policy.http_rules[0].body_contains = ["İ".repeat(512)];
    expect(validateUpstreamRetryPolicy(policy)).toContain("规范化后最多 512 个字符");

    policy.http_rules[0].body_contains = [];
    policy.transport_errors = Array.from({ length: 9 }, () => "connect");
    expect(validateUpstreamRetryPolicy(policy)).toContain("传输错误最多支持 8 个");
  });
});
