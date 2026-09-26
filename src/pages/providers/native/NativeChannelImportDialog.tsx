import { useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
  ChevronDown,
  ChevronUp,
  Eye,
  Layers,
  Radio,
  RefreshCw,
  ShieldAlert,
} from "lucide-react";
import type { NativeCliKey } from "../../../constants/clis";
import {
  nativeChannelPreview,
  SOURCE_CHANNEL_LABELS,
  type ChannelLifecycleInput,
  type ChannelPreview,
  type ChannelProvider,
  type ChannelSelection,
  type GatewayProtocol,
  type SourceChannel,
} from "../../../services/nativeChannels";
import {
  useNativeChannelCatalogQuery,
  useNativeChannelMutation,
} from "../../../query/nativeChannels";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { ConfirmDialog } from "../../../ui/ConfirmDialog";
import { Dialog } from "../../../ui/Dialog";
import { NativeChannelModelsDialog } from "./NativeChannelModelsDialog";
import { nativeFailureMessage } from "./nativeProviderDraft";
import { cn } from "../../../utils/cn";

export function NativeChannelImportDialog({
  client,
  targetId,
  targetPath,
  onClose,
  onApplied,
  initialSourceChannel,
  initialProtocol,
}: {
  client: NativeCliKey;
  targetId: string;
  targetPath?: string;
  onClose: () => void;
  onApplied?: () => void;
  initialSourceChannel?: SourceChannel;
  initialProtocol?: GatewayProtocol;
}) {
  const query = useNativeChannelCatalogQuery(targetId);
  const mutation = useNativeChannelMutation();

  const [selectedSources, setSelectedSources] = useState<Record<string, boolean>>(() => {
    if (initialSourceChannel && initialProtocol) {
      return { [`${initialSourceChannel}:${initialProtocol}`]: true };
    }
    return {};
  });

  const [selectedModels, setSelectedModels] = useState<Record<string, string[]>>({});
  const [expandedDetails, setExpandedDetails] = useState<Record<string, boolean>>({});
  const [supplementTarget, setSupplementTarget] = useState<{
    provider: ChannelProvider;
    protocol: GatewayProtocol;
  } | null>(null);

  const [preview, setPreview] = useState<{
    data: ChannelPreview;
    input: ChannelLifecycleInput;
  } | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  // Active target tracker to guard against target switching while requests are in flight
  const activeTargetIdRef = useRef(targetId);
  activeTargetIdRef.current = targetId;

  // When targetId changes, clear any existing preview or error to prevent applying old preview to new target
  useEffect(() => {
    setPreview(null);
    setPreviewError(null);
    setPreviewLoading(false);
    setSupplementTarget(null);
    setSelectedSources(
      initialSourceChannel && initialProtocol
        ? { [`${initialSourceChannel}:${initialProtocol}`]: true }
        : {}
    );
    setSelectedModels({});
  }, [targetId, initialSourceChannel, initialProtocol]);

  const catalog = query.data;

  const sourceKey = (sourceChannel: SourceChannel, protocol: GatewayProtocol) =>
    `${sourceChannel}:${protocol}`;

  // Default preserved models: when catalog loads, ensure selected sources have their models initialized.
  // If an existing binding exists, preserve its already published models!
  useEffect(() => {
    if (!catalog) return;
    setSelectedModels((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const [key, isSelected] of Object.entries(selectedSources)) {
        if (isSelected && !next[key]) {
          const [sChannel, protocol] = key.split(":") as [SourceChannel, GatewayProtocol];
          const existingBinding = catalog.bindings.find(
            (b) => b.sourceChannel === sChannel && b.protocol === protocol
          );
          const src = catalog.sources.find(
            (s) => s.sourceChannel === sChannel && s.protocol === protocol
          );
          if (existingBinding && existingBinding.models.length > 0) {
            // Default to preserving the already published models of the existing binding
            next[key] = existingBinding.models.map((m) => m.requestModelId);
            changed = true;
          } else if (src && src.models.length > 0) {
            // New binding: default to all currently available source models
            next[key] = src.models.map((m) => m.requestModelId);
            changed = true;
          }
        }
      }
      return changed ? next : prev;
    });
  }, [catalog, selectedSources]);

  // Stale detection: revision or catalogRevision or target mismatch
  const stale = Boolean(
    preview &&
    catalog &&
    (preview.data.revision !== catalog.revision ||
      preview.data.catalogRevision !== catalog.catalogRevision ||
      preview.data.targetId !== targetId ||
      preview.input.targetId !== targetId)
  );

  function toggleSource(sChannel: SourceChannel, protocol: GatewayProtocol) {
    const key = sourceKey(sChannel, protocol);
    const nextSelected = !selectedSources[key];
    setSelectedSources((prev) => ({ ...prev, [key]: nextSelected }));
    if (nextSelected && !selectedModels[key] && catalog) {
      const existingBinding = catalog.bindings.find(
        (b) => b.sourceChannel === sChannel && b.protocol === protocol
      );
      const src = catalog.sources.find(
        (s) => s.sourceChannel === sChannel && s.protocol === protocol
      );
      if (existingBinding && existingBinding.models.length > 0) {
        // Preserves already published models when updating/checking
        setSelectedModels((prev) => ({
          ...prev,
          [key]: existingBinding.models.map((m) => m.requestModelId),
        }));
      } else if (src && src.models.length > 0) {
        setSelectedModels((prev) => ({
          ...prev,
          [key]: src.models.map((m) => m.requestModelId),
        }));
      }
    }
  }

  function toggleModel(key: string, modelId: string) {
    setSelectedModels((prev) => {
      const current = prev[key] ?? [];
      const next = current.includes(modelId)
        ? current.filter((id) => id !== modelId)
        : [...current, modelId];
      return { ...prev, [key]: next };
    });
  }

  function selectAllModels(key: string, allIds: string[]) {
    setSelectedModels((prev) => ({ ...prev, [key]: allIds }));
  }

  function clearModels(key: string) {
    setSelectedModels((prev) => ({ ...prev, [key]: [] }));
  }

  const hasValidSelections = Object.entries(selectedSources).some(([key, isSelected]) => {
    if (!isSelected) return false;
    const models = selectedModels[key];
    return models && models.length > 0;
  });

  async function handlePreview() {
    if (!catalog || !hasValidSelections) return;
    setPreviewError(null);
    setPreviewLoading(true);

    const requestTargetId = targetId;

    const selections: ChannelSelection[] = [];
    for (const src of catalog.sources) {
      const key = sourceKey(src.sourceChannel, src.protocol);
      if (selectedSources[key]) {
        const modelIds = selectedModels[key] ?? [];
        if (modelIds.length > 0) {
          selections.push({
            sourceChannel: src.sourceChannel,
            protocol: src.protocol,
            modelIds,
          });
        }
      }
    }

    // Incremental update: omitted sources are retained, removeBindingIds is strictly []
    const input: ChannelLifecycleInput = {
      targetId,
      expectedRevision: catalog.revision,
      catalogRevision: catalog.catalogRevision,
      selections,
      removeBindingIds: [],
    };

    try {
      const result = await nativeChannelPreview(input);
      // Guard against target switching while preview was in flight
      if (activeTargetIdRef.current !== requestTargetId) {
        return;
      }
      setPreview({ data: result, input });
    } catch (cause) {
      if (activeTargetIdRef.current === requestTargetId) {
        setPreviewError(nativeFailureMessage(cause));
      }
    } finally {
      if (activeTargetIdRef.current === requestTargetId) {
        setPreviewLoading(false);
      }
    }
  }

  async function handleConfirmApply() {
    if (!preview || stale || mutation.isPending) return;

    // Strict guard: ensure preview target matches current target
    if (preview.input.targetId !== targetId || preview.data.targetId !== targetId) {
      setPreviewError("目标已在外部切换，旧预览不能应用到新目标。");
      setPreview(null);
      return;
    }

    try {
      await mutation.mutateAsync(preview.input);
      if (activeTargetIdRef.current !== preview.input.targetId) return;
      setPreview(null);
      onApplied?.();
      onClose();
    } catch (cause) {
      if (activeTargetIdRef.current !== preview.input.targetId) return;
      setPreviewError(nativeFailureMessage(cause));
    }
  }

  return (
    <>
      <Dialog
        open={preview === null}
        title="从 AIO 聚合网关接入"
        description={`${client === "pi" ? "Pi" : "OMP"} · 选择渠道与模型，上游选择、重试与限额由 AIO 统一管理。`}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        className="max-w-4xl"
      >
        <div className="space-y-4">
          <div className="rounded-xl border border-accent/20 bg-accent/5 px-3 py-2.5 text-xs">
            <p className="font-medium text-foreground">
              {client === "pi" ? "Pi" : "OMP"} → AIO 聚合网关 → 渠道可用上游
            </p>
            <p className="mt-1 text-muted-foreground">
              每个渠道与协议生成一个 AIO 本地入口，请求时按该渠道的当前规则调度上游。
            </p>
          </div>
          {/* 目标与监听器状态条 */}
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 rounded-xl border border-border/70 bg-surface-panel/40 p-3 text-xs">
            <div className="flex items-center gap-2 min-w-0">
              <Layers className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span
                className="font-mono text-foreground font-medium truncate"
                title={targetPath || targetId}
              >
                {targetPath || targetId}
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <span className="inline-flex items-center gap-1.5 rounded-full border border-border/60 bg-surface-panel/60 px-2.5 py-0.5 text-muted-foreground">
                <Radio className="h-3 w-3 text-muted-foreground" aria-hidden="true" />
                <span>
                  AIO 网关监听：
                  {query.isError
                    ? "未知"
                    : catalog
                      ? catalog.listenerReady
                        ? "就绪"
                        : "未就绪"
                      : "读取中…"}
                </span>
              </span>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="h-7 text-xs"
                disabled={query.isFetching}
                onClick={() => void query.refetch()}
              >
                <RefreshCw
                  className={cn("h-3 w-3 mr-1", query.isFetching && "animate-spin")}
                  aria-hidden="true"
                />
                刷新目录
              </Button>
            </div>
          </div>

          {!catalog?.listenerReady && !query.isPending && !query.isError ? (
            <div
              role="alert"
              className="flex items-start gap-2.5 rounded-xl border border-amber-400/60 bg-amber-50/50 p-3 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
            >
              <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5 text-amber-600 dark:text-amber-400" />
              <div className="space-y-0.5">
                <div className="font-medium">AIO 本地网关监听器尚未就绪。</div>
                <div className="text-muted-foreground">
                  接入配置写入后，需在 AIO 启动网关服务方可正常转发请求至各渠道上游。
                </div>
              </div>
            </div>
          ) : null}

          {query.isError ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-xl border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
            >
              <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
              <span>{nativeFailureMessage(query.error)}</span>
            </div>
          ) : null}

          {previewError ? (
            <div
              role="alert"
              className="flex items-center gap-2 rounded-xl border border-destructive/30 bg-destructive/5 p-3 text-xs text-destructive"
            >
              <AlertCircle className="h-4 w-4 shrink-0" aria-hidden="true" />
              <span>{previewError}</span>
            </div>
          ) : null}

          {/* 渠道来源卡片列表 */}
          <div className="space-y-3">
            <h3 className="text-xs font-semibold text-foreground uppercase tracking-wider">
              可选统一渠道来源
            </h3>

            {query.isPending ? (
              <p role="status" className="text-sm text-muted-foreground py-4 text-center">
                读取 AIO 统一渠道目录…
              </p>
            ) : null}

            {catalog?.sources.map((source) => {
              const key = sourceKey(source.sourceChannel, source.protocol);
              const isSelected = Boolean(selectedSources[key]);
              const existingBinding = catalog.bindings.find(
                (b) => b.sourceChannel === source.sourceChannel && b.protocol === source.protocol
              );
              const activeProviders = source.providers.filter((p) => !p.blockedReason);
              const isBlocked = Boolean(source.blockedReason) || source.providers.length === 0;
              const hasModels = source.models.length > 0;
              const currentModelIds = selectedModels[key] ?? [];
              const isExpanded = Boolean(expandedDetails[key]);

              return (
                <Card
                  key={key}
                  padding="sm"
                  aria-label={`${SOURCE_CHANNEL_LABELS[source.sourceChannel]} 渠道`}
                  className={cn(
                    "rounded-xl border transition-all space-y-3",
                    isSelected
                      ? "border-accent bg-accent/5 ring-1 ring-accent/30"
                      : "border-border bg-surface-panel/20 hover:border-border/90"
                  )}
                >
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <div className="flex items-center gap-3">
                      <input
                        type="checkbox"
                        id={`source-check-${key}`}
                        checked={isSelected}
                        disabled={isBlocked}
                        onChange={() => toggleSource(source.sourceChannel, source.protocol)}
                        className="h-4 w-4 rounded border-border text-accent focus:ring-accent"
                        aria-label={`选择 ${SOURCE_CHANNEL_LABELS[source.sourceChannel]} 渠道`}
                      />
                      <div>
                        <div className="flex items-center gap-2 flex-wrap">
                          <label
                            htmlFor={`source-check-${key}`}
                            className={cn(
                              "font-semibold text-sm cursor-pointer",
                              isBlocked && "cursor-not-allowed opacity-60"
                            )}
                          >
                            AIO · {SOURCE_CHANNEL_LABELS[source.sourceChannel]}
                          </label>
                          <span className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
                            {source.protocol}
                          </span>
                          {existingBinding ? (
                            <span
                              className={cn(
                                "shrink-0 rounded-full px-2 py-0.5 font-mono text-[10px] border",
                                existingBinding.state !== "applied"
                                  ? "bg-orange-50 text-orange-700 dark:bg-orange-950/40 dark:text-orange-300 border-orange-300/40"
                                  : "bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300 border-emerald-300/40"
                              )}
                            >
                              {existingBinding.state !== "applied"
                                ? `待恢复 (${existingBinding.nativeKey})`
                                : `已接入 (${existingBinding.nativeKey})`}
                            </span>
                          ) : null}
                        </div>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          调度池：{activeProviders.length} / {source.providers.length}{" "}
                          个候选通过准入
                          {existingBinding?.modified
                            ? " · 外部配置已修改"
                            : existingBinding?.stale
                              ? " · 配置待更新"
                              : ""}
                        </p>
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        className="h-7 px-2 text-[11px] text-muted-foreground gap-1"
                        aria-expanded={isExpanded}
                        onClick={() =>
                          setExpandedDetails((prev) => ({ ...prev, [key]: !prev[key] }))
                        }
                      >
                        <span>AIO 调度池</span>
                        {isExpanded ? (
                          <ChevronUp className="h-3 w-3" />
                        ) : (
                          <ChevronDown className="h-3 w-3" />
                        )}
                      </Button>
                    </div>
                  </div>

                  {/* 来源级阻塞提示 */}
                  {source.blockedReason ? (
                    <div
                      role="alert"
                      className="flex items-center gap-2 rounded-lg border border-amber-400/50 bg-amber-50/40 p-2.5 text-xs text-amber-900 dark:border-amber-700/50 dark:bg-amber-950/20 dark:text-amber-300"
                    >
                      <ShieldAlert className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                      <span>{source.blockedReason}</span>
                    </div>
                  ) : null}

                  {/* AIO 调度池与模型资料折叠区 */}
                  {isExpanded ? (
                    <div className="space-y-1.5 rounded-lg border border-border/60 bg-muted/20 p-2.5 text-xs">
                      <div className="font-medium text-foreground text-[11px] mb-1">
                        渠道候选池与准入状态
                      </div>
                      <p className="text-[11px] leading-relaxed text-muted-foreground">
                        AIO 会根据请求的模型、协议、额度与健康状态筛选上游，再按渠道规则选源和重试。
                        模型资料用于匹配上游能力；接入始终使用同一个渠道聚合入口。
                      </p>
                      {source.providers.map((p) => (
                        <div
                          key={p.providerUuid}
                          className="flex flex-wrap items-center justify-between gap-1.5 py-1 border-b border-border/40 last:border-0"
                        >
                          <div className="flex items-center gap-2">
                            <span className="font-medium">{p.name}</span>
                            <span className="text-muted-foreground font-mono text-[10px]">
                              ({p.authMode})
                            </span>
                            {p.blockedReason ? (
                              <span className="text-amber-700 dark:text-amber-400 font-medium">
                                [{p.blockedReason}]
                              </span>
                            ) : (
                              <span className="text-emerald-700 dark:text-emerald-400">
                                [准入通过]
                              </span>
                            )}
                          </div>
                          {!p.blockedReason ? (
                            <Button
                              type="button"
                              size="sm"
                              variant="secondary"
                              className="h-6 px-2 text-[10px]"
                              onClick={() =>
                                setSupplementTarget({ provider: p, protocol: source.protocol })
                              }
                            >
                              维护模型资料
                            </Button>
                          ) : null}
                        </div>
                      ))}
                      {source.sourceChannel === "gemini" ? (
                        <div className="mt-2 text-[11px] text-muted-foreground space-y-0.5">
                          <p>
                            • 标准 API 凭证是首期基础通路；企业 Code Assist OAuth
                            需验证授权与适配；旧个人免费/Pro/Ultra 服务已于 2026-06-18 停止。
                          </p>
                          <p>
                            • Antigravity（反重力）当前未适配为 AIO 统一渠道，不可作为可选来源。
                          </p>
                        </div>
                      ) : null}
                    </div>
                  ) : null}

                  {/* 模型选择区域 */}
                  {isSelected ? (
                    <div className="space-y-2 border-t border-border/60 pt-3">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-foreground">
                          通过此渠道使用的模型（至少选 1 项）：
                        </span>
                        {hasModels ? (
                          <div className="flex items-center gap-2">
                            <button
                              type="button"
                              className="text-[11px] text-accent hover:underline"
                              onClick={() =>
                                selectAllModels(
                                  key,
                                  source.models.map((m) => m.requestModelId)
                                )
                              }
                            >
                              全选
                            </button>
                            <span className="text-muted-foreground text-[10px]">·</span>
                            <button
                              type="button"
                              className="text-[11px] text-muted-foreground hover:underline"
                              onClick={() => clearModels(key)}
                            >
                              清空
                            </button>
                          </div>
                        ) : null}
                      </div>

                      {hasModels ? (
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                          {source.models.map((m) => {
                            const isModelChecked = currentModelIds.includes(m.requestModelId);
                            return (
                              <label
                                key={m.requestModelId}
                                className={cn(
                                  "flex items-start gap-2.5 p-2 rounded-lg border cursor-pointer text-xs transition-colors",
                                  isModelChecked
                                    ? "border-accent bg-accent/10 text-foreground"
                                    : "border-border/60 hover:bg-surface-panel/40 text-muted-foreground"
                                )}
                              >
                                <input
                                  type="checkbox"
                                  checked={isModelChecked}
                                  onChange={() => toggleModel(key, m.requestModelId)}
                                  className="h-3.5 w-3.5 mt-0.5 rounded border-border text-accent focus:ring-accent"
                                />
                                <div className="min-w-0 flex-1">
                                  <div className="font-mono font-medium truncate">
                                    {m.displayName || m.requestModelId}
                                  </div>
                                  <div className="text-[10px] text-muted-foreground">
                                    ID: {m.requestModelId} · ctx: {m.contextWindow} · max:{" "}
                                    {m.maxTokens}
                                    {m.supportsTools ? " · 工具✓" : ""}
                                    {m.reasoning ? " · 思考✓" : ""}
                                  </div>
                                </div>
                              </label>
                            );
                          })}
                        </div>
                      ) : (
                        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">
                          <span>缺少完整模型与工具能力声明，当前无法直接发布。</span>
                          {activeProviders[0] ? (
                            <Button
                              type="button"
                              size="sm"
                              variant="secondary"
                              className="h-7 text-xs"
                              onClick={() =>
                                setSupplementTarget({
                                  provider: activeProviders[0],
                                  protocol: source.protocol,
                                })
                              }
                            >
                              在流程内补齐模型声明
                            </Button>
                          ) : null}
                        </div>
                      )}
                    </div>
                  ) : null}
                </Card>
              );
            })}
          </div>

          {/* 底部按钮栏 */}
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 border-t border-border pt-4">
            <p className="text-xs text-muted-foreground">
              接入后生成独立的 AIO 聚合网关供应商入口，保留已有原生供应商与默认模型。
            </p>

            <div className="flex items-center justify-end gap-2">
              <Button type="button" variant="secondary" onClick={onClose}>
                取消
              </Button>
              <Button
                type="button"
                variant="primary"
                disabled={!hasValidSelections || previewLoading || query.isPending}
                onClick={() => void handlePreview()}
                className="gap-1.5"
              >
                <Eye className="h-3.5 w-3.5" aria-hidden="true" />
                {previewLoading ? "生成预览中…" : "预览入口变更"}
              </Button>
            </div>
          </div>
        </div>
      </Dialog>

      {/* 步骤 2：预览与确认弹窗 */}
      {preview ? (
        <ConfirmDialog
          open
          title="确认发布统一渠道入口"
          description="核对将写入原生目标文件的 AIO 聚合网关地址与模型清单。取消不产生任何文件修改。"
          confirming={mutation.isPending}
          confirmingLabel="正在写入原生配置…"
          confirmLabel="确认写入并接入"
          disabled={stale || mutation.isPending}
          onClose={() => {
            if (!mutation.isPending) setPreview(null);
          }}
          onConfirm={() => void handleConfirmApply()}
        >
          <div className="space-y-3">
            <div className="rounded-lg border border-border bg-muted/20 p-2.5 text-xs space-y-1">
              <p className="font-mono text-muted-foreground break-all">
                目标文件：{targetPath || preview.data.targetId}
              </p>
              <p className="text-muted-foreground text-[11px]">确认时会重新核对配置与来源版本。</p>
            </div>

            {stale ? (
              <div
                role="alert"
                className="flex items-center gap-2 rounded-lg border border-amber-400/60 bg-amber-50/50 p-2.5 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/20 dark:text-amber-300"
              >
                <AlertTriangle className="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                <span>目标配置或统一渠道目录已在外部发生变动，请返回重新预览。</span>
              </div>
            ) : null}

            <div className="space-y-2 max-h-72 overflow-y-auto pr-1">
              <div className="text-xs font-semibold text-foreground">将生成 / 更新的节点：</div>
              {preview.data.entries.map((entry) => (
                <div
                  key={entry.nativeKey}
                  className="rounded-lg border border-border/80 bg-surface-panel/40 p-3 space-y-1.5 text-xs"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-mono font-bold text-foreground">{entry.nativeKey}</span>
                    <span className="shrink-0 rounded-full bg-secondary px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
                      {entry.protocol}
                    </span>
                  </div>
                  <p className="font-mono text-[11px] text-muted-foreground break-all">
                    AIO 网关地址：{entry.baseUrl}
                  </p>
                  <div className="text-foreground">
                    <span className="text-muted-foreground">
                      发布模型（{entry.models.length} 个）：
                    </span>
                    <span className="font-mono font-medium">
                      {entry.models.map((m) => m.displayName || m.requestModelId).join("、")}
                    </span>
                  </div>
                </div>
              ))}

              {preview.data.removedNativeKeys.length > 0 ? (
                <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-2.5 text-xs text-destructive space-y-1">
                  <div className="font-medium">将撤回移除的原生节点：</div>
                  <p className="font-mono">{preview.data.removedNativeKeys.join(", ")}</p>
                </div>
              ) : null}
            </div>
          </div>
        </ConfirmDialog>
      ) : null}

      {/* 补齐模型声明弹窗 */}
      {supplementTarget ? (
        <NativeChannelModelsDialog
          client={client}
          targetId={targetId}
          provider={supplementTarget.provider}
          protocol={supplementTarget.protocol}
          onClose={() => setSupplementTarget(null)}
          onSaved={() => {
            setSupplementTarget(null);
            void query.refetch();
          }}
        />
      ) : null}
    </>
  );
}
