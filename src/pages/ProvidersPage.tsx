// Usage: Main page for managing providers and route orders. Backend commands: `providers_*`, `sort_modes_*`.

import { cliKeysWith, isCliKey, isNativeCliKey } from "../constants/clis";
import { useSearchParams } from "react-router-dom";
import { NativeCliProvidersView } from "./providers/native/NativeCliProvidersView";
import type { CliKey } from "../services/providers/providers";
import { useSettingsQuery } from "../query/settings";
import { getOrderedClis, pickDefaultCliByPriority } from "../services/cli/cliPriorityOrder";
import { PageHeader } from "../ui/PageHeader";
import { TabList } from "../ui/TabList";
import { ProvidersView } from "./providers/ProvidersView";

export function ProvidersPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const settingsQuery = useSettingsQuery();
  const providerCliKeys = cliKeysWith("provider");
  const orderedCliTabs = getOrderedClis(settingsQuery.data?.cli_priority_order, providerCliKeys);
  const orderedCliKeys = orderedCliTabs.map((cli) => cli.key);
  const defaultCli =
    pickDefaultCliByPriority(settingsQuery.data?.cli_priority_order, orderedCliKeys) ??
    providerCliKeys[0];
  const requestedCli = searchParams.get("cli");
  const effectiveCli =
    isCliKey(requestedCli) && providerCliKeys.includes(requestedCli) ? requestedCli : defaultCli;
  function setActiveCli(key: CliKey) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      next.set("cli", key);
      // A target ID belongs to one client; never carry it into another channel.
      if (key !== effectiveCli) next.delete("target");
      return next;
    });
  }
  const viewTabs: Array<{ key: CliKey; label: string }> = orderedCliTabs.map((cli) => ({
    key: cli.key,
    label: cli.name,
  }));

  return (
    <div className="flex flex-col gap-6 h-full overflow-hidden">
      <PageHeader
        title="供应商"
        actions={
          <div className="min-w-0 max-w-full overflow-x-auto scrollbar-none">
            <TabList
              ariaLabel="CLI 切换"
              items={viewTabs}
              value={effectiveCli}
              onChange={setActiveCli}
              className="w-max"
              buttonClassName="shrink-0 whitespace-nowrap"
            />
          </div>
        }
      />

      {isNativeCliKey(effectiveCli) ? (
        <NativeCliProvidersView
          key={effectiveCli}
          client={effectiveCli}
          setActiveCli={setActiveCli}
        />
      ) : (
        <ProvidersView activeCli={effectiveCli} setActiveCli={setActiveCli} />
      )}
    </div>
  );
}
