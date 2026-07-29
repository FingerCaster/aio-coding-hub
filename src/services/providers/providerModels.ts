import {
  commands,
  type ProviderModelCatalog as GeneratedProviderModelCatalog,
  type ProviderModelReasoningEffort as GeneratedProviderModelReasoningEffort,
} from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import type { ProviderSummary } from "./providers";
import { validateProviderId } from "./providers";
import { isCanonicalUuidV4 } from "./uuid";

const MODEL_ID_MAX_BYTES = 256;
export const MODEL_CONTEXT_WINDOW_MIN_TOKENS = 1_024;
export const MODEL_CONTEXT_WINDOW_MAX_TOKENS = 10_000_000;
export const PROVIDER_MODEL_REASONING_EFFORTS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
] as const satisfies readonly GeneratedProviderModelReasoningEffort[];
export type ProviderModelReasoningEffort = (typeof PROVIDER_MODEL_REASONING_EFFORTS)[number];
const PROVIDER_MODEL_REASONING_EFFORT_SET = new Set<string>(PROVIDER_MODEL_REASONING_EFFORTS);
const PROVIDER_MODEL_DISCOVERY_ERROR_CODES = [
  "unauthorized",
  "forbidden",
  "not_supported",
  "timeout",
  "network",
  "invalid_response",
  "empty",
  "limit",
] as const;
export type ProviderModelDiscoveryErrorCode = (typeof PROVIDER_MODEL_DISCOVERY_ERROR_CODES)[number];
const PROVIDER_MODEL_DISCOVERY_ERROR_CODE_SET = new Set<string>(
  PROVIDER_MODEL_DISCOVERY_ERROR_CODES
);
const DISCOVERY_ERROR_LABELS: Record<ProviderModelDiscoveryErrorCode, string> = {
  unauthorized: "认证失败，请检查该供应商的凭据",
  forbidden: "模型接口拒绝访问",
  not_supported: "该供应商未开放 OpenAI 兼容模型接口",
  timeout: "模型接口请求超时",
  network: "模型接口网络请求失败",
  invalid_response: "模型接口返回了无效数据",
  empty: "模型接口返回了空列表",
  limit: "模型列表超过安全限制",
};

const PROVIDER_MODEL_FEATURE_ERROR_LABELS: Readonly<Record<string, string>> = {
  AUTH_RELOGIN_REQUIRED: "需要重新登录后再试",
  OAUTH_REFRESH_FAILED: "凭据刷新失败，请重新登录后再试",
  SEC_INVALID_INPUT: "输入内容不符合要求",
  PROVIDER_MODELS_PROVIDER_IDENTITY_CHANGED: "供应商已变化，请重新打开后重试",
  PROVIDER_MODELS_UNSUPPORTED_PROVIDER: "该供应商不支持模型目录",
  PROVIDER_MODELS_INVALID_PROVIDER: "供应商配置无效",
  PROVIDER_MODELS_CONNECTION_CHANGED: "供应商连接信息已变化，请重新获取模型",
  PROVIDER_MODEL_MANAGED_PROFILE_REFERENCED: "该模型仍被 Codex Profile 使用",
  PROVIDER_MODEL_CAPABILITIES_REQUIRED: "请先配置该模型的能力",
  CODEX_MANAGED_PROFILE_NAME_EXISTS: "Profile 名称已存在",
  CODEX_MANAGED_PROFILE_FILE_EXISTS: "已存在同名 Profile 文件，未覆盖",
  CODEX_MANAGED_PROFILE_HOME_UNSAFE: "Codex 配置目录不可安全访问",
  CODEX_MANAGED_PROFILE_RECOVERY_REQUIRED: "Profile 状态需要恢复，请刷新后重试",
  CODEX_MANAGED_PROFILE_WRITE_FAILED: "Profile 文件写入失败",
  CODEX_MANAGED_MODEL_PROXY_DISABLED: "请先启用 Codex CLI 代理",
  CODEX_MANAGED_MODEL_PROXY_DRIFT: "Codex 代理配置尚未应用，请先同步代理",
  CODEX_MANAGED_MODEL_CONFIG_DRIFT: "Codex 配置已被外部修改，请检查后重试",
  CODEX_MANAGED_MODEL_CATALOG_MODIFIED: "AIO 模型目录已被外部修改，请检查后重试",
  CODEX_MANAGED_MODEL_CLI_NOT_FOUND: "未找到 Codex CLI",
  CODEX_MANAGED_MODEL_BUNDLED_TIMEOUT: "读取 Codex 内置模型目录超时",
  CODEX_MANAGED_MODEL_BUNDLED_UNAVAILABLE: "无法读取 Codex 内置模型目录",
  CODEX_MANAGED_MODEL_BUNDLED_INVALID: "Codex 内置模型目录无效",
  CODEX_MANAGED_MODEL_BASE_CATALOG_UNAVAILABLE: "用户配置的 Codex 模型目录不可读取",
  CODEX_MANAGED_MODEL_BASE_CATALOG_INVALID: "用户配置的 Codex 模型目录无效",
  CODEX_MANAGED_MODEL_ALIAS_CONFLICT: "Codex 模型目录中已存在同名 aio/ 条目",
  CODEX_MANAGED_MODEL_CATALOG_LIMIT: "合并后的 Codex 模型目录超过安全限制",
  CODEX_MANAGED_MODEL_CATALOG_WRITE_FAILED: "Codex 模型目录写入失败",
  CODEX_MANAGED_MODEL_CONFIG_WRITE_FAILED: "Codex 配置写入失败",
  CODEX_MANAGED_MODEL_RECOVERY_REQUIRED: "Codex 模型目录状态需要人工恢复",
  FS_WRITE_FAILED: "Profile 文件写入失败",
  FS_DELETE_FAILED: "Profile 文件删除失败",
  DB_NOT_FOUND: "目标已不存在，请刷新后重试",
};

export type ProviderModelSource = "discovered" | "manual";

export type ProviderModel = {
  modelUuid: string;
  providerId: number;
  remoteModelId: string;
  source: ProviderModelSource;
  stale: boolean;
  lastSeenAt: number | null;
  createdAt: number;
  updatedAt: number;
  capabilitiesConfigured: boolean;
  supportedReasoningEfforts: ProviderModelReasoningEffort[];
  defaultReasoningEffort: ProviderModelReasoningEffort | null;
  contextWindow: number | null;
};

export type ProviderModelCapabilitiesInput = {
  supportedReasoningEfforts: ProviderModelReasoningEffort[];
  defaultReasoningEffort: ProviderModelReasoningEffort | null;
  contextWindow: number | null;
};

export type ProviderModelCatalog = {
  providerId: number;
  providerUuid: string;
  protocol: "openai_compatible";
  stale: boolean;
  lastAttemptAt: number | null;
  lastSuccessAt: number | null;
  lastErrorCode: ProviderModelDiscoveryErrorCode | null;
  models: ProviderModel[];
};

export function formatProviderModelDiscoveryError(errorCode: string | null | undefined) {
  if (!errorCode) return null;
  return PROVIDER_MODEL_DISCOVERY_ERROR_CODE_SET.has(errorCode)
    ? DISCOVERY_ERROR_LABELS[errorCode as ProviderModelDiscoveryErrorCode]
    : `模型获取失败（${errorCode}）`;
}

function extractProviderModelFeatureErrorCode(error: unknown): string | null {
  if (error && typeof error === "object") {
    const code = (error as { code?: unknown }).code;
    if (typeof code === "string" && /^[A-Z][A-Z0-9_]*$/.test(code)) {
      return code;
    }
  }

  const message = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const normalized = message.trim().replace(/^Error:\s*/i, "");
  return /^([A-Z][A-Z0-9_]*)(?::|\s|$)/.exec(normalized)?.[1] ?? null;
}

/**
 * Keeps model-catalog and managed-profile errors from exposing raw Tauri,
 * filesystem, URL, or upstream-response details in the UI.
 */
export function formatProviderModelFeatureError(error: unknown, fallback = "请稍后重试"): string {
  const code = extractProviderModelFeatureErrorCode(error);
  return (code && PROVIDER_MODEL_FEATURE_ERROR_LABELS[code]) || fallback;
}

function requireCanonicalUuid(value: string, label: string): string {
  if (!isCanonicalUuidV4(value)) {
    throw new Error(`IPC_INVALID_UUID: ${label}`);
  }
  return value;
}

function requireBoolean(value: boolean, label: string): boolean {
  if (typeof value !== "boolean") {
    throw new Error(`IPC_INVALID_BOOLEAN: ${label}`);
  }
  return value;
}

function requireDiscoveryErrorCode(value: string | null): ProviderModelDiscoveryErrorCode | null {
  if (value == null || value.trim() === "") return null;
  const normalized = value.trim();
  if (!PROVIDER_MODEL_DISCOVERY_ERROR_CODE_SET.has(normalized)) {
    throw new Error(`IPC_INVALID_LITERAL: catalog.lastErrorCode=${normalized}`);
  }
  return normalized as ProviderModelDiscoveryErrorCode;
}

function requireTimestamp(value: number | null, label: string): number | null {
  if (value == null) return null;
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`IPC_INVALID_TIMESTAMP: ${label}`);
  }
  return value;
}

function requireRequiredTimestamp(value: number, label: string): number {
  const timestamp = requireTimestamp(value, label);
  if (timestamp == null) throw new Error(`IPC_INVALID_TIMESTAMP: ${label}`);
  return timestamp;
}

function requireReasoningEffort(value: unknown, label: string): ProviderModelReasoningEffort {
  if (typeof value !== "string" || !PROVIDER_MODEL_REASONING_EFFORT_SET.has(value)) {
    throw new Error(`IPC_INVALID_LITERAL: ${label}=${String(value)}`);
  }
  return value as ProviderModelReasoningEffort;
}

export function normalizeProviderModelCapabilities(
  input: ProviderModelCapabilitiesInput
): ProviderModelCapabilitiesInput {
  const supported = input.supportedReasoningEfforts.map((effort, index) =>
    requireReasoningEffort(effort, `supportedReasoningEfforts[${index}]`)
  );
  const unique = new Set(supported);
  if (unique.size !== supported.length) {
    throw new Error("SEC_INVALID_INPUT: duplicate supportedReasoningEfforts");
  }
  const normalizedSupported = PROVIDER_MODEL_REASONING_EFFORTS.filter((effort) =>
    unique.has(effort)
  );
  const defaultReasoningEffort =
    input.defaultReasoningEffort == null
      ? null
      : requireReasoningEffort(input.defaultReasoningEffort, "defaultReasoningEffort");
  if (normalizedSupported.length === 0 && defaultReasoningEffort != null) {
    throw new Error("SEC_INVALID_INPUT: defaultReasoningEffort requires supported efforts");
  }
  if (normalizedSupported.length > 0 && defaultReasoningEffort == null) {
    throw new Error("SEC_INVALID_INPUT: defaultReasoningEffort is required");
  }
  if (defaultReasoningEffort != null && !unique.has(defaultReasoningEffort)) {
    throw new Error("SEC_INVALID_INPUT: defaultReasoningEffort is not supported");
  }
  const contextWindow = input.contextWindow;
  if (
    contextWindow != null &&
    (!Number.isSafeInteger(contextWindow) ||
      contextWindow < MODEL_CONTEXT_WINDOW_MIN_TOKENS ||
      contextWindow > MODEL_CONTEXT_WINDOW_MAX_TOKENS)
  ) {
    throw new Error("SEC_INVALID_INPUT: invalid contextWindow");
  }
  return {
    supportedReasoningEfforts: normalizedSupported,
    defaultReasoningEffort,
    contextWindow,
  };
}

export function normalizeRemoteModelId(value: string): string {
  const modelId = value.trim();
  if (
    !modelId ||
    new TextEncoder().encode(modelId).byteLength > MODEL_ID_MAX_BYTES ||
    /[\u0000-\u001f\u007f]/.test(modelId)
  ) {
    throw new Error("SEC_INVALID_INPUT: invalid remoteModelId");
  }
  return modelId;
}

export function validateModelUuid(value: string, label = "modelUuid"): string {
  return requireCanonicalUuid(value, label);
}

export function validateProviderUuid(value: string, label = "providerUuid"): string {
  return requireCanonicalUuid(value, label);
}

function decodeProviderModel(
  value: GeneratedProviderModelCatalog["models"][number],
  expectedProviderId: number
): ProviderModel {
  const providerId = validateProviderId(value.providerId, "models.providerId");
  if (providerId !== expectedProviderId) {
    throw new Error("IPC_PROVIDER_MODEL_SCOPE_MISMATCH");
  }
  if (value.source !== "discovered" && value.source !== "manual") {
    throw new Error(`IPC_INVALID_LITERAL: models.source=${String(value.source)}`);
  }

  const capabilitiesConfigured = requireBoolean(
    value.capabilitiesConfigured,
    "models.capabilitiesConfigured"
  );
  const capabilities = normalizeProviderModelCapabilities({
    supportedReasoningEfforts: value.supportedReasoningEfforts.map((effort, index) =>
      requireReasoningEffort(effort, `models.supportedReasoningEfforts[${index}]`)
    ),
    defaultReasoningEffort:
      value.defaultReasoningEffort == null
        ? null
        : requireReasoningEffort(value.defaultReasoningEffort, "models.defaultReasoningEffort"),
    contextWindow: value.contextWindow,
  });
  if (
    !capabilitiesConfigured &&
    (capabilities.supportedReasoningEfforts.length > 0 ||
      capabilities.defaultReasoningEffort != null ||
      capabilities.contextWindow != null)
  ) {
    throw new Error("IPC_INVALID_MODEL_CAPABILITIES: unconfigured model has capability values");
  }

  return {
    modelUuid: requireCanonicalUuid(value.modelUuid, "models.modelUuid"),
    providerId,
    remoteModelId: normalizeRemoteModelId(value.remoteModelId),
    source: value.source,
    stale: requireBoolean(value.stale, "models.stale"),
    lastSeenAt: requireTimestamp(value.lastSeenAt, "models.lastSeenAt"),
    createdAt: requireRequiredTimestamp(value.createdAt, "models.createdAt"),
    updatedAt: requireRequiredTimestamp(value.updatedAt, "models.updatedAt"),
    capabilitiesConfigured,
    ...capabilities,
  };
}

function decodeExpectedProviderModelCatalog(
  value: GeneratedProviderModelCatalog,
  expectedProviderId: number,
  expectedProviderUuid: string
) {
  const catalog = decodeProviderModelCatalog(value);
  if (catalog.providerId !== expectedProviderId || catalog.providerUuid !== expectedProviderUuid) {
    throw new Error("IPC_PROVIDER_MODEL_SCOPE_MISMATCH");
  }
  return catalog;
}

export function decodeProviderModelCatalog(
  value: GeneratedProviderModelCatalog
): ProviderModelCatalog {
  const providerId = validateProviderId(value.providerId, "catalog.providerId");
  if (value.protocol !== "openai_compatible") {
    throw new Error(`IPC_INVALID_LITERAL: catalog.protocol=${String(value.protocol)}`);
  }

  return {
    providerId,
    providerUuid: requireCanonicalUuid(value.providerUuid, "catalog.providerUuid"),
    protocol: "openai_compatible",
    stale: requireBoolean(value.stale, "catalog.stale"),
    lastAttemptAt: requireTimestamp(value.lastAttemptAt, "catalog.lastAttemptAt"),
    lastSuccessAt: requireTimestamp(value.lastSuccessAt, "catalog.lastSuccessAt"),
    lastErrorCode: requireDiscoveryErrorCode(value.lastErrorCode),
    models: value.models.map((model) => decodeProviderModel(model, providerId)),
  };
}

export function isCodexDirectProvider(
  provider: Pick<ProviderSummary, "cli_key" | "source_provider_id" | "bridge_type">
): boolean {
  return (
    provider.cli_key === "codex" &&
    provider.source_provider_id == null &&
    provider.bridge_type == null
  );
}

export async function providerModelsGet(
  providerId: number,
  providerUuid: string
): Promise<ProviderModelCatalog> {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  return invokeGeneratedIpc<ProviderModelCatalog>({
    title: "读取供应商模型失败",
    cmd: "provider_models_get",
    args: { providerId: normalizedProviderId, providerUuid: normalizedProviderUuid },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerModelsGet(normalizedProviderId, normalizedProviderUuid),
        (value) =>
          decodeExpectedProviderModelCatalog(value, normalizedProviderId, normalizedProviderUuid)
      ),
  });
}

export async function providerModelsRefresh(
  providerId: number,
  providerUuid: string
): Promise<ProviderModelCatalog> {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  return invokeGeneratedIpc<ProviderModelCatalog>({
    title: "获取供应商模型失败",
    cmd: "provider_models_refresh",
    args: { providerId: normalizedProviderId, providerUuid: normalizedProviderUuid },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerModelsRefresh(normalizedProviderId, normalizedProviderUuid),
        (value) =>
          decodeExpectedProviderModelCatalog(value, normalizedProviderId, normalizedProviderUuid)
      ),
  });
}

export async function providerModelManualUpsert(
  providerId: number,
  providerUuid: string,
  remoteModelId: string
): Promise<ProviderModelCatalog> {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  const normalizedModelId = normalizeRemoteModelId(remoteModelId);
  return invokeGeneratedIpc<ProviderModelCatalog>({
    title: "添加手工模型失败",
    cmd: "provider_model_manual_upsert",
    args: {
      providerId: normalizedProviderId,
      providerUuid: normalizedProviderUuid,
      remoteModelId: normalizedModelId,
    },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerModelManualUpsert(
          normalizedProviderId,
          normalizedProviderUuid,
          normalizedModelId
        ),
        (value) =>
          decodeExpectedProviderModelCatalog(value, normalizedProviderId, normalizedProviderUuid)
      ),
  });
}

export async function providerModelManualDelete(
  providerId: number,
  providerUuid: string,
  modelUuid: string
): Promise<ProviderModelCatalog> {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  const normalizedModelUuid = validateModelUuid(modelUuid);
  return invokeGeneratedIpc<ProviderModelCatalog>({
    title: "删除手工模型失败",
    cmd: "provider_model_manual_delete",
    args: {
      providerId: normalizedProviderId,
      providerUuid: normalizedProviderUuid,
      modelUuid: normalizedModelUuid,
    },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerModelManualDelete(
          normalizedProviderId,
          normalizedProviderUuid,
          normalizedModelUuid
        ),
        (value) =>
          decodeExpectedProviderModelCatalog(value, normalizedProviderId, normalizedProviderUuid)
      ),
  });
}

export async function providerModelCapabilitiesUpdate(
  providerId: number,
  providerUuid: string,
  modelUuid: string,
  capabilities: ProviderModelCapabilitiesInput
): Promise<ProviderModelCatalog> {
  const normalizedProviderId = validateProviderId(providerId);
  const normalizedProviderUuid = validateProviderUuid(providerUuid);
  const normalizedModelUuid = validateModelUuid(modelUuid);
  const normalizedCapabilities = normalizeProviderModelCapabilities(capabilities);
  return invokeGeneratedIpc<ProviderModelCatalog>({
    title: "保存模型能力失败",
    cmd: "provider_model_capabilities_update",
    args: {
      providerId: normalizedProviderId,
      providerUuid: normalizedProviderUuid,
      modelUuid: normalizedModelUuid,
      capabilities: normalizedCapabilities,
    },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerModelCapabilitiesUpdate(
          normalizedProviderId,
          normalizedProviderUuid,
          normalizedModelUuid,
          normalizedCapabilities
        ),
        (value) =>
          decodeExpectedProviderModelCatalog(value, normalizedProviderId, normalizedProviderUuid)
      ),
  });
}
