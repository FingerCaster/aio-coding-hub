import { useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { AlertCircle, RefreshCw, Terminal } from "lucide-react";
import type { CliKey, NativeCliKey } from "../../../constants/clis";
import { useNativeCliTargetsQuery } from "../../../query/nativeCli";
import type { NativeTarget } from "../../../services/nativeCli";
import { Button } from "../../../ui/Button";
import { TabList } from "../../../ui/TabList";
import { ProvidersView } from "../ProvidersView";
import { NativeTargetPicker } from "./NativeTargetPicker";
import { NativeProvidersPanel } from "./NativeProvidersPanel";
import { NativeGatewayImportDialog } from "./NativeGatewayImportDialog";
import { NativeGatewayEntries } from "./NativeGatewayEntries";

export function NativeCliProvidersView({
  client,
  setActiveCli,
}: {
  client: NativeCliKey;
  setActiveCli: (key: CliKey) => void;
}) {
  const [searchParams, setSearchParams] = useSearchParams();
  const viewParam = searchParams.get("view");
  const targetParam = searchParams.get("target");

  const view = viewParam === "gateway" ? "gateway" : "native";
  const selectedId = targetParam;
  const [importSource, setImportSource] = useState<{ targetId: string; nativeKey: string } | null>(
    null
  );
  const targets = useNativeCliTargetsQuery(client);

  const target =
    targets.data?.find((row) => (selectedId ? row.targetId === selectedId : row.selected)) ??
    (selectedId ? undefined : targets.data?.[0]);

  const handleViewChange = (nextView: "native" | "gateway") => {
    setImportSource(null);
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("view", nextView);
      return next;
    });
  };

  const handleTargetSelect = (next: NativeTarget) => {
    setImportSource(null);
    setSearchParams((prev) => {
      const nextParams = new URLSearchParams(prev);
      nextParams.set("target", next.targetId);
      return nextParams;
    });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto scrollbar-overlay">
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-border/60 pb-3">
        <TabList
          ariaLabel="供应商配置视图"
          items={[
            { key: "native", label: "原生配置" },
            { key: "gateway", label: "AIO 网关" },
          ]}
          value={view}
          onChange={handleViewChange}
        />
        <div className="flex items-center gap-2">
          <Link
            to={`/cli-manager?tab=${client}`}
            className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-line bg-surface-panel px-3 text-xs font-medium text-foreground hover:bg-state-hover hover:border-line-strong transition-colors"
            title={`转到 ${client.toUpperCase()} CLI 管理`}
          >
            <Terminal className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
            <span>CLI 管理</span>
          </Link>
        </div>
      </div>

      {targets.isError ? (
        <div
          role="alert"
          className="flex items-center justify-between gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive"
        >
          <div className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 shrink-0" />
            <span>原生目标读取失败，无法确认读写路径。</span>
          </div>
          <Button variant="secondary" size="sm" onClick={() => void targets.refetch()}>
            <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
            重新读取目标
          </Button>
        </div>
      ) : targets.isPending ? (
        <p role="status" className="text-sm text-muted-foreground">
          读取原生配置目标…
        </p>
      ) : (
        <NativeTargetPicker
          client={client}
          targets={targets.data ?? []}
          targetId={target?.targetId ?? null}
          onSelect={handleTargetSelect}
        />
      )}

      {view === "native" && target && !targets.isError ? (
        <NativeProvidersPanel
          key={target.targetId}
          client={client}
          targetId={target.targetId}
          onImport={(nativeKey) => setImportSource({ targetId: target.targetId, nativeKey })}
        />
      ) : null}

      {view === "gateway" ? (
        <>
          {target && !targets.isError ? (
            <NativeGatewayEntries
              key={target.targetId}
              client={client}
              targetId={target.targetId}
              targetPath={target.modelsPath}
            />
          ) : (
            <p role="alert" className="text-sm text-muted-foreground">
              请选择有效原生目标后管理 AIO 入口。
            </p>
          )}
          <div className="min-h-[32rem] flex-1">
            <ProvidersView key={client} activeCli={client} setActiveCli={setActiveCli} />
          </div>
        </>
      ) : null}

      {view === "native" &&
      target &&
      !targets.isError &&
      importSource?.targetId === target.targetId ? (
        <NativeGatewayImportDialog
          key={`${target.targetId}:${importSource.nativeKey}`}
          targetId={target.targetId}
          nativeKey={importSource.nativeKey}
          onClose={() => setImportSource(null)}
          onImported={() => {
            setImportSource(null);
            handleViewChange("gateway");
          }}
        />
      ) : null}
    </div>
  );
}
