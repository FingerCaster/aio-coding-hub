import { describe, expect, it } from "vitest";
import {
  mergeProviderAccountUsageExtensionValues,
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
    ).toEqual({ adapterKind: "newapi", newApiUserId: "42" });
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
        values: { adapterKind: "newapi", newApiUserId: "7" },
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
