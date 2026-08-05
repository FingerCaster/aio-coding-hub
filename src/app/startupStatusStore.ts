import { useSyncExternalStore } from "react";
import type { AppStartupStatus } from "../services/app/startupStatus";
import {
  appStartupRetry,
  appStartupStatusGet,
  listenAppStartupStatusEvents,
} from "../services/app/startupStatus";
import { logToConsole } from "../services/consoleLog";

const IDLE_STARTUP_STATUS: AppStartupStatus = Object.freeze({
  running: false,
  currentStage: "idle",
  failedStage: null,
  errorMessage: null,
  canRetry: false,
});

let snapshot: AppStartupStatus = IDLE_STARTUP_STATUS;
let statusUpdateGeneration = 0;
let activeStartupStatusSubscription: symbol | null = null;
const listeners = new Set<() => void>();

function emitSnapshot() {
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getAppStartupStatusSnapshot(): AppStartupStatus {
  return snapshot;
}

function commitAppStartupStatusSnapshot(next: AppStartupStatus) {
  snapshot = next;
  emitSnapshot();
}

export function setAppStartupStatusSnapshot(next: AppStartupStatus) {
  statusUpdateGeneration += 1;
  commitAppStartupStatusSnapshot(next);
}

export function resetAppStartupStatusStore() {
  statusUpdateGeneration += 1;
  activeStartupStatusSubscription = null;
  snapshot = IDLE_STARTUP_STATUS;
  emitSnapshot();
}

async function syncAppStartupStatusSnapshotAt(
  startedAtGeneration: number,
  canCommit: () => boolean = () => true
): Promise<void> {
  const next = await appStartupStatusGet();
  if (canCommit() && statusUpdateGeneration === startedAtGeneration) {
    commitAppStartupStatusSnapshot(next);
  }
}

export async function syncAppStartupStatusSnapshot(): Promise<void> {
  await syncAppStartupStatusSnapshotAt(statusUpdateGeneration);
}

export async function retryAppStartupStatusSnapshot(): Promise<void> {
  const startedAtGeneration = statusUpdateGeneration;
  const next = await appStartupRetry();
  if (statusUpdateGeneration === startedAtGeneration) {
    commitAppStartupStatusSnapshot(next);
  }
}

export async function listenAppStartupStatusSnapshot(): Promise<() => void> {
  return listenAppStartupStatusEvents(setAppStartupStatusSnapshot);
}

export async function listenAndSyncAppStartupStatusSnapshot(): Promise<() => void> {
  const subscription = Symbol("app-startup-status-subscription");
  activeStartupStatusSubscription = subscription;
  const startedAtGeneration = statusUpdateGeneration;
  let unlisten: () => void;
  try {
    unlisten = await listenAppStartupStatusSnapshot();
  } catch (error) {
    if (activeStartupStatusSubscription === subscription) {
      activeStartupStatusSubscription = null;
    }
    throw error;
  }

  void syncAppStartupStatusSnapshotAt(
    startedAtGeneration,
    () => activeStartupStatusSubscription === subscription
  ).catch((error) => {
    if (activeStartupStatusSubscription === subscription) {
      logToConsole("warn", "启动状态同步失败", {
        stage: "syncAppStartupStatusSnapshot",
        error: String(error),
      });
    }
  });

  return () => {
    if (activeStartupStatusSubscription === subscription) {
      activeStartupStatusSubscription = null;
    }
    unlisten();
  };
}

export function useAppStartupStatus(): AppStartupStatus {
  return useSyncExternalStore(subscribe, getAppStartupStatusSnapshot, getAppStartupStatusSnapshot);
}
