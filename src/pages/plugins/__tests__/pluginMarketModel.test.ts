import { describe, expect, it } from "vitest";
import type { PluginMarketListing, PluginSummary } from "../../../services/plugins";
import {
  FEATURED_PLUGIN_CATALOG,
  buildFeaturedMarketCards,
  buildMarketListingCards,
  toMarketInstallInput,
} from "../pluginMarketModel";

function summary(overrides: Partial<PluginSummary> = {}): PluginSummary {
  return {
    id: 1,
    plugin_id: "official.privacy-filter",
    name: "Privacy Filter",
    current_version: "1.0.0",
    status: "enabled",
    runtime: "extensionHost",
    permission_risk: "high",
    update_available: false,
    last_error: null,
    created_at: 1,
    updated_at: 2,
    ...overrides,
  };
}

function listing(overrides: Partial<PluginMarketListing> = {}): PluginMarketListing {
  return {
    pluginId: "community.safe-helper",
    name: "Safe Helper",
    latestVersion: "1.0.0",
    downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
    marketSourceUrl: "https://plugins.example.test/index.json",
    checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    signature: "signed-safe",
    riskLabels: ["request.body.read"],
    revoked: false,
    compatible: true,
    updateAvailable: false,
    installBlockReason: null,
    ...overrides,
  };
}

describe("pluginMarketModel", () => {
  it("defines the initial featured catalog exactly", () => {
    expect(FEATURED_PLUGIN_CATALOG).toEqual([
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
    ]);
    expect(JSON.stringify(FEATURED_PLUGIN_CATALOG)).not.toContain("声明式规则");
  });

  it("builds featured cards without requiring market index input", () => {
    const cards = buildFeaturedMarketCards([], FEATURED_PLUGIN_CATALOG);

    expect(cards.map((card) => card.pluginId)).toEqual([
      "official.privacy-filter",
      "examples/prompt-helper",
      "examples/redactor",
      "examples/response-guard",
    ]);
    expect(cards[0]).toMatchObject({
      state: "installable",
      action: "install",
      actionLabel: "安装",
      sourceLabel: "官方来源",
    });
    expect(cards[1]).toMatchObject({
      state: "exampleOnly",
      action: "example",
      actionLabel: "示例",
      disabledReason: "示例插件暂未发布为可安装包",
    });
  });

  it("marks featured official plugins as installed when versions are present", () => {
    const cards = buildFeaturedMarketCards([summary()], FEATURED_PLUGIN_CATALOG);
    const privacyFilter = cards.find((card) => card.pluginId === "official.privacy-filter");

    expect(privacyFilter).toMatchObject({
      installedVersion: "1.0.0",
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
    });
  });

  it("marks installed plugins as installed even when the version is unavailable", () => {
    const cards = buildFeaturedMarketCards(
      [summary({ current_version: null })],
      FEATURED_PLUGIN_CATALOG
    );
    const privacyFilter = cards.find((card) => card.pluginId === "official.privacy-filter");

    expect(privacyFilter).toMatchObject({
      installedVersion: null,
      state: "installed",
      action: "installed",
      actionLabel: "已安装",
    });
  });

  it("maps parsed market listings to concise install states", () => {
    const cards = buildMarketListingCards(
      [],
      [
        listing(),
        listing({
          pluginId: "community.revoked",
          name: "Revoked Helper",
          revoked: true,
          compatible: false,
          installBlockReason: "revoked",
        }),
        listing({
          pluginId: "community.future",
          name: "Future Helper",
          compatible: false,
          installBlockReason: "incompatible",
        }),
        listing({
          pluginId: "community.missing",
          name: "Missing Trust",
          checksum: null,
        }),
      ]
    );

    expect(cards.map((card) => [card.pluginId, card.state, card.actionLabel])).toEqual([
      ["community.safe-helper", "installable", "安装"],
      ["community.revoked", "revoked", "已撤销"],
      ["community.future", "incompatible", "不兼容"],
      ["community.missing", "missingTrustData", "不可安装"],
    ]);
    expect(cards[1].disabledReason).toBe("插件已被市场撤销");
    expect(cards[2].disabledReason).toBe("当前宿主版本不兼容");
    expect(cards[3].disabledReason).toBe("缺少下载地址或校验信息");
  });

  it("blocks market listings with unrecognized install block reasons", () => {
    const [card] = buildMarketListingCards(
      [],
      [listing({ installBlockReason: "blocked-by-host" })]
    );

    expect(card).toMatchObject({
      state: "missingTrustData",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "缺少下载地址或校验信息",
    });
    expect(card.disabledReason).not.toBe("blocked-by-host");
    expect(toMarketInstallInput(card)).toBeNull();
  });

  it("blocks reserved official namespace listings from advanced market installs", () => {
    const cards = buildMarketListingCards(
      [],
      [
        listing({
          pluginId: "official.privacy-filter",
          name: "Fake Official",
          compatible: false,
          installBlockReason: "reserved_official_namespace",
        }),
      ]
    );

    expect(cards[0]).toMatchObject({
      state: "reservedOfficial",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "内置命名空间只能通过内置插件安装",
    });
    expect(toMarketInstallInput(cards[0])).toBeNull();
  });

  it("blocks reserved core namespace listings from advanced market installs", () => {
    const cards = buildMarketListingCards(
      [],
      [
        listing({
          pluginId: "core.provider-account-usage",
          name: "Fake Core",
          compatible: false,
          installBlockReason: "reserved_core_namespace",
        }),
      ]
    );

    expect(cards[0]).toMatchObject({
      state: "reservedOfficial",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "内置命名空间只能通过内置插件安装",
    });
    expect(toMarketInstallInput(cards[0])).toBeNull();
  });

  it("blocks featured market cards when no listing is present", () => {
    const [card] = buildFeaturedMarketCards(
      [],
      [
        {
          pluginId: "community.unroutable",
          name: "Unroutable Market Card",
          summary: "市场条目缺失 listing 时不可安装。",
          category: "developer",
          source: "market",
          riskLabels: ["request.body.read"],
        },
      ]
    );

    expect(card).toMatchObject({
      state: "missingTrustData",
      action: "unavailable",
      actionLabel: "不可安装",
      disabledReason: "缺少下载地址或校验信息",
      listing: null,
    });
    expect(toMarketInstallInput(card)).toBeNull();
  });

  it("marks parsed market listings as updateable when installed and update is available", () => {
    const cards = buildMarketListingCards(
      [summary({ plugin_id: "community.safe-helper", current_version: "0.9.0" })],
      [listing({ updateAvailable: true, latestVersion: "1.0.0" })]
    );

    expect(cards[0]).toMatchObject({
      installedVersion: "0.9.0",
      state: "updateAvailable",
      action: "update",
      actionLabel: "更新",
    });
  });

  it("creates remote install input only for installable or updateable market cards", () => {
    const [card] = buildMarketListingCards([], [listing()]);

    expect(toMarketInstallInput(card)).toEqual({
      pluginId: "community.safe-helper",
      downloadUrl: "https://plugins.example.test/safe-helper.aio-plugin",
      checksum: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      signature: "signed-safe",
      publicKey: null,
      marketSourceUrl: "https://plugins.example.test/index.json",
      source: "market",
    });

    const [update] = buildMarketListingCards(
      [summary({ plugin_id: "community.safe-helper", current_version: "0.9.0" })],
      [listing({ updateAvailable: true })]
    );
    expect(toMarketInstallInput(update)).toEqual(toMarketInstallInput(card));

    const [installed] = buildMarketListingCards(
      [summary({ plugin_id: "community.safe-helper" })],
      [listing()]
    );
    const [revoked] = buildMarketListingCards([], [listing({ revoked: true })]);
    const [unavailable] = buildMarketListingCards([], [listing({ checksum: null })]);
    const [example] = buildFeaturedMarketCards([], FEATURED_PLUGIN_CATALOG).filter(
      (candidate) => candidate.action === "example"
    );

    expect(toMarketInstallInput(installed)).toBeNull();
    expect(toMarketInstallInput(revoked)).toBeNull();
    expect(toMarketInstallInput(unavailable)).toBeNull();
    expect(toMarketInstallInput(example)).toBeNull();
  });
});
