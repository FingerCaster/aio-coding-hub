import { commands, type UpdateChannel } from "../../generated/bindings";
import { invokeGeneratedIpc } from "../generatedIpc";
import { createRiskyIpcConfirm } from "../ipcConfirm";

export type { UpdateChannel } from "../../generated/bindings";

export const STABLE_UPDATE_CHANNEL: UpdateChannel = "stable";
export const BETA_UPDATE_CHANNEL: UpdateChannel = "beta";
export const UPDATE_CHANNEL_SET_COMMAND = "settings_update_channel_set";

export function isUpdateChannel(value: unknown): value is UpdateChannel {
  return value === STABLE_UPDATE_CHANNEL || value === BETA_UPDATE_CHANNEL;
}

export function normalizeUpdateChannel(value: unknown): UpdateChannel {
  return isUpdateChannel(value) ? value : STABLE_UPDATE_CHANNEL;
}

/** Read the backend's normalized SettingsView without giving imported JSON authority. */
export function readUpdateChannelFromSettings(value: unknown): UpdateChannel {
  if (typeof value === "string") {
    return normalizeUpdateChannel(value);
  }

  if (!value || typeof value !== "object") {
    return STABLE_UPDATE_CHANNEL;
  }

  const object = value as Record<string, unknown>;
  return normalizeUpdateChannel(object.update_channel);
}

export function updateChannelLabel(channel: UpdateChannel): string {
  return channel === BETA_UPDATE_CHANNEL ? "Beta 更新" : "稳定更新";
}

function readMutationChannel(value: unknown): UpdateChannel | null {
  if (isUpdateChannel(value)) {
    return value;
  }

  if (!value || typeof value !== "object") {
    return null;
  }

  const object = value as Record<string, unknown>;
  const nestedSettings = object.settings;
  const nestedChannel = readUpdateChannelFromSettings(nestedSettings);
  const directValue = object.update_channel;

  if (isUpdateChannel(directValue)) {
    return directValue;
  }

  if (nestedSettings != null) {
    return nestedChannel;
  }

  return null;
}

/** Persist the channel through updater-core's dedicated writer. */
export async function settingsUpdateChannelSet(channel: UpdateChannel): Promise<UpdateChannel> {
  const normalizedChannel = normalizeUpdateChannel(channel);
  const confirm =
    normalizedChannel === BETA_UPDATE_CHANNEL
      ? createRiskyIpcConfirm(UPDATE_CHANNEL_SET_COMMAND, `update_channel:${normalizedChannel}`)
      : null;

  const result = await invokeGeneratedIpc<unknown>({
    title: "保存更新频道失败",
    cmd: UPDATE_CHANNEL_SET_COMMAND,
    args: { channel: normalizedChannel, confirm },
    invoke: () => commands.settingsUpdateChannelSet(normalizedChannel, confirm),
  });

  const canonical = readMutationChannel(result);
  if (!canonical) {
    throw new Error("UPDATER_CHANNEL_INVALID_RESPONSE: canonical channel is missing");
  }

  return canonical;
}

type ImportListener = () => void | Promise<void>;
const importListeners = new Set<ImportListener>();

/** Config import is normalized by updater-core; notify the renderer to hide old Beta state now. */
export async function notifyUpdateChannelImportSucceeded() {
  await Promise.all([...importListeners].map((listener) => listener()));
}

export function subscribeUpdateChannelImportSuccess(listener: ImportListener) {
  importListeners.add(listener);
  return () => {
    importListeners.delete(listener);
  };
}
