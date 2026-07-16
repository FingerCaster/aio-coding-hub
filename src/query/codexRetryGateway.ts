import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useDocumentVisibility } from "../hooks/useDocumentVisibility";
import {
  codexRetryGatewayApplyCommit,
  codexRetryGatewayCheckUpdate,
  codexRetryGatewayCreateDetailsSession,
  codexRetryGatewayEnablePlan,
  codexRetryGatewayRetry,
  codexRetryGatewayRevokeDetailsSession,
  codexRetryGatewaySetEnabled,
  codexRetryGatewaySetNodeOverride,
  codexRetryGatewayStatus,
  codexRetryGatewayUninstall,
  codexRetryGatewayValidateCommit,
  type CodexRetryGatewayApplyCommitRequest,
  type CodexRetryGatewaySetEnabledRequest,
  type CodexRetryGatewaySetNodeOverrideRequest,
  type CodexRetryGatewayStatus as CodexRetryGatewayStatusType,
  type CodexRetryGatewayUninstallRequest,
} from "../services/cli/codexRetryGateway";
import { cliProxyKeys, codexRetryGatewayKeys, gatewayKeys } from "./keys";

function preferNewerStatus(
  current: CodexRetryGatewayStatusType | null | undefined,
  next: CodexRetryGatewayStatusType
) {
  if (!current) return next;
  return current.generation > next.generation ? current : next;
}

function setGatewayStatusCache(
  queryClient: ReturnType<typeof useQueryClient>,
  next: CodexRetryGatewayStatusType
) {
  queryClient.setQueryData<CodexRetryGatewayStatusType | null>(
    codexRetryGatewayKeys.status(),
    (current) => preferNewerStatus(current, next)
  );
}

export function useCodexRetryGatewayStatusQuery(options?: {
  enabled?: boolean;
  refetchIntervalMs?: number | false;
}) {
  const queryClient = useQueryClient();
  const documentVisible = useDocumentVisibility();

  return useQuery({
    queryKey: codexRetryGatewayKeys.status(),
    queryFn: async () => {
      const next = await codexRetryGatewayStatus();
      return preferNewerStatus(
        queryClient.getQueryData<CodexRetryGatewayStatusType | null>(
          codexRetryGatewayKeys.status()
        ),
        next
      );
    },
    enabled: options?.enabled ?? true,
    placeholderData: keepPreviousData,
    refetchInterval: documentVisible ? (options?.refetchIntervalMs ?? false) : false,
    refetchIntervalInBackground: true,
  });
}

export function useCodexRetryGatewayEnablePlanMutation() {
  return useMutation({
    mutationFn: () => codexRetryGatewayEnablePlan(),
  });
}

export function useCodexRetryGatewaySetEnabledMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (request: CodexRetryGatewaySetEnabledRequest) =>
      codexRetryGatewaySetEnabled(request),
    onSuccess: (result) => {
      setGatewayStatusCache(queryClient, result.status);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() });
      void queryClient.invalidateQueries({ queryKey: cliProxyKeys.statusAll() });
      void queryClient.invalidateQueries({ queryKey: gatewayKeys.status() });
    },
  });
}

export function useCodexRetryGatewayCheckUpdateMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => codexRetryGatewayCheckUpdate(),
    onSuccess: (candidate) => {
      queryClient.setQueryData<CodexRetryGatewayStatusType | null>(
        codexRetryGatewayKeys.status(),
        (current) => (current ? { ...current, update_candidate: candidate } : current)
      );
    },
  });
}

export function useCodexRetryGatewayValidateCommitMutation() {
  return useMutation({
    mutationFn: (commit: string) => codexRetryGatewayValidateCommit(commit),
  });
}

export function useCodexRetryGatewayApplyCommitMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (request: CodexRetryGatewayApplyCommitRequest) =>
      codexRetryGatewayApplyCommit(request),
    onSuccess: (status) => {
      setGatewayStatusCache(queryClient, status);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() });
    },
  });
}

export function useCodexRetryGatewaySetNodeOverrideMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (request: CodexRetryGatewaySetNodeOverrideRequest) =>
      codexRetryGatewaySetNodeOverride(request),
    onSuccess: (nodeStatus, request) => {
      queryClient.setQueryData<CodexRetryGatewayStatusType | null>(
        codexRetryGatewayKeys.status(),
        (current) => {
          if (!current || current.generation !== request.generation) return current;
          return { ...current, node_status: nodeStatus };
        }
      );
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() });
    },
  });
}

export function useCodexRetryGatewayRetryMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (generation: number) => codexRetryGatewayRetry(generation),
    onSuccess: (status) => {
      setGatewayStatusCache(queryClient, status);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() });
    },
  });
}

export function useCodexRetryGatewayUninstallMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (request: CodexRetryGatewayUninstallRequest) => codexRetryGatewayUninstall(request),
    onSuccess: (status) => {
      setGatewayStatusCache(queryClient, status);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codexRetryGatewayKeys.status() });
      void queryClient.invalidateQueries({ queryKey: cliProxyKeys.statusAll() });
      void queryClient.invalidateQueries({ queryKey: gatewayKeys.status() });
    },
  });
}

export function useCodexRetryGatewayCreateDetailsSessionMutation() {
  return useMutation({
    mutationFn: () => codexRetryGatewayCreateDetailsSession(),
  });
}

export function useCodexRetryGatewayRevokeDetailsSessionMutation() {
  return useMutation({
    mutationFn: (viewId: string) => codexRetryGatewayRevokeDetailsSession(viewId),
  });
}
