import { useState, useSyncExternalStore } from "react";

let appSessionStartedAtMs: number | null = null;
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

export function initializeAppSessionStartedAtMs(now: () => number = Date.now): number {
  if (appSessionStartedAtMs == null) {
    appSessionStartedAtMs = now();
    emitSnapshot();
  }
  return appSessionStartedAtMs;
}

export function getAppSessionStartedAtMs(): number | null {
  return appSessionStartedAtMs;
}

export function resetAppSessionStartedAtMsForTests() {
  appSessionStartedAtMs = null;
  emitSnapshot();
}

export function useInitializeAppSession(): number {
  const [startedAt] = useState(() => initializeAppSessionStartedAtMs());
  return startedAt;
}

export function useAppSessionStartedAtMs(): number | null {
  return useSyncExternalStore(subscribe, getAppSessionStartedAtMs, getAppSessionStartedAtMs);
}
