import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import {
  configExport,
  configImport,
  normalizeConfigMigrateFilePath,
} from "../services/app/configMigrate";
import {
  cliManagerKeys,
  cliProxyKeys,
  codexManagedProfilesKeys,
  gatewayKeys,
  mcpKeys,
  promptsKeys,
  providerModelsKeys,
  providersKeys,
  settingsKeys,
  skillsKeys,
  sortModesKeys,
  workspacesKeys,
  wslKeys,
} from "./keys";
import { advanceProviderModelsGlobalGeneration } from "./providerModels";
import { notifyUpdateChannelImportSucceeded } from "../services/app/updateChannel";

export function useConfigExportMutation() {
  const [isPending, setIsPending] = useState(false);
  const mutateAsync = useCallback(async (input: { filePath: string }) => {
    setIsPending(true);
    try {
      return await configExport(normalizeConfigMigrateFilePath(input.filePath));
    } finally {
      setIsPending(false);
    }
  }, []);

  return useMemo(() => ({ isPending, mutateAsync }), [isPending, mutateAsync]);
}

export function useConfigImportMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: { filePath: string }) =>
      configImport(normalizeConfigMigrateFilePath(input.filePath)),
    onMutate: async () => {
      advanceProviderModelsGlobalGeneration(queryClient);
      await Promise.all([
        queryClient.cancelQueries({ queryKey: providerModelsKeys.all }),
        queryClient.cancelQueries({ queryKey: codexManagedProfilesKeys.all }),
      ]);
    },
    onSuccess: async (result) => {
      if (!result) return;
      // updater-core normalizes imported channel state to stable on the backend. Hide any
      // channel-bound candidate before the invalidated settings query repaints the page.
      await notifyUpdateChannelImportSucceeded();
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: settingsKeys.all }),
        queryClient.invalidateQueries({ queryKey: gatewayKeys.all }),
        queryClient.invalidateQueries({ queryKey: providersKeys.all }),
        queryClient.invalidateQueries({ queryKey: providerModelsKeys.all }),
        queryClient.invalidateQueries({ queryKey: codexManagedProfilesKeys.all }),
        queryClient.invalidateQueries({ queryKey: sortModesKeys.all }),
        queryClient.invalidateQueries({ queryKey: workspacesKeys.all }),
        queryClient.invalidateQueries({ queryKey: promptsKeys.all }),
        queryClient.invalidateQueries({ queryKey: mcpKeys.all }),
        queryClient.invalidateQueries({ queryKey: skillsKeys.all }),
        queryClient.invalidateQueries({ queryKey: wslKeys.all }),
        queryClient.invalidateQueries({ queryKey: cliProxyKeys.all }),
        queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfig() }),
        queryClient.invalidateQueries({ queryKey: cliManagerKeys.codexConfigToml() }),
        queryClient.resetQueries({ queryKey: cliManagerKeys.codexModelCatalogAll() }),
      ]);
    },
  });
}
