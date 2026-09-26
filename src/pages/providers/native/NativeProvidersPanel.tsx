import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import {
  AlertTriangle,
  ArrowDownToLine,
  ExternalLink,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Upload,
} from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import {
  nativeCliProviderReadForEdit,
  type NativeProviderEdit,
  type NativeProviderSummary,
} from "../../../services/nativeCli";
import { useNativeCliProviderMutation, useNativeCliProvidersQuery } from "../../../query/nativeCli";
import {
  useNativeChannelCatalogQuery,
  useNativeChannelMutation,
} from "../../../query/nativeChannels";
import {
  SOURCE_CHANNEL_LABELS,
  type ChannelBindingSummary,
  type GatewayProtocol,
  type SourceChannel,
} from "../../../services/nativeChannels";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { ConfirmDialog } from "../../../ui/ConfirmDialog";
import { EmptyState } from "../../../ui/EmptyState";
import { Input } from "../../../ui/Input";
import { NativeProviderEditor } from "./NativeProviderEditor";
import { NativeChannelImportDialog } from "./NativeChannelImportDialog";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { cn } from "../../../utils/cn";

export function NativeProvidersPanel({
  client,
  targetId,
  onImport,
}: {
  client: NativeCliKey;
  targetId: string;
  onImport: (nativeKey: string) => void;
}) {
  const query = useNativeCliProvidersQuery(targetId);
  const mutation = useNativeCliProviderMutation();
  const channelCatalogQuery = useNativeChannelCatalogQuery(targetId);
  const channelMutation = useNativeChannelMutation();
  const [editor, setEditor] = useState<{ edit: NativeProviderEdit | null } | null>(null);
  const [loadingKey, setLoadingKey] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<NativeProviderSummary | null>(null);
  const [channelImportState, setChannelImportState] = useState<{
    sourceChannel?: SourceChannel;
    protocol?: GatewayProtocol;
  } | null>(null);
  const [withdrawBinding, setWithdrawBinding] = useState<ChannelBindingSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<"all" | "present" | "archived">("all");
  const epoch = useRef(0);

  useEffect(
    () => () => {
      epoch.current += 1;
    },
    []
  );

  const snapshot = query.data;
  const writable = Boolean(
    snapshot?.target.writable &&
    snapshot.revision &&
    (snapshot.parseStatus === "ready" || snapshot.parseStatus === "missing") &&
    !query.isError
  );
  const busy = mutation.isPending || loadingKey !== null;

  const providers = snapshot?.providers ?? [];
  const effectiveStatusFilter = writable ? statusFilter : "all";
  const presentCount = providers.filter((row) => row.state === "present").length;
  const archivedCount = providers.filter((row) => row.state === "archived").length;
  const normalizedSearch = search.trim().toLowerCase();
  const filteredProviders = providers.filter((row) => {
    if (effectiveStatusFilter !== "all" && row.state !== effectiveStatusFilter) return false;
    return [row.displayName, row.nativeKey, row.api ?? ""].some((value) =>
      value.toLowerCase().includes(normalizedSearch)
    );
  });

  function closeEditor() {
    epoch.current += 1;
    setEditor(null);
    setLoadingKey(null);
  }

  async function edit(row: NativeProviderSummary) {
    const current = ++epoch.current;
    setLoadingKey(row.nativeKey);
    setError(null);
    try {
      const result = await nativeCliProviderReadForEdit(targetId, row.nativeKey);
      if (current === epoch.current) setEditor({ edit: result });
    } catch (cause) {
      if (current === epoch.current) setError(nativeFailureMessage(cause));
    } finally {
      if (current === epoch.current) setLoadingKey(null);
    }
  }

  async function act(action: "apply" | "remove" | "delete", row: NativeProviderSummary) {
    if (!writable || !snapshot?.revision || busy) return;
    const current = epoch.current;
    const input = {
      targetId,
      nativeKey: row.nativeKey,
      expectedRevision: snapshot.revision,
      expectedNodeDigest: row.nodeDigest,
      expectedProfileRevision: row.profileRevision,
    };
    setError(null);
    try {
      await mutation.mutateAsync(
        action === "delete"
          ? { action, input: { ...input, removeFromNative: row.state === "present" } }
          : { action, input }
      );
      if (current === epoch.current) setDeleteTarget(null);
    } catch (cause) {
      if (current === epoch.current) setError(nativeFailureMessage(cause));
    }
  }

  return (
    <section className="space-y-4" aria-label="原生供应商">
      {/* 紧凑工具栏 */}
      <div className="flex flex-col gap-2 xl:flex-row xl:items-center xl:justify-between">
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => setStatusFilter("all")}
            aria-pressed={effectiveStatusFilter === "all"}
            className={`inline-flex h-9 items-center rounded-full border px-3.5 text-xs font-medium transition-colors ${
              effectiveStatusFilter === "all"
                ? "border-accent bg-accent text-white shadow-sm"
                : "border-border bg-white text-muted-foreground hover:bg-secondary dark:border-border dark:bg-secondary dark:text-secondary-foreground"
            }`}
          >
            全部({providers.length})
          </button>
          <button
            type="button"
            onClick={() => setStatusFilter("present")}
            disabled={!writable}
            aria-pressed={effectiveStatusFilter === "present"}
            className={`inline-flex h-9 items-center rounded-full border px-3.5 text-xs font-medium transition-colors ${
              effectiveStatusFilter === "present"
                ? "border-accent bg-accent text-white shadow-sm"
                : "border-border bg-white text-muted-foreground hover:bg-secondary dark:border-border dark:bg-secondary dark:text-secondary-foreground"
            }`}
          >
            已加入({writable ? presentCount : "—"})
          </button>
          <button
            type="button"
            onClick={() => setStatusFilter("archived")}
            disabled={!writable}
            aria-pressed={effectiveStatusFilter === "archived"}
            className={`inline-flex h-9 items-center rounded-full border px-3.5 text-xs font-medium transition-colors ${
              effectiveStatusFilter === "archived"
                ? "border-accent bg-accent text-white shadow-sm"
                : "border-border bg-white text-muted-foreground hover:bg-secondary dark:border-border dark:bg-secondary dark:text-secondary-foreground"
            }`}
          >
            未加入({writable ? archivedCount : "—"})
          </button>
          <span className="text-[11px] text-muted-foreground hidden md:inline">
            共 {filteredProviders.length} / {providers.length} 条
          </span>
          <span className="text-[11px] text-muted-foreground hidden lg:inline">
            · 已加入仅表示节点存在，非默认或正在使用
          </span>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-2">
          <div className="relative min-w-0 flex-1 xl:w-60 xl:flex-none">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.currentTarget.value)}
              placeholder="搜索原生供应商"
              className="h-9 pl-8 text-sm"
              aria-label="搜索原生供应商"
            />
          </div>
          <Button
            variant="secondary"
            size="sm"
            className="h-9"
            disabled={query.isFetching || busy}
            onClick={() => {
              closeEditor();
              setDeleteTarget(null);
              void query.refetch();
            }}
          >
            <RefreshCw
              className={cn("h-3.5 w-3.5 mr-1.5", query.isFetching && "animate-spin")}
              aria-hidden="true"
            />
            刷新原生配置
          </Button>
          <Button
            variant="secondary"
            size="sm"
            className="h-9"
            disabled={!writable || busy}
            onClick={() => {
              closeEditor();
              setEditor({ edit: null });
            }}
          >
            <Plus className="h-3.5 w-3.5 mr-1.5" aria-hidden="true" />
            新增原生供应商
          </Button>
          <Button
            variant="secondary"
            size="sm"
            className="h-9"
            disabled={!writable || busy}
            onClick={() => setChannelImportState({})}
          >
            <ArrowDownToLine className="h-3.5 w-3.5 mr-1.5" aria-hidden="true" />从 AIO 渠道接入
          </Button>
        </div>
      </div>

      {query.isPending ? (
        <p role="status" className="text-sm text-muted-foreground">
          读取原生配置…
        </p>
      ) : null}

      {query.isError || (snapshot && !writable) ? (
        <div
          role="alert"
          className="flex items-start gap-2.5 rounded-xl border border-amber-400/60 bg-amber-50/50 p-3.5 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
        >
          <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
          <div className="space-y-1">
            <div className="font-medium">配置状态未知或只读，已禁用修改。</div>
            <div className="text-muted-foreground">
              请检查文件格式、权限和目标；OMP 旧 JSON 需通过原生 CLI
              迁移后再刷新。已有档案不会被当成已移除。
            </div>
          </div>
        </div>
      ) : null}

      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}

      {providers.length === 0 && writable ? (
        <EmptyState
          title="暂无显式原生供应商"
          description="内置模型目录不会被复制到此处。可以新增节点，或在 CLI 修改后刷新。"
          variant="dashed"
        />
      ) : null}

      {providers.length > 0 && filteredProviders.length === 0 ? (
        <EmptyState
          title="无匹配的原生供应商"
          description="当前名称搜索或状态筛选无结果，请调整筛选条件。"
        />
      ) : null}

      <div className="space-y-3">
        {filteredProviders.map((row) => {
          const channelBinding = channelCatalogQuery.data?.bindings.find(
            (b) => b.nativeKey === row.nativeKey
          );

          return (
            <Card
              key={row.nativeKey}
              padding="sm"
              className="flex flex-col gap-3 rounded-xl transition-shadow duration-200 sm:flex-row sm:items-stretch sm:justify-between"
            >
              <div className="flex min-w-0 flex-1 flex-col justify-between gap-3">
                <div>
                  <div className="flex min-w-0 items-center gap-2 flex-wrap">
                    <h3
                      className="min-w-0 truncate text-base font-semibold text-foreground"
                      title={row.displayName}
                    >
                      {row.displayName}
                    </h3>
                    <span
                      className={cn(
                        "shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px]",
                        query.isError || !writable || row.state === "unknown"
                          ? "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400"
                          : row.state === "present"
                            ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400"
                            : "bg-secondary text-muted-foreground dark:bg-secondary dark:text-secondary-foreground"
                      )}
                    >
                      {query.isError || !writable || row.state === "unknown"
                        ? "配置状态未知"
                        : row.state === "present"
                          ? `已加入 ${client === "pi" ? "Pi" : "OMP"}`
                          : "未加入（已归档）"}
                    </span>
                    {row.managed && (
                      <span className="shrink-0 rounded-full bg-violet-50 px-2 py-0.5 font-mono text-[10px] text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
                        {channelBinding
                          ? `AIO 托管 · ${SOURCE_CHANNEL_LABELS[channelBinding.sourceChannel]}`
                          : "AIO 托管入口"}
                      </span>
                    )}
                    {channelBinding?.modified ? (
                      <span className="shrink-0 rounded-full bg-amber-50 px-2 py-0.5 font-mono text-[10px] text-amber-700 dark:bg-amber-950/40 dark:text-amber-300 border border-amber-300">
                        配置被外部修改
                      </span>
                    ) : channelBinding?.stale ? (
                      <span className="shrink-0 rounded-full bg-amber-50 px-2 py-0.5 font-mono text-[10px] text-amber-700 dark:bg-amber-950/40 dark:text-amber-300 border border-amber-300">
                        配置待刷新
                      </span>
                    ) : channelBinding && channelBinding.state !== "applied" ? (
                      <span className="shrink-0 rounded-full bg-orange-50 px-2 py-0.5 font-mono text-[10px] text-orange-700 dark:bg-orange-950/40 dark:text-orange-300 border border-orange-300">
                        待恢复（可重试更新或撤回）
                      </span>
                    ) : null}
                  </div>

                  <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-2">
                    <span
                      className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground"
                      title={`原生标识: ${row.nativeKey}`}
                    >
                      {row.nativeKey}
                    </span>
                    <span
                      className="shrink-0 rounded-full bg-sky-50 px-2 py-0.5 font-mono text-[10px] text-sky-700 dark:bg-sky-900/30 dark:text-sky-400"
                      title={`协议: ${row.api ?? "按模型独立配置"}`}
                    >
                      {row.api ?? "协议按模型配置"}
                    </span>
                    <span
                      className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground"
                      title={`显式声明模型数: ${row.modelCount}`}
                    >
                      {row.modelCount} 个显式模型
                    </span>
                    <span
                      className={cn(
                        "shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px]",
                        row.apiKeyConfigured
                          ? "bg-cyan-50 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-300"
                          : "bg-secondary text-muted-foreground"
                      )}
                      title={row.apiKeyConfigured ? "已配置原生凭证字段" : "未配置原生凭证字段"}
                    >
                      {row.apiKeyConfigured ? "已配置原生凭证字段" : "未配置原生凭证字段"}
                    </span>
                  </div>
                </div>

                {!row.managed ? (
                  <div className="flex flex-wrap items-center gap-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      className="px-2 py-1 text-[11px] gap-1.5"
                      disabled={!writable || busy}
                      onClick={() => void edit(row)}
                      title="编辑此原生供应商"
                    >
                      <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
                      {loadingKey === row.nativeKey ? "读取中…" : "编辑"}
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      className="px-2 py-1 text-[11px] gap-1.5"
                      disabled={!writable || busy}
                      onClick={() => onImport(row.nativeKey)}
                      title="创建独立 AIO 网关上游快照"
                    >
                      <Upload className="h-3.5 w-3.5" aria-hidden="true" />
                      添加到 AIO 网关
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      className="px-2 py-1 text-[11px] gap-1.5"
                      disabled={!writable || busy}
                      onClick={() => setDeleteTarget(row)}
                      title="删除此档案"
                    >
                      <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                      删除档案
                    </Button>
                  </div>
                ) : channelBinding ? (
                  <div className="flex flex-wrap items-center gap-2">
                    <Link
                      to={`/providers?cli=${channelBinding.sourceChannel}`}
                      className="inline-flex items-center gap-1 rounded-md border border-line bg-surface-panel px-2.5 py-1 text-[11px] font-medium text-foreground hover:bg-state-hover"
                      title={`转到 ${SOURCE_CHANNEL_LABELS[channelBinding.sourceChannel]} 统一渠道来源`}
                    >
                      <ExternalLink className="h-3 w-3 text-muted-foreground" aria-hidden="true" />
                      <span>管理来源渠道</span>
                    </Link>
                    <Button
                      size="sm"
                      variant="secondary"
                      className="px-2 py-1 text-[11px] gap-1"
                      disabled={!writable || busy}
                      onClick={() =>
                        setChannelImportState({
                          sourceChannel: channelBinding.sourceChannel,
                          protocol: channelBinding.protocol,
                        })
                      }
                      title="更新此统一渠道入口模型"
                    >
                      <RefreshCw className="h-3 w-3" aria-hidden="true" />
                      更新入口
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      className="px-2 py-1 text-[11px] gap-1"
                      disabled={!writable || busy}
                      onClick={() => setWithdrawBinding(channelBinding)}
                      title="精确撤回此统一渠道入口"
                    >
                      <Trash2 className="h-3 w-3" aria-hidden="true" />
                      撤回接入
                    </Button>
                  </div>
                ) : (
                  <p className="text-xs text-muted-foreground">
                    请在 AIO 网关视图预览、更新或移除此入口。
                  </p>
                )}
              </div>

              {!row.managed ? (
                <div className="flex w-32 shrink-0 items-center justify-end border-t border-border pt-2 sm:border-l sm:border-t-0 sm:pl-3 sm:pt-0">
                  <Button
                    size="sm"
                    variant={row.state === "present" ? "secondary" : "primary"}
                    className="w-full text-xs"
                    disabled={!writable || busy || row.state === "unknown"}
                    onClick={() => void act(row.state === "present" ? "remove" : "apply", row)}
                  >
                    {row.state === "present" ? "从原生 CLI 移除" : "加入原生 CLI"}
                  </Button>
                </div>
              ) : null}
            </Card>
          );
        })}
      </div>

      {editor && snapshot ? (
        <NativeProviderEditor
          key={editor.edit?.provider.nativeKey ?? "new"}
          client={client}
          snapshot={snapshot}
          edit={editor.edit}
          readOnly={!writable}
          onClose={closeEditor}
        />
      ) : null}

      {channelImportState !== null ? (
        <NativeChannelImportDialog
          client={client}
          targetId={targetId}
          targetPath={snapshot?.target.modelsPath}
          initialSourceChannel={channelImportState.sourceChannel}
          initialProtocol={channelImportState.protocol}
          onClose={() => setChannelImportState(null)}
          onApplied={() => {
            setChannelImportState(null);
            void query.refetch();
            void channelCatalogQuery.refetch();
          }}
        />
      ) : null}

      {withdrawBinding ? (
        <ConfirmDialog
          open
          title={`确认撤回 AIO · ${SOURCE_CHANNEL_LABELS[withdrawBinding.sourceChannel]} 入口`}
          description="撤回将精确移除原生目标中的该托管节点，不会删除来源渠道上游、凭证、其他目标或已有原生供应商配置。"
          confirming={channelMutation.isPending}
          confirmingLabel="撤回中…"
          confirmLabel="确认撤回"
          onClose={() => {
            if (!channelMutation.isPending) setWithdrawBinding(null);
          }}
          onConfirm={async () => {
            if (!snapshot?.revision || !channelCatalogQuery.data) return;
            try {
              await channelMutation.mutateAsync({
                targetId,
                expectedRevision: snapshot.revision,
                catalogRevision: channelCatalogQuery.data.catalogRevision,
                selections: [],
                removeBindingIds: [withdrawBinding.bindingId],
              });
              setWithdrawBinding(null);
              void query.refetch();
              void channelCatalogQuery.refetch();
            } catch (cause) {
              setError(nativeFailureMessage(cause));
            }
          }}
        >
          <div className="space-y-2 text-xs">
            <p className="font-mono text-muted-foreground break-all">目标：{targetId}</p>
            <p className="text-foreground">
              将精确移除的原生节点：
              <span className="font-mono font-bold">{withdrawBinding.nativeKey}</span>
            </p>
          </div>
        </ConfirmDialog>
      ) : null}

      <ConfirmDialog
        open={deleteTarget !== null}
        title="删除原生供应商档案"
        description={
          deleteTarget?.state === "present"
            ? "该节点已加入原生 CLI。确认后将同时移除当前目标中的节点和档案，不修改默认模型或登录数据。"
            : "确认后删除停用档案；独立的 AIO 网关上游快照会保留。"
        }
        confirmLabel="确认删除"
        confirmingLabel="删除中…"
        confirming={mutation.isPending}
        disabled={!writable}
        onClose={() => {
          if (!mutation.isPending) setDeleteTarget(null);
        }}
        onConfirm={() => {
          if (deleteTarget) void act("delete", deleteTarget);
        }}
      />
    </section>
  );
}
