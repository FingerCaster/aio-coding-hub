import { describe, expect, it } from "vitest";
import { validateSettingsSetInput } from "../settingsValidation";

describe("services/settings/settingsValidation", () => {
  it("accepts backend-aligned numeric boundary values", () => {
    expect(
      validateSettingsSetInput({
        preferredPort: 1024,
        logRetentionDays: 3650,
        providerCooldownSeconds: 0,
        providerBaseUrlPingCacheTtlSeconds: 1,
        upstreamFirstByteTimeoutSeconds: 3600,
        upstreamStreamIdleTimeoutSeconds: 0,
        upstreamRequestTimeoutNonStreamingSeconds: 86400,
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 5,
        circuitBreakerFailureThreshold: 50,
        circuitBreakerOpenDurationMinutes: 1440,
      })
    ).toBeNull();

    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 60 })).toBeNull();
  });

  it("rejects numeric settings outside backend bounds before IPC", () => {
    expect(validateSettingsSetInput({ preferredPort: 1023 })).toContain("首选端口必须 >= 1024");
    expect(validateSettingsSetInput({ logRetentionDays: 3651 })).toContain(
      "日志保留天数必须 <= 3650"
    );
    expect(validateSettingsSetInput({ providerCooldownSeconds: 3601 })).toContain(
      "Provider 冷却时间必须 <= 3600"
    );
    expect(validateSettingsSetInput({ providerBaseUrlPingCacheTtlSeconds: 0 })).toContain(
      "Provider Base URL 探测缓存 TTL必须 >= 1"
    );
    expect(validateSettingsSetInput({ upstreamFirstByteTimeoutSeconds: 3601 })).toContain(
      "首字节超时必须 <= 3600"
    );
    expect(
      validateSettingsSetInput({ upstreamRequestTimeoutNonStreamingSeconds: 86401 })
    ).toContain("非流式请求超时必须 <= 86400");
    expect(validateSettingsSetInput({ circuitBreakerFailureThreshold: 0 })).toContain(
      "熔断失败阈值必须 >= 1"
    );
    expect(validateSettingsSetInput({ circuitBreakerOpenDurationMinutes: 1441 })).toContain(
      "熔断打开时长必须 <= 1440"
    );
  });

  it("rejects fractional values and stream idle timeout values in the forbidden gap", () => {
    expect(validateSettingsSetInput({ preferredPort: 37123.5 })).toContain("首选端口必须是整数");
    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 30 })).toContain(
      "流式空闲超时必须为 0"
    );
    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 3601 })).toContain(
      "流式空闲超时必须 <= 3600"
    );
  });

  it("rejects failover product overflow when both dimensions are present", () => {
    expect(
      validateSettingsSetInput({
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 6,
      })
    ).toContain("Failover 总尝试次数必须 <= 100");
  });

  it("validates Codex reasoning guard rule mode", () => {
    expect(
      validateSettingsSetInput({
        codexReasoningGuardRuleMode: "final_answer_only_high_xhigh",
      })
    ).toBeNull();
    expect(
      validateSettingsSetInput({
        codexReasoningGuardRuleMode: "bad_mode" as never,
      })
    ).toContain("Codex 降智拦截规则模式仅支持");
  });

  it("validates Codex reasoning guard custom rule templates", () => {
    const validTemplate = {
      id: "custom-fast-token",
      name: "Fast token guard",
      description: "test",
      rules: [
        {
          id: "token-516",
          name: "516",
          reasoning_tokens: 516,
          action: "intercept",
          logic: "and",
          filters: [
            {
              id: "duration-fast",
              field: "duration_ms",
              operator: "less_than",
              number_value: 1200,
              bool_value: null,
              string_value: null,
              string_values: [],
            },
          ],
        },
        {
          id: "wildcard-allow",
          name: "allow",
          reasoning_tokens: null,
          action: "no_intercept",
          logic: "and",
          filters: [
            {
              id: "model",
              field: "requested_model",
              operator: "in",
              number_value: null,
              bool_value: null,
              string_value: null,
              string_values: ["gpt-5.5", "gpt-5.5-pro"],
            },
          ],
        },
      ],
    } as const;

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [validTemplate as any],
      })
    ).toBeNull();

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [
          {
            ...validTemplate,
            rules: Array.from({ length: 65 }, (_, index) => ({
              ...validTemplate.rules[0],
              id: `token-${index}`,
              name: `token ${index}`,
              reasoning_tokens: 10_000 + index,
            })),
          } as any,
        ],
      })
    ).toBeNull();

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "missing-template",
        codexReasoningGuardCustomTemplates: [validTemplate as any],
      })
    ).toContain("active template 不存在");

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "legacy-compatibility",
        codexReasoningGuardCustomTemplates: [validTemplate as any],
      })
    ).toContain("active template 不存在");

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "builtin-legacy-reasoning-tokens",
        codexReasoningGuardCustomTemplates: [
          { ...validTemplate, id: "legacy-compatibility" } as any,
        ],
      })
    ).toContain("id 不能使用内置模板 id：legacy-compatibility");

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [
          {
            ...validTemplate,
            rules: [validTemplate.rules[0], { ...validTemplate.rules[0], id: "token-516-copy" }],
          } as any,
        ],
      })
    ).toContain("不能重复配置 token：516");

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [
          {
            ...validTemplate,
            rules: [
              { ...validTemplate.rules[1], filters: [] },
              { ...validTemplate.rules[1], id: "wildcard-allow-copy", filters: [] },
            ],
          } as any,
        ],
      })
    ).toContain("只能有一条无过滤 wildcard 规则");

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [
          {
            ...validTemplate,
            rules: [
              validTemplate.rules[1],
              {
                ...validTemplate.rules[1],
                id: "wildcard-filtered",
                filters: [
                  {
                    id: "duration-under-30s",
                    field: "duration_ms",
                    operator: "less_than",
                    number_value: 30_000,
                    bool_value: null,
                    string_value: null,
                    string_values: [],
                  },
                ],
              },
            ],
          } as any,
        ],
      })
    ).toBeNull();

    expect(
      validateSettingsSetInput({
        codexReasoningGuardActiveTemplateId: "custom-fast-token",
        codexReasoningGuardCustomTemplates: [
          {
            ...validTemplate,
            rules: [
              {
                ...validTemplate.rules[0],
                filters: [
                  {
                    id: "bad-bool",
                    field: "final_answer_only",
                    operator: "less_than",
                    number_value: 1,
                    bool_value: null,
                    string_value: null,
                    string_values: [],
                  },
                ],
              },
            ],
          } as any,
        ],
      })
    ).toContain("operator 不支持布尔字段");
  });
});
