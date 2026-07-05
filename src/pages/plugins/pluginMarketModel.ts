// Usage: Pure view model helpers for plugin market cards.

import type { PluginMarketListing, PluginSummary } from "../../services/plugins";

export type PluginMarketCardState =
  | "installable"
  | "installed"
  | "updateAvailable"
  | "incompatible"
  | "revoked"
  | "reservedOfficial"
  | "missingTrustData"
  | "exampleOnly";

export type PluginMarketCardAction = "install" | "update" | "installed" | "unavailable" | "example";

export type PluginFeaturedCatalogItem = {
  pluginId: string;
  name: string;
  summary: string;
  category: "privacy" | "prompt" | "safety" | "developer";
  source: "official" | "example" | "market";
  riskLabels: string[];
  listing?: PluginMarketListing;
};

export type PluginMarketCardView = {
  pluginId: string;
  name: string;
  summary: string;
  category: string;
  latestVersion: string | null;
  installedVersion: string | null;
  state: PluginMarketCardState;
  action: PluginMarketCardAction;
  actionLabel: string;
  disabledReason: string | null;
  riskLabel: string;
  trustLabel: string;
  sourceLabel: string;
  listing: PluginMarketListing | null;
};

export type MarketInstallInput = {
  pluginId: string;
  downloadUrl: string;
  checksum: string;
  signature: string | null;
  publicKey: null;
  marketSourceUrl: string | null;
  source: "market";
};

export const FEATURED_PLUGIN_CATALOG: PluginFeaturedCatalogItem[] = [
  {
    pluginId: "official.privacy-filter",
    name: "Privacy Filter",
    summary: "发送前脱敏敏感信息，并在日志保存前做不可逆脱敏。",
    category: "privacy",
    source: "official",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/prompt-helper",
    name: "Prompt Helper",
    summary: "示例：请求发送前补充提示词约束，覆盖 Claude 和 Codex 请求形态。",
    category: "prompt",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容"],
  },
  {
    pluginId: "examples/redactor",
    name: "Redactor",
    summary: "示例：发送前清理敏感内容，日志保存前同步脱敏。",
    category: "privacy",
    source: "example",
    riskLabels: ["读取请求内容", "修改请求内容", "日志脱敏"],
  },
  {
    pluginId: "examples/response-guard",
    name: "Response Guard",
    summary: "示例：响应返回前做轻量检查、告警或阻断。",
    category: "safety",
    source: "example",
    riskLabels: ["读取响应内容", "修改响应内容"],
  },
];

const EXAMPLE_ONLY_REASON = "示例插件暂未发布为可安装包";
const MISSING_TRUST_DATA_STATE: CardStateDetails = {
  state: "missingTrustData",
  action: "unavailable",
  actionLabel: "不可安装",
  disabledReason: "缺少下载地址或校验信息",
};

const SOURCE_LABELS: Record<PluginFeaturedCatalogItem["source"] | "custom", string> = {
  official: "官方来源",
  example: "示例来源",
  market: "市场来源",
  custom: "自定义来源",
};

type CardStateDetails = Pick<
  PluginMarketCardView,
  "state" | "action" | "actionLabel" | "disabledReason"
>;

export function buildFeaturedMarketCards(
  installed: readonly PluginSummary[],
  catalog: readonly PluginFeaturedCatalogItem[] = FEATURED_PLUGIN_CATALOG
): PluginMarketCardView[] {
  const installedPlugins = buildInstalledPluginMap(installed);

  return catalog.map((item) => {
    const installedPlugin = installedPlugins.get(item.pluginId);
    const installedVersion = installedPlugin?.current_version ?? null;
    const listing = item.listing ?? null;
    const state = item.listing
      ? getListingState(item.listing, Boolean(installedPlugin))
      : getFeaturedState(item.source, Boolean(installedPlugin));

    return {
      pluginId: item.pluginId,
      name: item.name,
      summary: item.summary,
      category: item.category,
      latestVersion: listing?.latestVersion ?? null,
      installedVersion,
      ...state,
      riskLabel: formatRiskLabel(item.riskLabels),
      trustLabel: getTrustLabel(listing, item.source),
      sourceLabel: SOURCE_LABELS[item.source],
      listing,
    };
  });
}

export function buildMarketListingCards(
  installed: readonly PluginSummary[],
  listings: readonly PluginMarketListing[]
): PluginMarketCardView[] {
  const installedPlugins = buildInstalledPluginMap(installed);

  return listings.map((listing) => {
    const installedPlugin = installedPlugins.get(listing.pluginId);
    const installedVersion = installedPlugin?.current_version ?? null;

    return {
      pluginId: listing.pluginId,
      name: listing.name,
      summary: "来自自定义市场源的插件。",
      category: "developer",
      latestVersion: listing.latestVersion,
      installedVersion,
      ...getListingState(listing, Boolean(installedPlugin)),
      riskLabel: formatRiskLabel(listing.riskLabels),
      trustLabel: getTrustLabel(listing, "market"),
      sourceLabel: SOURCE_LABELS.custom,
      listing,
    };
  });
}

export function toMarketInstallInput(card: PluginMarketCardView): MarketInstallInput | null {
  if (card.state !== "installable" && card.state !== "updateAvailable") return null;
  if (card.action !== "install" && card.action !== "update") return null;
  if (!card.listing?.downloadUrl || !card.listing.checksum) return null;

  return {
    pluginId: card.pluginId,
    downloadUrl: card.listing.downloadUrl,
    checksum: card.listing.checksum,
    signature: card.listing.signature,
    publicKey: null,
    marketSourceUrl: card.listing.marketSourceUrl,
    source: "market",
  };
}

function buildInstalledPluginMap(installed: readonly PluginSummary[]) {
  const plugins = new Map<string, PluginSummary>();
  for (const item of installed) {
    plugins.set(item.plugin_id, item);
  }
  return plugins;
}

function getFeaturedState(
  source: PluginFeaturedCatalogItem["source"],
  installed: boolean
): CardStateDetails {
  if (installed) {
    return {
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
      disabledReason: null,
    };
  }

  if (source === "example") {
    return {
      state: "exampleOnly",
      action: "example",
      actionLabel: "示例",
      disabledReason: EXAMPLE_ONLY_REASON,
    };
  }

  if (source === "market") {
    return MISSING_TRUST_DATA_STATE;
  }

  return {
    state: "installable",
    action: "install",
    actionLabel: "安装",
    disabledReason: null,
  };
}

function getListingState(listing: PluginMarketListing, installed: boolean): CardStateDetails {
  if (
    listing.installBlockReason === "reserved_official_namespace" ||
    listing.installBlockReason === "reserved_core_namespace"
  ) {
    return {
      state: "reservedOfficial",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "内置命名空间只能通过内置插件安装",
    };
  }

  if (listing.revoked) {
    return {
      state: "revoked",
      action: "unavailable",
      actionLabel: "已撤销",
      disabledReason: "插件已被市场撤销",
    };
  }

  if (!listing.compatible) {
    return {
      state: "incompatible",
      action: "unavailable",
      actionLabel: "不兼容",
      disabledReason: "当前宿主版本不兼容",
    };
  }

  if (listing.installBlockReason || !listing.downloadUrl || !listing.checksum) {
    return MISSING_TRUST_DATA_STATE;
  }

  if (installed && listing.updateAvailable) {
    return {
      state: "updateAvailable",
      action: "update",
      actionLabel: "更新",
      disabledReason: null,
    };
  }

  if (installed) {
    return {
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
      disabledReason: null,
    };
  }

  return {
    state: "installable",
    action: "install",
    actionLabel: "安装",
    disabledReason: null,
  };
}

function formatRiskLabel(riskLabels: readonly string[]) {
  return riskLabels.length > 0 ? riskLabels.join("、") : "无风险标签";
}

function getTrustLabel(
  listing: PluginMarketListing | null,
  source: PluginFeaturedCatalogItem["source"]
) {
  if (source === "official") return "官方来源";
  if (source === "example") return "示例未发布";
  if (!listing) return "未签名";
  return listing.signature ? "已签名" : "仅校验和";
}
