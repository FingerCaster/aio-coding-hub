import {
  commands,
  type ProviderShareCredentialStatus,
  type ProviderShareExtensionCompatibility,
  type ProviderShareImportPreview as GeneratedProviderShareImportPreview,
} from "../../generated/bindings";
import {
  invokeGeneratedIpc,
  mapGeneratedCommandResponse,
  type GeneratedCommandResult,
} from "../generatedIpc";
import { createRiskyIpcConfirm } from "../ipcConfirm";
import {
  toProviderSummary,
  validateProviderCliKey,
  validateProviderId,
  type CliKey,
  type ProviderAuthMode,
  type ProviderSummary,
} from "./providers";

const PROVIDER_SHARE_MAX_BYTES = 8 * 1024 * 1024;
const PREVIEW_TOKEN_PATTERN = /^[0-9a-f]{64}$/i;
const CREDENTIAL_STATUSES = new Set<ProviderShareCredentialStatus>([
  "configured",
  "needs_api_key",
  "not_required",
  "available",
  "refreshable",
  "needs_login",
]);
const EXTENSION_COMPATIBILITIES = new Set<ProviderShareExtensionCompatibility>([
  "compatible",
  "missing_plugin",
  "plugin_unavailable",
  "version_mismatch",
  "namespace_mismatch",
]);
const AUTH_MODES = new Set<ProviderAuthMode>(["api_key", "oauth"]);

export type ProviderShareImportPreview = Omit<GeneratedProviderShareImportPreview, "cliKey"> & {
  cliKey: CliKey;
};

function invalidPreview(message: string): never {
  throw new Error(`IPC_INVALID_RESPONSE: ${message}`);
}

function requireString(value: unknown, field: string, options?: { nonEmpty?: boolean }) {
  if (typeof value !== "string" || (options?.nonEmpty && !value.trim())) {
    invalidPreview(`provider share preview ${field} is invalid`);
  }
  return value as string;
}

export function validateProviderSharePreviewToken(previewToken: string) {
  const normalized = previewToken.trim();
  if (!PREVIEW_TOKEN_PATTERN.test(normalized)) {
    throw new Error("SEC_INVALID_INPUT: invalid provider share preview token");
  }
  return normalized;
}

export function decodeProviderShareImportPreview(
  value: GeneratedProviderShareImportPreview
): ProviderShareImportPreview {
  if (!value || typeof value !== "object") {
    invalidPreview("provider share preview is invalid");
  }
  const previewToken = validateProviderSharePreviewToken(
    requireString(value.previewToken, "previewToken", { nonEmpty: true })
  );
  const cliKey = validateProviderCliKey(requireString(value.cliKey, "cliKey", { nonEmpty: true }));
  const sourceName = requireString(value.sourceName, "sourceName", { nonEmpty: true });
  const finalName = requireString(value.finalName, "finalName", { nonEmpty: true });
  const authMode = value.authMode;
  if (!AUTH_MODES.has(authMode)) invalidPreview("provider share preview authMode is invalid");
  if (!CREDENTIAL_STATUSES.has(value.credentialStatus)) {
    invalidPreview("provider share preview credentialStatus is invalid");
  }
  if (typeof value.sourceEnabled !== "boolean" || value.importEnabled !== false) {
    invalidPreview("provider share preview enabled state is invalid");
  }
  if (typeof value.canImport !== "boolean" || !Array.isArray(value.extensions)) {
    invalidPreview("provider share preview compatibility is invalid");
  }
  if (
    !Number.isSafeInteger(value.extensionCount) ||
    value.extensionCount < 0 ||
    value.extensionCount !== value.extensions.length
  ) {
    invalidPreview("provider share preview extensionCount is invalid");
  }

  const extensions = value.extensions.map((extension) => {
    const pluginId = requireString(extension?.pluginId, "extension.pluginId", { nonEmpty: true });
    const namespace = requireString(extension?.namespace, "extension.namespace", {
      nonEmpty: true,
    });
    const requiredVersion = requireString(extension?.requiredVersion, "extension.requiredVersion", {
      nonEmpty: true,
    });
    const installedVersion = extension?.installedVersion;
    if (installedVersion !== null && typeof installedVersion !== "string") {
      invalidPreview("provider share preview extension.installedVersion is invalid");
    }
    if (!EXTENSION_COMPATIBILITIES.has(extension?.compatibility)) {
      invalidPreview("provider share preview extension.compatibility is invalid");
    }
    return {
      pluginId,
      namespace,
      requiredVersion,
      installedVersion,
      compatibility: extension.compatibility,
    };
  });
  const compatibilityAllowsImport = extensions.every(
    (extension) => extension.compatibility === "compatible"
  );
  if (value.canImport !== compatibilityAllowsImport) {
    invalidPreview("provider share preview canImport is inconsistent");
  }

  return {
    previewToken,
    cliKey,
    sourceName,
    finalName,
    sourceEnabled: value.sourceEnabled,
    importEnabled: false,
    authMode,
    credentialStatus: value.credentialStatus,
    extensionCount: value.extensionCount,
    extensions,
    canImport: value.canImport,
  };
}

export async function providerShareCopyToClipboard(providerId: number) {
  const normalizedProviderId = validateProviderId(providerId);
  const confirm = createRiskyIpcConfirm(
    "provider_share_copy_to_clipboard",
    `provider:${normalizedProviderId}:share`
  );
  return invokeGeneratedIpc<boolean>({
    title: "复制供应商分享失败",
    cmd: "provider_share_copy_to_clipboard",
    args: { providerId: normalizedProviderId },
    invoke: () =>
      commands.providerShareCopyToClipboard(normalizedProviderId, confirm) as Promise<
        GeneratedCommandResult<boolean>
      >,
  });
}

export async function providerShareSaveToFile(providerId: number) {
  const normalizedProviderId = validateProviderId(providerId);
  const confirm = createRiskyIpcConfirm(
    "provider_share_save_to_file",
    `provider:${normalizedProviderId}:share`
  );
  return invokeGeneratedIpc<boolean>({
    title: "保存供应商分享失败",
    cmd: "provider_share_save_to_file",
    args: { providerId: normalizedProviderId },
    invoke: () =>
      commands.providerShareSaveToFile(normalizedProviderId, confirm) as Promise<
        GeneratedCommandResult<boolean>
      >,
  });
}

export async function providerShareImportPreviewFromFile() {
  return invokeGeneratedIpc<ProviderShareImportPreview, null>({
    title: "读取供应商分享文件失败",
    cmd: "provider_share_import_preview_from_file",
    fallback: null,
    nullResultBehavior: "return_fallback",
    invoke: async () =>
      mapGeneratedCommandResponse(
        (await commands.providerShareImportPreviewFromFile()) as GeneratedCommandResult<GeneratedProviderShareImportPreview>,
        decodeProviderShareImportPreview
      ),
  });
}

export async function providerShareImportPreviewFromContent(content: string) {
  const byteLength = new TextEncoder().encode(content).byteLength;
  if (byteLength === 0 || byteLength > PROVIDER_SHARE_MAX_BYTES) {
    throw new Error(
      `SEC_INVALID_INPUT: provider share content must be between 1 and ${PROVIDER_SHARE_MAX_BYTES} encoded bytes`
    );
  }
  return invokeGeneratedIpc<ProviderShareImportPreview>({
    title: "校验供应商分享内容失败",
    cmd: "provider_share_import_preview_from_content",
    args: { byteLength, content: "[REDACTED]" },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerShareImportPreviewFromContent(content),
        decodeProviderShareImportPreview
      ),
  });
}

export async function providerShareImportConfirm(previewToken: string): Promise<ProviderSummary> {
  const normalizedToken = validateProviderSharePreviewToken(previewToken);
  const confirm = createRiskyIpcConfirm(
    "provider_share_import_confirm",
    `provider-share-preview:${normalizedToken}`
  );
  return invokeGeneratedIpc<ProviderSummary>({
    title: "导入供应商失败",
    cmd: "provider_share_import_confirm",
    args: { previewToken: "[REDACTED]" },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.providerShareImportConfirm(normalizedToken, confirm),
        toProviderSummary
      ),
  });
}

export async function providerShareImportPreviewDiscard(previewToken: string) {
  const normalizedToken = validateProviderSharePreviewToken(previewToken);
  return invokeGeneratedIpc<boolean>({
    title: "释放供应商分享预览失败",
    cmd: "provider_share_import_preview_discard",
    args: { previewToken: "[REDACTED]" },
    invoke: () =>
      commands.providerShareImportPreviewDiscard(normalizedToken) as Promise<
        GeneratedCommandResult<boolean>
      >,
  });
}
