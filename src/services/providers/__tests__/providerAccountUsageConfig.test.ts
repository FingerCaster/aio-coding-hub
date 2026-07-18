import { describe, expect, it } from "vitest";
import {
  isProviderAccountUsageAccountCredentialsRequired,
  mergeProviderAccountUsageExtensionValues,
  normalizeProviderAccountUsageRefreshIntervalSeconds,
  readProviderAccountUsageConfig,
} from "../providerAccountUsageConfig";

describe("providerAccountUsageConfig", () => {
  it("defaults legacy NewAPI config to billing without reading historical User ID", () => {
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
      newApiQueryMode: "billing",
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
      newApiQueryMode: "billing",
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
        newApiQueryMode: "account",
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
          newApiQueryMode: "account",
          timedRefreshEnabled: false,
          refreshIntervalSeconds: 120,
        },
      },
    ]);
  });

  it("retains the explicit query mode when disabled without dropping unrelated rows", () => {
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
        newApiQueryMode: "account",
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
      {
        pluginId: "core.provider-account-usage",
        namespace: "accountUsage",
        values: {
          adapterKind: "disabled",
          newApiQueryMode: "account",
          timedRefreshEnabled: true,
          refreshIntervalSeconds: 300,
        },
      },
    ]);
  });

  it("requires both private credentials only for explicit NewAPI account mode", () => {
    const extension_values = [
      {
        pluginId: "core.provider-account-usage",
        namespace: "accountUsage",
        values: { adapterKind: "newapi", newApiQueryMode: "account" },
        updatedAt: 1,
      },
    ];
    expect(
      isProviderAccountUsageAccountCredentialsRequired({
        extension_values,
        newapi_account_user_id: "42",
        newapi_account_access_token_configured: false,
      })
    ).toBe(true);
    expect(
      isProviderAccountUsageAccountCredentialsRequired({
        extension_values,
        newapi_account_user_id: "42",
        newapi_account_access_token_configured: true,
      })
    ).toBe(false);
  });
});
