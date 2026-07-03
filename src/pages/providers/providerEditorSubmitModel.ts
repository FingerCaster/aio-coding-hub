import {
  createProviderEditorDialogSchema,
  validateProviderClaudeModels,
  validateProviderModelMapping,
} from "../../schemas/providerEditorDialog";
import type {
  ProviderEditorPayloadBuildError,
  ProviderEditorPayloadBuildSuccess,
  ProviderEditorPayloadContext,
} from "./providerEditorActionContext";
import { normalizeBaseUrlRows } from "./baseUrl";
import { resolveStreamIdleTimeoutSeconds } from "./providerEditorTimeout";
import { validateUpstreamRetryPolicy } from "../../services/gateway/upstreamRetryPolicy";
import {
  CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE,
  CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE,
} from "./providerEditorUtils";

export function buildProviderEditorUpsertInput(
  ctx: ProviderEditorPayloadContext
):
  | { ok: true; value: ProviderEditorPayloadBuildSuccess }
  | { ok: false; error: ProviderEditorPayloadBuildError } {
  const parsed = createProviderEditorDialogSchema({
    mode: ctx.mode,
    skipApiKeyCheck: ctx.authMode === "cx2cc",
  }).safeParse({
    ...ctx.formValues,
    auth_mode: ctx.authMode === "cx2cc" ? "api_key" : ctx.authMode,
  });

  if (!parsed.success) {
    return {
      ok: false,
      error: {
        kind: "schema",
        issues: parsed.error.issues,
      },
    };
  }

  const parsedTimeout = resolveStreamIdleTimeoutSeconds(ctx.streamIdleTimeoutSeconds);
  if (parsedTimeout === undefined) {
    return {
      ok: false,
      error: {
        kind: "message",
        message: "流式空闲超时必须为 0-3600 秒",
      },
    };
  }

  if (ctx.authMode === "api_key" && ctx.mode === "edit") {
    const nextApiKey = parsed.data.api_key.trim();
    if (!nextApiKey && !ctx.apiKeyConfigured) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: "请输入 API Key",
        },
      };
    }
  }

  if (ctx.upstreamRetryPolicyOverrideEnabled) {
    const retryPolicyError = validateUpstreamRetryPolicy(ctx.upstreamRetryPolicyDraft);
    if (retryPolicyError) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: retryPolicyError,
        },
      };
    }
  }

  let finalBaseUrls: string[] = [];
  let finalBaseUrlMode = ctx.baseUrlMode;

  if (ctx.authMode === "oauth") {
    finalBaseUrls = [];
  } else if (ctx.authMode === "cx2cc") {
    finalBaseUrls = [];
    finalBaseUrlMode = "order";

    if (ctx.cliKey === "claude" && !ctx.sourceProviderId && !ctx.isCodexGatewaySource) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: "请选择源 Codex 来源",
        },
      };
    }
    if (ctx.cliKey === "codex" && !ctx.sourceProviderId) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: "请选择上游来源",
        },
      };
    }
  } else {
    const normalized = normalizeBaseUrlRows(ctx.baseUrlRows);
    if (!normalized.ok) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: normalized.message,
        },
      };
    }
    finalBaseUrls = normalized.baseUrls;
  }

  if (ctx.cliKey === "claude") {
    const modelError = validateProviderClaudeModels(ctx.claudeModels);
    if (modelError) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: modelError,
        },
      };
    }
  }

  if (ctx.cliKey === "codex" && ctx.authMode === "cx2cc") {
    const modelMappingError = validateProviderModelMapping(ctx.modelMapping);
    if (modelMappingError) {
      return {
        ok: false,
        error: {
          kind: "message",
          message: modelMappingError,
        },
      };
    }
  }

  const effectiveCostMultiplier =
    ctx.authMode === "cx2cc" && ctx.cliKey === "claude" && ctx.isCodexGatewaySource
      ? 0
      : ctx.authMode === "cx2cc" && ctx.selectedCx2ccSourceProvider
        ? ctx.selectedCx2ccSourceProvider.cost_multiplier
        : parsed.data.cost_multiplier;
  const bridgeType =
    ctx.authMode !== "cx2cc"
      ? null
      : ctx.cliKey === "codex"
        ? ctx.codexBridgeTarget === "openai_responses"
          ? CODEX_TO_OPENAI_RESPONSES_BRIDGE_TYPE
          : CODEX_TO_OPENAI_CHAT_BRIDGE_TYPE
        : "cx2cc";

  const payload = {
    ...(ctx.editingProviderId ? { providerId: ctx.editingProviderId } : {}),
    cliKey: ctx.cliKey,
    name: parsed.data.name,
    baseUrls: finalBaseUrls,
    baseUrlMode: finalBaseUrlMode,
    authMode: ctx.authMode === "cx2cc" ? "api_key" : ctx.authMode,
    apiKey:
      ctx.authMode === "oauth" || ctx.authMode === "cx2cc"
        ? null
        : parsed.data.api_key.trim() || null,
    enabled: parsed.data.enabled,
    costMultiplier: effectiveCostMultiplier,
    availabilityTestModel: ctx.cliKey === "codex" ? ctx.testModel : null,
    limit5hUsd: parsed.data.limit_5h_usd,
    limitDailyUsd: parsed.data.limit_daily_usd,
    dailyResetMode: parsed.data.daily_reset_mode,
    dailyResetTime: parsed.data.daily_reset_time,
    limitWeeklyUsd: parsed.data.limit_weekly_usd,
    limitMonthlyUsd: parsed.data.limit_monthly_usd,
    limitTotalUsd: parsed.data.limit_total_usd,
    tags: ctx.tags,
    note: parsed.data.note,
    streamIdleTimeoutSeconds: parsedTimeout,
    upstreamRetryPolicyOverride: ctx.upstreamRetryPolicyOverrideEnabled
      ? ctx.upstreamRetryPolicyDraft
      : null,
    ...(ctx.cliKey === "claude" ? { claudeModels: ctx.claudeModels } : {}),
    ...(ctx.cliKey === "codex" && ctx.authMode === "cx2cc"
      ? { modelMapping: normalizeProviderModelMapping(ctx.modelMapping) }
      : {}),
    sourceProviderId:
      ctx.authMode === "cx2cc" && !(ctx.cliKey === "claude" && ctx.isCodexGatewaySource)
        ? ctx.sourceProviderId
        : null,
    bridgeType,
    extensionValues: ctx.extensionValues ?? null,
  };

  return {
    ok: true,
    value: {
      payload,
      parsedName: parsed.data.name,
    },
  };
}

function normalizeProviderModelMapping(input: {
  default_model?: string | null;
  exact?: Record<string, string | undefined> | null;
}) {
  const exact: Record<string, string> = {};
  for (const [source, target] of Object.entries(input.exact ?? {})) {
    const sourceModel = source.trim();
    const targetModel = (target ?? "").trim();
    if (!sourceModel || !targetModel) continue;
    exact[sourceModel] = targetModel;
  }

  const defaultModel = (input.default_model ?? "").trim();
  return {
    default_model: defaultModel || null,
    exact,
  };
}
