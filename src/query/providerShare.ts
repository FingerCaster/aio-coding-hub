import { useMutation, useQueryClient } from "@tanstack/react-query";
import { providerShareImportConfirm } from "../services/providers/providerShare";
import type { ProviderSummary } from "../services/providers/providers";
import { providersKeys } from "./keys";

export function useProviderShareImportMutation() {
  const queryClient = useQueryClient();

  return useMutation<ProviderSummary, Error, { previewToken: string }>({
    mutationFn: ({ previewToken }) => providerShareImportConfirm(previewToken),
    onSuccess: (imported) => {
      queryClient.setQueryData<ProviderSummary[] | null>(
        providersKeys.list(imported.cli_key),
        (previous) => {
          if (!previous) return [imported];
          return [...previous.filter((provider) => provider.id !== imported.id), imported];
        }
      );
      void queryClient.invalidateQueries({ queryKey: providersKeys.list(imported.cli_key) });
    },
  });
}
