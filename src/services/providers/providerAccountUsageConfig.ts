import type { JsonValue } from "../../generated/bindings";
import type { ProviderExtensionValuesInput, ProviderSummary } from "./providers";

export const PROVIDER_ACCOUNT_USAGE_PLUGIN_ID = "core.provider-account-usage";
export const PROVIDER_ACCOUNT_USAGE_NAMESPACE = "accountUsage";
export const PROVIDER_ACCOUNT_USAGE_MIN_REFRESH_INTERVAL_SECONDS = 60;
export const PROVIDER_ACCOUNT_USAGE_MAX_REFRESH_INTERVAL_SECONDS = 300;
export const PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS = 300;
export const PROVIDER_ACCOUNT_USAGE_MIN_CUSTOM_TIMEOUT_SECONDS = 2;
export const PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_TIMEOUT_SECONDS = 15;
export const PROVIDER_ACCOUNT_USAGE_DEFAULT_CUSTOM_TIMEOUT_SECONDS = 10;
export const PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ALLOWED_ORIGINS = 16;
export const PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH = 512;
export const PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES = 32 * 1024;
// Kept for callers that imported the original limit name. The limit is bytes, not UTF-16 units.
export const PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_LENGTH =
  PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES;
export const PROVIDER_ACCOUNT_USAGE_CUSTOM_SCRIPT_TEMPLATE = [
  "({",
  "  request: (ctx) => ({",
  '    url: ctx.baseUrl + "/v1/usage",',
  '    method: "GET",',
  "    headers: {",
  "      Authorization: `Bearer ${ctx.apiKey}`,",
  "    },",
  "  }),",
  "  parse: (response) => ({",
  '    status: "available",',
  "    balance: response.data.balance,",
  "    used: response.data.used,",
  "    total: response.data.limit,",
  '    unit: "USD",',
  "  }),",
  "})",
].join("\n");

export type ProviderAccountUsageAdapterKind = "disabled" | "sub2api" | "newapi" | "custom";
export type ProviderAccountUsageNewApiQueryMode = "billing" | "account";

export type ProviderAccountUsageConfig = {
  adapterKind: ProviderAccountUsageAdapterKind;
  newApiQueryMode: ProviderAccountUsageNewApiQueryMode;
  timedRefreshEnabled: boolean;
  refreshIntervalSeconds: number;
  customScript: string;
  customAllowedOrigins: string[];
  customTimeoutSeconds: number;
  customEnabled: boolean;
};

const DEFAULT_CONFIG: ProviderAccountUsageConfig = {
  adapterKind: "disabled",
  newApiQueryMode: "billing",
  timedRefreshEnabled: true,
  refreshIntervalSeconds: PROVIDER_ACCOUNT_USAGE_DEFAULT_REFRESH_INTERVAL_SECONDS,
  customScript: "",
  customAllowedOrigins: [],
  customTimeoutSeconds: PROVIDER_ACCOUNT_USAGE_DEFAULT_CUSTOM_TIMEOUT_SECONDS,
  customEnabled: false,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function rowKey(pluginId: string, namespace: string) {
  return `${pluginId}\u0000${namespace}`;
}

function defaultProviderAccountUsageConfig(): ProviderAccountUsageConfig {
  return { ...DEFAULT_CONFIG, customAllowedOrigins: [] };
}

export function isProviderAccountUsageAdapterKind(
  value: unknown
): value is ProviderAccountUsageAdapterKind {
  return value === "disabled" || value === "sub2api" || value === "newapi" || value === "custom";
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

export function normalizeProviderAccountUsageCustomTimeoutSeconds(value: unknown): number {
  const numeric =
    typeof value === "number"
      ? value
      : typeof value === "string" && value.trim()
        ? Number(value)
        : PROVIDER_ACCOUNT_USAGE_DEFAULT_CUSTOM_TIMEOUT_SECONDS;
  if (!Number.isFinite(numeric)) return PROVIDER_ACCOUNT_USAGE_DEFAULT_CUSTOM_TIMEOUT_SECONDS;
  return Math.min(
    PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_TIMEOUT_SECONDS,
    Math.max(PROVIDER_ACCOUNT_USAGE_MIN_CUSTOM_TIMEOUT_SECONDS, Math.round(numeric))
  );
}

function utf8CodePointByteLength(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

function getUtf8ByteLength(value: string): number {
  let byteLength = 0;
  for (const character of value) {
    byteLength += utf8CodePointByteLength(character.codePointAt(0) ?? 0);
  }
  return byteLength;
}

// The backend hashes length-prefixed UTF-8 segments; config reads need the same synchronous digest.
const SHA256_INITIAL_STATE = new Uint32Array([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

const SHA256_ROUND_CONSTANTS = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function rotateRight(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
}

function sha256Hex(input: Uint8Array): string {
  const paddedLength = Math.ceil((input.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(input);
  padded[input.length] = 0x80;

  const bitLength = input.length * 8;
  const paddedView = new DataView(padded.buffer);
  paddedView.setUint32(paddedLength - 8, Math.floor(bitLength / 0x1_0000_0000), false);
  paddedView.setUint32(paddedLength - 4, bitLength >>> 0, false);

  const state = new Uint32Array(SHA256_INITIAL_STATE);
  const words = new Uint32Array(64);
  for (let offset = 0; offset < paddedLength; offset += 64) {
    for (let index = 0; index < 16; index += 1) {
      words[index] = paddedView.getUint32(offset + index * 4, false);
    }
    for (let index = 16; index < words.length; index += 1) {
      const left = words[index - 15];
      const right = words[index - 2];
      const sigma0 = rotateRight(left, 7) ^ rotateRight(left, 18) ^ (left >>> 3);
      const sigma1 = rotateRight(right, 17) ^ rotateRight(right, 19) ^ (right >>> 10);
      words[index] = (words[index - 16] + sigma0 + words[index - 7] + sigma1) >>> 0;
    }

    let a = state[0];
    let b = state[1];
    let c = state[2];
    let d = state[3];
    let e = state[4];
    let f = state[5];
    let g = state[6];
    let h = state[7];

    for (let index = 0; index < words.length; index += 1) {
      const sum1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const choice = (e & f) ^ (~e & g);
      const temp1 = (h + sum1 + choice + SHA256_ROUND_CONSTANTS[index] + words[index]) >>> 0;
      const sum0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (sum0 + majority) >>> 0;

      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }

    state[0] = (state[0] + a) >>> 0;
    state[1] = (state[1] + b) >>> 0;
    state[2] = (state[2] + c) >>> 0;
    state[3] = (state[3] + d) >>> 0;
    state[4] = (state[4] + e) >>> 0;
    state[5] = (state[5] + f) >>> 0;
    state[6] = (state[6] + g) >>> 0;
    state[7] = (state[7] + h) >>> 0;
  }

  return [...state].map((value) => value.toString(16).padStart(8, "0")).join("");
}

function customPermissionFingerprint(script: string, normalizedOrigins: string[]): string {
  const encoder = new TextEncoder();
  const segments = [script, ...normalizedOrigins].map((value) => encoder.encode(value));
  const payloadLength = segments.reduce((total, segment) => total + 8 + segment.length, 0);
  const payload = new Uint8Array(payloadLength);
  const view = new DataView(payload.buffer);
  let offset = 0;

  for (const segment of segments) {
    view.setUint32(offset, 0, false);
    view.setUint32(offset + 4, segment.length, false);
    payload.set(segment, offset + 8);
    offset += 8 + segment.length;
  }

  return sha256Hex(payload);
}

export function getProviderAccountUsageCustomScriptUtf8ByteLength(value: string): number {
  return getUtf8ByteLength(value);
}

export function truncateProviderAccountUsageCustomScriptUtf8(value: string): string {
  let byteLength = 0;
  let end = 0;

  for (const character of value) {
    const nextByteLength = byteLength + utf8CodePointByteLength(character.codePointAt(0) ?? 0);
    if (nextByteLength > PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_SCRIPT_BYTES) break;
    byteLength = nextByteLength;
    end += character.length;
  }

  return end === value.length ? value : value.slice(0, end);
}

function normalizeProviderAccountUsageCustomAllowedOrigin(value: string): string | null {
  if (!value || getUtf8ByteLength(value) > PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH) {
    return null;
  }
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (trimmed.includes("?") || trimmed.includes("#")) return null;

  try {
    const url = new URL(trimmed);
    if (
      url.protocol !== "https:" ||
      !url.hostname ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      (url.pathname !== "/" && url.pathname !== "")
    ) {
      return null;
    }
    return url.origin === "null" ? null : url.origin;
  } catch {
    return null;
  }
}

export type ProviderAccountUsageCustomAllowedOriginsValidation = {
  normalizedOrigins: string[];
  error: string | null;
};

export function validateProviderAccountUsageCustomAllowedOrigins(
  value: unknown
): ProviderAccountUsageCustomAllowedOriginsValidation {
  if (!Array.isArray(value)) return { normalizedOrigins: [], error: null };

  const origins = new Set<string>();
  let firstError: string | null = null;

  value.forEach((candidate, index) => {
    if (typeof candidate !== "string") {
      firstError ??= `第 ${index + 1} 行必须是 HTTPS Origin`;
      return;
    }

    if (!candidate) return;
    if (getUtf8ByteLength(candidate) > PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH) {
      firstError ??= `第 ${index + 1} 行 Origin 超过 ${PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ORIGIN_LENGTH} 字节`;
      return;
    }
    const trimmed = candidate.trim();
    if (!trimmed) return;

    const normalized = normalizeProviderAccountUsageCustomAllowedOrigin(trimmed);
    if (!normalized) {
      firstError ??= `第 ${index + 1} 行必须是仅含协议、主机和端口的 HTTPS Origin`;
      return;
    }
    origins.add(normalized);
  });

  const normalizedOrigins = [...origins].sort();
  if (!firstError && normalizedOrigins.length > PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ALLOWED_ORIGINS) {
    firstError = `规范化去重后最多允许 ${PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_ALLOWED_ORIGINS} 个 HTTPS Origin（当前 ${normalizedOrigins.length} 个）`;
  }

  return { normalizedOrigins, error: firstError };
}

/**
 * Mirrors the backend's origin comparison so cosmetic input changes do not
 * revoke a user's local acknowledgement of the same host permissions.
 */
export function normalizeProviderAccountUsageCustomAllowedOrigins(value: unknown): string[] {
  if (!Array.isArray(value)) return [];

  const origins = new Set<string>();
  for (const candidate of value) {
    if (typeof candidate !== "string") continue;
    const normalized = normalizeProviderAccountUsageCustomAllowedOrigin(candidate);
    if (normalized) origins.add(normalized);
  }
  return [...origins].sort();
}

export function hasProviderAccountUsageCustomPermissionChange(
  previous: Pick<ProviderAccountUsageConfig, "customScript" | "customAllowedOrigins">,
  next: Pick<ProviderAccountUsageConfig, "customScript" | "customAllowedOrigins">
): boolean {
  if (previous.customScript !== next.customScript) return true;

  const previousValidation = validateProviderAccountUsageCustomAllowedOrigins(
    previous.customAllowedOrigins
  );
  const nextValidation = validateProviderAccountUsageCustomAllowedOrigins(
    next.customAllowedOrigins
  );
  if (previousValidation.error || nextValidation.error) {
    const comparableRows = (origins: string[]) =>
      origins.map((origin) => origin.trim()).filter(Boolean);
    return (
      JSON.stringify(comparableRows(previous.customAllowedOrigins)) !==
      JSON.stringify(comparableRows(next.customAllowedOrigins))
    );
  }

  const previousOrigins = previousValidation.normalizedOrigins;
  const nextOrigins = nextValidation.normalizedOrigins;
  return (
    previousOrigins.length !== nextOrigins.length ||
    previousOrigins.some((origin, index) => origin !== nextOrigins[index])
  );
}

function readCustomAllowedOrigins(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((origin): origin is string => typeof origin === "string");
}

function normalizeProviderAccountUsageBaseOrigin(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (trimmed.includes("?") || trimmed.includes("#")) return null;
  try {
    const url = new URL(trimmed);
    if (
      url.protocol !== "https:" ||
      !url.hostname ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.origin === "null"
    ) {
      return null;
    }
    return url.origin;
  } catch {
    return null;
  }
}

export function prepareProviderAccountUsageCustomAllowedOrigins(origins: string[]): string[] {
  const validation = validateProviderAccountUsageCustomAllowedOrigins(origins);
  if (!validation.error) return validation.normalizedOrigins;

  // Preserve invalid rows so the backend rejects them instead of silently broadening trust.
  return origins.map((origin) => origin.trim()).filter(Boolean);
}

export function readProviderAccountUsageConfig(
  provider: Partial<Pick<ProviderSummary, "extension_values" | "base_urls">> | null | undefined
): ProviderAccountUsageConfig {
  const row = provider?.extension_values?.find(
    (value) =>
      value.pluginId === PROVIDER_ACCOUNT_USAGE_PLUGIN_ID &&
      value.namespace === PROVIDER_ACCOUNT_USAGE_NAMESPACE
  );
  if (!row || !isRecord(row.values)) return defaultProviderAccountUsageConfig();

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
  const rawCustomScript =
    typeof row.values.customScript === "string" ? row.values.customScript : "";
  const customScript = truncateProviderAccountUsageCustomScriptUtf8(rawCustomScript);
  const rawCustomAllowedOrigins = row.values.customAllowedOrigins;
  const customAllowedOriginsShapeValid =
    rawCustomAllowedOrigins == null ||
    (Array.isArray(rawCustomAllowedOrigins) &&
      rawCustomAllowedOrigins.every((origin) => typeof origin === "string"));
  const customAllowedOrigins = readCustomAllowedOrigins(rawCustomAllowedOrigins);
  const customAllowedOriginsValidation =
    validateProviderAccountUsageCustomAllowedOrigins(customAllowedOrigins);
  const customAllowedOriginsPersistedValid =
    customAllowedOriginsShapeValid &&
    customAllowedOrigins.every((origin) => Boolean(origin.trim())) &&
    !customAllowedOriginsValidation.error;
  const rawCustomTimeoutSeconds = row.values.customTimeoutSeconds;
  const customTimeoutSeconds =
    normalizeProviderAccountUsageCustomTimeoutSeconds(rawCustomTimeoutSeconds);
  const customTimeoutPersistedValid =
    typeof rawCustomTimeoutSeconds === "number" &&
    Number.isInteger(rawCustomTimeoutSeconds) &&
    rawCustomTimeoutSeconds >= PROVIDER_ACCOUNT_USAGE_MIN_CUSTOM_TIMEOUT_SECONDS &&
    rawCustomTimeoutSeconds <= PROVIDER_ACCOUNT_USAGE_MAX_CUSTOM_TIMEOUT_SECONDS;
  const expectedPermissionFingerprint = customPermissionFingerprint(
    customScript,
    customAllowedOriginsValidation.normalizedOrigins
  );
  const permissionFingerprintMatches =
    row.values.customPermissionFingerprint === expectedPermissionFingerprint;
  const permissionBaseOrigin = normalizeProviderAccountUsageCustomAllowedOrigin(
    typeof row.values.customPermissionBaseOrigin === "string"
      ? row.values.customPermissionBaseOrigin
      : ""
  );
  const primaryBaseUrl = provider?.base_urls?.find((baseUrl) => Boolean(baseUrl.trim()));
  const currentBaseOrigin = normalizeProviderAccountUsageBaseOrigin(primaryBaseUrl);
  const customEnabled =
    adapterKind === "custom" &&
    row.values.customEnabled === true &&
    rawCustomScript === customScript &&
    customAllowedOriginsPersistedValid &&
    customTimeoutPersistedValid &&
    permissionFingerprintMatches &&
    permissionBaseOrigin !== null &&
    permissionBaseOrigin === currentBaseOrigin &&
    Boolean(customScript.trim());

  return {
    adapterKind,
    newApiQueryMode,
    timedRefreshEnabled,
    refreshIntervalSeconds,
    customScript,
    customAllowedOrigins,
    customTimeoutSeconds,
    customEnabled,
  };
}

export function isProviderAccountUsageAccountCredentialsRequired(
  provider: Pick<
    ProviderSummary,
    | "base_urls"
    | "extension_values"
    | "newapi_account_user_id"
    | "newapi_account_access_token_configured"
  >
): boolean {
  const config = readProviderAccountUsageConfig(provider);
  return (
    config.adapterKind === "newapi" &&
    config.newApiQueryMode === "account" &&
    (!provider.newapi_account_user_id || !provider.newapi_account_access_token_configured)
  );
}

export function isProviderAccountUsageConfigured(
  provider: Pick<
    ProviderSummary,
    "auth_mode" | "source_provider_id" | "base_urls" | "extension_values"
  >
): boolean {
  if (provider.auth_mode !== "api_key" || provider.source_provider_id != null) return false;
  const config = readProviderAccountUsageConfig(provider);
  return (
    config.adapterKind === "sub2api" ||
    config.adapterKind === "newapi" ||
    (config.adapterKind === "custom" && config.customEnabled && Boolean(config.customScript.trim()))
  );
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

  const values: Record<string, JsonValue> = {
    adapterKind: config.adapterKind,
    newApiQueryMode: config.newApiQueryMode,
    timedRefreshEnabled: config.timedRefreshEnabled,
    refreshIntervalSeconds: normalizeProviderAccountUsageRefreshIntervalSeconds(
      config.refreshIntervalSeconds
    ),
  };
  if (config.adapterKind === "custom") {
    const customScript = truncateProviderAccountUsageCustomScriptUtf8(config.customScript);
    const customAllowedOriginsValidation = validateProviderAccountUsageCustomAllowedOrigins(
      config.customAllowedOrigins
    );
    values.customScript = customScript;
    values.customAllowedOrigins = prepareProviderAccountUsageCustomAllowedOrigins(
      config.customAllowedOrigins
    );
    values.customTimeoutSeconds = normalizeProviderAccountUsageCustomTimeoutSeconds(
      config.customTimeoutSeconds
    );
    values.customEnabled =
      config.customEnabled &&
      customScript === config.customScript &&
      !customAllowedOriginsValidation.error &&
      Boolean(customScript.trim());
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
