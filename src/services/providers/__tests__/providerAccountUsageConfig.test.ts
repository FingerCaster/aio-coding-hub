import { describe, expect, it } from "vitest";
import {
  mergeProviderAccountUsageExtensionValues,
  normalizeProviderAccountUsageRefreshIntervalSeconds,
  readProviderAccountUsageConfig,
} from "../providerAccountUsageConfig";

describe("providerAccountUsageConfig", () => {
  it("reads existing NewAPI config from provider extension values", () => {
    expect(
      readProviderAccountUsageConfig([
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "newapi", newApiUserId: " 42 " },
          updatedAt: 1,
        },
      ])
    ).toEqual({
      adapterKind: "newapi",
      newApiUserId: "42",
      timedRefreshEnabled: true,
      refreshIntervalSeconds: 300,
    });
  });

  it("reads timed refresh config and clamps interval bounds", () => {
    expect(
      readProviderAccountUsageConfig([
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "sub2api",
            timedRefreshEnabled: false,
            refreshIntervalSeconds: 15,
          },
          updatedAt: 1,
        },
      ])
    ).toEqual({
      adapterKind: "sub2api",
      newApiUserId: "",
      timedRefreshEnabled: false,
      refreshIntervalSeconds: 60,
    });

    expect(normalizeProviderAccountUsageRefreshIntervalSeconds(600)).toBe(300);
    expect(normalizeProviderAccountUsageRefreshIntervalSeconds("90")).toBe(90);
    expect(normalizeProviderAccountUsageRefreshIntervalSeconds("bad")).toBe(300);
  });

  it("merges exact core payload while preserving unrelated extension rows", () => {
    const merged = mergeProviderAccountUsageExtensionValues({
      rows: [
        {
          pluginId: "community.other",
          namespace: "settings",
          values: { mode: "keep" },
        },
      ],
      existingRows: [],
      config: {
        adapterKind: "newapi",
        newApiUserId: "7",
        timedRefreshEnabled: false,
        refreshIntervalSeconds: 120,
      },
    });

    expect(merged).toEqual([
      {
        pluginId: "community.other",
        namespace: "settings",
        values: { mode: "keep" },
      },
      {
        pluginId: "core.provider-account-usage",
        namespace: "accountUsage",
        values: {
          adapterKind: "newapi",
          timedRefreshEnabled: false,
          refreshIntervalSeconds: 120,
          newApiUserId: "7",
        },
      },
    ]);
  });

  it("removes account usage row when disabled without dropping unrelated rows", () => {
    const merged = mergeProviderAccountUsageExtensionValues({
      rows: null,
      existingRows: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: { adapterKind: "sub2api" },
          updatedAt: 1,
        },
        {
          pluginId: "community.other",
          namespace: "settings",
          values: { mode: "keep" },
          updatedAt: 2,
        },
      ],
      config: {
        adapterKind: "disabled",
        newApiUserId: "",
        timedRefreshEnabled: true,
        refreshIntervalSeconds: 300,
      },
    });

    expect(merged).toEqual([
      {
        pluginId: "community.other",
        namespace: "settings",
        values: { mode: "keep" },
      },
    ]);
  });
});
