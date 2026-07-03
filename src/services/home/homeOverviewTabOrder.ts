export type HomeOverviewTabKey =
  | "workspaceConfig"
  | "circuit"
  | "sessions"
  | "providerLimit"
  | "oauthQuota";

export const HOME_OVERVIEW_TAB_ORDER_STORAGE_KEY = "aio-home-overview-tab-order";

export const HOME_OVERVIEW_TABS: Array<{ key: HomeOverviewTabKey; label: string }> = [
  { key: "workspaceConfig", label: "配置信息" },
  { key: "circuit", label: "熔断信息" },
  { key: "sessions", label: "活跃 Session" },
  { key: "providerLimit", label: "供应商限额" },
  { key: "oauthQuota", label: "OAuth 配额" },
];

const DEFAULT_HOME_OVERVIEW_TAB_KEYS = HOME_OVERVIEW_TABS.map((item) => item.key);
const DEFAULT_HOME_OVERVIEW_TAB_KEY_SET = new Set(DEFAULT_HOME_OVERVIEW_TAB_KEYS);

export function normalizeHomeOverviewTabOrder(input: unknown): HomeOverviewTabKey[] {
  if (!Array.isArray(input)) return DEFAULT_HOME_OVERVIEW_TAB_KEYS;

  const seen = new Set<HomeOverviewTabKey>();
  const normalized: HomeOverviewTabKey[] = [];

  for (const value of input) {
    if (
      typeof value === "string" &&
      DEFAULT_HOME_OVERVIEW_TAB_KEY_SET.has(value as HomeOverviewTabKey) &&
      !seen.has(value as HomeOverviewTabKey)
    ) {
      normalized.push(value as HomeOverviewTabKey);
      seen.add(value as HomeOverviewTabKey);
    }
  }

  for (const key of DEFAULT_HOME_OVERVIEW_TAB_KEYS) {
    if (!seen.has(key)) normalized.push(key);
  }

  return normalized;
}

export function readHomeOverviewTabOrderFromStorage(): HomeOverviewTabKey[] {
  if (typeof window === "undefined") return DEFAULT_HOME_OVERVIEW_TAB_KEYS;

  try {
    const raw = window.localStorage.getItem(HOME_OVERVIEW_TAB_ORDER_STORAGE_KEY);
    if (!raw) return DEFAULT_HOME_OVERVIEW_TAB_KEYS;
    return normalizeHomeOverviewTabOrder(JSON.parse(raw));
  } catch {
    return DEFAULT_HOME_OVERVIEW_TAB_KEYS;
  }
}

export function writeHomeOverviewTabOrderToStorage(order: HomeOverviewTabKey[]) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(
      HOME_OVERVIEW_TAB_ORDER_STORAGE_KEY,
      JSON.stringify(normalizeHomeOverviewTabOrder(order))
    );
  } catch {}
}
