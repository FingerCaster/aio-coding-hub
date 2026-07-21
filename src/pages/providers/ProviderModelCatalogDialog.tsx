import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  FilePlus2,
  Library,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import {
  useCodexManagedProfileCreateMutation,
  useCodexManagedProfileDeleteMutation,
  useCodexManagedProfilesQuery,
  useProviderModelCatalogQuery,
  useProviderModelCapabilitiesUpdateMutation,
  useProviderModelManualDeleteMutation,
  useProviderModelManualUpsertMutation,
  useProviderModelsRefreshMutation,
} from "../../query/providerModels";
import { useCliProxyStatusAllQuery } from "../../query/cliProxy";
import type { CodexManagedProfile } from "../../services/providers/codexManagedProfiles";
import { isCanonicalUuidV4 } from "../../services/providers/uuid";
import {
  formatProviderModelFeatureError,
  formatProviderModelDiscoveryError,
  MODEL_CONTEXT_WINDOW_MAX_TOKENS,
  MODEL_CONTEXT_WINDOW_MIN_TOKENS,
  PROVIDER_MODEL_REASONING_EFFORTS,
  type ProviderModel,
  type ProviderModelReasoningEffort,
} from "../../services/providers/providerModels";
import type { ProviderSummary } from "../../services/providers/providers";
import { Button } from "../../ui/Button";
import { ConfirmDialog } from "../../ui/ConfirmDialog";
import { Dialog } from "../../ui/Dialog";
import { EmptyState } from "../../ui/EmptyState";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { Spinner } from "../../ui/Spinner";
import { Switch } from "../../ui/Switch";
import { cn } from "../../utils/cn";
import { formatUnixSeconds } from "../../utils/formatters";

type ProviderModelCatalogDialogProps = {
  open: boolean;
  provider: ProviderSummary | null;
  onOpenChange: (open: boolean) => void;
};

export function suggestCodexProfileName(remoteModelId: string) {
  const normalized = remoteModelId
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
  if (!normalized || !/^[a-z0-9]/.test(normalized)) return "model-profile";
  return isCanonicalUuidV4(normalized) ? `profile-${normalized}`.slice(0, 64) : normalized;
}

function fileStatusLabel(status: CodexManagedProfile["fileStatus"]) {
  if (status === "missing") return "文件缺失";
  if (status === "modified") return "文件已修改";
  return "已受管";
}

function fileStatusClassName(status: CodexManagedProfile["fileStatus"]) {
  if (status === "missing") {
    return "bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300";
  }
  if (status === "modified") {
    return "bg-rose-50 text-rose-700 dark:bg-rose-900/30 dark:text-rose-300";
  }
  return "bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300";
}

function formatContextWindow(contextWindow: number | null) {
  return contextWindow == null ? "未知上下文" : `上下文 ${contextWindow.toLocaleString()} tokens`;
}

function modelCapabilitySummary(model: ProviderModel) {
  if (!model.capabilitiesConfigured) return "能力未配置";
  const reasoning =
    model.supportedReasoningEfforts.length > 0
      ? `推理 ${model.supportedReasoningEfforts.join(" / ")} · 默认 ${model.defaultReasoningEffort}`
      : "不发送推理强度";
  return `${formatContextWindow(model.contextWindow)} · ${reasoning}`;
}

export function ProviderModelCatalogDialog({
  open,
  provider,
  onOpenChange,
}: ProviderModelCatalogDialogProps) {
  const providerId = provider?.id ?? null;
  const providerUuid = provider?.provider_uuid ?? null;
  const catalogQuery = useProviderModelCatalogQuery(providerId, providerUuid, { enabled: open });
  const profilesQuery = useCodexManagedProfilesQuery({ enabled: open });
  const cliProxyStatusQuery = useCliProxyStatusAllQuery({ enabled: open });
  const refreshMutation = useProviderModelsRefreshMutation();
  const manualUpsertMutation = useProviderModelManualUpsertMutation();
  const manualDeleteMutation = useProviderModelManualDeleteMutation();
  const capabilitiesUpdateMutation = useProviderModelCapabilitiesUpdateMutation();
  const profileCreateMutation = useCodexManagedProfileCreateMutation();
  const profileDeleteMutation = useCodexManagedProfileDeleteMutation();
  const [search, setSearch] = useState("");
  const [manualModelId, setManualModelId] = useState("");
  const [profileModel, setProfileModel] = useState<ProviderModel | null>(null);
  const [profileName, setProfileName] = useState("");
  const [deleteProfileTarget, setDeleteProfileTarget] = useState<CodexManagedProfile | null>(null);
  const [capabilityModel, setCapabilityModel] = useState<ProviderModel | null>(null);
  const [reasoningEnabled, setReasoningEnabled] = useState(false);
  const [selectedReasoningEfforts, setSelectedReasoningEfforts] = useState<
    ProviderModelReasoningEffort[]
  >([]);
  const [defaultReasoningEffort, setDefaultReasoningEffort] =
    useState<ProviderModelReasoningEffort | null>(null);
  const [contextWindowText, setContextWindowText] = useState("");

  useEffect(() => {
    if (!open) return;
    setSearch("");
    setManualModelId("");
    setProfileModel(null);
    setProfileName("");
    setDeleteProfileTarget(null);
    setCapabilityModel(null);
  }, [open, providerId, providerUuid]);

  const catalog = catalogQuery.data ?? null;
  const providerProfiles = useMemo(
    () =>
      (profilesQuery.data ?? []).filter(
        (profile) => profile.providerId === providerId && profile.providerUuid === providerUuid
      ),
    [profilesQuery.data, providerId, providerUuid]
  );
  const profilesByModel = useMemo(() => {
    const next = new Map<string, CodexManagedProfile[]>();
    for (const profile of providerProfiles) {
      const profiles = next.get(profile.modelUuid) ?? [];
      profiles.push(profile);
      next.set(profile.modelUuid, profiles);
    }
    return next;
  }, [providerProfiles]);
  const visibleModels = useMemo(() => {
    const normalizedSearch = search.trim().toLowerCase();
    const rows = catalog?.models ?? [];
    return rows
      .filter((model) =>
        normalizedSearch ? model.remoteModelId.toLowerCase().includes(normalizedSearch) : true
      )
      .slice()
      .sort((left, right) => {
        if (left.source !== right.source) return left.source === "manual" ? -1 : 1;
        return left.remoteModelId.localeCompare(right.remoteModelId);
      });
  }, [catalog?.models, search]);
  const codexProxyStatus = (cliProxyStatusQuery.data ?? []).find(
    (status) => status.cli_key === "codex"
  );
  const codexProxyReady =
    codexProxyStatus?.enabled === true && codexProxyStatus.applied_to_current_gateway !== false;

  const busy =
    refreshMutation.isPending ||
    manualUpsertMutation.isPending ||
    manualDeleteMutation.isPending ||
    profileCreateMutation.isPending ||
    profileDeleteMutation.isPending ||
    capabilitiesUpdateMutation.isPending;

  const parsedContextWindow = useMemo(() => {
    const value = contextWindowText.trim();
    if (!value) return null;
    const parsed = Number(value);
    if (
      !Number.isSafeInteger(parsed) ||
      parsed < MODEL_CONTEXT_WINDOW_MIN_TOKENS ||
      parsed > MODEL_CONTEXT_WINDOW_MAX_TOKENS
    ) {
      return undefined;
    }
    return parsed;
  }, [contextWindowText]);
  const capabilityFormValid =
    parsedContextWindow !== undefined &&
    (!reasoningEnabled || (selectedReasoningEfforts.length > 0 && defaultReasoningEffort != null));

  function openCapabilityConfig(model: ProviderModel) {
    const supported = model.capabilitiesConfigured ? model.supportedReasoningEfforts : [];
    setCapabilityModel(model);
    setReasoningEnabled(supported.length > 0);
    setSelectedReasoningEfforts(supported);
    setDefaultReasoningEffort(model.capabilitiesConfigured ? model.defaultReasoningEffort : null);
    setContextWindowText(
      model.capabilitiesConfigured && model.contextWindow != null ? String(model.contextWindow) : ""
    );
  }

  function setReasoningConfigurationEnabled(enabled: boolean) {
    setReasoningEnabled(enabled);
    if (enabled) {
      setSelectedReasoningEfforts(["medium"]);
      setDefaultReasoningEffort("medium");
    } else {
      setSelectedReasoningEfforts([]);
      setDefaultReasoningEffort(null);
    }
  }

  function toggleReasoningEffort(effort: ProviderModelReasoningEffort, checked: boolean) {
    const selected = checked
      ? PROVIDER_MODEL_REASONING_EFFORTS.filter(
          (candidate) => candidate === effort || selectedReasoningEfforts.includes(candidate)
        )
      : selectedReasoningEfforts.filter((candidate) => candidate !== effort);
    setSelectedReasoningEfforts(selected);
    if (selected.length === 0) {
      setDefaultReasoningEffort(null);
    } else if (defaultReasoningEffort == null || !selected.includes(defaultReasoningEffort)) {
      setDefaultReasoningEffort(selected[0]);
    }
  }

  async function refreshModels() {
    if (providerId == null || providerUuid == null) return;
    try {
      const next = await refreshMutation.mutateAsync({ providerId, providerUuid });
      const errorMessage = formatProviderModelDiscoveryError(next.lastErrorCode);
      if (errorMessage) {
        toast(`模型获取失败：${errorMessage}，已保留历史目录`);
      } else {
        toast(`已获取 ${next.models.length} 个模型`);
      }
    } catch (error) {
      await catalogQuery.refetch();
      toast(`模型获取失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  async function addManualModel() {
    if (providerId == null || providerUuid == null || !manualModelId.trim()) return;
    try {
      await manualUpsertMutation.mutateAsync({
        providerId,
        providerUuid,
        remoteModelId: manualModelId,
      });
      setManualModelId("");
      toast("手工模型已保存");
    } catch (error) {
      toast(`添加模型失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  async function deleteManualModel(model: ProviderModel) {
    if (providerId == null || providerUuid == null) return;
    try {
      await manualDeleteMutation.mutateAsync({
        providerId,
        providerUuid,
        modelUuid: model.modelUuid,
      });
      toast("手工模型已删除");
    } catch (error) {
      toast(`删除模型失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  async function createProfile() {
    if (!profileModel || !profileName.trim() || providerId == null || providerUuid == null) return;
    try {
      const created = await profileCreateMutation.mutateAsync({
        profileName,
        modelUuid: profileModel.modelUuid,
        providerId,
        providerUuid,
      });
      setProfileModel(null);
      setProfileName("");
      toast(
        `Profile ${created.profileName} 已创建；请新建或重启 Codex 会话，然后通过 /model 选择 ${created.canonicalModel}`
      );
    } catch (error) {
      toast(`创建 Profile 失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  async function saveCapabilities() {
    if (
      !capabilityModel ||
      providerId == null ||
      providerUuid == null ||
      !capabilityFormValid ||
      parsedContextWindow === undefined
    ) {
      return;
    }
    const shouldPromptRestart =
      profilesQuery.error != null ||
      (profilesByModel.get(capabilityModel.modelUuid) ?? []).length > 0;
    try {
      await capabilitiesUpdateMutation.mutateAsync({
        providerId,
        providerUuid,
        modelUuid: capabilityModel.modelUuid,
        supportedReasoningEfforts: reasoningEnabled ? selectedReasoningEfforts : [],
        defaultReasoningEffort: reasoningEnabled ? defaultReasoningEffort : null,
        contextWindow: parsedContextWindow,
      });
      setCapabilityModel(null);
      toast(
        shouldPromptRestart ? "模型能力已保存；请新建或重启 Codex 会话后生效" : "模型能力已保存"
      );
    } catch (error) {
      toast(`保存模型能力失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  async function deleteProfile() {
    const target = deleteProfileTarget;
    if (!target) return;
    try {
      const result = await profileDeleteMutation.mutateAsync({
        profileUuid: target.profileUuid,
        providerId: target.providerId,
        providerUuid: target.providerUuid,
      });
      setDeleteProfileTarget(null);
      toast(
        result.externalFilePreserved
          ? `Profile ${target.profileName} 已解除管理；外部修改的文件已保留`
          : `Profile ${target.profileName} 已删除`
      );
    } catch (error) {
      toast(`删除 Profile 失败：${formatProviderModelFeatureError(error)}`);
    }
  }

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (!nextOpen && busy) return;
          onOpenChange(nextOpen);
        }}
        title={provider ? `${provider.name} · 模型目录` : "模型目录"}
        className="h-[min(88vh,760px)] max-w-5xl"
      >
        <div className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-3">
            <div className="min-w-0 text-sm text-muted-foreground">
              {catalog?.lastSuccessAt != null
                ? `上次成功：${formatUnixSeconds(catalog.lastSuccessAt)}`
                : "尚未成功获取远端模型"}
              {catalog ? ` · ${catalog.models.length} 个模型` : ""}
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void refreshModels()}
              disabled={providerId == null || providerUuid == null || busy}
              title="刷新模型"
            >
              <RefreshCw
                className={cn("h-4 w-4", refreshMutation.isPending && "animate-spin")}
                aria-hidden="true"
              />
              {refreshMutation.isPending ? "获取中…" : "刷新模型"}
            </Button>
          </div>

          {catalog?.stale || catalog?.lastErrorCode ? (
            <div className="flex items-start gap-2 border-l-2 border-amber-400 bg-amber-50/70 px-3 py-2 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>
                {formatProviderModelDiscoveryError(catalog.lastErrorCode) ??
                  "供应商连接信息已变化，当前发现结果可能已过期"}
                ；历史目录和手工模型仍可使用。
              </span>
            </div>
          ) : null}

          {cliProxyStatusQuery.error ? (
            <div className="flex items-start gap-2 border-l-2 border-amber-400 bg-amber-50/70 px-3 py-2 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>暂时无法确认 Codex CLI 代理状态；创建 Profile 前请先确认代理已启用。</span>
            </div>
          ) : !codexProxyStatus?.enabled ? (
            <div className="flex items-start gap-2 border-l-2 border-amber-400 bg-amber-50/70 px-3 py-2 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>
                请先在 CLI 代理中开启 Codex 代理，AIO 才能把 Profile 加入 Codex 的 /model 目录。
              </span>
            </div>
          ) : codexProxyStatus.applied_to_current_gateway === false ? (
            <div className="flex items-start gap-2 border-l-2 border-amber-400 bg-amber-50/70 px-3 py-2 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-200">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>Codex 代理配置尚未应用到当前网关，请先同步代理或启动网关。</span>
            </div>
          ) : (
            <div className="border-l-2 border-emerald-500 bg-emerald-50/70 px-3 py-2 text-sm text-emerald-800 dark:bg-emerald-900/20 dark:text-emerald-200">
              创建 Profile 后，请新建或重启 Codex 会话，再通过 /model 选择对应的 aio/ 条目。
            </div>
          )}

          <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
            <label className="relative block min-w-0">
              <Search
                className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
                aria-hidden="true"
              />
              <Input
                value={search}
                onChange={(event) => setSearch(event.currentTarget.value)}
                placeholder="搜索模型"
                aria-label="搜索模型"
                className="pl-9"
              />
            </label>
            <div className="flex min-w-0 flex-col gap-2 sm:flex-row">
              <Input
                value={manualModelId}
                onChange={(event) => setManualModelId(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void addManualModel();
                }}
                placeholder="手工输入远端模型 ID"
                aria-label="手工输入远端模型 ID"
                maxLength={256}
                disabled={busy}
                className="min-w-0 flex-1"
              />
              <Button
                variant="secondary"
                onClick={() => void addManualModel()}
                disabled={!manualModelId.trim() || busy}
                className="w-full shrink-0 sm:w-auto"
                title="添加手工模型"
              >
                <Plus className="h-4 w-4" aria-hidden="true" />
                添加
              </Button>
            </div>
          </div>

          {catalogQuery.isLoading || profilesQuery.isLoading ? (
            <div className="flex min-h-48 items-center justify-center gap-2 text-sm text-muted-foreground">
              <Spinner size="sm" />
              加载模型目录…
            </div>
          ) : catalogQuery.error ? (
            <div className="border-l-2 border-rose-500 bg-rose-50/70 px-3 py-3 text-sm text-rose-800 dark:bg-rose-900/20 dark:text-rose-200">
              <div className="font-medium">读取模型目录失败</div>
              <div className="mt-1 break-words">
                {formatProviderModelFeatureError(catalogQuery.error)}
              </div>
              <Button
                variant="secondary"
                size="sm"
                className="mt-3"
                onClick={() => void catalogQuery.refetch()}
              >
                重试
              </Button>
            </div>
          ) : (
            <div className="space-y-3">
              {profilesQuery.error ? (
                <div className="border-l-2 border-rose-500 bg-rose-50/70 px-3 py-3 text-sm text-rose-800 dark:bg-rose-900/20 dark:text-rose-200">
                  <div className="font-medium">读取 Codex Profile 失败</div>
                  <div className="mt-1 break-words">
                    {formatProviderModelFeatureError(profilesQuery.error)}
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    className="mt-3"
                    onClick={() => void profilesQuery.refetch()}
                    disabled={profilesQuery.isFetching}
                  >
                    {profilesQuery.isFetching ? "重试中…" : "重试 Profile"}
                  </Button>
                </div>
              ) : null}

              {visibleModels.length === 0 ? (
                <EmptyState
                  variant="dashed"
                  icon={<Library className="h-7 w-7" aria-hidden="true" />}
                  title={search.trim() ? "没有匹配的模型" : "模型目录为空"}
                />
              ) : (
                <div className="max-h-[46vh] divide-y divide-border overflow-y-auto border-y border-border scrollbar-overlay">
                  {visibleModels.map((model) => (
                    <ModelRow
                      key={model.modelUuid}
                      model={model}
                      profiles={profilesByModel.get(model.modelUuid) ?? []}
                      disabled={busy}
                      profileCreationDisabledReason={
                        !model.capabilitiesConfigured
                          ? "请先配置模型能力"
                          : !codexProxyReady || cliProxyStatusQuery.isLoading
                            ? "请先启用并同步 Codex CLI 代理"
                            : null
                      }
                      onConfigureCapabilities={() => openCapabilityConfig(model)}
                      onCreateProfile={() => {
                        setProfileModel(model);
                        setProfileName(suggestCodexProfileName(model.remoteModelId));
                      }}
                      onDeleteManual={() => void deleteManualModel(model)}
                      onDeleteProfile={setDeleteProfileTarget}
                    />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      </Dialog>

      <Dialog
        open={capabilityModel != null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen && capabilitiesUpdateMutation.isPending) return;
          if (!nextOpen) setCapabilityModel(null);
        }}
        title="配置模型能力"
        description={capabilityModel ? `模型：${capabilityModel.remoteModelId}` : undefined}
        className="max-w-xl"
      >
        <div className="space-y-5">
          <div className="flex items-center justify-between gap-4 border-b border-border pb-4">
            <label htmlFor="provider-model-reasoning-enabled" className="text-sm font-medium">
              推理强度
            </label>
            <Switch
              id="provider-model-reasoning-enabled"
              checked={reasoningEnabled}
              onCheckedChange={setReasoningConfigurationEnabled}
              disabled={capabilitiesUpdateMutation.isPending}
              aria-label="启用推理强度"
            />
          </div>

          {reasoningEnabled ? (
            <div className="space-y-3">
              <div className="text-sm font-medium">支持档位</div>
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                {PROVIDER_MODEL_REASONING_EFFORTS.map((effort) => (
                  <label
                    key={effort}
                    className={cn(
                      "flex h-9 min-w-0 items-center gap-2 border px-2.5 text-sm transition",
                      selectedReasoningEfforts.includes(effort)
                        ? "border-primary bg-primary/5 text-foreground"
                        : "border-border text-muted-foreground"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={selectedReasoningEfforts.includes(effort)}
                      onChange={(event) =>
                        toggleReasoningEffort(effort, event.currentTarget.checked)
                      }
                      disabled={capabilitiesUpdateMutation.isPending}
                      className="h-4 w-4 shrink-0 accent-primary"
                    />
                    <span className="min-w-0 truncate font-mono">{effort}</span>
                  </label>
                ))}
              </div>

              <label className="block space-y-1.5 text-sm font-medium">
                <span>默认档位</span>
                <Select
                  value={defaultReasoningEffort ?? ""}
                  onChange={(event) =>
                    setDefaultReasoningEffort(
                      event.currentTarget.value as ProviderModelReasoningEffort
                    )
                  }
                  disabled={
                    selectedReasoningEfforts.length === 0 || capabilitiesUpdateMutation.isPending
                  }
                  aria-label="默认推理强度"
                  className="w-full"
                  mono
                >
                  <option value="">请选择</option>
                  {selectedReasoningEfforts.map((effort) => (
                    <option key={effort} value={effort}>
                      {effort}
                    </option>
                  ))}
                </Select>
              </label>
            </div>
          ) : null}

          <label className="block space-y-1.5 text-sm font-medium">
            <span>上下文窗口（tokens）</span>
            <Input
              type="number"
              inputMode="numeric"
              min={MODEL_CONTEXT_WINDOW_MIN_TOKENS}
              max={MODEL_CONTEXT_WINDOW_MAX_TOKENS}
              step={1}
              value={contextWindowText}
              onChange={(event) => setContextWindowText(event.currentTarget.value)}
              placeholder="未知"
              aria-label="上下文窗口"
              disabled={capabilitiesUpdateMutation.isPending}
            />
          </label>

          {parsedContextWindow === undefined ? (
            <div className="text-sm text-rose-600 dark:text-rose-300">
              上下文窗口需为 {MODEL_CONTEXT_WINDOW_MIN_TOKENS.toLocaleString()} 至{" "}
              {MODEL_CONTEXT_WINDOW_MAX_TOKENS.toLocaleString()} 的整数
            </div>
          ) : reasoningEnabled && selectedReasoningEfforts.length === 0 ? (
            <div className="text-sm text-rose-600 dark:text-rose-300">至少选择一个推理档位</div>
          ) : null}

          <div className="flex justify-end gap-2">
            <Button
              variant="secondary"
              onClick={() => setCapabilityModel(null)}
              disabled={capabilitiesUpdateMutation.isPending}
            >
              取消
            </Button>
            <Button
              variant="primary"
              onClick={() => void saveCapabilities()}
              disabled={!capabilityFormValid || capabilitiesUpdateMutation.isPending}
            >
              <Settings2 className="h-4 w-4" aria-hidden="true" />
              {capabilitiesUpdateMutation.isPending ? "保存中…" : "保存"}
            </Button>
          </div>
        </div>
      </Dialog>

      <Dialog
        open={profileModel != null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen && profileCreateMutation.isPending) return;
          if (!nextOpen) setProfileModel(null);
        }}
        title="创建 Codex Profile"
        description={profileModel ? `模型：${profileModel.remoteModelId}` : undefined}
        className="max-w-lg"
      >
        <div className="space-y-4">
          <Input
            value={profileName}
            onChange={(event) => setProfileName(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void createProfile();
            }}
            placeholder="Profile 名称"
            aria-label="Profile 名称"
            maxLength={64}
            autoFocus
            disabled={profileCreateMutation.isPending}
          />
          {profileName.trim() ? (
            <div className="text-sm text-muted-foreground">
              Codex /model 标识：
              <span className="ml-1 break-all font-mono text-foreground">
                aio/{profileName.trim().toLowerCase()}
              </span>
            </div>
          ) : null}
          <div className="flex justify-end gap-2">
            <Button
              variant="secondary"
              onClick={() => setProfileModel(null)}
              disabled={profileCreateMutation.isPending}
            >
              取消
            </Button>
            <Button
              variant="primary"
              onClick={() => void createProfile()}
              disabled={!profileName.trim() || profileCreateMutation.isPending}
            >
              <FilePlus2 className="h-4 w-4" aria-hidden="true" />
              {profileCreateMutation.isPending ? "创建中…" : "创建"}
            </Button>
          </div>
        </div>
      </Dialog>

      <ConfirmDialog
        open={deleteProfileTarget != null}
        title="删除 Codex Profile"
        description={
          deleteProfileTarget
            ? deleteProfileTarget.fileStatus === "modified"
              ? `将解除受管 Profile：${deleteProfileTarget.profileName}。文件已被外部修改，将保留原文件。`
              : `将删除受管 Profile：${deleteProfileTarget.profileName}`
            : undefined
        }
        onClose={() => {
          if (!profileDeleteMutation.isPending) setDeleteProfileTarget(null);
        }}
        onConfirm={() => void deleteProfile()}
        confirmLabel="删除"
        confirmingLabel="删除中…"
        confirming={profileDeleteMutation.isPending}
        confirmVariant="danger"
      />
    </>
  );
}

function ModelRow({
  model,
  profiles,
  disabled,
  profileCreationDisabledReason,
  onConfigureCapabilities,
  onCreateProfile,
  onDeleteManual,
  onDeleteProfile,
}: {
  model: ProviderModel;
  profiles: CodexManagedProfile[];
  disabled: boolean;
  profileCreationDisabledReason: string | null;
  onConfigureCapabilities: () => void;
  onCreateProfile: () => void;
  onDeleteManual: () => void;
  onDeleteProfile: (profile: CodexManagedProfile) => void;
}) {
  return (
    <div className="grid gap-3 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
      <div className="min-w-0">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <span className="break-all font-mono text-sm font-medium text-foreground">
            {model.remoteModelId}
          </span>
          <span className="rounded-full bg-secondary px-2 py-0.5 text-[10px] text-muted-foreground">
            {model.source === "manual" ? "手工" : "发现"}
          </span>
          {model.stale ? (
            <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[10px] text-amber-700 dark:bg-amber-900/30 dark:text-amber-300">
              已过期
            </span>
          ) : null}
        </div>
        <div
          className={cn(
            "mt-1.5 text-xs",
            model.capabilitiesConfigured
              ? "text-muted-foreground"
              : "text-amber-700 dark:text-amber-300"
          )}
        >
          {modelCapabilitySummary(model)}
        </div>
        {profiles.length > 0 ? (
          <div className="mt-2 flex flex-wrap gap-2">
            {profiles.map((profile) => (
              <span
                key={profile.profileUuid}
                className="inline-flex min-w-0 items-center gap-1 rounded-md border border-border px-2 py-1 text-xs"
              >
                <span className="max-w-40 truncate font-medium" title={profile.canonicalModel}>
                  {profile.canonicalModel}
                </span>
                <span
                  className={cn(
                    "rounded-full px-1.5 py-0.5 text-[10px]",
                    fileStatusClassName(profile.fileStatus)
                  )}
                >
                  {fileStatusLabel(profile.fileStatus)}
                </span>
                <button
                  type="button"
                  onClick={() => onDeleteProfile(profile)}
                  disabled={disabled}
                  className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted-foreground transition hover:bg-secondary hover:text-rose-600 disabled:opacity-50"
                  title={`删除 Profile ${profile.profileName}`}
                  aria-label={`删除 Profile ${profile.profileName}`}
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </span>
            ))}
          </div>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
        <Button
          variant="secondary"
          size="sm"
          onClick={onConfigureCapabilities}
          disabled={disabled}
          className="h-8 w-8 p-0"
          title="配置模型能力"
          aria-label={`配置模型能力 ${model.remoteModelId}`}
        >
          <Settings2 className="h-4 w-4" aria-hidden="true" />
        </Button>
        <Button
          variant="secondary"
          size="sm"
          onClick={onCreateProfile}
          disabled={disabled || profileCreationDisabledReason != null}
          title={profileCreationDisabledReason ?? "创建 Profile"}
        >
          <FilePlus2 className="h-4 w-4" aria-hidden="true" />
          创建 Profile
        </Button>
        {model.source === "manual" ? (
          <Button
            variant="danger"
            size="sm"
            onClick={onDeleteManual}
            disabled={disabled}
            title="删除手工模型"
            aria-label={`删除手工模型 ${model.remoteModelId}`}
          >
            <Trash2 className="h-4 w-4" aria-hidden="true" />
          </Button>
        ) : null}
      </div>
    </div>
  );
}
