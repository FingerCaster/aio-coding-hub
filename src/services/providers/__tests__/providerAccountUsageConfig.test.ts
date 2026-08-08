import { describe, expect, it } from "vitest";
import {
  PROVIDER_ACCOUNT_USAGE_CUSTOM_SCRIPT_TEMPLATE,
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH,
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES,
  getProviderAccountUsageCustomScriptUtf8ByteLength,
  hasProviderAccountUsageCustomPermissionChange,
  isProviderAccountUsageAccountCredentialsRequired,
  isProviderAccountUsageConfigured,
  mergeProviderAccountUsageExtensionValues,
  normalizeProviderAccountUsageCustomAllowedOrigins,
  normalizeProviderAccountUsageCustomTimeoutSeconds,
  normalizeProviderAccountUsageRefreshIntervalSeconds,
  prepareProviderAccountUsageCustomAllowedOrigins,
  readProviderAccountUsageConfig,
  truncateProviderAccountUsageCustomScriptUtf8,
  validateProviderAccountUsageCustomAllowedOrigins,
} from "../providerAccountUsageConfig";

const customDefaults = {
  customScript: "",
  customAllowedOrigins: [] as string[],
  customTimeoutSeconds: 10,
  customEnabled: false,
};

const CUSTOM_SCRIPT = "({ request: () => ({}), parse: () => ({}) })";
const CUSTOM_ORIGIN = "https://usage.example.invalid";
const CUSTOM_PERMISSION_FINGERPRINT =
  "12a854a6a3be44a980ac0e115c8b0f4a1b5eabdd030c220b2526e677a0d03111";
const EMPTY_ORIGIN_CUSTOM_SCRIPT = "({ request() {}, parse() {} })";
const EMPTY_ORIGIN_CUSTOM_PERMISSION_FINGERPRINT =
  "a5404aed75c3b2048427b970650dd64c4ce70cfe73506f15ab1e7513d8cf8117";

type ProviderConfigSource = Required<
  NonNullable<Parameters<typeof readProviderAccountUsageConfig>[0]>
>;

function configSource(
  extension_values: ProviderConfigSource["extension_values"],
  primaryBaseUrl = "https://api.example.invalid/v1"
): ProviderConfigSource {
  return { extension_values, base_urls: [primaryBaseUrl] };
}

describe("providerAccountUsageConfig", () => {
  it("defaults legacy NewAPI config to billing without reading historical User ID", () => {
    expect(
      readProviderAccountUsageConfig(
        configSource([
          {
            pluginId: "core.provider-account-usage",
            namespace: "accountUsage",
            values: { adapterKind: "newapi", newApiUserId: " 42 " },
            updatedAt: 1,
          },
        ])
      )
    ).toEqual({
      adapterKind: "newapi",
      newApiQueryMode: "billing",
      timedRefreshEnabled: true,
      refreshIntervalSeconds: 300,
      ...customDefaults,
    });
  });

  it("returns isolated default origin arrays", () => {
    const first = readProviderAccountUsageConfig(undefined);
    first.customAllowedOrigins.push("https://mutated.example.invalid");

    expect(readProviderAccountUsageConfig(undefined).customAllowedOrigins).toEqual([]);
    expect(readProviderAccountUsageConfig({})).toEqual(
      expect.objectContaining({ adapterKind: "disabled", customEnabled: false })
    );
  });

  it("reads timed refresh config and clamps interval bounds", () => {
    expect(
      readProviderAccountUsageConfig(
        configSource([
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
      )
    ).toEqual({
      adapterKind: "sub2api",
      newApiQueryMode: "billing",
      timedRefreshEnabled: false,
      refreshIntervalSeconds: 60,
      ...customDefaults,
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
        ...customDefaults,
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
        ...customDefaults,
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
        base_urls: ["https://api.example.invalid/v1"],
        extension_values,
        newapi_account_user_id: "42",
        newapi_account_access_token_configured: false,
      })
    ).toBe(true);
    expect(
      isProviderAccountUsageAccountCredentialsRequired({
        base_urls: ["https://api.example.invalid/v1"],
        extension_values,
        newapi_account_user_id: "42",
        newapi_account_access_token_configured: true,
      })
    ).toBe(false);
  });

  it("reads, normalizes, and merges a local custom script configuration", () => {
    const extensionValues = [
      {
        pluginId: "core.provider-account-usage",
        namespace: "accountUsage",
        values: {
          adapterKind: "custom",
          customScript: CUSTOM_SCRIPT,
          customAllowedOrigins: [CUSTOM_ORIGIN],
          customTimeoutSeconds: 10,
          customEnabled: true,
          customPermissionFingerprint: CUSTOM_PERMISSION_FINGERPRINT,
          customPermissionBaseOrigin: "https://api.example.invalid",
        },
        updatedAt: 1,
      },
    ];

    expect(readProviderAccountUsageConfig(configSource(extensionValues))).toEqual({
      adapterKind: "custom",
      newApiQueryMode: "billing",
      timedRefreshEnabled: true,
      refreshIntervalSeconds: 300,
      customScript: CUSTOM_SCRIPT,
      customAllowedOrigins: [CUSTOM_ORIGIN],
      customTimeoutSeconds: 10,
      customEnabled: true,
    });

    expect(
      mergeProviderAccountUsageExtensionValues({
        rows: null,
        existingRows: extensionValues,
        config: readProviderAccountUsageConfig(configSource(extensionValues)),
      })
    ).toEqual([
      {
        pluginId: "core.provider-account-usage",
        namespace: "accountUsage",
        values: {
          adapterKind: "custom",
          newApiQueryMode: "billing",
          timedRefreshEnabled: true,
          refreshIntervalSeconds: 300,
          customScript: CUSTOM_SCRIPT,
          customAllowedOrigins: [CUSTOM_ORIGIN],
          customTimeoutSeconds: 10,
          customEnabled: true,
        },
      },
    ]);
  });

  it("fails closed when persisted custom origins are not a pure string array", () => {
    for (const customAllowedOrigins of [
      "https://usage.example.invalid",
      ["https://usage.example.invalid", 42],
    ]) {
      const config = readProviderAccountUsageConfig(
        configSource([
          {
            pluginId: "core.provider-account-usage",
            namespace: "accountUsage",
            values: {
              adapterKind: "custom",
              customScript: CUSTOM_SCRIPT,
              customAllowedOrigins,
              customTimeoutSeconds: 10,
              customEnabled: true,
              customPermissionFingerprint: CUSTOM_PERMISSION_FINGERPRINT,
              customPermissionBaseOrigin: "https://api.example.invalid",
            },
            updatedAt: 1,
          },
        ])
      );

      expect(config.customEnabled).toBe(false);
    }
  });

  it("normalizes HTTPS origins for acknowledgement comparisons", () => {
    expect(
      normalizeProviderAccountUsageCustomAllowedOrigins([
        " https://EXAMPLE.invalid:443/ ",
        "https://example.invalid",
        "https://other.example.invalid:8443/",
        "http://example.invalid",
        "https://example.invalid/path",
        "https://example.invalid?",
        "https://example.invalid#",
        "https://user@example.invalid",
        "not-a-url",
      ])
    ).toEqual(["https://example.invalid", "https://other.example.invalid:8443"]);
  });

  it("normalizes and deduplicates origins before enforcing the 16-origin limit", () => {
    const origins = Array.from(
      { length: 16 },
      (_, index) => `https://usage-${index + 1}.example.invalid`
    );
    const withDuplicates = [
      "",
      "   ",
      ...origins,
      "https://USAGE-1.example.invalid:443/",
      "https://usage-2.example.invalid",
    ];

    expect(validateProviderAccountUsageCustomAllowedOrigins(withDuplicates)).toEqual({
      normalizedOrigins: [...origins].sort(),
      error: null,
    });
    expect(prepareProviderAccountUsageCustomAllowedOrigins(withDuplicates)).toEqual(
      [...origins].sort()
    );

    const tooMany = validateProviderAccountUsageCustomAllowedOrigins([
      ...origins,
      "https://usage-17.example.invalid",
    ]);
    expect(tooMany.normalizedOrigins).toHaveLength(17);
    expect(tooMany.error).toContain("规范化去重后最多允许 16 个");
  });

  it("validates each origin with its UTF-8 byte length and reports invalid rows", () => {
    const multibyteOrigin = `https://${"中".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH / 3)}.invalid`;
    expect(multibyteOrigin.length).toBeLessThan(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH);

    expect(validateProviderAccountUsageCustomAllowedOrigins([multibyteOrigin]).error).toBe(
      `第 1 行 Origin 超过 ${PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH} 字节`
    );
    const paddedOrigin = `${" ".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH)}https://valid.example.invalid`;
    expect(validateProviderAccountUsageCustomAllowedOrigins([paddedOrigin]).error).toBe(
      `第 1 行 Origin 超过 ${PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH} 字节`
    );
    expect(
      validateProviderAccountUsageCustomAllowedOrigins([
        "https://valid.example.invalid",
        "http://not-https.example.invalid/path",
      ]).error
    ).toBe("第 2 行必须是仅含协议、主机和端口的 HTTPS Origin");
  });

  it("uses the opaque Base URL placeholder directly in the starter template", () => {
    expect(PROVIDER_ACCOUNT_USAGE_CUSTOM_SCRIPT_TEMPLATE).toContain(
      'url: ctx.baseUrl + "/v1/usage"'
    );
    expect(PROVIDER_ACCOUNT_USAGE_CUSTOM_SCRIPT_TEMPLATE).not.toContain("ctx.baseUrl.replace");
  });

  it("counts and safely truncates custom scripts by UTF-8 bytes", () => {
    expect(getProviderAccountUsageCustomScriptUtf8ByteLength("a中😀")).toBe(8);

    const prefix = "a".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 2);
    const truncated = truncateProviderAccountUsageCustomScriptUtf8(`${prefix}😀`);
    expect(truncated).toBe(prefix);
    expect(getProviderAccountUsageCustomScriptUtf8ByteLength(truncated)).toBe(
      PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 2
    );

    const exact = `${"a".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 4)}😀`;
    expect(truncateProviderAccountUsageCustomScriptUtf8(`${exact}b`)).toBe(exact);
    expect(getProviderAccountUsageCustomScriptUtf8ByteLength(exact)).toBe(
      PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES
    );
  });

  it("revokes custom confirmation when an oversized persisted script must be truncated", () => {
    const oversizedScript = `${"a".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 1)}中`;
    const config = readProviderAccountUsageConfig(
      configSource([
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "custom",
            customScript: oversizedScript,
            customEnabled: true,
          },
          updatedAt: 1,
        },
      ])
    );

    expect(getProviderAccountUsageCustomScriptUtf8ByteLength(config.customScript)).toBe(
      PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 1
    );
    expect(config.customEnabled).toBe(false);
    expect(
      mergeProviderAccountUsageExtensionValues({
        rows: null,
        existingRows: [],
        config: { ...config, customScript: oversizedScript, customEnabled: true },
      })?.[0]?.values
    ).toEqual(
      expect.objectContaining({
        customScript: "a".repeat(PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES - 1),
        customEnabled: false,
      })
    );
  });

  it("revokes custom confirmation only for script or normalized permission changes", () => {
    const previous = {
      customScript: "({ request() {}, parse() {} })",
      customAllowedOrigins: ["https://EXAMPLE.invalid:443/", "https://other.example.invalid"],
    };

    expect(
      hasProviderAccountUsageCustomPermissionChange(previous, {
        customScript: previous.customScript,
        customAllowedOrigins: [
          "https://other.example.invalid/",
          "https://example.invalid",
          "https://example.invalid",
        ],
      })
    ).toBe(false);
    expect(
      hasProviderAccountUsageCustomPermissionChange(previous, {
        ...previous,
        customScript: `${previous.customScript}\n`,
      })
    ).toBe(true);
    expect(
      hasProviderAccountUsageCustomPermissionChange(previous, {
        ...previous,
        customAllowedOrigins: ["https://new.example.invalid"],
      })
    ).toBe(true);
  });

  it("fails closed for invalid persisted timeout or permission bindings", () => {
    const validValues = {
      adapterKind: "custom",
      customScript: CUSTOM_SCRIPT,
      customAllowedOrigins: [CUSTOM_ORIGIN],
      customTimeoutSeconds: 10,
      customEnabled: true,
      customPermissionFingerprint: CUSTOM_PERMISSION_FINGERPRINT,
      customPermissionBaseOrigin: "https://api.example.invalid",
    };
    const withoutField = (field: keyof typeof validValues) =>
      Object.fromEntries(Object.entries(validValues).filter(([key]) => key !== field));
    const read = (
      values: ProviderConfigSource["extension_values"][number]["values"],
      baseUrl = "https://api.example.invalid/v1"
    ) =>
      readProviderAccountUsageConfig(
        configSource(
          [
            {
              pluginId: "core.provider-account-usage",
              namespace: "accountUsage",
              values,
              updatedAt: 1,
            },
          ],
          baseUrl
        )
      );

    expect(read(validValues).customEnabled).toBe(true);
    expect(
      readProviderAccountUsageConfig({
        ...configSource([
          {
            pluginId: "core.provider-account-usage",
            namespace: "accountUsage",
            values: validValues,
            updatedAt: 1,
          },
        ]),
        base_urls: ["   ", "https://api.example.invalid/v1"],
      }).customEnabled
    ).toBe(true);
    expect(
      read({
        ...validValues,
        customPermissionBaseOrigin: "https://API.example.invalid:443/",
      }).customEnabled
    ).toBe(true);
    expect(read(validValues, "https://api.example.invalid/v2").customEnabled).toBe(true);

    expect(read(withoutField("customTimeoutSeconds")).customEnabled).toBe(false);
    for (const customTimeoutSeconds of ["10", 7.5, 1, 16]) {
      expect(read({ ...validValues, customTimeoutSeconds }).customEnabled).toBe(false);
    }
    expect(read(withoutField("customPermissionFingerprint")).customEnabled).toBe(false);
    for (const customPermissionFingerprint of [
      "0".repeat(64),
      CUSTOM_PERMISSION_FINGERPRINT.toUpperCase(),
    ]) {
      expect(read({ ...validValues, customPermissionFingerprint }).customEnabled).toBe(false);
    }
    expect(read(withoutField("customPermissionBaseOrigin")).customEnabled).toBe(false);
    for (const customPermissionBaseOrigin of [
      "http://api.example.invalid",
      "https://other.example.invalid",
    ]) {
      expect(read({ ...validValues, customPermissionBaseOrigin }).customEnabled).toBe(false);
    }
    expect(read(validValues, "https://other.example.invalid/v1").customEnabled).toBe(false);
    expect(read({ ...validValues, customScript: `${CUSTOM_SCRIPT}\n` }).customEnabled).toBe(false);
    expect(
      read({ ...validValues, customAllowedOrigins: ["https://other.example.invalid"] })
        .customEnabled
    ).toBe(false);
  });

  it("matches the backend permission fingerprint for Unicode and multiple origins", () => {
    const config = readProviderAccountUsageConfig(
      configSource([
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "custom",
            customScript: "中😀",
            customAllowedOrigins: ["https://z.example.invalid", "https://例子.测试"],
            customTimeoutSeconds: 10,
            customEnabled: true,
            customPermissionFingerprint:
              "b1845cb5861fe773ffa65346ddd0034ac26b76a1ff35a673ca32898d5c923be6",
            customPermissionBaseOrigin: "https://api.example.invalid",
          },
          updatedAt: 1,
        },
      ])
    );

    expect(config.customEnabled).toBe(true);
  });

  it("bounds the custom request timeout", () => {
    expect(normalizeProviderAccountUsageCustomTimeoutSeconds(1)).toBe(2);
    expect(normalizeProviderAccountUsageCustomTimeoutSeconds("7.6")).toBe(8);
    expect(normalizeProviderAccountUsageCustomTimeoutSeconds(20)).toBe(15);
    expect(normalizeProviderAccountUsageCustomTimeoutSeconds("bad")).toBe(10);
  });

  it("configures custom usage only after confirmation with a non-empty script", () => {
    const provider = {
      auth_mode: "api_key" as const,
      source_provider_id: null,
      base_urls: ["https://api.example.invalid/v1"],
      extension_values: [
        {
          pluginId: "core.provider-account-usage",
          namespace: "accountUsage",
          values: {
            adapterKind: "custom",
            customScript: "  ",
            customAllowedOrigins: [],
            customTimeoutSeconds: 10,
            customEnabled: true,
            customPermissionFingerprint: EMPTY_ORIGIN_CUSTOM_PERMISSION_FINGERPRINT,
            customPermissionBaseOrigin: "https://api.example.invalid",
          },
          updatedAt: 1,
        },
      ],
    };

    expect(isProviderAccountUsageConfigured(provider)).toBe(false);
    provider.extension_values[0].values.customScript = EMPTY_ORIGIN_CUSTOM_SCRIPT;
    expect(isProviderAccountUsageConfigured(provider)).toBe(true);
    provider.extension_values[0].values.customEnabled = false;
    expect(isProviderAccountUsageConfigured(provider)).toBe(false);
    expect(isProviderAccountUsageConfigured({ ...provider, source_provider_id: 2 })).toBe(false);
  });
});
