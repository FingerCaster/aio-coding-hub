import { useMemo } from "react";
import type { ActiveUiContribution } from "../../generated/bindings";
import { usePluginActiveContributionsQuery } from "../../query/plugins";
import type { UiContributionSlotId } from "./types";

export function useActiveContributions(options?: { enabled?: boolean }) {
  const query = usePluginActiveContributionsQuery(options);
  const queryUi = query.data?.ui;
  const ui = useMemo(() => queryUi ?? [], [queryUi]);

  const uiBySlot = useMemo(() => {
    const bySlot = new Map<string, ActiveUiContribution[]>();
    for (const contribution of ui) {
      const slotContributions = bySlot.get(contribution.slotId) ?? [];
      slotContributions.push(contribution);
      bySlot.set(contribution.slotId, slotContributions);
    }
    return bySlot;
  }, [ui]);

  return {
    ...query,
    ui,
    uiBySlot,
  };
}

export function useContributionsForSlot(slotId: UiContributionSlotId) {
  const { uiBySlot, ...query } = useActiveContributions();
  return {
    ...query,
    contributions: uiBySlot.get(slotId) ?? [],
  };
}
