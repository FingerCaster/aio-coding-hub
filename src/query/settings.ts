import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  settingsGet,
  settingsPatch,
  settingsSet,
  type AppSettingsPatch,
  type AppSettings,
  type SettingsMutationResult,
  type SettingsSetInput,
} from "../services/settings/settings";
import { settingsCircuitBreakerNoticeSet } from "../services/settings/settingsCircuitBreakerNotice";
import { settingsCodexSessionIdCompletionSet } from "../services/settings/settingsCodexSessionIdCompletion";
import { settingsCodexModelContextRulesSet } from "../services/settings/settingsCodexModelContextRules";
import type { CodexModelContextRule } from "../services/settings/codexModelContextRules";
import {
  settingsGatewayRectifierSet,
  type GatewayRectifierSettingsPatch,
} from "../services/settings/settingsGatewayRectifier";
import {
  CODEX_CONFIG_MUTATION_SCOPE,
  cliManagerKeys,
  cliProxyKeys,
  gatewayKeys,
  settingsKeys,
} from "./keys";

export const SETTINGS_READONLY_MESSAGE =
  "设置文件读取失败，已进入只读保护。请先修复或恢复 settings.json 后刷新页面。";

const ORDINARY_SETTINGS_MUTATION_SCOPE = { id: "ordinary-settings-write" } as const;

export type SettingsReadProtection = {
  settingsReadErrorMessage: string | null;
  settingsWriteBlocked: boolean;
};

export function getSettingsReadProtection(query: {
  data?: AppSettings | null;
  isError?: boolean;
}): SettingsReadProtection {
  if (query.isError) {
    return {
      settingsReadErrorMessage: SETTINGS_READONLY_MESSAGE,
      settingsWriteBlocked: true,
    };
  }

  if (query.data != null) {
    return {
      settingsReadErrorMessage: null,
      settingsWriteBlocked: false,
    };
  }

  return {
    settingsReadErrorMessage: null,
    settingsWriteBlocked: false,
  };
}

export function useSettingsQuery(options?: { enabled?: boolean }) {
  return useQuery({
    queryKey: settingsKeys.get(),
    queryFn: () => settingsGet(),
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
  });
}

export { type SettingsSetInput } from "../services/settings/settings";
export { type SettingsMutationResult } from "../services/settings/settings";

export function useSettingsSetMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    scope: ORDINARY_SETTINGS_MUTATION_SCOPE,
    mutationFn: (input: SettingsSetInput) => settingsSet(input),
    onSuccess: (result) => syncSettingsMutationCaches(queryClient, result),
    onSettled: (_data, _error, input) => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
      if (input.codexOauthCompatibleProxyMode != null) {
        invalidateCodexProxyProjectionQueries(queryClient);
      }
    },
  });
}

function invalidateCodexProxyProjectionQueries(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfig() });
  queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfigToml() });
  queryClient.invalidateQueries({ queryKey: cliProxyKeys.statusAll() });
}

function syncSettingsMutationCaches(
  queryClient: ReturnType<typeof useQueryClient>,
  result: SettingsMutationResult | null | undefined
) {
  if (!result) return;
  queryClient.setQueryData<AppSettings | null>(settingsKeys.get(), result.settings);
  queryClient.setQueryData(gatewayKeys.status(), result.runtime.gateway_status);
}

export function useSettingsPatchMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    scope: ORDINARY_SETTINGS_MUTATION_SCOPE,
    mutationFn: async (patch: AppSettingsPatch) => {
      const current = await settingsGet();
      return settingsPatch(current, patch);
    },
    onSuccess: (result) => syncSettingsMutationCaches(queryClient, result),
    onSettled: (_data, _error, patch) => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
      if (Object.prototype.hasOwnProperty.call(patch, "codex_oauth_compatible_proxy_mode")) {
        invalidateCodexProxyProjectionQueries(queryClient);
      }
    },
  });
}

export function useSettingsGatewayRectifierSetMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: GatewayRectifierSettingsPatch) => settingsGatewayRectifierSet(input),
    onSuccess: (updated) => {
      if (!updated) return;
      queryClient.setQueryData<AppSettings | null>(settingsKeys.get(), updated);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
    },
  });
}

export function useSettingsCircuitBreakerNoticeSetMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (enable: boolean) => settingsCircuitBreakerNoticeSet(enable),
    onSuccess: (updated) => {
      if (!updated) return;
      queryClient.setQueryData<AppSettings | null>(settingsKeys.get(), updated);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
    },
  });
}

export function useSettingsCodexSessionIdCompletionSetMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (enable: boolean) => settingsCodexSessionIdCompletionSet(enable),
    onSuccess: (updated) => {
      if (!updated) return;
      queryClient.setQueryData<AppSettings | null>(settingsKeys.get(), updated);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
    },
  });
}

export function useSettingsCodexModelContextRulesSetMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    scope: CODEX_CONFIG_MUTATION_SCOPE,
    mutationFn: (rules: CodexModelContextRule[]) => settingsCodexModelContextRulesSet(rules),
    onSuccess: (updated) => {
      if (!updated) return;
      queryClient.setQueryData<AppSettings | null>(settingsKeys.get(), updated);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsKeys.get() });
      queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfig() });
      queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfigToml() });
      // Catalog queries self-disable after an error; reset lets active snapshots retry.
      queryClient.resetQueries({ queryKey: cliManagerKeys.codexModelCatalogAll() });
      queryClient.resetQueries({ queryKey: cliManagerKeys.codexModelContextCandidatesAll() });
      queryClient.invalidateQueries({ queryKey: cliProxyKeys.statusAll() });
    },
  });
}
