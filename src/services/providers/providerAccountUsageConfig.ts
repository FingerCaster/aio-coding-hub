import type { ProviderExtensionValuesInput, ProviderSummary } from "./providers";

export const PROVIDER_ACCOUNT_USAGE_PLUGIN_ID = "core.provider-account-usage";
export const PROVIDER_ACCOUNT_USAGE_NAMESPACE = "accountUsage";
export const PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS = 60;
export const PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS = 300;
export const PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS = 300;

export type ProviderAccountUsageAdapterKind = "disabled" | "sub2api" | "newapi";
export type ProviderAccountUsageNewApiQueryMode = "billing" | "account";

export type ProviderAccountUsageConfig = {
  adapterKind: ProviderAccountUsageAdapterKind;
  newApiQueryMode: ProviderAccountUsageNewApiQueryMode;
  timedRefreshEnabled: boolean;
  refreshIntervalSeconds: number;
};

const DEFAULT_CONFIG: ProviderAccountUsageConfig = {
  adapterKind: "disabled",
  newApiQueryMode: "billing",
  timedRefreshEnabled: true,
  refreshIntervalSeconds: PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function rowKey(pluginId: string, namespace: string) {
  return `${pluginId}\u0000${namespace}`;
}

export function isProviderAccountUsageAdapterKind(
  value: unknown
): value is ProviderAccountUsageAdapterKind {
  return value === "disabled" || value === "sub2api" || value === "newapi";
}

export function isProviderAccountUsageNewApiQueryMode(
  value: unknown
): value is ProviderAccountUsageNewApiQueryMode {
  return value === "billing" || value === "account";
}

export function normalizeProviderAccountUsageRefreshIntervalSeconds(value: unknown): number {
  const numeric =
    typeof value === "number"
      ? value
      : typeof value === "string" && value.trim()
        ? Number(value)
        : PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS;
  if (!Number.isFinite(numeric)) return PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS;
  return Math.min(
    PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS,
    Math.max(PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS, Math.round(numeric))
  );
}

export function readProviderAccountUsageConfig(
  extensionValues: Pick<ProviderSummary, "extension_values">["extension_values"] | null | undefined
): ProviderAccountUsageConfig {
  const row = extensionValues?.find(
    (value) =>
      value.pluginId === PROVIDER_ACCOUNT_USAGE_PLUGIN_ID &&
      value.namespace === PROVIDER_ACCOUNT_USAGE_NAMESPACE
  );
  if (!row || !isRecord(row.values)) return DEFAULT_CONFIG;

  const rawAdapterKind = row.values.adapterKind;
  const adapterKind = isProviderAccountUsageAdapterKind(rawAdapterKind)
    ? rawAdapterKind
    : "disabled";
  const newApiQueryMode = isProviderAccountUsageNewApiQueryMode(row.values.newApiQueryMode)
    ? row.values.newApiQueryMode
    : "billing";
  const timedRefreshEnabled =
    typeof row.values.timedRefreshEnabled === "boolean" ? row.values.timedRefreshEnabled : true;
  const refreshIntervalSeconds = normalizeProviderAccountUsageRefreshIntervalSeconds(
    row.values.refreshIntervalSeconds
  );

  return {
    adapterKind,
    newApiQueryMode,
    timedRefreshEnabled,
    refreshIntervalSeconds,
  };
}

export function isProviderAccountUsageAccountCredentialsRequired(
  provider: Pick<
    ProviderSummary,
    "extension_values" | "newapi_account_user_id" | "newapi_account_access_token_configured"
  >
): boolean {
  const config = readProviderAccountUsageConfig(provider.extension_values);
  return (
    config.adapterKind === "newapi" &&
    config.newApiQueryMode === "account" &&
    (!provider.newapi_account_user_id || !provider.newapi_account_access_token_configured)
  );
}

export function isProviderAccountUsageConfigured(
  provider: Pick<ProviderSummary, "auth_mode" | "source_provider_id" | "extension_values">
): boolean {
  if (provider.auth_mode !== "api_key" || provider.source_provider_id != null) return false;
  const config = readProviderAccountUsageConfig(provider.extension_values);
  return config.adapterKind === "sub2api" || config.adapterKind === "newapi";
}

export function mergeProviderAccountUsageExtensionValues({
  rows,
  existingRows,
  config,
}: {
  rows: ProviderExtensionValuesInput[] | null;
  existingRows: Pick<ProviderSummary, "extension_values">["extension_values"];
  config: ProviderAccountUsageConfig;
}): ProviderExtensionValuesInput[] | null {
  const sourceRows =
    rows ??
    existingRows.map((value) => ({
      pluginId: value.pluginId,
      namespace: value.namespace,
      values: value.values,
    }));
  const accountUsageKey = rowKey(
    PROVIDER_ACCOUNT_USAGE_PLUGIN_ID,
    PROVIDER_ACCOUNT_USAGE_NAMESPACE
  );
  const withoutAccountUsage = sourceRows.filter(
    (row) => rowKey(row.pluginId, row.namespace) !== accountUsageKey
  );

  const existingAccountUsage = sourceRows.some(
    (row) => rowKey(row.pluginId, row.namespace) === accountUsageKey
  );
  if (
    config.adapterKind === "disabled" &&
    config.newApiQueryMode === "billing" &&
    !existingAccountUsage
  ) {
    if (rows == null && withoutAccountUsage.length === existingRows.length) return null;
    return withoutAccountUsage.length > 0 ? withoutAccountUsage : [];
  }

  const values: Record<string, string | number | boolean> = {
    adapterKind: config.adapterKind,
    newApiQueryMode: config.newApiQueryMode,
    timedRefreshEnabled: config.timedRefreshEnabled,
    refreshIntervalSeconds: normalizeProviderAccountUsageRefreshIntervalSeconds(
      config.refreshIntervalSeconds
    ),
  };
  return [
    ...withoutAccountUsage,
    {
      pluginId: PROVIDER_ACCOUNT_USAGE_PLUGIN_ID,
      namespace: PROVIDER_ACCOUNT_USAGE_NAMESPACE,
      values,
    },
  ];
}
