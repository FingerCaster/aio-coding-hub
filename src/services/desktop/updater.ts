import { Channel } from "@tauri-apps/api/core";
import {
  commands,
  type DesktopUpdaterMetadata,
  type UpdateChannel,
} from "../../generated/bindings";
import { AIO_REPO_URL } from "../../constants/urls";
import { invokeGeneratedIpc } from "../generatedIpc";
import { createRiskyIpcConfirm } from "../ipcConfirm";
import { invokeTauriOrNull } from "../tauriInvoke";

export const DESKTOP_UPDATER_HANDWRITTEN_COMMAND = "desktop_updater_download_and_install";
export const DESKTOP_UPDATER_HANDWRITTEN_REASON =
  "Requires a Tauri Channel callback, so this desktop updater path stays as the single handwritten desktop IPC exception.";

export type DesktopUpdaterCheck = {
  rid: number;
  channel: UpdateChannel;
  version: string;
  currentVersion: string;
  date?: string;
  body?: string;
  releaseUrl: string;
};

export type DesktopUpdaterDownloadEvent =
  | { event: "started"; data?: { contentLength?: number } }
  | { event: "progress"; data?: { chunkLength?: number } }
  | { event: "finished"; data?: unknown };

export function validateDesktopUpdaterRid(rid: number): number {
  if (!Number.isSafeInteger(rid) || rid < 0) {
    throw new Error(`SEC_INVALID_INPUT: invalid updater rid=${rid}`);
  }
  return rid;
}

export function normalizeDesktopUpdaterTimeoutMs(timeoutMs?: number | null): number | null {
  if (timeoutMs == null) return null;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`SEC_INVALID_INPUT: invalid updater timeoutMs=${timeoutMs}`);
  }
  return timeoutMs;
}

function normalizeDesktopUpdaterChannel(channel: unknown): UpdateChannel {
  if (channel !== "stable" && channel !== "beta") {
    throw new Error(`SEC_INVALID_INPUT: invalid updater channel=${String(channel)}`);
  }
  return channel;
}

function asOptionalString(value: unknown) {
  return typeof value === "string" ? value : undefined;
}

function asRequiredNonEmptyString(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function asOptionalNonNegativeSafeInteger(value: unknown) {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

export function parseDesktopUpdaterCheck(value: unknown): DesktopUpdaterCheck | null {
  if (value == null || value === false || typeof value !== "object") {
    return null;
  }

  const obj = value as Record<string, unknown>;
  const rid = asOptionalNonNegativeSafeInteger(obj.rid);
  const channel = obj.channel;
  const currentVersion = asRequiredNonEmptyString(obj.currentVersion);
  const version = asRequiredNonEmptyString(obj.version);
  const releaseUrl = asRequiredNonEmptyString(obj.releaseUrl);
  const date = asOptionalString(obj.date);
  const body = asOptionalString(obj.body);
  const versionMatchesChannel =
    version != null &&
    /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-beta\.[1-9]\d*)?$/.test(version) &&
    (channel === "beta" || !version.includes("-beta."));
  const expectedReleaseUrl =
    version == null ? null : `${AIO_REPO_URL}/releases/tag/aio-coding-hub-v${version}`;

  if (
    rid == null ||
    (channel !== "stable" && channel !== "beta") ||
    currentVersion == null ||
    !versionMatchesChannel ||
    releaseUrl == null ||
    releaseUrl !== expectedReleaseUrl ||
    (obj.date != null && date == null) ||
    (obj.body != null && body == null)
  ) {
    return null;
  }

  return {
    rid,
    channel,
    version,
    currentVersion,
    date,
    body,
    releaseUrl,
  };
}

function parseDesktopUpdaterDownloadEvent(value: unknown): DesktopUpdaterDownloadEvent | null {
  if (value == null || typeof value !== "object") {
    return null;
  }

  const obj = value as Record<string, unknown>;
  const event = obj.event;
  if (event !== "started" && event !== "progress" && event !== "finished") {
    return null;
  }

  const data = obj.data;
  if (event === "started") {
    const startedData =
      data && typeof data === "object"
        ? {
            contentLength: asOptionalNonNegativeSafeInteger(
              (data as Record<string, unknown>).contentLength
            ),
          }
        : undefined;
    return { event, data: startedData };
  }

  if (event === "progress") {
    const progressData =
      data && typeof data === "object"
        ? {
            chunkLength: asOptionalNonNegativeSafeInteger(
              (data as Record<string, unknown>).chunkLength
            ),
          }
        : undefined;
    return { event, data: progressData };
  }

  return { event, data };
}

export async function desktopUpdaterCheck(options?: {
  channel?: UpdateChannel;
  timeoutMs?: number | null;
}): Promise<DesktopUpdaterCheck | null> {
  const expectedChannel = normalizeDesktopUpdaterChannel(options?.channel ?? "stable");
  const timeout = normalizeDesktopUpdaterTimeoutMs(options?.timeoutMs);
  const result = await invokeGeneratedIpc<DesktopUpdaterMetadata, null>({
    title: "检查更新失败",
    cmd: "desktop_updater_check",
    args: { expectedChannel, timeout },
    invoke: () => commands.desktopUpdaterCheck(expectedChannel, timeout),
    nullResultBehavior: "return_fallback",
    fallback: null,
  });
  return parseDesktopUpdaterCheck(result);
}

export async function desktopUpdaterDiscard(rid: number): Promise<boolean> {
  const normalizedRid = validateDesktopUpdaterRid(rid);
  return invokeGeneratedIpc<boolean>({
    title: "丢弃更新候选失败",
    cmd: "desktop_updater_discard",
    args: { rid: normalizedRid },
    invoke: () => commands.desktopUpdaterDiscard(normalizedRid),
  });
}

export async function desktopUpdaterDownloadAndInstall(options: {
  rid: number;
  onEvent?: (event: DesktopUpdaterDownloadEvent) => void;
  timeoutMs?: number;
}) {
  // Generated bindings cover the check path. Install stays handwritten because
  // the Rust command accepts a Channel callback and Specta cannot express it.
  const rid = validateDesktopUpdaterRid(options.rid);
  const timeout = normalizeDesktopUpdaterTimeoutMs(options.timeoutMs);
  const onEvent = typeof options.onEvent === "function" ? options.onEvent : undefined;
  const channel = new Channel<unknown>((message) => {
    const evt = parseDesktopUpdaterDownloadEvent(message);
    if (!evt) {
      return;
    }
    onEvent?.(evt);
  });
  const confirm = createRiskyIpcConfirm("desktop_updater_download_and_install", `updater:${rid}`);

  return invokeTauriOrNull<boolean>(
    DESKTOP_UPDATER_HANDWRITTEN_COMMAND,
    {
      rid,
      onEvent: channel,
      timeout,
      confirm,
    },
    { timeoutMs: null }
  );
}
