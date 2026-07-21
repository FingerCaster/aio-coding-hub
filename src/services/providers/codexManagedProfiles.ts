import {
  commands,
  type CodexManagedProfile as GeneratedCodexManagedProfile,
  type CodexManagedProfileDeleteResult as GeneratedCodexManagedProfileDeleteResult,
} from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import { validateProviderId } from "./providers";
import { normalizeRemoteModelId, validateModelUuid, validateProviderUuid } from "./providerModels";
import { isCanonicalUuidV4 } from "./uuid";

const PROFILE_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/;

export type CodexManagedProfileFileStatus = "managed" | "missing" | "modified";

export type CodexManagedProfile = {
  profileUuid: string;
  profileName: string;
  modelUuid: string;
  providerId: number;
  providerUuid: string;
  providerName: string;
  remoteModelId: string;
  canonicalModel: string;
  fileStatus: CodexManagedProfileFileStatus;
  createdAt: number;
  updatedAt: number;
};

export type CodexManagedProfileDeleteResult = {
  deleted: boolean;
  externalFilePreserved: boolean;
};

export function normalizeCodexProfileName(value: string): string {
  const profileName = value.trim();
  if (!PROFILE_NAME_RE.test(profileName)) {
    throw new Error("SEC_INVALID_INPUT: invalid profileName");
  }
  return profileName;
}

export function validateProfileUuid(value: string): string {
  if (!isCanonicalUuidV4(value)) {
    throw new Error("SEC_INVALID_INPUT: invalid profileUuid");
  }
  return value;
}

function requireTimestamp(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`IPC_INVALID_TIMESTAMP: ${label}`);
  }
  return value;
}

export function decodeCodexManagedProfile(
  value: GeneratedCodexManagedProfile
): CodexManagedProfile {
  const profileUuid = validateProfileUuid(value.profileUuid);
  const profileName = normalizeCodexProfileName(value.profileName);
  const modelUuid = validateModelUuid(value.modelUuid, "profile.modelUuid");
  const canonicalModel = value.canonicalModel.trim();
  if (canonicalModel !== `aio/${modelUuid}`) {
    throw new Error("IPC_MANAGED_PROFILE_ALIAS_MISMATCH");
  }
  if (
    value.fileStatus !== "managed" &&
    value.fileStatus !== "missing" &&
    value.fileStatus !== "modified"
  ) {
    throw new Error(`IPC_INVALID_LITERAL: profile.fileStatus=${String(value.fileStatus)}`);
  }

  return {
    profileUuid,
    profileName,
    modelUuid,
    providerId: validateProviderId(value.providerId, "profile.providerId"),
    providerUuid: validateProviderUuid(value.providerUuid, "profile.providerUuid"),
    providerName: value.providerName.trim() || `Provider #${value.providerId}`,
    remoteModelId: normalizeRemoteModelId(value.remoteModelId),
    canonicalModel,
    fileStatus: value.fileStatus,
    createdAt: requireTimestamp(value.createdAt, "profile.createdAt"),
    updatedAt: requireTimestamp(value.updatedAt, "profile.updatedAt"),
  };
}

function decodeDeleteResult(
  value: GeneratedCodexManagedProfileDeleteResult
): CodexManagedProfileDeleteResult {
  if (typeof value.deleted !== "boolean" || typeof value.externalFilePreserved !== "boolean") {
    throw new Error("IPC_INVALID_BOOLEAN: managedProfileDeleteResult");
  }
  return {
    deleted: value.deleted,
    externalFilePreserved: value.externalFilePreserved,
  };
}

export async function codexManagedProfilesList(): Promise<CodexManagedProfile[]> {
  return invokeGeneratedIpc<CodexManagedProfile[]>({
    title: "读取 Codex Profile 失败",
    cmd: "codex_managed_profiles_list",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.codexManagedProfilesList(), (rows) =>
        rows.map(decodeCodexManagedProfile)
      ),
  });
}

export async function codexManagedProfileCreate(
  profileName: string,
  modelUuid: string
): Promise<CodexManagedProfile> {
  const normalizedName = normalizeCodexProfileName(profileName);
  const normalizedModelUuid = validateModelUuid(modelUuid);
  return invokeGeneratedIpc<CodexManagedProfile>({
    title: "创建 Codex Profile 失败",
    cmd: "codex_managed_profile_create",
    args: { profileName: normalizedName, modelUuid: normalizedModelUuid },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.codexManagedProfileCreate(normalizedName, normalizedModelUuid),
        decodeCodexManagedProfile
      ),
  });
}

export async function codexManagedProfileDelete(
  profileUuid: string
): Promise<CodexManagedProfileDeleteResult> {
  const normalizedProfileUuid = validateProfileUuid(profileUuid);
  return invokeGeneratedIpc<CodexManagedProfileDeleteResult>({
    title: "删除 Codex Profile 失败",
    cmd: "codex_managed_profile_delete",
    args: { profileUuid: normalizedProfileUuid },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.codexManagedProfileDelete(normalizedProfileUuid),
        decodeDeleteResult
      ),
  });
}
