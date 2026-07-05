import type { ProviderExtensionValuesInput, ProviderSummary } from "./providers";

export const PROVIDER_ACCOUNT_USAGE_PLUGIN_ID = "core.provider-account-usage";
export const PROVIDER_ACCOUNT_USAGE_NAMESPACE = "accountUsage";

export type ProviderAccountUsageAdapterKind = "disabled" | "sub2api" | "newapi";

export type ProviderAccountUsageConfig = {
  adapterKind: ProviderAccountUsageAdapterKind;
  newApiUserId: string;
};

const DEFAULT_CONFIG: ProviderAccountUsageConfig = {
  adapterKind: "disabled",
  newApiUserId: "",
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
  const newApiUserId =
    typeof row.values.newApiUserId === "string" ? row.values.newApiUserId.trim() : "";

  return {
    adapterKind,
    newApiUserId: adapterKind === "newapi" ? newApiUserId : "",
  };
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

  if (config.adapterKind === "disabled") {
    if (rows == null && withoutAccountUsage.length === existingRows.length) return null;
    return withoutAccountUsage.length > 0 ? withoutAccountUsage : [];
  }

  const values: Record<string, string> = {
    adapterKind: config.adapterKind,
  };
  const newApiUserId = config.newApiUserId.trim();
  if (config.adapterKind === "newapi" && newApiUserId) {
    values.newApiUserId = newApiUserId;
  }

  return [
    ...withoutAccountUsage,
    {
      pluginId: PROVIDER_ACCOUNT_USAGE_PLUGIN_ID,
      namespace: PROVIDER_ACCOUNT_USAGE_NAMESPACE,
      values,
    },
  ];
}
