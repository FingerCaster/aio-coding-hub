import { useQuery } from "@tanstack/react-query";
import { ompSettingsRead } from "../services/ompSettings";

export const ompSettingsKey = (targetId: string) =>
  ["native-cli", "omp-settings", targetId] as const;
export function useOmpSettingsQuery(targetId: string) {
  return useQuery({
    queryKey: ompSettingsKey(targetId),
    queryFn: () => ompSettingsRead(targetId),
    staleTime: 10_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
}
