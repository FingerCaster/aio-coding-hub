import { useQuery } from "@tanstack/react-query";
import { updaterCheck } from "../services/app/updater";
import type { UpdateChannel } from "../services/app/updateChannel";
import { updaterKeys } from "./keys";

export function useUpdaterCheckQuery(options?: { channel?: UpdateChannel; enabled?: boolean }) {
  const channel = options?.channel ?? "stable";

  return useQuery({
    queryKey: updaterKeys.check(channel),
    queryFn: () => updaterCheck(channel),
    enabled: options?.enabled ?? false,
    staleTime: 1000 * 60 * 30,
  });
}
