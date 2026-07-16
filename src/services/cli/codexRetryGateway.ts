import {
  commands,
  type CodexRetryGatewayApplyCommitRequest,
  type CodexRetryGatewayCommitValidation,
  type CodexRetryGatewayDetailsSession,
  type CodexRetryGatewayEnablePlan,
  type CodexRetryGatewayGenerationRequest,
  type CodexRetryGatewayNodeStatus,
  type CodexRetryGatewayRevokeDetailsSessionRequest,
  type CodexRetryGatewaySetEnabledRequest,
  type CodexRetryGatewaySetEnabledResult,
  type CodexRetryGatewaySetNodeOverrideRequest,
  type CodexRetryGatewayStatus,
  type CodexRetryGatewayUninstallRequest,
  type CodexRetryGatewayUpdateCandidate,
  type CodexRetryGatewayValidateCommitRequest,
} from "../../generated/bindings";
import { invokeGeneratedIpc, type GeneratedCommandResult } from "../generatedIpc";

export type {
  CodexRetryGatewayApplyCommitRequest,
  CodexRetryGatewayCommitValidation,
  CodexRetryGatewayDetailsSession,
  CodexRetryGatewayEnablePlan,
  CodexRetryGatewayNodeStatus,
  CodexRetryGatewayRevokeDetailsSessionRequest,
  CodexRetryGatewaySetEnabledRequest,
  CodexRetryGatewaySetEnabledResult,
  CodexRetryGatewaySetNodeOverrideRequest,
  CodexRetryGatewayStatus,
  CodexRetryGatewayUninstallRequest,
  CodexRetryGatewayUpdateCandidate,
};

function assertSafeInteger(label: string, value: number) {
  if (!Number.isSafeInteger(value)) {
    throw new Error(`SEC_INVALID_INPUT: invalid ${label}=${value}`);
  }
}

function normalizeGenerationRequest(generation: number): CodexRetryGatewayGenerationRequest {
  assertSafeInteger("codex retry gateway generation", generation);
  if (generation < 0) {
    throw new Error(`SEC_INVALID_INPUT: invalid codex retry gateway generation=${generation}`);
  }
  return { generation };
}

function normalizeCommitInput(commit: string): string {
  if (typeof commit !== "string") {
    throw new Error("SEC_INVALID_INPUT: commit must be a string");
  }
  const normalized = commit.trim();
  if (!normalized) {
    throw new Error("SEC_INVALID_INPUT: commit is required");
  }
  if (normalized.length > 128) {
    throw new Error("SEC_INVALID_INPUT: commit is too long");
  }
  return normalized;
}

function normalizeSetEnabledRequest(
  request: CodexRetryGatewaySetEnabledRequest
): CodexRetryGatewaySetEnabledRequest {
  if (typeof request.enabled !== "boolean") {
    throw new Error("SEC_INVALID_INPUT: enabled must be a boolean");
  }
  return {
    enabled: request.enabled,
    planGeneration: normalizeGenerationRequest(request.planGeneration).generation,
    confirmation: {
      acceptedFirstDownload: Boolean(request.confirmation.acceptedFirstDownload),
      acceptedUnreviewedCommit: Boolean(request.confirmation.acceptedUnreviewedCommit),
      acceptedCliProxyEnable: Boolean(request.confirmation.acceptedCliProxyEnable),
      acceptedProviderSync: Boolean(request.confirmation.acceptedProviderSync),
      acceptedWslUnprotected: Boolean(request.confirmation.acceptedWslUnprotected),
    },
  };
}

function normalizeApplyCommitRequest(
  request: CodexRetryGatewayApplyCommitRequest
): CodexRetryGatewayApplyCommitRequest {
  return {
    planGeneration: normalizeGenerationRequest(request.planGeneration).generation,
    commit: normalizeCommitInput(request.commit),
    acceptedUpdate: Boolean(request.acceptedUpdate),
    acceptedUnreviewedCommit: Boolean(request.acceptedUnreviewedCommit),
  };
}

function normalizeNodeOverrideRequest(
  request: CodexRetryGatewaySetNodeOverrideRequest
): CodexRetryGatewaySetNodeOverrideRequest {
  const executable =
    typeof request.executable === "string" ? request.executable.trim() : request.executable;
  return {
    generation: normalizeGenerationRequest(request.generation).generation,
    executable: executable || null,
  };
}

function normalizeUninstallRequest(
  request: CodexRetryGatewayUninstallRequest
): CodexRetryGatewayUninstallRequest {
  return {
    generation: normalizeGenerationRequest(request.generation).generation,
    confirmedDataRemoval: Boolean(request.confirmedDataRemoval),
  };
}

function normalizeRevokeDetailsSessionRequest(
  viewId: string
): CodexRetryGatewayRevokeDetailsSessionRequest {
  if (typeof viewId !== "string" || !/^[0-9a-fA-F]{32}$/.test(viewId)) {
    throw new Error("SEC_INVALID_INPUT: invalid Codex retry gateway details view id");
  }
  return { viewId };
}

export async function codexRetryGatewayStatus() {
  return invokeGeneratedIpc<CodexRetryGatewayStatus>({
    title: "读取 Codex 外部网关状态失败",
    cmd: "codex_retry_gateway_status",
    invoke: () => commands.codexRetryGatewayStatus(),
  });
}

export async function codexRetryGatewayEnablePlan() {
  return invokeGeneratedIpc<CodexRetryGatewayEnablePlan>({
    title: "生成 Codex 外部网关启用计划失败",
    cmd: "codex_retry_gateway_enable_plan",
    invoke: () =>
      commands.codexRetryGatewayEnablePlan() as Promise<
        GeneratedCommandResult<CodexRetryGatewayEnablePlan>
      >,
  });
}

export async function codexRetryGatewaySetEnabled(request: CodexRetryGatewaySetEnabledRequest) {
  const payload = normalizeSetEnabledRequest(request);
  return invokeGeneratedIpc<CodexRetryGatewaySetEnabledResult>({
    title: payload.enabled ? "启用 Codex 外部网关失败" : "停用 Codex 外部网关失败",
    cmd: "codex_retry_gateway_set_enabled",
    args: { request: payload },
    invoke: () =>
      commands.codexRetryGatewaySetEnabled(payload) as Promise<
        GeneratedCommandResult<CodexRetryGatewaySetEnabledResult>
      >,
  });
}

export async function codexRetryGatewayCheckUpdate() {
  return invokeGeneratedIpc<CodexRetryGatewayUpdateCandidate | null, null>({
    title: "检查 Codex 外部网关更新失败",
    cmd: "codex_retry_gateway_check_update",
    invoke: () =>
      commands.codexRetryGatewayCheckUpdate() as Promise<
        GeneratedCommandResult<CodexRetryGatewayUpdateCandidate | null>
      >,
    fallback: null,
    nullResultBehavior: "return_fallback",
  });
}

export async function codexRetryGatewayValidateCommit(commit: string) {
  const request: CodexRetryGatewayValidateCommitRequest = {
    commit: normalizeCommitInput(commit),
  };
  return invokeGeneratedIpc<CodexRetryGatewayCommitValidation>({
    title: "校验 Codex 外部网关提交失败",
    cmd: "codex_retry_gateway_validate_commit",
    args: { request },
    invoke: () =>
      commands.codexRetryGatewayValidateCommit(request) as Promise<
        GeneratedCommandResult<CodexRetryGatewayCommitValidation>
      >,
  });
}

export async function codexRetryGatewayApplyCommit(request: CodexRetryGatewayApplyCommitRequest) {
  const payload = normalizeApplyCommitRequest(request);
  return invokeGeneratedIpc<CodexRetryGatewayStatus>({
    title: "应用 Codex 外部网关提交失败",
    cmd: "codex_retry_gateway_apply_commit",
    args: { request: payload },
    invoke: () =>
      commands.codexRetryGatewayApplyCommit(payload) as Promise<
        GeneratedCommandResult<CodexRetryGatewayStatus>
      >,
  });
}

export async function codexRetryGatewaySetNodeOverride(
  request: CodexRetryGatewaySetNodeOverrideRequest
) {
  const payload = normalizeNodeOverrideRequest(request);
  return invokeGeneratedIpc<CodexRetryGatewayNodeStatus>({
    title: "更新 Codex 外部网关 Node 运行时失败",
    cmd: "codex_retry_gateway_set_node_override",
    args: { request: payload },
    invoke: () =>
      commands.codexRetryGatewaySetNodeOverride(payload) as Promise<
        GeneratedCommandResult<CodexRetryGatewayNodeStatus>
      >,
  });
}

export async function codexRetryGatewayRetry(generation: number) {
  const request = normalizeGenerationRequest(generation);
  return invokeGeneratedIpc<CodexRetryGatewayStatus>({
    title: "重试 Codex 外部网关恢复失败",
    cmd: "codex_retry_gateway_retry",
    args: { request },
    invoke: () =>
      commands.codexRetryGatewayRetry(request) as Promise<
        GeneratedCommandResult<CodexRetryGatewayStatus>
      >,
  });
}

export async function codexRetryGatewayUninstall(request: CodexRetryGatewayUninstallRequest) {
  const payload = normalizeUninstallRequest(request);
  return invokeGeneratedIpc<CodexRetryGatewayStatus>({
    title: "卸载 Codex 外部网关失败",
    cmd: "codex_retry_gateway_uninstall",
    args: { request: payload },
    invoke: () =>
      commands.codexRetryGatewayUninstall(payload) as Promise<
        GeneratedCommandResult<CodexRetryGatewayStatus>
      >,
  });
}

export async function codexRetryGatewayCreateDetailsSession() {
  return invokeGeneratedIpc<CodexRetryGatewayDetailsSession>({
    title: "创建 Codex 外部网关详情会话失败",
    cmd: "codex_retry_gateway_create_details_session",
    invoke: () =>
      commands.codexRetryGatewayCreateDetailsSession() as Promise<
        GeneratedCommandResult<CodexRetryGatewayDetailsSession>
      >,
  });
}

export async function codexRetryGatewayRevokeDetailsSession(viewId: string) {
  const request = normalizeRevokeDetailsSessionRequest(viewId);
  return invokeGeneratedIpc<null>({
    title: "撤销 Codex 外部网关详情会话失败",
    cmd: "codex_retry_gateway_revoke_details_session",
    args: { request },
    invoke: () =>
      commands.codexRetryGatewayRevokeDetailsSession(request) as Promise<
        GeneratedCommandResult<null>
      >,
    fallback: null,
    nullResultBehavior: "return_fallback",
  });
}
