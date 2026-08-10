import { useEffect, useSyncExternalStore } from "react";
import { toast } from "sonner";
import { queryClient } from "../query/queryClient";
import { updaterKeys, settingsKeys } from "../query/keys";
import { useAppAboutQuery } from "../query/appAbout";
import { useSettingsQuery } from "../query/settings";
import { useUpdaterCheckQuery } from "../query/updater";
import { getDevPreviewEnabled, useDevPreviewData } from "./useDevPreviewData";
import { logToConsole } from "../services/consoleLog";
import { emitListenerSnapshot } from "../utils/listeners";
import {
  isExactAioReleaseUrl,
  updaterCandidateChannel,
  updaterCheck,
  updaterDiscard,
  updaterDownloadAndInstall,
  type UpdaterCheckUpdate,
  type UpdaterDownloadEvent,
} from "../services/app/updater";
import { openDesktopUrl } from "../services/desktop/opener";
import {
  BETA_UPDATE_CHANNEL,
  normalizeUpdateChannel,
  notifyUpdateChannelImportSucceeded,
  readUpdateChannelFromSettings,
  settingsUpdateChannelSet,
  subscribeUpdateChannelImportSuccess,
  type UpdateChannel,
} from "../services/app/updateChannel";
import { settingsGet } from "../services/settings/settings";
import type { AppAboutInfo } from "../services/app/appAbout";
import { AIO_REPO_URL } from "../constants/urls";

const STORAGE_KEY_LAST_CHECKED_AT_MS = "updater.lastCheckedAtMs";
const STORAGE_KEY_BETA_CONFIRMED = "updater.betaParticipationConfirmed";
const DEV_PREVIEW_UPDATE_RID = 9_999_001;

export type UpdateMeta = {
  about: AppAboutInfo | null;
  updateCandidate: UpdaterCheckUpdate | null;
  checkingUpdate: boolean;
  dialogOpen: boolean;

  installingUpdate: boolean;
  installError: string | null;
  installTotalBytes: number | null;
  installDownloadedBytes: number;

  updateChannel: UpdateChannel;
  updateChannelReady: boolean;
  updateChannelSaving: boolean;
  updateChannelError: string | null;
  updateCheckError: string | null;
  updateGeneration: number;
  betaParticipationConfirmed: boolean;
  setUpdateChannel: (channel: UpdateChannel) => Promise<boolean>;
};

type Listener = () => void;
type UpdateContext = { channel: UpdateChannel; generation: number };

type UpdateUiState = Pick<
  UpdateMeta,
  | "dialogOpen"
  | "installingUpdate"
  | "installError"
  | "installTotalBytes"
  | "installDownloadedBytes"
> & {
  checkingUpdate: boolean;
  dialogContext: UpdateContext | null;
};

type ChannelRuntimeState = {
  channel: UpdateChannel;
  ready: boolean;
  generation: number;
  saving: boolean;
  channelError: string | null;
  checkError: string | null;
  betaConfirmed: boolean;
};

const listeners = new Set<Listener>();
let started = false;
let updateMetaRevision = 0;
let checkingOperation: {
  context: UpdateContext;
  promise: Promise<UpdaterCheckUpdate | null>;
} | null = null;
let installingOperation: { context: UpdateContext; promise: Promise<boolean | null> } | null = null;

let channelRuntime: ChannelRuntimeState = {
  channel: "stable",
  ready: false,
  generation: 0,
  saving: false,
  channelError: null,
  checkError: null,
  betaConfirmed: readBetaConfirmationMarker(),
};

let uiSnapshot: UpdateUiState = {
  dialogOpen: false,
  dialogContext: null,
  checkingUpdate: false,
  installingUpdate: false,
  installError: null,
  installTotalBytes: null,
  installDownloadedBytes: 0,
};

function readBetaConfirmationMarker(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY_BETA_CONFIRMED) === "1";
  } catch {
    return false;
  }
}

function writeBetaConfirmationMarker() {
  try {
    localStorage.setItem(STORAGE_KEY_BETA_CONFIRMED, "1");
  } catch {
    // The backend channel remains authoritative when storage is unavailable.
  }
}

function writeLastCheckedAtMs(channel: UpdateChannel, ms: number) {
  try {
    localStorage.setItem(`${STORAGE_KEY_LAST_CHECKED_AT_MS}.${channel}`, String(ms));
  } catch {
    // A storage quota failure must not turn a successful check into a failed update state.
  }
}

function emit() {
  updateMetaRevision += 1;
  emitListenerSnapshot(
    listeners,
    (listener) => listener(),
    (error) => logToConsole("warn", "更新状态订阅处理失败", { error: String(error) })
  );
}

function setUiSnapshot(patch: Partial<UpdateUiState>) {
  uiSnapshot = { ...uiSnapshot, ...patch };
  emit();
}

function setChannelRuntime(patch: Partial<ChannelRuntimeState>) {
  channelRuntime = { ...channelRuntime, ...patch };
  emit();
}

function ensureStarted() {
  if (started) return;
  started = true;
  channelRuntime = {
    ...channelRuntime,
    betaConfirmed: channelRuntime.betaConfirmed || readBetaConfirmationMarker(),
  };
}

function currentContext(): UpdateContext {
  return { channel: channelRuntime.channel, generation: channelRuntime.generation };
}

function contextsEqual(left: UpdateContext, right: UpdateContext) {
  return left.channel === right.channel && left.generation === right.generation;
}

export function getUpdateChannelSnapshot() {
  return { ...channelRuntime };
}

export function getUpdateGeneration() {
  return channelRuntime.generation;
}

export function isUpdateContextCurrent(context: UpdateContext) {
  return (
    context.channel === channelRuntime.channel && context.generation === channelRuntime.generation
  );
}

function candidateMatchesCurrent(update: UpdaterCheckUpdate | null | undefined) {
  if (!update) return false;
  if (updaterCandidateChannel(update) !== channelRuntime.channel) return false;
  return update.generation === channelRuntime.generation;
}

function attachContext(update: UpdaterCheckUpdate | null, context: UpdateContext) {
  if (!update) return null;
  const candidateChannel = updaterCandidateChannel(update);
  if (candidateChannel == null || candidateChannel !== context.channel) return null;
  return {
    ...update,
    channel: candidateChannel,
    generation: context.generation,
  } satisfies UpdaterCheckUpdate;
}

function candidateRidsInCache() {
  const rids = new Set<number>();
  for (const [, value] of queryClient.getQueriesData<UpdaterCheckUpdate | null>({
    queryKey: updaterKeys.all,
  })) {
    if (value && Number.isSafeInteger(value.rid) && value.rid >= 0) {
      rids.add(value.rid);
    }
  }
  return rids;
}

function isDevPreviewUpdateCandidate(value: UpdaterCheckUpdate | null) {
  return value?.rid === DEV_PREVIEW_UPDATE_RID;
}

function clearChannelBoundState(previousContext: UpdateContext) {
  const rids = candidateRidsInCache();

  // Remove immediately so a new channel can never render the old candidate while cancellation
  // waits on an uncancellable Tauri Promise.
  void queryClient.cancelQueries({ queryKey: updaterKeys.all }).catch(() => {});
  queryClient.removeQueries({ queryKey: updaterKeys.all });

  setUiSnapshot({
    dialogOpen: false,
    dialogContext: null,
    checkingUpdate: false,
    installingUpdate: false,
    installError: null,
    installTotalBytes: null,
    installDownloadedBytes: 0,
  });
  checkingOperation = null;
  installingOperation = null;

  for (const rid of rids) {
    if (rid === DEV_PREVIEW_UPDATE_RID) continue;
    void updaterDiscard(rid).catch((error) => {
      if (isUpdateContextCurrent(previousContext)) return;
      setChannelRuntime({
        channelError: `更新候选清理失败：${error instanceof Error ? error.message : String(error)}`,
      });
      toast("更新候选清理失败：请稍后重试");
    });
  }
}

/** Apply only the canonical backend channel and invalidate all channel-bound renderer state. */
export async function syncCanonicalUpdateChannel(value: unknown): Promise<boolean> {
  ensureStarted();
  const nextChannel = normalizeUpdateChannel(value);
  const changed = !channelRuntime.ready || channelRuntime.channel !== nextChannel;
  if (!changed) {
    if (nextChannel === BETA_UPDATE_CHANNEL && !channelRuntime.betaConfirmed) {
      writeBetaConfirmationMarker();
      setChannelRuntime({ betaConfirmed: true, ready: true });
    } else {
      setChannelRuntime({ ready: true });
    }
    return false;
  }

  const previousContext = currentContext();
  const nextGeneration = channelRuntime.generation + 1;
  clearChannelBoundState(previousContext);
  if (nextChannel === BETA_UPDATE_CHANNEL) {
    writeBetaConfirmationMarker();
  }
  setChannelRuntime({
    channel: nextChannel,
    ready: true,
    generation: nextGeneration,
    checkError: null,
    channelError: null,
    betaConfirmed: channelRuntime.betaConfirmed || nextChannel === BETA_UPDATE_CHANNEL,
  });
  return true;
}

/** Read canonical settings before any manual/background check. Failure is always stable-only. */
export async function ensureUpdateChannelReady(): Promise<UpdateChannel> {
  ensureStarted();
  if (channelRuntime.ready) return channelRuntime.channel;

  try {
    const settings = await queryClient.fetchQuery({
      queryKey: settingsKeys.get(),
      queryFn: () => settingsGet(),
      staleTime: 0,
    });
    await syncCanonicalUpdateChannel(readUpdateChannelFromSettings(settings));
  } catch (error) {
    logToConsole("warn", "读取更新频道失败，按稳定频道处理", { error: String(error) });
  }

  return channelRuntime.channel;
}

async function refreshCanonicalUpdateChannelAfterImport() {
  try {
    await queryClient.cancelQueries({ queryKey: settingsKeys.get(), exact: true });
    const settings = await queryClient.fetchQuery({
      queryKey: settingsKeys.get(),
      queryFn: () => settingsGet(),
      staleTime: 0,
    });
    await syncCanonicalUpdateChannel(readUpdateChannelFromSettings(settings));
  } catch (error) {
    const message = String(error);
    logToConsole("error", "导入后读取更新频道失败", { error: message });
    setChannelRuntime({ channelError: message });
  }
}

function buildDevPreviewUpdateCandidate(
  channel: UpdateChannel,
  currentVersion?: string
): UpdaterCheckUpdate | null {
  if (channel !== "stable") return null;
  const version = "0.37.1-dev-preview";
  return {
    rid: DEV_PREVIEW_UPDATE_RID,
    channel: "stable",
    version,
    currentVersion: currentVersion ?? "0.0.0",
    date: "2026-04-05T11:12:44Z",
    body: [
      "## Dev 预览更新日志",
      "",
      "- 更新弹窗现在会展示 release 正文，而不只是 GitHub 链接。",
      "- 更新日志内容支持多行文本和长内容滚动。",
      "- 这是一条仅开发环境可见的预览数据，不会参与真实安装。",
    ].join("\n"),
    releaseUrl: `${AIO_REPO_URL}/releases/tag/aio-coding-hub-v${version}`,
  };
}

export async function updateCheckNow(options: {
  silent: boolean;
  openDialogIfUpdate: boolean;
}): Promise<UpdaterCheckUpdate | null> {
  ensureStarted();
  const channel = await ensureUpdateChannelReady();
  const context = currentContext();
  const existing = checkingOperation;
  if (existing && contextsEqual(existing.context, context)) {
    return existing.promise;
  }
  const queryKey = updaterKeys.check(channel);
  const previousCandidate = queryClient.getQueryData<UpdaterCheckUpdate | null>(queryKey) ?? null;
  let previousCandidateDiscarded = false;

  function discardPreviousCandidate(nextRid?: number) {
    if (
      previousCandidateDiscarded ||
      !previousCandidate ||
      previousCandidate.rid === nextRid ||
      isDevPreviewUpdateCandidate(previousCandidate)
    ) {
      return;
    }
    previousCandidateDiscarded = true;
    void updaterDiscard(previousCandidate.rid).catch((error) => {
      if (!isUpdateContextCurrent(context)) return;
      const message = error instanceof Error ? error.message : String(error);
      logToConsole("error", "清理旧更新候选失败", {
        error: message,
        rid: previousCandidate.rid,
        channel,
      });
      setChannelRuntime({ channelError: `更新候选清理失败：${message}` });
      toast("更新候选清理失败：请稍后重试");
    });
  }

  function closeContextDialog() {
    if (uiSnapshot.dialogContext && contextsEqual(uiSnapshot.dialogContext, context)) {
      setUiSnapshot({ dialogOpen: false, dialogContext: null });
    }
  }

  const operation = {
    context,
    promise: Promise.resolve(null) as Promise<UpdaterCheckUpdate | null>,
  };
  operation.promise = (async () => {
    setUiSnapshot({ checkingUpdate: true });
    try {
      const rawUpdate = getDevPreviewEnabled()
        ? buildDevPreviewUpdateCandidate(channel)
        : await queryClient.fetchQuery({
            queryKey: updaterKeys.check(channel),
            queryFn: async () => {
              const update = await updaterCheck(channel);
              if (!isUpdateContextCurrent(context)) {
                if (update && !isDevPreviewUpdateCandidate(update)) {
                  void updaterDiscard(update.rid).catch(() => {});
                }
                return null;
              }
              return attachContext(update, context);
            },
            staleTime: 0,
          });

      if (!isUpdateContextCurrent(context)) {
        if (rawUpdate && !isDevPreviewUpdateCandidate(rawUpdate)) {
          void updaterDiscard(rawUpdate.rid).catch(() => {});
        }
        return null;
      }

      const update = attachContext(rawUpdate, context);
      if (update) {
        queryClient.setQueryData(queryKey, update);
        discardPreviousCandidate(update.rid);
        setChannelRuntime({ checkError: null });
      } else {
        queryClient.removeQueries({ queryKey, exact: true });
        discardPreviousCandidate();
        closeContextDialog();
      }
      writeLastCheckedAtMs(channel, Date.now());

      if (update && options.openDialogIfUpdate) {
        setUiSnapshot({
          dialogOpen: true,
          dialogContext: context,
          installError: null,
          installDownloadedBytes: 0,
          installTotalBytes: null,
          installingUpdate: false,
        });
      }

      if (!update && !options.silent) {
        toast(channel === BETA_UPDATE_CHANNEL ? "Beta 已是最新版本" : "已是最新版本");
      }

      return update;
    } catch (error) {
      if (!isUpdateContextCurrent(context)) return null;
      const message = String(error);
      queryClient.removeQueries({ queryKey, exact: true });
      discardPreviousCandidate();
      closeContextDialog();
      logToConsole("error", "检查更新失败", { error: message, channel });
      writeLastCheckedAtMs(channel, Date.now());
      setChannelRuntime({ checkError: message });
      if (!options.silent) toast(`检查更新失败：${message}`);
      return null;
    } finally {
      if (checkingOperation === operation) {
        checkingOperation = null;
        setUiSnapshot({ checkingUpdate: false });
      }
    }
  })();
  checkingOperation = operation;
  return operation.promise;
}

function onUpdaterDownloadEvent(evt: UpdaterDownloadEvent, context: UpdateContext) {
  if (!isUpdateContextCurrent(context)) return;
  if (evt.event === "started") {
    const total = evt.data?.contentLength;
    setUiSnapshot({ installTotalBytes: typeof total === "number" ? total : null });
    return;
  }
  if (evt.event === "progress") {
    const chunk = evt.data?.chunkLength;
    if (typeof chunk === "number" && Number.isFinite(chunk) && chunk > 0) {
      setUiSnapshot({ installDownloadedBytes: uiSnapshot.installDownloadedBytes + chunk });
    }
  }
}

export async function updateDownloadAndInstall(): Promise<boolean | null> {
  ensureStarted();
  await ensureUpdateChannelReady();
  const context = currentContext();
  const updateCandidate =
    queryClient.getQueryData<UpdaterCheckUpdate | null>(updaterKeys.check(context.channel)) ?? null;

  if (!updateCandidate || !candidateMatchesCurrent(updateCandidate)) {
    if (uiSnapshot.dialogOpen) {
      setUiSnapshot({ dialogOpen: false, dialogContext: null });
    }
    return null;
  }
  if (isDevPreviewUpdateCandidate(updateCandidate)) {
    toast("Dev 预览更新仅用于展示，不能安装");
    return false;
  }
  const candidate = updateCandidate;

  if (uiSnapshot.installingUpdate) {
    return installingOperation && contextsEqual(installingOperation.context, context)
      ? installingOperation.promise
      : false;
  }

  setUiSnapshot({
    installError: null,
    installDownloadedBytes: 0,
    installTotalBytes: null,
    installingUpdate: true,
  });

  const operation = {
    context,
    promise: Promise.resolve(null) as Promise<boolean | null>,
  };
  operation.promise = (async () => {
    try {
      const ok = await updaterDownloadAndInstall({
        rid: candidate.rid,
        onEvent: (event) => onUpdaterDownloadEvent(event, context),
      });
      return isUpdateContextCurrent(context) ? ok : false;
    } catch (error) {
      const message = String(error);
      if (isUpdateContextCurrent(context)) {
        setUiSnapshot({ installError: message });
        logToConsole("error", "安装更新失败", { error: message, channel: context.channel });
        toast("安装更新失败：请稍后重试");
      }
      return false;
    } finally {
      if (installingOperation === operation) {
        installingOperation = null;
        if (isUpdateContextCurrent(context)) {
          setUiSnapshot({ installingUpdate: false });
        }
      }
    }
  })();
  installingOperation = operation;
  return operation.promise;
}

export function updateDialogSetError(message: string | null) {
  setUiSnapshot({ installError: message });
}

export async function openUpdateCandidateReleaseUrl(
  candidate: UpdaterCheckUpdate | null | undefined
): Promise<void> {
  const releaseUrl = candidate?.releaseUrl;
  if (!isExactAioReleaseUrl(releaseUrl)) {
    throw new Error("UPDATER_RELEASE_URL_INVALID: exact candidate Release URL is unavailable");
  }

  try {
    await openDesktopUrl(releaseUrl);
  } catch (error) {
    try {
      window.open(releaseUrl, "_blank", "noopener,noreferrer");
    } catch {
      // Preserve the original opener error for the caller's visible error state.
    }
    throw error;
  }
}

export function updateDialogSetOpen(open: boolean) {
  if (!open && uiSnapshot.installingUpdate) return;

  if (open) {
    const context = currentContext();
    const candidate =
      queryClient.getQueryData<UpdaterCheckUpdate | null>(updaterKeys.check(context.channel)) ??
      null;
    if (candidate && !candidateMatchesCurrent(candidate)) return;
    setUiSnapshot({ dialogOpen: true, dialogContext: context });
    return;
  }

  setUiSnapshot({
    dialogOpen: false,
    dialogContext: null,
    installError: null,
    installDownloadedBytes: 0,
    installTotalBytes: null,
    installingUpdate: false,
  });
}

export async function setUpdateChannel(channel: UpdateChannel): Promise<boolean> {
  ensureStarted();
  const nextChannel = normalizeUpdateChannel(channel);
  if (channelRuntime.saving) return false;
  if (channelRuntime.ready && channelRuntime.channel === nextChannel) return true;

  setChannelRuntime({ saving: true, channelError: null, checkError: null });
  try {
    const canonical = await settingsUpdateChannelSet(nextChannel);
    if (canonical !== nextChannel) {
      throw new Error("UPDATER_CHANNEL_CHANGED: backend did not persist the requested channel");
    }
    await syncCanonicalUpdateChannel(canonical);
    await updateCheckNow({
      silent: nextChannel === "stable",
      openDialogIfUpdate: true,
    });
    return true;
  } catch (error) {
    const message = String(error);
    setChannelRuntime({ channelError: message });
    toast(`切换更新频道失败：${message}`);
    return false;
  } finally {
    setChannelRuntime({ saving: false });
  }
}

export function subscribeUpdateMeta(listener: Listener) {
  listeners.add(listener);
  void ensureStarted();
  return () => {
    listeners.delete(listener);
  };
}

export function useUpdateMeta(): UpdateMeta {
  const aboutQuery = useAppAboutQuery();
  const settingsQuery = useSettingsQuery();
  const updaterCheckQuery = useUpdaterCheckQuery({
    channel: channelRuntime.channel,
    enabled: false,
  });
  const devPreview = useDevPreviewData();

  useSyncExternalStore(
    subscribeUpdateMeta,
    () => updateMetaRevision,
    () => updateMetaRevision
  );
  const ui = uiSnapshot;

  useEffect(() => {
    const unsubscribe = subscribeUpdateChannelImportSuccess(
      refreshCanonicalUpdateChannelAfterImport
    );
    return unsubscribe;
  }, []);

  useEffect(() => {
    if (settingsQuery.isError) {
      // A failed first read cannot authorize Beta. Once a canonical value was read, keep it
      // instead of changing channel without a matching backend write and generation transition.
      if (!channelRuntime.ready) {
        setChannelRuntime({ channel: "stable", ready: false });
      }
      return;
    }
    if (!settingsQuery.data || settingsQuery.isPlaceholderData) return;
    if (settingsQuery.isFetchedAfterMount === false) return;

    const canonical = readUpdateChannelFromSettings(settingsQuery.data);
    void syncCanonicalUpdateChannel(canonical).then((changed) => {
      if (changed && canonical === BETA_UPDATE_CHANNEL) {
        void updateCheckNow({ silent: true, openDialogIfUpdate: false });
      }
    });
  }, [
    settingsQuery.data,
    settingsQuery.isError,
    settingsQuery.isFetchedAfterMount,
    settingsQuery.isPlaceholderData,
  ]);

  const channel = channelRuntime;
  const cachedCandidate =
    queryClient.getQueryData<UpdaterCheckUpdate | null>(updaterKeys.check(channel.channel)) ??
    updaterCheckQuery.data ??
    null;
  const currentCandidate = candidateMatchesCurrent(cachedCandidate) ? cachedCandidate : null;
  const updateCandidate = devPreview.enabled
    ? attachContext(
        buildDevPreviewUpdateCandidate(channel.channel, aboutQuery.data?.app_version),
        currentContext()
      )
    : currentCandidate;

  return {
    about: aboutQuery.data ?? null,
    updateCandidate,
    checkingUpdate: ui.checkingUpdate || updaterCheckQuery.isFetching,
    dialogOpen: ui.dialogOpen,

    installingUpdate: ui.installingUpdate,
    installError: ui.installError,
    installTotalBytes: ui.installTotalBytes,
    installDownloadedBytes: ui.installDownloadedBytes,

    updateChannel: channel.channel,
    updateChannelReady: channel.ready,
    updateChannelSaving: channel.saving,
    updateChannelError: channel.channelError,
    updateCheckError: channel.checkError,
    updateGeneration: channel.generation,
    betaParticipationConfirmed: channel.betaConfirmed,
    setUpdateChannel,
  };
}

// Keep the import-side effect reachable from the query layer without exposing updater internals.
export { notifyUpdateChannelImportSucceeded };
