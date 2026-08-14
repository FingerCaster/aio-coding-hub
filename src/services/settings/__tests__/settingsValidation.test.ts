import { describe, expect, it } from "vitest";
import {
  cloneUpstreamRetryPolicy,
  DEFAULT_UPSTREAM_RETRY_POLICY,
} from "../../gateway/upstreamRetryPolicy";
import { createDefaultCx2ccReasoningEffortMappings } from "../../../constants/cx2cc";
import { validateSettingsSetInput } from "../settingsValidation";

describe("services/settings/settingsValidation", () => {
  it("accepts backend-aligned numeric boundary values", () => {
    expect(
      validateSettingsSetInput({
        preferredPort: 1024,
        logRetentionDays: 3650,
        providerCooldownSeconds: 0,
        providerFailbackStrategy: "aggressive",
        naturalProbeMaxWaitSeconds: 86400,
        providerBaseUrlPingCacheTtlSeconds: 1,
        upstreamFirstByteTimeoutSeconds: 3600,
        upstreamStreamIdleTimeoutSeconds: 0,
        upstreamRequestTimeoutNonStreamingSeconds: 86400,
        failoverMaxAttemptsPerProvider: 20,
        failoverMaxProvidersToTry: 5,
        circuitBreakerFailureThreshold: 50,
        circuitBreakerOpenDurationMinutes: 1440,
        codexInfiniteRetryTestIntervalMs: 60_000,
      })
    ).toBeNull();

    expect(validateSettingsSetInput({ upstreamStreamIdleTimeoutSeconds: 60 })).toBeNull();
    expect(validateSettingsSetInput({ providerFailbackStrategy: "disabled" })).toBeNull();
  });

  it("rejects numeric settings outside backend bounds before IPC", () => {
    expect(validateSettingsSetInput({ preferredPort: 1023 })).toContain("首选端口必须 >= 1024");
    expect(validateSettingsSetInput({ logRetentionDays: 3651 })).toContain(
      "日志保留天数必须 <= 3650"
    );
    expect(validateSettingsSetInput({ providerCooldownSeconds: 3601 })).toContain(
      "Provider 冷却时间必须 <= 3600"
    );
    expect(validateSettingsSetInput({ naturalProbeMaxWaitSeconds: 0 })).toContain(
      "自然模式最长试探等待必须 >= 1"
    );
    expect(validateSettingsSetInput({ naturalProbeMaxWaitSeconds: 86401 })).toContain(
      "自然模式最长试探等待必须 <= 86400"
    );
    expect(validateSettingsSetInput({ providerFailbackStrategy: "future" as any })).toContain(
      "回切策略仅支持 natural、aggressive 或 disabled"
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
    expect(validateSettingsSetInput({ codexInfiniteRetryTestIntervalMs: 60_001 })).toContain(
      "Codex 无限重试测试间隔必须 <= 60000"
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

  it("validates nested retry rules before settings IPC", () => {
    const policy = cloneUpstreamRetryPolicy(DEFAULT_UPSTREAM_RETRY_POLICY);
    policy.http_rules[0] = {
      enabled: true,
      status_code: 599,
      body_contains: ["Quota", "literal *.json"],
      description: "Temporary quota",
    };
    expect(validateSettingsSetInput({ upstreamRetryPolicy: policy })).toBeNull();

    policy.http_rules[0].status_code = 600;
    expect(validateSettingsSetInput({ upstreamRetryPolicy: policy })).toContain("400-599");
    policy.http_rules[0].status_code = 503;
    policy.http_rules[0].description = "unsafe\nline";
    expect(validateSettingsSetInput({ upstreamRetryPolicy: policy })).toContain("控制字符");
  });

  it("validates upstream error response rules before settings IPC", () => {
    const rule = {
      id: "8ca12e7b-4f19-45f7-9185-cc6fbd951c51",
      name: "限额响应",
      description: "",
      enabled: true,
      priority: 10,
      status_codes: [429],
      keywords: ["quota"],
      match_mode: "all" as const,
      cli_keys: ["codex"],
      provider_ids: [7],
      status_behavior: { mode: "override" as const, status_code: 503 },
      message_behavior: { mode: "passthrough" as const },
    };
    expect(validateSettingsSetInput({ upstreamErrorResponseRules: [rule] })).toBeNull();
    expect(
      validateSettingsSetInput({
        upstreamErrorResponseRules: [{ ...rule, keywords: [], status_codes: [] }],
      })
    ).toContain("至少需要");
  });

  it("accepts the default CX2CC reasoning effort mapping", () => {
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: createDefaultCx2ccReasoningEffortMappings(),
      })
    ).toBeNull();
  });

  it("rejects empty and duplicate CX2CC reasoning effort mapping fields", () => {
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: [{ source: " ", target: "max" }],
      })
    ).toContain("来源强度不能为空");
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: [{ source: "ultra", target: " " }],
      })
    ).toContain("目标强度不能为空");
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: [
          { source: "ultra", target: "max" },
          { source: " ultra ", target: "xhigh" },
        ],
      })
    ).toContain("来源强度“ultra”重复");
  });

  it("rejects CX2CC reasoning effort mapping count, byte and control-character overflow", () => {
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: Array.from({ length: 33 }, (_, index) => ({
          source: `source-${index}`,
          target: "max",
        })),
      })
    ).toContain("最多 32 条");
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: [{ source: "思".repeat(22), target: "max" }],
      })
    ).toContain("必须 <= 64 字节");
    expect(
      validateSettingsSetInput({
        cx2CcReasoningEffortMappings: [{ source: "ultra", target: "ma\u0001x" }],
      })
    ).toContain("不能包含控制字符");
  });
});
