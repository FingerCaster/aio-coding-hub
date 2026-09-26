import { useState } from "react";
import { Link } from "react-router-dom";
import {
  ArrowDownToLine,
  ExternalLink,
  Eye,
  Network,
  Radio,
  RefreshCw,
  Trash2,
} from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import type { GatewayCatalogPreview } from "../../../services/nativeGateway";
import {
  useNativeGatewayCatalogQuery,
  useNativeGatewayMutation,
} from "../../../query/nativeGateway";
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
import { useRequestLogsListAllQuery } from "../../../query/requestLogs";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { ConfirmDialog } from "../../../ui/ConfirmDialog";
import { NativeChannelImportDialog } from "./NativeChannelImportDialog";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { cn } from "../../../utils/cn";

export function NativeGatewayEntries({
  client,
  targetId,
  targetPath,
}: {
  client: NativeCliKey;
  targetId: string;
  targetPath?: string;
}) {
  const query = useNativeGatewayCatalogQuery(targetId);
  const channelCatalogQuery = useNativeChannelCatalogQuery(targetId);
  const channelMutation = useNativeChannelMutation();
  const logs = useRequestLogsListAllQuery(100);
  const mutation = useNativeGatewayMutation();
  const [confirmation, setConfirmation] = useState<{
    action: "apply" | "remove";
    preview: GatewayCatalogPreview;
  } | null>(null);
  const [channelImportState, setChannelImportState] = useState<{
    sourceChannel?: SourceChannel;
    protocol?: GatewayProtocol;
  } | null>(null);
  const [withdrawBinding, setWithdrawBinding] = useState<ChannelBindingSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const current = query.data;
  const lastRequest = logs.data?.find((row) => row.cli_key === client);
  const stale = Boolean(
    confirmation &&
    (confirmation.preview.revision !== current?.revision ||
      confirmation.preview.catalogRevision !== current?.catalogRevision)
  );

  async function preview(action: "apply" | "remove") {
    setError(null);
    setConfirmation(null);
    setNotice(null);
    const result = await query.refetch();
    if (result.isError || !result.data) {
      setError(nativeFailureMessage(result.error));
      return;
    }
    setConfirmation({ action, preview: result.data });
  }

  async function confirm() {
    if (!confirmation || stale || mutation.isPending) return;
    const { action, preview: data } = confirmation;
    try {
      await mutation.mutateAsync({
        action,
        input: { targetId, expectedRevision: data.revision, catalogRevision: data.catalogRevision },
      });
      setConfirmation(null);
      setNotice(
        action === "apply"
          ? "入口已写入。请在原生 CLI 的模型选择器中选择 AIO 模型；当前会话与默认模型未自动切换。"
          : "已移除仍由 AIO 拥有的入口。原生供应商、上游快照和默认设置保持不变；手工默认引用可能需要你调整。"
      );
    } catch (cause) {
      setError(nativeFailureMessage(cause));
      setConfirmation(null);
    }
  }

  const applyBlocked =
    !current?.listenerReady ||
    !current.entries.length ||
    current.manifests.some((manifest) => manifest.modified);

  return (
    <Card
      padding="sm"
      aria-label="独立 AIO 入口"
      className="shrink-0 space-y-3.5 rounded-xl border border-border"
    >
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-2">
          <Network className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
          <h2 className="text-sm font-semibold text-foreground">独立 AIO 入口</h2>
        </div>
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="inline-flex items-center gap-1.5 rounded-full border border-border/60 bg-surface-panel/40 px-2.5 py-1 text-muted-foreground">
            <Radio className="h-3 w-3 text-muted-foreground" aria-hidden="true" />
            <span>
              监听器：
              {query.isError
                ? "未知"
                : current
                  ? current.listenerReady
                    ? "就绪"
                    : "未就绪"
                  : "读取中"}
            </span>
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full border border-border/60 bg-surface-panel/40 px-2.5 py-1 text-muted-foreground">
            <span>
              实际流量：
              {logs.isError
                ? "观测不可用"
                : lastRequest
                  ? `已观测到 ${client.toUpperCase()} 请求（记录 #${lastRequest.id}）`
                  : logs.isPending
                    ? "读取中"
                    : "最近 100 条记录中未观测到此 CLI 请求"}
            </span>
          </span>
        </div>
      </div>

      <p className="text-xs text-muted-foreground">
        最多四个协议节点与原生供应商并存。应用只发布明确声明的模型，不修改原生默认或接管所有原生节点。
      </p>

      {current?.manifests.length === 0 && !query.isError ? (
        <div className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">
          入口未配置
        </div>
      ) : (
        <div className="space-y-2">
          {current?.manifests.map((manifest) => (
            <div
              key={manifest.nativeKey}
              className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border/70 bg-surface-panel/30 p-2.5 text-xs"
            >
              <p className="font-mono text-xs font-medium text-foreground">
                {manifest.nativeKey} ·{" "}
                {query.isError
                  ? "配置状态未知"
                  : manifest.modified
                    ? "配置被外部修改"
                    : manifest.stale
                      ? "配置待刷新"
                      : manifest.state === "applied"
                        ? "已配置"
                        : `状态待确认（${manifest.state}）`}
              </p>
              <span className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
                {manifest.protocol}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* 从 AIO 统一渠道接入列表 */}
      <div className="space-y-2 pt-3 border-t border-border/60">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-xs font-semibold text-foreground">
              从 AIO 统一渠道接入 ({channelCatalogQuery.data?.bindings.length ?? 0})
            </h3>
            <span className="text-[11px] text-muted-foreground hidden sm:inline">
              · 实时引用 AIO 统一渠道上游池与模型
            </span>
          </div>
        </div>

        {channelCatalogQuery.data?.bindings.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">
            暂无从 AIO 统一渠道接入的入口。点击下方“从 AIO 渠道接入”可将已有的 Codex、Claude
            Code、Grok、Gemini 统一渠道引入当前 CLI。
          </div>
        ) : (
          <div className="space-y-2">
            {channelCatalogQuery.data?.bindings.map((b) => (
              <div
                key={b.bindingId}
                className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border/70 bg-surface-panel/30 p-2.5 text-xs"
              >
                <div>
                  <div className="flex items-center gap-2">
                    <span className="font-mono font-medium text-foreground">{b.nativeKey}</span>
                    <span className="shrink-0 rounded-full bg-violet-50 px-2 py-0.5 font-mono text-[10px] text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
                      AIO · {SOURCE_CHANNEL_LABELS[b.sourceChannel]}
                    </span>
                    <span className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
                      {b.protocol}
                    </span>
                    {b.modified ? (
                      <span className="shrink-0 rounded-full bg-amber-50 px-2 py-0.5 font-mono text-[10px] text-amber-700 dark:bg-amber-950/40 dark:text-amber-300 border border-amber-300">
                        配置被外部修改
                      </span>
                    ) : b.stale ? (
                      <span className="shrink-0 rounded-full bg-amber-50 px-2 py-0.5 font-mono text-[10px] text-amber-700 dark:bg-amber-950/40 dark:text-amber-300 border border-amber-300">
                        配置待刷新
                      </span>
                    ) : b.state !== "applied" ? (
                      <span className="shrink-0 rounded-full bg-orange-50 px-2 py-0.5 font-mono text-[10px] text-orange-700 dark:bg-orange-950/30 dark:text-orange-300 border border-orange-300/40">
                        待恢复（可重试更新或撤回）
                      </span>
                    ) : (
                      <span className="shrink-0 rounded-full bg-emerald-50 px-2 py-0.5 font-mono text-[10px] text-emerald-700 dark:bg-emerald-950/30 dark:text-emerald-300">
                        已配置
                      </span>
                    )}
                  </div>
                  <p className="text-[11px] text-muted-foreground mt-0.5">
                    模型：
                    {b.models.map((m) => m.displayName || m.requestModelId).join("、") || "无模型"}
                  </p>
                </div>

                <div className="flex items-center gap-1.5">
                  <Link
                    to={`/providers?cli=${b.sourceChannel}`}
                    className="inline-flex items-center gap-1 rounded-md border border-line bg-surface-panel px-2 py-1 text-[11px] font-medium text-foreground hover:bg-state-hover"
                    title={`转到 ${SOURCE_CHANNEL_LABELS[b.sourceChannel]} 统一渠道来源`}
                  >
                    <ExternalLink className="h-3 w-3 text-muted-foreground" aria-hidden="true" />
                    <span>管理来源渠道</span>
                  </Link>
                  <Button
                    variant="secondary"
                    size="sm"
                    className="h-7 text-xs px-2"
                    onClick={() =>
                      setChannelImportState({
                        sourceChannel: b.sourceChannel,
                        protocol: b.protocol,
                      })
                    }
                  >
                    更新入口
                  </Button>
                  <Button
                    variant="danger"
                    size="sm"
                    className="h-7 text-xs px-2"
                    onClick={() => setWithdrawBinding(b)}
                  >
                    撤回
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {query.isError ? (
        <p role="alert" className="text-xs text-destructive">
          {nativeFailureMessage(query.error)}
        </p>
      ) : null}

      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}

      {notice ? (
        <p
          role="status"
          className="rounded-lg bg-emerald-50/60 dark:bg-emerald-950/20 p-2 text-xs text-emerald-700 dark:text-emerald-400 border border-emerald-300/40"
        >
          {notice}
        </p>
      ) : null}

      {current && !query.isError && current.entries.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          暂无可发布模型。请为已启用的网关上游选择协议并完成模型与工具能力声明；其他目标的旧入口可能也需要更新或撤回。
        </p>
      ) : null}

      <div className="flex flex-wrap items-center gap-2 pt-1">
        <Button
          variant="primary"
          size="sm"
          className="h-9 gap-1.5"
          disabled={query.isFetching || mutation.isPending || query.isError || applyBlocked}
          onClick={() => void preview("apply")}
        >
          <Eye className="h-3.5 w-3.5" aria-hidden="true" />
          预览入口变更
        </Button>
        <Button
          variant="secondary"
          size="sm"
          className="h-9 gap-1.5"
          disabled={
            query.isFetching || mutation.isPending || query.isError || !current?.manifests.length
          }
          onClick={() => void preview("remove")}
        >
          <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
          预览移除入口
        </Button>
        <Button
          variant="secondary"
          size="sm"
          className="h-9 gap-1.5"
          onClick={() => setChannelImportState({})}
        >
          <ArrowDownToLine className="h-3.5 w-3.5" aria-hidden="true" />从 AIO 渠道接入
        </Button>
        <Button
          variant="secondary"
          size="sm"
          className="h-9 gap-1.5"
          disabled={query.isFetching || mutation.isPending}
          onClick={() => {
            setConfirmation(null);
            void query.refetch();
            void channelCatalogQuery.refetch();
            void logs.refetch();
          }}
        >
          <RefreshCw
            className={cn("h-3.5 w-3.5", query.isFetching && "animate-spin")}
            aria-hidden="true"
          />
          刷新状态
        </Button>
      </div>

      <ConfirmDialog
        open={confirmation !== null}
        title={
          confirmation?.action === "remove" ? "确认移除独立 AIO 入口" : "确认发布独立 AIO 入口"
        }
        description="核对目标和协议模型。后端会再次验证文件修订、目录版本和节点所有权。"
        confirming={mutation.isPending}
        confirmingLabel="处理中…"
        confirmLabel={confirmation?.action === "remove" ? "确认移除" : "确认应用"}
        disabled={stale || query.isError || (confirmation?.action === "apply" && applyBlocked)}
        onClose={() => {
          if (!mutation.isPending) setConfirmation(null);
        }}
        onConfirm={() => void confirm()}
      >
        <div className="space-y-3">
          <p className="break-all font-mono text-xs text-muted-foreground">目标：{targetId}</p>
          {stale ? (
            <p role="alert" className="text-xs text-destructive">
              预览已过期，请重新预览。
            </p>
          ) : null}
          {confirmation?.action === "apply" ? (
            <div className="space-y-2">
              {confirmation.preview.entries.map((entry) => (
                <div
                  key={entry.nativeKey}
                  className="rounded-lg border border-border bg-muted/20 p-2.5 space-y-1 text-xs"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-mono font-medium">{entry.nativeKey}</span>
                    <span className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px]">
                      {entry.protocol}
                    </span>
                  </div>
                  <p className="break-all font-mono text-muted-foreground">{entry.baseUrl}</p>
                  <p className="text-foreground">
                    发布模型：{entry.models.map((model) => model.requestModelId).join("、")}
                  </p>
                </div>
              ))}
            </div>
          ) : (
            <div className="space-y-1">
              {confirmation?.preview.manifests.map((manifest) => (
                <p key={manifest.nativeKey} className="text-xs">
                  <span className="font-mono">{manifest.nativeKey}</span>
                  {manifest.modified ? (
                    <span className="text-amber-600 dark:text-amber-400">
                      （已外改，将阻止移除）
                    </span>
                  ) : (
                    ""
                  )}
                </p>
              ))}
            </div>
          )}
        </div>
      </ConfirmDialog>

      {channelImportState !== null ? (
        <NativeChannelImportDialog
          client={client}
          targetId={targetId}
          targetPath={targetPath}
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
            if (!channelCatalogQuery.data) return;
            try {
              await channelMutation.mutateAsync({
                targetId,
                expectedRevision: channelCatalogQuery.data.revision,
                catalogRevision: channelCatalogQuery.data.catalogRevision,
                selections: [],
                removeBindingIds: [withdrawBinding.bindingId],
              });
              setWithdrawBinding(null);
              void channelCatalogQuery.refetch();
              void query.refetch();
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
    </Card>
  );
}
