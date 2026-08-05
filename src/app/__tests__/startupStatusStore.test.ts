import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppStartupStatus } from "../../services/app/startupStatus";
import {
  appStartupStatusGet,
  listenAppStartupStatusEvents,
} from "../../services/app/startupStatus";
import { logToConsole } from "../../services/consoleLog";
import { createDeferred } from "../../test/utils/deferred";
import {
  getAppStartupStatusSnapshot,
  listenAndSyncAppStartupStatusSnapshot,
  resetAppStartupStatusStore,
  setAppStartupStatusSnapshot,
  syncAppStartupStatusSnapshot,
} from "../startupStatusStore";

vi.mock("../../services/app/startupStatus", () => ({
  appStartupStatusGet: vi.fn(),
  appStartupRetry: vi.fn(),
  listenAppStartupStatusEvents: vi.fn(),
}));
vi.mock("../../services/consoleLog", () => ({ logToConsole: vi.fn() }));

const READY_STATUS: AppStartupStatus = {
  running: false,
  currentStage: "ready",
  failedStage: null,
  errorMessage: null,
  canRetry: false,
};

const INITIALIZING_STATUS: AppStartupStatus = {
  running: true,
  currentStage: "initializing_db",
  failedStage: null,
  errorMessage: null,
  canRetry: false,
};

describe("app/startupStatusStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetAppStartupStatusStore();
    vi.mocked(listenAppStartupStatusEvents).mockResolvedValue(vi.fn());
  });

  it("does not let an older GET response overwrite a newer status update", async () => {
    const pendingGet = createDeferred<AppStartupStatus>();
    vi.mocked(appStartupStatusGet).mockReturnValue(pendingGet.promise);

    const sync = syncAppStartupStatusSnapshot();
    setAppStartupStatusSnapshot(READY_STATUS);
    pendingGet.resolve(INITIALIZING_STATUS);
    await sync;

    expect(getAppStartupStatusSnapshot()).toEqual(READY_STATUS);
  });

  it("invalidates an in-flight GET when the store resets", async () => {
    const pendingGet = createDeferred<AppStartupStatus>();
    vi.mocked(appStartupStatusGet).mockReturnValue(pendingGet.promise);

    const sync = syncAppStartupStatusSnapshot();
    resetAppStartupStatusStore();
    pendingGet.resolve(INITIALIZING_STATUS);
    await sync;

    expect(getAppStartupStatusSnapshot().currentStage).toBe("idle");
  });

  it("starts the initial GET only after event listener registration succeeds", async () => {
    const listenerReady = createDeferred<() => void>();
    vi.mocked(listenAppStartupStatusEvents).mockReturnValue(listenerReady.promise);
    vi.mocked(appStartupStatusGet).mockResolvedValue(INITIALIZING_STATUS);

    const subscription = listenAndSyncAppStartupStatusSnapshot();
    expect(appStartupStatusGet).not.toHaveBeenCalled();

    const unlisten = vi.fn();
    listenerReady.resolve(unlisten);
    const cleanup = await subscription;

    await vi.waitFor(() => expect(appStartupStatusGet).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(getAppStartupStatusSnapshot()).toEqual(INITIALIZING_STATUS));
    cleanup();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("preserves an event received while listener registration is settling", async () => {
    vi.mocked(listenAppStartupStatusEvents).mockImplementation(async (onStatus) => {
      onStatus(READY_STATUS);
      return vi.fn();
    });
    vi.mocked(appStartupStatusGet).mockResolvedValue(INITIALIZING_STATUS);

    await listenAndSyncAppStartupStatusSnapshot();

    await vi.waitFor(() => expect(appStartupStatusGet).toHaveBeenCalledTimes(1));
    expect(getAppStartupStatusSnapshot()).toEqual(READY_STATUS);
  });

  it("ignores an initial GET response after its subscription is cleaned up", async () => {
    const pendingGet = createDeferred<AppStartupStatus>();
    const unlisten = vi.fn();
    vi.mocked(listenAppStartupStatusEvents).mockResolvedValue(unlisten);
    vi.mocked(appStartupStatusGet).mockReturnValue(pendingGet.promise);

    const cleanup = await listenAndSyncAppStartupStatusSnapshot();
    cleanup();
    pendingGet.resolve(INITIALIZING_STATUS);
    await pendingGet.promise;

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(getAppStartupStatusSnapshot().currentStage).toBe("idle");
  });

  it("does not let an older subscription cleanup invalidate the current initial GET", async () => {
    const firstListenerReady = createDeferred<() => void>();
    const currentGet = createDeferred<AppStartupStatus>();
    vi.mocked(listenAppStartupStatusEvents)
      .mockReturnValueOnce(firstListenerReady.promise)
      .mockResolvedValueOnce(vi.fn());
    vi.mocked(appStartupStatusGet).mockReturnValue(currentGet.promise);

    const firstSubscription = listenAndSyncAppStartupStatusSnapshot();
    await listenAndSyncAppStartupStatusSnapshot();

    firstListenerReady.resolve(vi.fn());
    const cleanupFirst = await firstSubscription;
    cleanupFirst();
    currentGet.resolve(INITIALIZING_STATUS);
    await currentGet.promise;

    await vi.waitFor(() => expect(getAppStartupStatusSnapshot()).toEqual(INITIALIZING_STATUS));
  });

  it("keeps the listener and logs when the initial GET fails", async () => {
    const unlisten = vi.fn();
    vi.mocked(listenAppStartupStatusEvents).mockResolvedValue(unlisten);
    vi.mocked(appStartupStatusGet).mockRejectedValue(new Error("snapshot boom"));

    const cleanup = await listenAndSyncAppStartupStatusSnapshot();

    await vi.waitFor(() =>
      expect(logToConsole).toHaveBeenCalledWith(
        "warn",
        "启动状态同步失败",
        expect.objectContaining({
          stage: "syncAppStartupStatusSnapshot",
          error: "Error: snapshot boom",
        })
      )
    );
    cleanup();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
