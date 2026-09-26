import { useMemo, useState } from "react";
import { clisWith, cliShortLabel, type CliFilterKey } from "../../constants/clis";
import { useProvidersListQuery } from "../../query/providers";
import type { CliKey, ProviderSummary } from "../../services/providers/providers";

export type UsageProviderOption = {
  id: number;
  cliKey: CliKey;
  label: string;
};

const EMPTY_PROVIDERS: ProviderSummary[] = [];

function buildProviderOption(provider: ProviderSummary): UsageProviderOption {
  return {
    id: provider.id,
    cliKey: provider.cli_key,
    label: `${cliShortLabel(provider.cli_key)} · ${provider.name}`,
  };
}

function providersForCli(
  cliKey: CliFilterKey,
  providersByCli: Record<CliKey, ProviderSummary[]>
): ProviderSummary[] {
  if (cliKey === "all") {
    return clisWith("usage").flatMap((cli) => providersByCli[cli.key]);
  }
  return providersByCli[cliKey];
}

export function useUsagePageProviderFilter(cliKey: CliFilterKey) {
  const [providerId, setProviderId] = useState<number | null>(null);

  const claudeProvidersQuery = useProvidersListQuery("claude");
  const codexProvidersQuery = useProvidersListQuery("codex");
  const geminiProvidersQuery = useProvidersListQuery("gemini");
  const grokProvidersQuery = useProvidersListQuery("grok");
  const piProvidersQuery = useProvidersListQuery("pi", {
    enabled: clisWith("provider").some((cli) => cli.key === "pi"),
  });
  const ompProvidersQuery = useProvidersListQuery("omp", {
    enabled: clisWith("provider").some((cli) => cli.key === "omp"),
  });

  const providerOptions = useMemo(() => {
    const providersByCli = {
      claude: claudeProvidersQuery.data ?? EMPTY_PROVIDERS,
      codex: codexProvidersQuery.data ?? EMPTY_PROVIDERS,
      gemini: geminiProvidersQuery.data ?? EMPTY_PROVIDERS,
      grok: grokProvidersQuery.data ?? EMPTY_PROVIDERS,
      pi: piProvidersQuery.data ?? EMPTY_PROVIDERS,
      omp: ompProvidersQuery.data ?? EMPTY_PROVIDERS,
    } satisfies Record<CliKey, ProviderSummary[]>;

    return providersForCli(cliKey, providersByCli).map(buildProviderOption);
  }, [
    cliKey,
    claudeProvidersQuery.data,
    codexProvidersQuery.data,
    geminiProvidersQuery.data,
    grokProvidersQuery.data,
    piProvidersQuery.data,
    ompProvidersQuery.data,
  ]);

  if (providerId != null && !providerOptions.some((option) => option.id === providerId)) {
    setProviderId(null);
  }

  const providersLoading =
    claudeProvidersQuery.isFetching ||
    codexProvidersQuery.isFetching ||
    geminiProvidersQuery.isFetching ||
    grokProvidersQuery.isFetching ||
    piProvidersQuery.isFetching ||
    ompProvidersQuery.isFetching;

  return { providerId, setProviderId, providerOptions, providersLoading };
}
